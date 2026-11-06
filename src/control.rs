//! Host-side client for the System VM control channel (GH #13).
//!
//! The channel is a vsock port inside the guest, reached from macOS through the launcher's bridge:
//! a Unix socket that takes the Firecracker-style `CONNECT <port>\n` preface and then relays bytes
//! to the guest port (`docs/process/systemvm-bringup.md`, "Proxy bridge convention"). The guest
//! agent (`systemvm/overlay/usr/local/bin/vesper-agent`) speaks a line protocol, version 1.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

pub const AGENT_PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_AGENT_PORT: u32 = 1024;

/// Where the bridge socket lives and which guest port the agent listens on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEndpoint {
    pub bridge_path: PathBuf,
    pub port: u32,
    pub io_timeout: Duration,
}

impl ControlEndpoint {
    pub fn new(bridge_path: impl Into<PathBuf>, port: u32) -> Self {
        ControlEndpoint {
            bridge_path: bridge_path.into(),
            port,
            io_timeout: Duration::from_secs(10),
        }
    }
}

/// Greeting sent by the agent right after the bridge connects the guest port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handshake {
    pub protocol_version: u32,
    pub hostname: String,
    pub connect_ms: u64,
}

#[derive(Debug)]
pub struct Session {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    pub handshake: Handshake,
}

/// Connects through the bridge, sends the `CONNECT` preface and reads the greeting.
pub fn connect(endpoint: &ControlEndpoint) -> Result<Session, String> {
    let started = Instant::now();
    let stream = UnixStream::connect(&endpoint.bridge_path).map_err(|err| {
        format!(
            "connect bridge {}: {err} (is the VM up? `make systemvm-up`)",
            endpoint.bridge_path.display()
        )
    })?;
    stream
        .set_read_timeout(Some(endpoint.io_timeout))
        .and_then(|_| stream.set_write_timeout(Some(endpoint.io_timeout)))
        .map_err(|err| format!("set bridge timeouts: {err}"))?;
    let writer = stream.try_clone().map_err(|err| format!("clone bridge stream: {err}"))?;
    let mut session = Session {
        reader: BufReader::new(stream),
        writer,
        handshake: Handshake {
            protocol_version: 0,
            hostname: String::new(),
            connect_ms: 0,
        },
    };
    session
        .send(&format!("CONNECT {}", endpoint.port))
        .map_err(|err| format!("send CONNECT preface: {err}"))?;
    let greeting = session.read_line()?;
    let mut parts = greeting.split_whitespace();
    let (Some("VESPER-AGENT"), Some(version), Some(hostname)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(format!("unexpected greeting from the agent: {greeting:?}"));
    };
    let protocol_version: u32 = version
        .parse()
        .map_err(|_| format!("agent protocol version is not a number: {greeting:?}"))?;
    if protocol_version != AGENT_PROTOCOL_VERSION {
        return Err(format!(
            "agent speaks protocol {protocol_version}, this client speaks {AGENT_PROTOCOL_VERSION}"
        ));
    }
    session.handshake = Handshake {
        protocol_version,
        hostname: hostname.to_string(),
        connect_ms: started.elapsed().as_millis() as u64,
    };
    Ok(session)
}

/// `connect` with retries and a fixed backoff; returns the session and the attempts used.
pub fn connect_with_retry(
    endpoint: &ControlEndpoint,
    attempts: u32,
    backoff: Duration,
) -> Result<(Session, u32), String> {
    let mut last = String::new();
    for attempt in 1..=attempts.max(1) {
        match connect(endpoint) {
            Ok(session) => return Ok((session, attempt)),
            Err(err) => {
                last = err;
                if attempt < attempts {
                    thread::sleep(backoff);
                }
            }
        }
    }
    Err(format!("gave up after {attempts} attempts: {last}"))
}

impl Session {
    fn send(&mut self, line: &str) -> Result<(), String> {
        self.writer
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|_| self.writer.flush())
            .map_err(|err| format!("write to agent: {err}"))
    }

    fn read_line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .map_err(|err| format!("read from agent: {err}"))?;
        if read == 0 {
            return Err("agent closed the connection".to_string());
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    fn expect(&mut self, command: &str, reply: &str) -> Result<Duration, String> {
        let started = Instant::now();
        self.send(command)?;
        let line = self.read_line()?;
        if line != reply {
            return Err(format!("{command}: expected {reply:?}, got {line:?}"));
        }
        Ok(started.elapsed())
    }

    /// Round trip of `PING`/`PONG`.
    pub fn ping(&mut self) -> Result<Duration, String> {
        self.expect("PING", "PONG")
    }

    pub fn version(&mut self) -> Result<u32, String> {
        self.send("VERSION")?;
        let line = self.read_line()?;
        line.strip_prefix("VERSION ")
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("VERSION: unexpected reply {line:?}"))
    }

    /// Sends `payload` through `ECHO` and checks the bytes come back unchanged. The payload is
    /// written from a second thread while this one reads, because the agent streams the bytes back
    /// as it receives them and a one-sided writer would deadlock once the socket buffers fill.
    pub fn echo(&mut self, payload: &[u8]) -> Result<Duration, String> {
        let started = Instant::now();
        self.send(&format!("ECHO {}", payload.len()))?;
        let line = self.read_line()?;
        if line != format!("ECHO {}", payload.len()) {
            return Err(format!("ECHO: unexpected reply {line:?}"));
        }
        let mut writer = self
            .writer
            .try_clone()
            .map_err(|err| format!("clone bridge stream: {err}"))?;
        let to_send = payload.to_vec();
        let sender = thread::spawn(move || {
            writer
                .write_all(&to_send)
                .and_then(|_| writer.flush())
                .map_err(|err| format!("write echo payload: {err}"))
        });
        let mut back = vec![0u8; payload.len()];
        let read = self
            .reader
            .read_exact(&mut back)
            .map_err(|err| format!("read echo payload: {err}"));
        sender.join().map_err(|_| "echo writer thread panicked".to_string())??;
        read?;
        if back != payload {
            let first = back.iter().zip(payload).position(|(a, b)| a != b).unwrap_or(0);
            return Err(format!("ECHO: payload corrupted at byte {first}"));
        }
        Ok(started.elapsed())
    }

    /// The agent's `HEALTH` JSON document.
    pub fn health(&mut self) -> Result<serde_json::Value, String> {
        self.send("HEALTH")?;
        let line = self.read_line()?;
        let json = line
            .strip_prefix("HEALTH ")
            .ok_or_else(|| format!("HEALTH: unexpected reply {line:?}"))?;
        serde_json::from_str(json).map_err(|err| format!("HEALTH: invalid JSON {json:?}: {err}"))
    }

    /// Sends SIGKILL to a supervised daemon in the guest (`containerd` or `docker`); returns its pid.
    pub fn kill_service(&mut self, service: &str) -> Result<u32, String> {
        self.send(&format!("KILL {service}"))?;
        let line = self.read_line()?;
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next(), parts.next()) {
            (Some("KILL"), Some(name), Some(pid)) if name == service => {
                pid.parse().map_err(|_| format!("KILL: bad pid in {line:?}"))
            }
            _ => Err(format!("KILL {service}: unexpected reply {line:?}")),
        }
    }

    /// Last `count` lines of a guest log (`messages`, `containerd` or `docker`).
    pub fn logs(&mut self, log: &str, count: u32) -> Result<Vec<String>, String> {
        self.send(&format!("LOGS {log} {count}"))?;
        let line = self.read_line()?;
        let n: usize = line
            .strip_prefix("LOGS ")
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("LOGS {log}: unexpected reply {line:?}"))?;
        (0..n).map(|_| self.read_line()).collect()
    }

    /// Runs `vesper-microvm <args>` in the guest; returns its exit code and output lines.
    pub fn microvm(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        // A launch waits for the guest's boot marker (up to 30 s), longer than the default I/O timeout.
        let stream = self.reader.get_ref();
        let previous = stream.read_timeout().map_err(|err| format!("read timeout: {err}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .map_err(|err| format!("set read timeout: {err}"))?;
        let result = self.microvm_inner(args);
        let _ = self.reader.get_ref().set_read_timeout(previous);
        result
    }

    fn microvm_inner(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.send(&format!("MICROVM {}", args.join(" ")))?;
        let line = self.read_line()?;
        let mut parts = line.split_whitespace();
        let (Some("MICROVM"), Some(rc), Some(count)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("MICROVM: unexpected reply {line:?}"));
        };
        let rc: i32 = rc.parse().map_err(|_| format!("MICROVM: bad rc in {line:?}"))?;
        let count: usize = count.parse().map_err(|_| format!("MICROVM: bad count in {line:?}"))?;
        let lines = (0..count).map(|_| self.read_line()).collect::<Result<Vec<_>, _>>()?;
        Ok((rc, lines))
    }

    /// Asks the agent to close this connection (server-side drop).
    pub fn drop_connection(&mut self) -> Result<(), String> {
        self.expect("DROP", "DROP").map(|_| ())
    }

    /// Asks the guest to power off.
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.expect("SHUTDOWN", "BYE").map(|_| ())
    }

    /// Pings every `interval` for `total`, proving the channel survives idle gaps.
    pub fn keepalive(&mut self, interval: Duration, total: Duration) -> Result<u32, String> {
        let deadline = Instant::now() + total;
        let mut pings = 0;
        loop {
            self.ping()?;
            pings += 1;
            if Instant::now() >= deadline {
                return Ok(pings);
            }
            thread::sleep(interval);
        }
    }
}

/// Result of one checklist step of the GH #13 control suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteStep {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteReport {
    pub bridge_path: String,
    pub port: u32,
    pub steps: Vec<SuiteStep>,
    pub passed: bool,
}

fn step<T>(
    steps: &mut Vec<SuiteStep>,
    name: &str,
    run: impl FnOnce() -> Result<(T, String), String>,
) -> Option<T> {
    let started = Instant::now();
    match run() {
        Ok((value, detail)) => {
            steps.push(SuiteStep {
                name: name.to_string(),
                ok: true,
                detail,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
            Some(value)
        }
        Err(err) => {
            steps.push(SuiteStep {
                name: name.to_string(),
                ok: false,
                detail: err,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
            None
        }
    }
}

/// Runs the GH #13 checklist: handshake, keepalive under idle, reconnect after a drop, >1 MB echo.
pub fn run_suite(
    endpoint: &ControlEndpoint,
    keepalive_interval: Duration,
    keepalive_total: Duration,
    payload_bytes: usize,
) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!(
            "protocol {} host {} in {} ms",
            session.handshake.protocol_version, session.handshake.hostname, session.handshake.connect_ms
        );
        Ok((session, detail))
    });
    let session = session.and_then(|mut session| {
        step(&mut steps, "keepalive", || {
            let pings = session.keepalive(keepalive_interval, keepalive_total)?;
            Ok((session, format!("{pings} pings over {} s", keepalive_total.as_secs())))
        })
    });
    let session = session.and_then(|mut session| {
        step(&mut steps, "reconnect", || {
            session.drop_connection()?;
            let dropped = session.ping().is_err();
            if !dropped {
                return Err("connection survived DROP".to_string());
            }
            let (fresh, attempts) = connect_with_retry(endpoint, 5, Duration::from_millis(500))?;
            Ok((fresh, format!("reconnected after a server-side drop in {attempts} attempt(s)")))
        })
    });
    session.and_then(|mut session| {
        step(&mut steps, "large_payload", || {
            let payload: Vec<u8> = (0..payload_bytes).map(|i| (i % 251) as u8).collect();
            let elapsed = session.echo(&payload)?;
            Ok(((), format!("{payload_bytes} bytes echoed intact in {} ms", elapsed.as_millis())))
        })
    });
    let passed = steps.len() == 4 && steps.iter().all(|s| s.ok);
    SuiteReport {
        bridge_path: endpoint.bridge_path.display().to_string(),
        port: endpoint.port,
        steps,
        passed,
    }
}

/// Runs the GH #14 checklist per service: healthy at boot, SIGKILL, supervised restart within
/// `recovery_timeout`, and log lines that record the startup and the recovery.
pub fn run_services_suite(
    endpoint: &ControlEndpoint,
    services: &[&str],
    recovery_timeout: Duration,
) -> SuiteReport {
    let mut steps = Vec::new();
    let healthy = |session: &mut Session, service: &str| -> Result<serde_json::Value, String> {
        let health = session.health()?;
        let ok = health.get(format!("{service}_ok")).and_then(|v| v.as_bool()) == Some(true);
        let state = health["services"][service].as_str().unwrap_or("?").to_string();
        if !ok || state != "started" {
            return Err(format!("{service} not healthy: state {state}, health {health}"));
        }
        Ok(health)
    };
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed: false };
    };
    for service in services {
        let service = *service;
        let before = step(&mut steps, &format!("{service}_healthy_at_boot"), || {
            let health = healthy(&mut session, service)?;
            Ok(((), format!("state started, {service}_ok true, health {health}")))
        });
        if before.is_none() {
            continue;
        }
        let killed = step(&mut steps, &format!("{service}_simulated_crash"), || {
            let pid = session.kill_service(service)?;
            Ok((pid, format!("SIGKILL sent to pid {pid}")))
        });
        let Some(old_pid) = killed else { continue };
        step(&mut steps, &format!("{service}_recovers"), || {
            let deadline = Instant::now() + recovery_timeout;
            let started = Instant::now();
            loop {
                if let Ok(health) = healthy(&mut session, service) {
                    return Ok(((), format!("healthy again after {} ms (old pid {old_pid}); health {health}", started.elapsed().as_millis())));
                }
                if Instant::now() >= deadline {
                    return Err(format!("{service} did not recover within {} s", recovery_timeout.as_secs()));
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        step(&mut steps, &format!("{service}_logs_capture_startup_and_recovery"), || {
            let lines = session.logs(service, 200)?;
            let startups = lines.iter().filter(|l| l.contains("starting containerd") || l.contains("Starting up")).count();
            let messages = session.logs("messages", 200)?;
            let respawns = messages.iter().filter(|l| l.contains("supervise-daemon") && l.contains(service)).count();
            if startups < 2 {
                return Err(format!("expected at least two startups in /var/log/{service}.log, found {startups}"));
            }
            Ok(((), format!("{startups} startup lines in {service}.log, {respawns} supervise-daemon lines for {service} in messages")))
        });
    }
    let passed = steps.iter().all(|s| s.ok) && steps.len() == 1 + services.len() * 4;
    SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed }
}

fn microvm_step(
    session: &mut Session,
    steps: &mut Vec<SuiteStep>,
    name: &str,
    args: &[&str],
    expect: &str,
) -> Option<Vec<String>> {
    step(steps, name, || {
        let (rc, lines) = session.microvm(args)?;
        let first = lines.first().cloned().unwrap_or_default();
        if rc != 0 || !first.starts_with(expect) {
            return Err(format!("`vesper-microvm {}` rc={rc}: {}", args.join(" "), lines.join(" | ")));
        }
        Ok((lines.clone(), first))
    })
}

/// Runs the GH #18 checklist: launch under the jailer, isolation (uid, chroot, cgroup, seccomp), clean
/// stop and cleanup, cleanup after SIGKILL, and a guest-initiated power-off.
pub fn run_microvm_suite(endpoint: &ControlEndpoint, id: &str) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed: false };
    };
    let _ = microvm_step(&mut session, &mut steps, "cleanup_before", &["cleanup", id], "CLEAN");
    if steps.last().map(|s| !s.ok).unwrap_or(true) {
        steps.pop(); // a dirty pre-state is cleaned, not a failure
    }
    let prepared = microvm_step(&mut session, &mut steps, "prepare", &["prepare"], "PREPARED");
    if prepared.is_none() {
        return SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed: false };
    }
    if microvm_step(&mut session, &mut steps, "launch", &["launch", id], "LAUNCHED").is_some() {
        let isolation = microvm_step(&mut session, &mut steps, "isolation_output", &["isolation", id], "{");
        steps.pop();
        if let Some(lines) = isolation {
            step(&mut steps, "jailer_isolation", || {
                let json: serde_json::Value = serde_json::from_str(&lines[0]).map_err(|e| format!("isolation JSON: {e}: {}", lines[0]))?;
                let checks = ["uid_ok", "chroot_ok", "cgroup_ok", "seccomp_ok"];
                let failed: Vec<&str> = checks.iter().copied().filter(|k| json[k] != serde_json::Value::Bool(true)).collect();
                if !failed.is_empty() {
                    return Err(format!("isolation checks failed: {}; {}", failed.join(", "), lines[0]));
                }
                Ok(((), format!("uid {}, jail files {}, cgroup {}, seccomp mode {}", json["uid"], json["jail_files"], json["cgroup"], json["seccomp_mode"])))
            });
        } else {
            steps.push(SuiteStep { name: "jailer_isolation".into(), ok: false, detail: "no isolation output".into(), elapsed_ms: 0 });
        }
        microvm_step(&mut session, &mut steps, "stop", &["stop", id], "STOPPED");
        microvm_step(&mut session, &mut steps, "cleanup_after_stop", &["cleanup", id], "CLEAN");
        if microvm_step(&mut session, &mut steps, "relaunch", &["launch", id], "LAUNCHED").is_some() {
            microvm_step(&mut session, &mut steps, "abnormal_termination", &["crash", id], "CRASHED");
            microvm_step(&mut session, &mut steps, "cleanup_after_crash", &["cleanup", id], "CLEAN");
        }
        if microvm_step(&mut session, &mut steps, "guest_poweroff_launch", &["launch", id, "--exit"], "LAUNCHED").is_some() {
            step(&mut steps, "guest_poweroff_exits", || {
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    let (_, lines) = session.microvm(&["status", id])?;
                    let first = lines.first().cloned().unwrap_or_default();
                    if first.starts_with("EXITED") {
                        return Ok(((), "firecracker exited after the guest powered off".to_string()));
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("still {first} after 15 s"));
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            });
            microvm_step(&mut session, &mut steps, "cleanup_after_poweroff", &["cleanup", id], "CLEAN");
        }
    }
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 11;
    SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed }
}

pub fn write_report(path: impl AsRef<Path>, report: &SuiteReport) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    let mut json = serde_json::to_string_pretty(report).map_err(|err| format!("serialize report: {err}"))?;
    json.push('\n');
    std::fs::write(path, json).map_err(|err| format!("write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use tempfile::tempdir;

    /// A bridge plus agent in one thread: expects the CONNECT preface, greets, speaks protocol 1.
    fn fake_bridge(path: &Path, greeting: &'static str, drop_after_greeting: bool) {
        let listener = UnixListener::bind(path).unwrap();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let greeting = greeting;
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut writer = stream;
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    assert_eq!(line, "CONNECT 1024\n");
                    writer.write_all(format!("{greeting}\n").as_bytes()).unwrap();
                    if drop_after_greeting {
                        return;
                    }
                    loop {
                        line.clear();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        let mut parts = line.split_whitespace();
                        match parts.next() {
                            Some("PING") => writer.write_all(b"PONG\n").unwrap(),
                            Some("VERSION") => writer.write_all(b"VERSION 1\n").unwrap(),
                            Some("ECHO") => {
                                let n: usize = parts.next().unwrap().parse().unwrap();
                                writer.write_all(format!("ECHO {n}\n").as_bytes()).unwrap();
                                let mut buf = vec![0u8; n];
                                reader.read_exact(&mut buf).unwrap();
                                writer.write_all(&buf).unwrap();
                            }
                            Some("HEALTH") => writer
                                .write_all(b"HEALTH {\"packages_ready\":true,\"services\":{\"vesper-agent\":\"started\",\"containerd\":\"started\",\"docker\":\"started\"},\"containerd_ok\":true,\"docker_ok\":true}\n")
                                .unwrap(),
                            Some("MICROVM") => {
                                let sub = parts.next().unwrap_or("");
                                let body: &str = match sub {
                                    "cleanup" => "CLEAN demo\n",
                                    "prepare" => "PREPARED kernel=1 initrd=1\n",
                                    "launch" => "LAUNCHED demo pid=77 boot_ms=400\n",
                                    "isolation" => "{\"id\":\"demo\",\"pid\":77,\"uid\":10000,\"root\":\"/srv/jailer/firecracker/demo/root\",\"cgroup\":\"/firecracker/demo\",\"seccomp_mode\":2,\"uid_ok\":true,\"chroot_ok\":true,\"cgroup_ok\":true,\"seccomp_ok\":true}\n",
                                    "stop" => "STOPPED demo\n",
                                    "crash" => "CRASHED demo pid=77\n",
                                    "status" => "EXITED demo\n",
                                    _ => "ERR usage\n",
                                };
                                writer.write_all(format!("MICROVM 0 {}\n{body}", body.lines().count()).as_bytes()).unwrap();
                            }
                            Some("KILL") => {
                                let name = parts.next().unwrap_or("");
                                writer.write_all(format!("KILL {name} 4242\n").as_bytes()).unwrap();
                            }
                            Some("LOGS") => {
                                let name = parts.next().unwrap_or("");
                                let body = if name == "messages" {
                                    "supervise-daemon[1]: containerd: respawning\nsupervise-daemon[1]: docker: respawning\n"
                                } else {
                                    "level=info msg=\"starting containerd\"\nlevel=info msg=\"Starting up\"\n"
                                };
                                writer.write_all(format!("LOGS {}\n{body}", body.lines().count()).as_bytes()).unwrap();
                            }
                            Some("DROP") => {
                                writer.write_all(b"DROP\n").unwrap();
                                return;
                            }
                            Some("SHUTDOWN") | Some("QUIT") => {
                                writer.write_all(b"BYE\n").unwrap();
                                return;
                            }
                            _ => writer.write_all(b"ERR unknown command\n").unwrap(),
                        }
                    }
                });
            }
        });
    }

    #[test]
    fn handshake_ping_version_health_and_echo() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake_bridge(&path, "VESPER-AGENT 1 vesper", false);
        let endpoint = ControlEndpoint::new(&path, DEFAULT_AGENT_PORT);
        let mut session = connect(&endpoint).unwrap();
        assert_eq!(session.handshake.protocol_version, 1);
        assert_eq!(session.handshake.hostname, "vesper");
        session.ping().unwrap();
        assert_eq!(session.version().unwrap(), 1);
        let health = session.health().unwrap();
        assert_eq!(health["packages_ready"], serde_json::Value::Bool(true));
        let payload: Vec<u8> = (0..2 * 1024 * 1024 + 7).map(|i| (i % 253) as u8).collect();
        session.echo(&payload).unwrap();
        session.shutdown().unwrap();
    }

    #[test]
    fn rejects_wrong_protocol_and_reports_missing_bridge() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake_bridge(&path, "VESPER-AGENT 2 vesper", true);
        let err = connect(&ControlEndpoint::new(&path, DEFAULT_AGENT_PORT)).unwrap_err();
        assert!(err.contains("protocol 2"));
        let missing = dir.path().join("missing.sock");
        let err = connect(&ControlEndpoint::new(&missing, DEFAULT_AGENT_PORT)).unwrap_err();
        assert!(err.contains("make systemvm-up"));
        let err = connect_with_retry(&ControlEndpoint::new(&missing, 1024), 2, Duration::from_millis(10)).unwrap_err();
        assert!(err.contains("gave up after 2 attempts"));
    }

    #[test]
    fn suite_covers_the_checklist_and_writes_a_report() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake_bridge(&path, "VESPER-AGENT 1 vesper", false);
        let endpoint = ControlEndpoint::new(&path, DEFAULT_AGENT_PORT);
        let report = run_suite(&endpoint, Duration::from_millis(20), Duration::from_millis(100), 1024 * 1024 + 1);
        let names: Vec<&str> = report.steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["handshake", "keepalive", "reconnect", "large_payload"]);
        assert!(report.passed, "{:?}", report.steps);
        let out = dir.path().join("control-report.json");
        write_report(&out, &report).unwrap();
        let written: SuiteReport = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert!(written.passed);
    }

    #[test]
    fn microvm_suite_covers_launch_isolation_stop_crash_and_poweroff() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake_bridge(&path, "VESPER-AGENT 1 vesper", false);
        let endpoint = ControlEndpoint::new(&path, DEFAULT_AGENT_PORT);
        let mut session = connect(&endpoint).unwrap();
        let (rc, lines) = session.microvm(&["status", "demo"]).unwrap();
        assert_eq!((rc, lines[0].as_str()), (0, "EXITED demo"));
        let report = run_microvm_suite(&endpoint, "demo");
        let names: Vec<&str> = report.steps.iter().map(|s| s.name.as_str()).collect();
        for n in ["launch", "jailer_isolation", "stop", "cleanup_after_stop", "abnormal_termination", "cleanup_after_crash", "guest_poweroff_exits", "cleanup_after_poweroff"] {
            assert!(names.contains(&n), "missing {n} in {names:?}");
        }
        assert!(report.passed, "{:?}", report.steps);
    }

    #[test]
    fn services_suite_kills_and_waits_for_recovery() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake_bridge(&path, "VESPER-AGENT 1 vesper", false);
        let endpoint = ControlEndpoint::new(&path, DEFAULT_AGENT_PORT);
        let mut session = connect(&endpoint).unwrap();
        assert_eq!(session.kill_service("docker").unwrap(), 4242);
        assert_eq!(session.logs("docker", 10).unwrap().len(), 2);
        let report = run_services_suite(&endpoint, &["containerd", "docker"], Duration::from_secs(2));
        let names: Vec<&str> = report.steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names.len(), 9);
        assert!(names.contains(&"docker_recovers") && names.contains(&"containerd_logs_capture_startup_and_recovery"));
        assert!(report.passed, "{:?}", report.steps);
    }
}
