//! ADR 0007 slice 4 checklist: `vesperd --backend systemvm` started from macOS and driven only through its own
//! socket. It boots, pauses and resumes a microVM; a Firecracker error and Firecracker's own connection limit pass
//! through byte for byte; an idle connection is closed while Firecracker keeps one of its own open; a second
//! `vesperd` on the socket or with the id is refused; SIGTERM, SIGKILL and a killed Firecracker each leave no
//! microVM behind. The control channel is used only to inspect and fault the guest.

use super::launchapi::{first, wait_absent};
use super::{connect, report, step, ControlEndpoint, SuiteReport, SuiteStep};
use super::super::{http_request, parse_http_response, read_http_response, HttpResponse};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// `vesperd`'s idle timeout (src/bin/vesperd/systemvm.rs); the idle step waits a little longer.
pub const VESPERD_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// `vesperd`'s per-request ceiling; the snapshot step checks the measured create stays well under it.
pub const VESPERD_REQUEST_CEILING: Duration = Duration::from_secs(120);
/// `vesperd`'s wait on `ERR busy` before it refuses a reused id.
const REUSE_WAIT: Duration = Duration::from_secs(15);
/// The first launch after boot builds the microVM kernel and initramfs.
pub(super) const START_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off rdinit=/init";
/// micro_http's reply to a connection beyond its 10 (`SERVER_FULL_ERROR_MESSAGE`, micro-http 876f3fe).
const FIRECRACKER_SERVER_FULL: &[u8] =
    b"HTTP/1.1 503\r\nServer: Firecracker API\r\nConnection: close\r\nContent-Length: 40\r\n\r\n{ \"error\": \"Too many open connections\" }";
const FIRECRACKER_MAX_CONNECTIONS: usize = 10;

/// How to start `vesperd`: the binary, the launch config or bridge it reads, and a private directory for sockets.
pub struct VesperdRun {
    pub binary: PathBuf,
    pub workdir: PathBuf,
    pub extra_args: Vec<String>,
}

pub(super) struct Vesperd {
    pub(super) child: Child,
    pub(super) socket: PathBuf,
}

impl Drop for Vesperd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl VesperdRun {
    fn spawn(&self, id: &str, socket: &str) -> Result<Vesperd, String> {
        self.spawn_with(id, socket, &[])
    }

    /// `spawn` with more flags, for example `--stage`.
    pub(super) fn spawn_with(&self, id: &str, socket: &str, flags: &[String]) -> Result<Vesperd, String> {
        let socket = self.workdir.join(socket);
        let child = Command::new(&self.binary)
            .args(["--backend", "systemvm", "--id", id, "--socket"])
            .arg(&socket)
            .args(&self.extra_args)
            .args(flags)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("spawn {}: {err}", self.binary.display()))?;
        Ok(Vesperd { child, socket })
    }

    /// Starts `vesperd` and waits until its socket accepts a connection; returns it, its first line of output
    /// (`vesperd listening on … Firecracker <v> pid <n>`) and the time taken.
    fn start(&self, id: &str, socket: &str) -> Result<(Vesperd, String, Duration), String> {
        self.start_with(id, socket, &[])
    }

    /// `start` with more flags, for example `--stage`.
    pub(super) fn start_with(&self, id: &str, socket: &str, flags: &[String]) -> Result<(Vesperd, String, Duration), String> {
        let started = Instant::now();
        let mut v = self.spawn_with(id, socket, flags)?;
        loop {
            if UnixStream::connect(&v.socket).is_ok() {
                let mut line = String::new();
                if let Some(out) = v.child.stdout.take() {
                    let _ = BufReader::new(out).read_line(&mut line);
                }
                return Ok((v, line.trim().to_string(), started.elapsed()));
            }
            if let Some(status) = v.child.try_wait().map_err(|err| err.to_string())? {
                return Err(format!("vesperd exited with {status} before listening: {}", stderr(&mut v)));
            }
            if started.elapsed() > START_TIMEOUT {
                return Err(format!("vesperd not listening on {} after {} s", v.socket.display(), START_TIMEOUT.as_secs()));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

pub(super) fn stderr(v: &mut Vesperd) -> String {
    let mut err = String::new();
    if let Some(mut pipe) = v.child.stderr.take() {
        let _ = pipe.read_to_string(&mut err);
    }
    err.trim().replace('\n', " | ")
}

/// Waits for `vesperd` to exit; returns its exit code (`None` for a signal), stderr and the time taken.
pub(super) fn wait_exit(v: &mut Vesperd, within: Duration) -> Result<(Option<i32>, String, Duration), String> {
    let started = Instant::now();
    loop {
        if let Some(status) = v.child.try_wait().map_err(|err| err.to_string())? {
            return Ok((status.code(), stderr(v), started.elapsed()));
        }
        if started.elapsed() > within {
            return Err(format!("vesperd still running after {} s", within.as_secs()));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn signal(v: &Vesperd, name: &str) -> Result<(), String> {
    let status = Command::new("kill").args([&format!("-{name}"), &v.child.id().to_string()]).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("kill -{name} {} failed", v.child.id()));
    }
    Ok(())
}

/// One keep-alive client connection to a `vesperd` socket.
pub(super) struct Client {
    reader: BufReader<UnixStream>,
    pub(super) writer: UnixStream,
}

impl Client {
    pub(super) fn connect(socket: &Path) -> Result<Client, String> {
        let stream = UnixStream::connect(socket).map_err(|err| format!("connect {}: {err}", socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(60))).map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        Ok(Client { reader: BufReader::new(stream), writer })
    }

    pub(super) fn raw(&mut self, request: &[u8]) -> Result<Vec<u8>, String> {
        self.writer.write_all(request).and_then(|_| self.writer.flush()).map_err(|err| format!("write request: {err}"))?;
        read_http_response(&mut self.reader)
    }

    pub(super) fn call(&mut self, method: &str, path: &str, body: Option<&str>) -> Result<HttpResponse, String> {
        let raw = self.raw(&http_request(method, path, body)).map_err(|err| format!("{method} {path}: {err}"))?;
        parse_http_response(&raw).map_err(|err| format!("{method} {path}: {err}"))
    }

    pub(super) fn expect(&mut self, method: &str, path: &str, body: Option<&str>, want: u16) -> Result<String, String> {
        let response = self.call(method, path, body)?;
        if response.status != want {
            return Err(format!("{method} {path}: status {} (want {want}): {}", response.status, response.body));
        }
        Ok(response.body)
    }

    /// Reads until the server closes; returns the bytes it sent before closing.
    fn read_to_close(&mut self, within: Duration) -> Result<Vec<u8>, String> {
        self.reader.get_ref().set_read_timeout(Some(within)).map_err(|e| e.to_string())?;
        let mut rest = Vec::new();
        match self.reader.read_to_end(&mut rest) {
            Ok(_) => Ok(rest),
            Err(err) if matches!(err.kind(), std::io::ErrorKind::ConnectionReset) => Ok(rest),
            Err(err) => Err(format!("connection still open or failed: {err}")),
        }
    }
}

pub(super) fn state_of(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["state"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("unparsed {body:?}"))
}

/// Runs the slice 4 checklist; microVM ids are derived from `id`. `expected_version` is the pinned swagger's.
pub fn run_vesperd_suite(endpoint: &ControlEndpoint, run: &VesperdRun, id: &str, expected_version: &str) -> SuiteReport {
    let mut steps: Vec<SuiteStep> = Vec::new();
    let ids = [id.to_string(), format!("{id}-kill"), format!("{id}-fcexit")];
    let [main, killed, fcexit] = &ids;
    let control = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut control) = control else {
        return report(endpoint, steps, false);
    };
    let prepared = std::fs::create_dir_all(&run.workdir)
        .and_then(|_| std::fs::set_permissions(&run.workdir, std::fs::Permissions::from_mode(0o700)));
    if let Err(err) = prepared {
        steps.push(SuiteStep { name: "workdir".into(), ok: false, detail: err.to_string(), elapsed_ms: 0 });
        return report(endpoint, steps, false);
    }
    // Leftovers of an earlier run: their vesperd processes are gone, so the reaper removes their microVMs.
    for each in &ids {
        let _ = control.vesperd(&["test-age", each, "3600"]);
    }
    let _ = control.vesperd(&["reap"]);

    let started = step(&mut steps, "vesperd_starts", || {
        let (v, line, took) = run.start(main, "main.sock")?;
        let mode = std::fs::metadata(&v.socket).map_err(|e| e.to_string())?.permissions().mode() & 0o777;
        if mode != 0o600 {
            return Err(format!("socket mode {mode:o}"));
        }
        if !line.contains(&format!("Firecracker {expected_version} ")) {
            return Err(format!("expected the pinned Firecracker {expected_version}: {line:?}"));
        }
        let (_, status) = control.vesperd(&["status", main])?;
        Ok((v, format!("{line} (mode 600) after {} ms; guest: {}", took.as_millis(), first(&status))))
    });
    if let Some(mut v) = started {
        step(&mut steps, "second_vesperd_on_socket_refused", || {
            let mut second = run.spawn(&format!("{main}-second"), "main.sock")?;
            let (code, err, _) = wait_exit(&mut second, Duration::from_secs(15))?;
            if code == Some(0) || !err.contains("another vesperd holds") {
                return Err(format!("exit {code:?}: {err}"));
            }
            let (_, status) = control.vesperd(&["status", &format!("{main}-second")])?;
            if !first(&status).starts_with("ABSENT") {
                return Err(format!("the refused vesperd launched a microVM: {}", first(&status)));
            }
            Ok(((), format!("exit {code:?}: {err}; no microVM launched")))
        });
        step(&mut steps, "get_root_not_started", || {
            let body = Client::connect(&v.socket)?.expect("GET", "/", None, 200)?;
            if state_of(&body) != "Not started" || !body.contains(&format!("\"id\":\"{main}\"")) {
                return Err(format!("GET /: {body}"));
            }
            Ok(((), format!("GET / -> 200 {body}")))
        });
        let booted = step(&mut steps, "boot_through_vesperd", || {
            let mut api = Client::connect(&v.socket)?;
            api.expect("PUT", "/machine-config", Some(r#"{"vcpu_count":1,"mem_size_mib":256}"#), 204)?;
            let boot = format!(r#"{{"kernel_image_path":"/Image","initrd_path":"/initrd.gz","boot_args":"{BOOT_ARGS}"}}"#);
            api.expect("PUT", "/boot-source", Some(&boot), 204)?;
            let t = Instant::now();
            api.expect("PUT", "/actions", Some(r#"{"action_type":"InstanceStart"}"#), 204)?;
            Ok(((), format!("PUT /machine-config (1 vCPU, 256 MiB), PUT /boot-source, PUT /actions InstanceStart -> 204 on one connection; InstanceStart took {} ms", t.elapsed().as_millis())))
        });
        if booted.is_some() {
            step(&mut steps, "get_root_running", || {
                let body = Client::connect(&v.socket)?.expect("GET", "/", None, 200)?;
                if state_of(&body) != "Running" {
                    return Err(format!("GET /: {body}"));
                }
                Ok(((), format!("GET / -> 200 {body}")))
            });
            step(&mut steps, "guest_boots", || {
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    let (_, lines) = control.vesperd(&["log", main, "200"])?;
                    if lines.iter().any(|l| l.contains("VESPER-MICROVM-OK")) {
                        return Ok(((), "the guest init printed VESPER-MICROVM-OK on the serial console".to_string()));
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("no boot marker within 20 s: {}", lines.iter().rev().take(6).cloned().collect::<Vec<_>>().join(" | ")));
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            });
            step(&mut steps, "firecracker_error_byte_for_byte", || {
                let request = http_request("PUT", "/machine-config", Some(r#"{"vcpu_count":2,"mem_size_mib":256}"#));
                let through = Client::connect(&v.socket)?.raw(&request)?;
                let mut direct = connect(endpoint)?;
                direct.fcapi(main)?;
                let direct = direct.http_raw(&request)?;
                let parsed = parse_http_response(&through)?;
                if parsed.status != 400 || !parsed.body.contains("fault_message") {
                    return Err(format!("expected a Firecracker 400: {}", String::from_utf8_lossy(&through)));
                }
                if through != direct {
                    return Err(format!("vesperd {:?} != FCAPI {:?}", String::from_utf8_lossy(&through), String::from_utf8_lossy(&direct)));
                }
                Ok(((), format!("{} bytes identical through vesperd and FCAPI: {}", through.len(), String::from_utf8_lossy(&through).replace("\r\n", "\\r\\n"))))
            });
            step(&mut steps, "pause", || {
                let mut api = Client::connect(&v.socket)?;
                api.expect("PATCH", "/vm", Some(r#"{"state":"Paused"}"#), 204)?;
                let body = api.expect("GET", "/", None, 200)?;
                if state_of(&body) != "Paused" {
                    return Err(format!("GET / after Paused: {body}"));
                }
                Ok(((), format!("PATCH /vm Paused -> 204; GET / -> {body}")))
            });
            step(&mut steps, "snapshot_create_under_ceiling", || {
                let mut api = Client::connect(&v.socket)?;
                let t = Instant::now();
                api.expect("PUT", "/snapshot/create", Some(r#"{"snapshot_type":"Full","snapshot_path":"/snap.vmstate","mem_file_path":"/snap.mem"}"#), 204)?;
                let took = t.elapsed();
                if took * 10 > VESPERD_REQUEST_CEILING {
                    return Err(format!("PUT /snapshot/create took {} ms, over a tenth of the {} s ceiling", took.as_millis(), VESPERD_REQUEST_CEILING.as_secs()));
                }
                Ok(((), format!("PUT /snapshot/create (Full, 256 MiB guest, jail on the data disk) -> 204 in {} ms; ceiling {} s", took.as_millis(), VESPERD_REQUEST_CEILING.as_secs())))
            });
            step(&mut steps, "resume", || {
                let mut api = Client::connect(&v.socket)?;
                api.expect("PATCH", "/vm", Some(r#"{"state":"Resumed"}"#), 204)?;
                let body = api.expect("GET", "/", None, 200)?;
                if state_of(&body) != "Running" {
                    return Err(format!("GET / after Resumed: {body}"));
                }
                Ok(((), format!("PATCH /vm Resumed -> 204; GET / -> {body}")))
            });
            step(&mut steps, "firecracker_connection_limit_passes_through", || {
                let mut held = Vec::new();
                for _ in 0..FIRECRACKER_MAX_CONNECTIONS {
                    let mut c = Client::connect(&v.socket)?;
                    c.expect("GET", "/", None, 200)?;
                    held.push(c);
                }
                let mut extra = Client::connect(&v.socket)?;
                extra.writer.write_all(&http_request("GET", "/", None)).map_err(|e| e.to_string())?;
                let got = extra.read_to_close(Duration::from_secs(10))?;
                if got != FIRECRACKER_SERVER_FULL {
                    return Err(format!("connection {} got {:?}", FIRECRACKER_MAX_CONNECTIONS + 1, String::from_utf8_lossy(&got)));
                }
                drop(held);
                thread::sleep(Duration::from_millis(500));
                Client::connect(&v.socket)?.expect("GET", "/", None, 200)?;
                Ok(((), format!("{FIRECRACKER_MAX_CONNECTIONS} connections open; the next got Firecracker's own 503 byte for byte; after closing them a new one works")))
            });
            step(&mut steps, "idle_timeout_closes_only_vesperd_connections", || {
                let mut relayed = Client::connect(&v.socket)?;
                relayed.expect("GET", "/", None, 200)?;
                let mut direct = connect(endpoint)?;
                direct.fcapi(main)?;
                direct.http("GET", "/", None)?;
                let wait = VESPERD_IDLE_TIMEOUT + Duration::from_secs(5);
                let t = Instant::now();
                let rest = relayed.read_to_close(wait + Duration::from_secs(10))?;
                let closed_after = t.elapsed();
                if !rest.is_empty() || closed_after + Duration::from_secs(2) < VESPERD_IDLE_TIMEOUT {
                    return Err(format!("closed after {} ms with {} bytes", closed_after.as_millis(), rest.len()));
                }
                thread::sleep(wait.saturating_sub(t.elapsed()));
                let again = direct.http("GET", "/", None)?;
                if again.status != 200 {
                    return Err(format!("Firecracker's own connection after {} s: {}", wait.as_secs(), again.status));
                }
                Ok(((), format!("vesperd closed an idle connection after {} ms with no bytes; a direct FCAPI connection idle for {} s still answered GET / 200", closed_after.as_millis(), t.elapsed().as_secs())))
            });
        }
        step(&mut steps, "same_id_refused", || {
            let mut other = run.spawn(main, "other.sock")?;
            let (code, err, took) = wait_exit(&mut other, REUSE_WAIT + Duration::from_secs(20))?;
            if code == Some(0) || !err.contains("busy") {
                return Err(format!("exit {code:?}: {err}"));
            }
            let body = Client::connect(&v.socket)?.expect("GET", "/", None, 200)?;
            Ok(((), format!("refused after {} ms: {err}; the first still answers GET / ({})", took.as_millis(), state_of(&body))))
        });
        step(&mut steps, "sigterm_stops_microvm", || {
            signal(&v, "TERM")?;
            let (code, err, took) = wait_exit(&mut v, TEARDOWN_TIMEOUT)?;
            if code != Some(0) {
                return Err(format!("exit {code:?}: {err}"));
            }
            if v.socket.exists() {
                return Err("socket left behind".to_string());
            }
            let gone = wait_absent(&mut control, main, Duration::from_secs(5))?;
            Ok(((), format!("exit 0 after {} ms; socket removed; ABSENT {main} {} ms later (jail, cgroup, record and process gone)", took.as_millis(), gone.as_millis())))
        });
    }

    step(&mut steps, "sigkill_removes_microvm", || {
        let (mut v, _, _) = run.start(killed, "kill.sock")?;
        v.child.kill().map_err(|e| e.to_string())?;
        let _ = v.child.wait();
        let gone = wait_absent(&mut control, killed, TEARDOWN_TIMEOUT)?;
        // SIGKILL leaves the socket file; nothing serves it, and the next vesperd on the path would replace it.
        let stale = v.socket.exists();
        let _ = std::fs::remove_file(&v.socket);
        Ok(((), format!("SIGKILL to vesperd; ABSENT {killed} after {} ms; stale socket file left: {stale} (removed)", gone.as_millis())))
    });

    step(&mut steps, "firecracker_killed_vesperd_exits", || {
        let (mut v, _, _) = run.start(fcexit, "fcexit.sock")?;
        let (rc, lines) = control.vesperd(&["kill-fc", fcexit])?;
        if rc != 0 || !first(&lines).starts_with("KILLED-FC") {
            return Err(format!("kill-fc: rc {rc}: {}", lines.join(" | ")));
        }
        let (code, err, took) = wait_exit(&mut v, TEARDOWN_TIMEOUT)?;
        if code == Some(0) || !err.contains("lease of microVM") {
            return Err(format!("exit {code:?}: {err}"));
        }
        if v.socket.exists() {
            return Err("socket left behind".to_string());
        }
        let gone = wait_absent(&mut control, fcexit, TEARDOWN_TIMEOUT)?;
        Ok(((), format!("{}; vesperd exited {code:?} after {} ms: {err}; ABSENT {} ms later", first(&lines), took.as_millis(), gone.as_millis())))
    });

    step(&mut steps, "nothing_left", || {
        let sockets: Vec<String> = std::fs::read_dir(&run.workdir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| std::os::unix::fs::FileTypeExt::is_socket(&t)).unwrap_or(false))
            .map(|e| e.path().display().to_string())
            .collect();
        if !sockets.is_empty() {
            return Err(format!("sockets left in {}: {sockets:?}", run.workdir.display()));
        }
        let (_, lines) = control.vesperd(&["list"])?;
        let ours: Vec<&String> = lines.iter().filter(|l| ids.iter().any(|i| l.split_whitespace().nth(1) == Some(i.as_str()))).collect();
        if !ours.is_empty() {
            return Err(format!("leftovers: {ours:?}"));
        }
        Ok(((), format!("no lease or jail for {}; no socket in {}", ids.join(", "), run.workdir.display())))
    });

    let passed = steps.iter().all(|s| s.ok) && steps.len() == 18;
    report(endpoint, steps, passed)
}
