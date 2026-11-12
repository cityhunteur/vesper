//! Host-side client for the System VM control channel (GH #13).
//!
//! The channel is a vsock port inside the guest, reached from macOS through the launcher's bridge:
//! a Unix socket that takes the Firecracker-style `CONNECT <port>\n` preface and then relays bytes
//! to the guest port (`docs/process/systemvm-bringup.md`, "Proxy bridge convention"). The guest
//! agent (`systemvm/overlay/usr/local/bin/vesper-agent`) speaks a line protocol, version 1.

mod launchapi;
mod suites;
pub use launchapi::{
    http_request, parse_http_response, parse_ready, read_http_response, valid_microvm_id, HttpResponse, Ready,
};
pub use suites::*;

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
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
        self.guest_tool("MICROVM", args)
    }

    /// Runs `vesper-rootfs <args>` in the guest; returns its exit code and output lines.
    pub fn rootfs(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.guest_tool("ROOTFS", args)
    }

    /// Runs `vesper-workload <args>` in the guest; returns its exit code and output lines.
    pub fn workload(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.guest_tool("WORKLOAD", args)
    }

    /// Runs `vesper-fcparity <args>` in the guest; returns its exit code and output lines.
    pub fn fcparity(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.guest_tool("FCPARITY", args)
    }

    /// Runs `vesper-ctr <args>` in the guest (containerd with the Vesper shim); returns its exit code and output lines.
    pub fn ctr(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.guest_tool("CTR", args)
    }

    /// Runs `vesper-share <args>` in the guest (virtio-fs mounts); returns its exit code and output lines.
    pub fn share(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.guest_tool("SHARE", args)
    }

    /// Runs `vesper-data <args>` in the guest (the persistent data disk, GH #164); returns its exit code and lines.
    pub fn data(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.guest_tool("DATA", args)
    }

    /// Runs `vesper-arch <args>` in the guest (execution-mode metadata); returns its exit code and output lines.
    pub fn arch(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.guest_tool("ARCH", args)
    }

    /// Runs a guest tool command (`MICROVM`, `ROOTFS`, `WORKLOAD`, `FCPARITY`, `CTR`): waits up to 10 min
    /// because launches, image builds and performance runs take longer than the default I/O timeout.
    fn guest_tool(&mut self, tool: &str, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        let stream = self.reader.get_ref();
        let previous = stream.read_timeout().map_err(|err| format!("read timeout: {err}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(600)))
            .map_err(|err| format!("set read timeout: {err}"))?;
        let result = self.guest_tool_inner(tool, args);
        let _ = self.reader.get_ref().set_read_timeout(previous);
        result
    }

    fn guest_tool_inner(&mut self, tool: &str, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.send(&format!("{tool} {}", args.join(" ")))?;
        let line = self.read_line()?;
        let mut parts = line.split_whitespace();
        let (Some(reply), Some(rc), Some(count)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("{tool}: unexpected reply {line:?}"));
        };
        if reply != tool {
            return Err(format!("{tool}: unexpected reply {line:?}"));
        }
        let rc: i32 = rc.parse().map_err(|_| format!("{tool}: bad rc in {line:?}"))?;
        let count: usize = count.parse().map_err(|_| format!("{tool}: bad count in {line:?}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
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
                                .write_all(b"HEALTH {\"packages_ready\":true,\"services\":{\"vesper-agent\":\"started\",\"containerd\":\"started\",\"docker\":\"started\"},\"containerd_ok\":true,\"docker_ok\":true,\"tun\":true}\n")
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
        assert_eq!(names.len(), 10);
        assert!(names.contains(&"tun_loaded_at_boot"));
        assert!(names.contains(&"docker_recovers") && names.contains(&"containerd_logs_capture_startup_and_recovery"));
        assert!(report.passed, "{:?}", report.steps);
    }
}
