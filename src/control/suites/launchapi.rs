//! ADR 0007 slice 3 checklist: `LAUNCHAPI` starts one jailed Firecracker built from API requests, `FCAPI` relays
//! to its API socket, the microVM boots through that socket, and the three teardown paths (lease closed,
//! Firecracker killed, lease shell killed) leave nothing behind. The reaper ignores a live holder whose pid
//! was reused and spares a new jail without a record. A default Docker container cannot open `AF_VSOCK`.

use super::{connect, report, step, ControlEndpoint, Session, SuiteReport, SuiteStep};
use std::thread;
use std::time::{Duration, Instant};

/// How long `LAUNCHAPI` may take: the first run builds the microVM kernel and initramfs (`vesper-microvm prepare`).
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(120);
/// Teardown after a lease ends: SIGTERM, 3 s grace, SIGKILL, removal; well under this.
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(20);
/// The reaper runs every 10 s; a SIGKILLed lease shell's microVM must be gone within two intervals plus teardown.
const REAPER_TIMEOUT: Duration = Duration::from_secs(35);

const BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off rdinit=/init";

fn lease(endpoint: &ControlEndpoint, id: &str) -> Result<(Session, super::super::Ready), String> {
    let mut session = connect(endpoint)?;
    let ready = session.launch_api(id, None, LAUNCH_TIMEOUT)?;
    Ok((session, ready))
}

pub(super) fn first(lines: &[String]) -> String {
    lines.first().cloned().unwrap_or_default()
}

/// Polls `VESPERD status <id>` until it reports `ABSENT`; returns the elapsed time.
pub(super) fn wait_absent(session: &mut Session, id: &str, timeout: Duration) -> Result<Duration, String> {
    let started = Instant::now();
    loop {
        let (_, lines) = session.vesperd(&["status", id])?;
        let line = first(&lines);
        if line == format!("ABSENT {id}") {
            return Ok(started.elapsed());
        }
        if started.elapsed() >= timeout {
            return Err(format!("{id} not clean after {} s: {line}", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn api_exchange(session: &mut Session, method: &str, path: &str, body: Option<&str>, want: u16) -> Result<String, String> {
    let response = session.http(method, path, body)?;
    if response.status != want {
        return Err(format!("{method} {path}: status {} (want {want}): {}", response.status, response.body));
    }
    Ok(response.body)
}

/// Runs the slice 3 checklist with microVM ids derived from `id`; `expected_version` is the pinned swagger's
/// `info.version`, which `READY` must report.
pub fn run_launchapi_suite(endpoint: &ControlEndpoint, id: &str, expected_version: &str) -> SuiteReport {
    let mut steps: Vec<SuiteStep> = Vec::new();
    let ids = [
        id.to_string(),
        format!("{id}-fcexit"),
        format!("{id}-reap"),
        format!("{id}-reuse"),
        format!("{id}-orphan"),
    ];
    let [main, fcexit, reaped, reused, orphan] = &ids;
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut control) = session else {
        return report(endpoint, steps, false);
    };
    // Leftovers of an earlier run: age any jail past the grace period and reap; holders from that run are gone.
    for each in &ids {
        let _ = control.vesperd(&["test-age", each, "3600"]);
    }
    let _ = control.vesperd(&["reap"]);
    let _ = control.microvm(&["cleanup", main]);

    let main_lease = step(&mut steps, "launch", || {
        let (session, ready) = lease(endpoint, main)?;
        if ready.firecracker_version != expected_version {
            return Err(format!("READY reports Firecracker {}, the pinned swagger is {expected_version}", ready.firecracker_version));
        }
        Ok((session, format!("READY {main} pid={} firecracker_version={}", ready.pid, ready.firecracker_version)))
    });
    if let Some(first_lease) = main_lease {
        let mut held = Some(first_lease);
        step(&mut steps, "duplicate_id_refused", || {
            let mut second = connect(endpoint)?;
            match second.launch_api(main, None, LAUNCH_TIMEOUT) {
                Ok(ready) => Err(format!("a second LAUNCHAPI {main} succeeded: pid {}", ready.pid)),
                Err(err) if err.contains("busy") => Ok(((), err)),
                Err(err) => Err(format!("expected ERR busy, got {err}")),
            }
        });
        step(&mut steps, "fcapi_get_not_started", || {
            let mut api = connect(endpoint)?;
            api.fcapi(main)?;
            let body = api_exchange(&mut api, "GET", "/", None, 200)?;
            if !body.contains("\"state\":\"Not started\"") || !body.contains(&format!("\"id\":\"{main}\"")) {
                return Err(format!("GET / before boot: {body}"));
            }
            Ok(((), format!("GET / -> 200 {body}")))
        });
        let booted = step(&mut steps, "boot_through_api", || {
            let mut api = connect(endpoint)?;
            api.fcapi(main)?;
            api_exchange(&mut api, "PUT", "/machine-config", Some(r#"{"vcpu_count":1,"mem_size_mib":256}"#), 204)?;
            let boot = format!(r#"{{"kernel_image_path":"/Image","initrd_path":"/initrd.gz","boot_args":"{BOOT_ARGS}"}}"#);
            api_exchange(&mut api, "PUT", "/boot-source", Some(&boot), 204)?;
            api_exchange(&mut api, "PUT", "/actions", Some(r#"{"action_type":"InstanceStart"}"#), 204)?;
            Ok(((), "PUT /machine-config, PUT /boot-source, PUT /actions InstanceStart -> 204 on one keep-alive connection".to_string()))
        });
        if booted.is_some() {
            step(&mut steps, "get_root_running", || {
                let mut api = connect(endpoint)?;
                api.fcapi(main)?;
                let body = api_exchange(&mut api, "GET", "/", None, 200)?;
                if !body.contains("\"state\":\"Running\"") {
                    return Err(format!("GET / after InstanceStart: {body}"));
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
        }
        // Firecracker's VMM thread installs its seccomp filter at InstanceStart, so isolation is judged after boot.
        step(&mut steps, "jailer_isolation", || {
            let (rc, lines) = control.vesperd(&["isolation", main])?;
            let json: serde_json::Value = serde_json::from_str(&first(&lines)).map_err(|e| format!("isolation JSON (rc {rc}): {e}: {}", lines.join(" | ")))?;
            let failed: Vec<&str> = ["uid_ok", "chroot_ok", "cgroup_ok", "seccomp_ok"].into_iter().filter(|k| json[k] != serde_json::Value::Bool(true)).collect();
            if !failed.is_empty() || json["uid"] != 20000 || json["cgroup"] != format!("/vesperd/{main}") {
                return Err(format!("isolation checks failed {failed:?}: {}", lines[0]));
            }
            Ok(((), format!("uid {}, cgroup {}, memory.max {}, seccomp mode {}, jail {}", json["uid"], json["cgroup"], json["memory_max"], json["seccomp_mode"], json["jail"])))
        });
        step(&mut steps, "fcapi_refuses_bad_and_unknown_ids", || {
            let mut bad = connect(endpoint)?;
            let bad_err = bad.fcapi("../x").err().ok_or("FCAPI ../x was accepted")?;
            bad.wait_closed(Duration::from_secs(5))?;
            let mut unknown = connect(endpoint)?;
            let unknown_err = unknown.fcapi(&format!("{main}-none")).err().ok_or("FCAPI of an unknown id was accepted")?;
            if !bad_err.contains("bad id") || !unknown_err.contains("no vesperd microVM") {
                return Err(format!("unexpected refusals: {bad_err}; {unknown_err}"));
            }
            Ok(((), format!("{bad_err}; {unknown_err}")))
        });
        step(&mut steps, "fcparity_refuses_bad_id", || {
            let (rc, lines) = control.fcparity(&["api", "../../x", "GET", "/"])?;
            if rc == 0 || !lines.iter().any(|l| l == "ERR bad id") {
                return Err(format!("FCPARITY api ../../x: rc {rc}: {}", lines.join(" | ")));
            }
            Ok(((), format!("rc {rc}: {}", lines.join(" | "))))
        });
        // The shim's namespace: the same id as a uid 10000 microVM; the uid filter keeps the two apart.
        step(&mut steps, "shim_namespace_separate", || {
            let (rc, lines) = control.microvm(&["launch", main, "--exit"])?;
            let shim = first(&lines);
            if rc != 0 || !shim.starts_with("LAUNCHED") {
                return Err(format!("shim launch of {main}: rc {rc}: {}", lines.join(" | ")));
            }
            let (_, cleaned) = control.microvm(&["cleanup", main])?;
            let (_, status) = control.vesperd(&["status", main])?;
            if first(&cleaned) != format!("CLEAN {main}") || !first(&status).starts_with("RUNNING") {
                return Err(format!("after the shim's cleanup of {main}: {} / vesperd {}", first(&cleaned), first(&status)));
            }
            Ok(((), format!("shim {shim}; shim cleanup {}; vesperd microVM still {}", first(&cleaned), first(&status))))
        });
        step(&mut steps, "teardown_lease_closed", || {
            drop(held.take());
            let took = wait_absent(&mut control, main, TEARDOWN_TIMEOUT)?;
            Ok(((), format!("lease closed; jail, cgroup, record and process gone in {} ms", took.as_millis())))
        });
        step(&mut steps, "reused_id_after_teardown", || {
            let (first_lease, _) = lease(endpoint, main)?;
            drop(first_lease);
            let started = Instant::now();
            let mut busy = 0;
            let again = loop {
                let mut session = connect(endpoint)?;
                match session.launch_api(main, None, LAUNCH_TIMEOUT) {
                    Ok(ready) => break (session, ready),
                    Err(err) if err.contains("busy") && started.elapsed() < TEARDOWN_TIMEOUT => {
                        busy += 1;
                        thread::sleep(Duration::from_millis(500));
                    }
                    Err(err) => return Err(format!("relaunch of {main}: {err}")),
                }
            };
            held = Some(again.0);
            Ok(((), format!("relaunched {main} pid={} {} ms after the lease closed ({busy} ERR busy while the old jail was removed)", again.1.pid, started.elapsed().as_millis())))
        });
        step(&mut steps, "cleanup_reused_id", || {
            drop(held.take());
            let took = wait_absent(&mut control, main, TEARDOWN_TIMEOUT)?;
            Ok(((), format!("ABSENT {main} after {} ms", took.as_millis())))
        });
    }

    step(&mut steps, "teardown_firecracker_killed", || {
        let (mut held, ready) = lease(endpoint, fcexit)?;
        let (rc, lines) = control.vesperd(&["kill-fc", fcexit])?;
        if rc != 0 || !first(&lines).starts_with("KILLED-FC") {
            return Err(format!("kill-fc: rc {rc}: {}", lines.join(" | ")));
        }
        let closed = held.wait_closed(TEARDOWN_TIMEOUT)?;
        let took = wait_absent(&mut control, fcexit, TEARDOWN_TIMEOUT)?;
        Ok(((), format!("SIGKILL to pid {}; the agent closed the lease after {} ms; clean {} ms later", ready.pid, closed.as_millis(), took.as_millis())))
    });

    step(&mut steps, "teardown_lease_shell_killed_reaped", || {
        let (mut held, ready) = lease(endpoint, reaped)?;
        let (rc, lines) = control.vesperd(&["kill-holder", reaped])?;
        if rc != 0 || !first(&lines).starts_with("KILLED-HOLDER") {
            return Err(format!("kill-holder: rc {rc}: {}", lines.join(" | ")));
        }
        // No manual reap: the periodic reaper must find the dead holder, stop Firecracker and close the lease.
        let closed = held.wait_closed(REAPER_TIMEOUT)?;
        let took = wait_absent(&mut control, reaped, TEARDOWN_TIMEOUT)?;
        Ok(((), format!("{}; Firecracker pid {} outlived it until the periodic reaper closed the lease after {} ms; clean {} ms later", first(&lines), ready.pid, closed.as_millis(), took.as_millis())))
    });

    step(&mut steps, "reaper_ignores_reused_pid", || {
        let (mut held, _) = lease(endpoint, reused)?;
        let (_, pass) = control.vesperd(&["reap"])?;
        if !pass.iter().any(|l| l.starts_with(&format!("KEPT {reused} reason=holder-alive"))) {
            return Err(format!("a live holder was not kept: {}", pass.join(" | ")));
        }
        let (rc, forged) = control.vesperd(&["test-forge-start", reused])?;
        if rc != 0 {
            return Err(format!("test-forge-start: {}", forged.join(" | ")));
        }
        let (_, pass) = control.vesperd(&["reap"])?;
        let by = if pass.iter().any(|l| l.starts_with(&format!("REAPED {reused} reason=holder-gone"))) { "this pass" } else { "the periodic reaper" };
        let took = wait_absent(&mut control, reused, TEARDOWN_TIMEOUT)?;
        held.wait_closed(TEARDOWN_TIMEOUT)?;
        Ok(((), format!("{}; the holder pid still runs but with another start time; reaped by {by}, clean after {} ms, lease closed", first(&forged), took.as_millis())))
    });

    step(&mut steps, "reaper_spares_new_unrecorded_jail", || {
        let (rc, lines) = control.vesperd(&["test-orphan", orphan])?;
        if rc != 0 {
            return Err(format!("test-orphan: {}", lines.join(" | ")));
        }
        let (_, pass) = control.vesperd(&["reap"])?;
        let kept = pass.iter().find(|l| l.starts_with(&format!("KEPT {orphan} reason=grace"))).cloned().ok_or_else(|| format!("a new jail was not spared: {}", pass.join(" | ")))?;
        control.vesperd(&["test-age", orphan, "120"])?;
        let (_, pass) = control.vesperd(&["reap"])?;
        let by = pass.iter().find(|l| l.starts_with(&format!("REAPED {orphan} reason=no-record"))).cloned().unwrap_or_else(|| "by the periodic reaper".to_string());
        wait_absent(&mut control, orphan, TEARDOWN_TIMEOUT)?;
        Ok(((), format!("{kept}; after 120 s: {by}")))
    });

    step(&mut steps, "default_container_cannot_open_vsock", || {
        let (rc, lines) = control.vesperd(&["test-container-vsock"])?;
        let default: Vec<&String> = lines.iter().filter(|l| l.starts_with("VSOCK default=")).collect();
        if rc != 0 || default.len() != 2 || default.iter().any(|l| !l.starts_with("VSOCK default=denied")) {
            return Err(format!("rc {rc}: {}", lines.join(" | ")));
        }
        Ok(((), lines.join(" | ")))
    });

    step(&mut steps, "nothing_left", || {
        let (_, lines) = control.vesperd(&["list"])?;
        let ours: Vec<&String> = lines.iter().filter(|l| ids.iter().any(|i| l.split_whitespace().nth(1) == Some(i.as_str()))).collect();
        if !ours.is_empty() {
            return Err(format!("leftovers: {ours:?}"));
        }
        Ok(((), format!("no lease or jail for {}", ids.join(", "))))
    });

    let passed = steps.iter().all(|s| s.ok) && steps.len() == 20;
    report(endpoint, steps, passed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    /// Guest state for the fake agent: live leases (id -> lease connection still open) and the booted ids.
    #[derive(Default)]
    struct Guest {
        leases: Vec<String>,
        running: Vec<String>,
    }

    fn tool_reply(verb: &str, lines: &[String]) -> String {
        let mut out = format!("{verb} 0 {}\n", lines.len());
        for l in lines {
            out.push_str(l);
            out.push('\n');
        }
        out
    }

    /// A bridge plus an agent that models LAUNCHAPI, FCAPI and the VESPERD, MICROVM and FCPARITY replies the
    /// suite reads, so the checklist's flow is tested without a System VM.
    fn fake(path: &Path) {
        let listener = UnixListener::bind(path).unwrap();
        let guest = Arc::new(Mutex::new(Guest::default()));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let guest = guest.clone();
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut writer = stream;
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    writer.write_all(b"VESPER-AGENT 1 vesper\n").unwrap();
                    loop {
                        line.clear();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
                        let arg = |i: usize| words.get(i).cloned().unwrap_or_default();
                        match words.first().map(String::as_str) {
                            Some("LAUNCHAPI") => {
                                let id = arg(1);
                                if guest.lock().unwrap().leases.contains(&id) {
                                    writer.write_all(format!("ERR busy {id}\n").as_bytes()).unwrap();
                                    return;
                                }
                                guest.lock().unwrap().leases.push(id.clone());
                                writer.write_all(format!("READY {id} pid=4242 firecracker_version=1.16.0\n").as_bytes()).unwrap();
                                // The lease ends on the client's close, or when a test hook "kills" the microVM.
                                writer.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
                                let mut sink = [0u8; 64];
                                loop {
                                    match reader.read(&mut sink) {
                                        Ok(0) => break,
                                        Ok(_) => {}
                                        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                                        Err(_) => break,
                                    }
                                    if !guest.lock().unwrap().leases.contains(&id) {
                                        break;
                                    }
                                }
                                let mut g = guest.lock().unwrap();
                                g.leases.retain(|l| *l != id);
                                g.running.retain(|l| *l != id);
                                return;
                            }
                            Some("FCAPI") => {
                                let id = arg(1);
                                if id.contains('/') {
                                    writer.write_all(b"ERR bad id\n").unwrap();
                                    return;
                                }
                                if !guest.lock().unwrap().leases.contains(&id) {
                                    writer.write_all(format!("ERR no vesperd microVM {id}\n").as_bytes()).unwrap();
                                    return;
                                }
                                writer.write_all(b"OK\n").unwrap();
                                loop {
                                    let mut head = String::new();
                                    if reader.read_line(&mut head).unwrap_or(0) == 0 {
                                        return;
                                    }
                                    let mut length = 0;
                                    loop {
                                        let mut h = String::new();
                                        reader.read_line(&mut h).unwrap();
                                        if h == "\r\n" {
                                            break;
                                        }
                                        if let Some(v) = h.strip_prefix("Content-Length: ") {
                                            length = v.trim().parse().unwrap();
                                        }
                                    }
                                    let mut body = vec![0u8; length];
                                    reader.read_exact(&mut body).unwrap();
                                    let reply = if head.starts_with("GET / ") {
                                        let state = if guest.lock().unwrap().running.contains(&id) { "Running" } else { "Not started" };
                                        let body = format!("{{\"id\":\"{id}\",\"state\":\"{state}\",\"vmm_version\":\"1.16.0\",\"app_name\":\"Firecracker\"}}");
                                        format!("HTTP/1.1 200 \r\nContent-Length: {}\r\n\r\n{body}", body.len())
                                    } else {
                                        if head.starts_with("PUT /actions") {
                                            guest.lock().unwrap().running.push(id.clone());
                                        }
                                        "HTTP/1.1 204 \r\n\r\n".to_string()
                                    };
                                    writer.write_all(reply.as_bytes()).unwrap();
                                }
                            }
                            Some("VESPERD") => {
                                let (sub, id) = (arg(1), arg(2));
                                let mut g = guest.lock().unwrap();
                                let lines: Vec<String> = match sub.as_str() {
                                    "status" if g.leases.contains(&id) => vec![format!("RUNNING {id} pid=4242 holder=99")],
                                    "status" => vec![format!("ABSENT {id}")],
                                    "isolation" => vec![format!("{{\"id\":\"{id}\",\"pid\":4242,\"uid\":20000,\"jail\":\"/srv/vesperd/firecracker/{id}/root\",\"cgroup\":\"/vesperd/{id}\",\"memory_max\":\"671088640\",\"seccomp_mode\":2,\"uid_ok\":true,\"chroot_ok\":true,\"cgroup_ok\":true,\"seccomp_ok\":true}}")],
                                    "log" => vec!["VESPER-MICROVM-OK".to_string()],
                                    "kill-fc" | "kill-holder" | "test-forge-start" => {
                                        g.leases.retain(|l| *l != id);
                                        vec![format!("{} {id} pid=4242", if sub == "kill-fc" { "KILLED-FC" } else if sub == "kill-holder" { "KILLED-HOLDER" } else { "FORGED" })]
                                    }
                                    "reap" => {
                                        let mut out: Vec<String> = g.leases.iter().map(|l| format!("KEPT {l} reason=holder-alive holder=99")).collect();
                                        out.push("KEPT vesper-launchapi-orphan reason=grace age=0".to_string());
                                        out.push("REAP reaped=0 kept=1".to_string());
                                        out
                                    }
                                    "test-orphan" => vec![format!("ORPHAN {id}")],
                                    "test-age" => vec![format!("AGED {id} seconds=120")],
                                    "test-container-vsock" => vec![
                                        "VSOCK default=denied cid=1 detail=socket(40, 1, 0): Operation not permitted".to_string(),
                                        "VSOCK default=denied cid=3 detail=socket(40, 1, 0): Operation not permitted".to_string(),
                                    ],
                                    _ => vec![],
                                };
                                writer.write_all(tool_reply("VESPERD", &lines).as_bytes()).unwrap();
                            }
                            Some("MICROVM") => {
                                let (sub, id) = (arg(1), arg(2));
                                let lines = match sub.as_str() {
                                    "launch" => vec![format!("LAUNCHED {id} pid=555 boot_ms=900")],
                                    "cleanup" => vec![format!("CLEAN {id}")],
                                    _ => vec![],
                                };
                                writer.write_all(tool_reply("MICROVM", &lines).as_bytes()).unwrap();
                            }
                            Some("FCPARITY") => {
                                writer.write_all(b"FCPARITY 2 2\nSTATUS 000\nERR bad id\n").unwrap();
                            }
                            _ => writer.write_all(b"ERR unknown command\n").unwrap(),
                        }
                    }
                });
            }
        });
    }

    #[test]
    fn suite_covers_launch_boot_teardowns_and_the_reaper() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake(&path);
        let endpoint = ControlEndpoint::new(&path, 1024);
        let result = run_launchapi_suite(&endpoint, "vesper-launchapi", "1.16.0");
        let names: Vec<&str> = result.steps.iter().map(|s| s.name.as_str()).collect();
        for n in [
            "launch",
            "boot_through_api",
            "get_root_running",
            "teardown_lease_closed",
            "teardown_firecracker_killed",
            "teardown_lease_shell_killed_reaped",
            "reaper_ignores_reused_pid",
            "reaper_spares_new_unrecorded_jail",
            "default_container_cannot_open_vsock",
        ] {
            assert!(names.contains(&n), "missing {n} in {names:?}");
        }
        assert!(result.passed, "{:#?}", result.steps);
        assert_eq!(result.steps.len(), 20);
    }

    #[test]
    fn suite_fails_on_a_version_other_than_the_pinned_swagger() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake(&path);
        let endpoint = ControlEndpoint::new(&path, 1024);
        let result = run_launchapi_suite(&endpoint, "vesper-launchapi", "9.9.9");
        let launch = result.steps.iter().find(|s| s.name == "launch").unwrap();
        assert!(!launch.ok && launch.detail.contains("9.9.9"), "{launch:?}");
        assert!(!result.passed);
    }
}
