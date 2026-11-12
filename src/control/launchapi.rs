//! Client side of the agent's `LAUNCHAPI` and `FCAPI` verbs (ADR 0007, slice 3).
//!
//! `LAUNCHAPI <id> [ceiling]` starts one jailed Firecracker built from API requests and answers
//! `READY <id> pid=<n> firecracker_version=<v>` or `ERR <reason>`; after `READY` the connection is the microVM's
//! lease. `FCAPI <id>` answers `OK` and then relays bytes to that microVM's API socket, or `ERR <reason>`.
//! This module parses only those fixed forms; the HTTP helper exists for the System VM suite.

use super::Session;
use std::io::{BufRead, Read, Write};
use std::time::{Duration, Instant};

/// Longest status, header or reply line the client accepts.
const MAX_LINE: usize = 8 * 1024;
/// Largest HTTP body the suite's helper accepts; Firecracker's bodies are a few KiB.
const MAX_BODY: usize = 1024 * 1024;

/// A `READY` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    pub id: String,
    pub pid: u32,
    pub firecracker_version: String,
}

/// One HTTP response read from a relayed connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// The jailer's id rule: 1 to 64 ASCII letters, digits and hyphens.
pub fn valid_microvm_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Parses `READY <id> pid=<n> firecracker_version=<v>` for `id`; an `ERR <reason>` line becomes the error.
pub fn parse_ready(line: &str, id: &str) -> Result<Ready, String> {
    if let Some(reason) = line.strip_prefix("ERR ") {
        return Err(format!("LAUNCHAPI {id}: {reason}"));
    }
    let parts: Vec<&str> = line.split(' ').collect();
    let [verb, got_id, pid, version] = parts.as_slice() else {
        return Err(format!("LAUNCHAPI {id}: unexpected reply {line:?}"));
    };
    if *verb != "READY" || *got_id != id {
        return Err(format!("LAUNCHAPI {id}: unexpected reply {line:?}"));
    }
    let pid: u32 = pid
        .strip_prefix("pid=")
        .and_then(|v| v.parse().ok())
        .filter(|p| *p > 0)
        .ok_or_else(|| format!("LAUNCHAPI {id}: bad pid in {line:?}"))?;
    let version = version
        .strip_prefix("firecracker_version=")
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
        .ok_or_else(|| format!("LAUNCHAPI {id}: bad firecracker_version in {line:?}"))?;
    Ok(Ready { id: id.to_string(), pid, firecracker_version: version.to_string() })
}

impl Session {
    /// Reads one line of at most `MAX_LINE` bytes.
    fn read_bounded_line(&mut self) -> Result<String, String> {
        let mut buf = Vec::new();
        let read = (&mut self.reader)
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buf)
            .map_err(|err| format!("read from agent: {err}"))?;
        if read == 0 {
            return Err("agent closed the connection".to_string());
        }
        if buf.last() != Some(&b'\n') {
            return Err(format!("line longer than {MAX_LINE} bytes or cut short"));
        }
        let line = String::from_utf8(buf).map_err(|_| "line is not UTF-8".to_string())?;
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    fn with_read_timeout<T>(&mut self, timeout: Duration, run: impl FnOnce(&mut Self) -> T) -> Result<T, String> {
        let previous = self.reader.get_ref().read_timeout().map_err(|err| format!("read timeout: {err}"))?;
        self.reader
            .get_ref()
            .set_read_timeout(Some(timeout))
            .map_err(|err| format!("set read timeout: {err}"))?;
        let result = run(self);
        let _ = self.reader.get_ref().set_read_timeout(previous);
        Ok(result)
    }

    /// Sends `LAUNCHAPI <id> [ceiling]` and waits up to `timeout` for `READY`. On success this session is the
    /// lease: dropping it stops the microVM.
    pub fn launch_api(&mut self, id: &str, ceiling: Option<u64>, timeout: Duration) -> Result<Ready, String> {
        if !valid_microvm_id(id) {
            return Err(format!("LAUNCHAPI: invalid id {id:?}"));
        }
        let line = match ceiling {
            Some(bytes) => format!("LAUNCHAPI {id} {bytes}"),
            None => format!("LAUNCHAPI {id}"),
        };
        self.send(&line)?;
        let reply = self.with_read_timeout(timeout, |s| s.read_bounded_line())??;
        parse_ready(&reply, id)
    }

    /// Sends `FCAPI <id>` and waits for `OK`; afterwards the session carries raw bytes to the API socket.
    pub fn fcapi(&mut self, id: &str) -> Result<(), String> {
        self.send(&format!("FCAPI {id}"))?;
        let reply = self.read_bounded_line()?;
        match reply.as_str() {
            "OK" => Ok(()),
            _ => match reply.strip_prefix("ERR ") {
                Some(reason) => Err(format!("FCAPI {id}: {reason}")),
                None => Err(format!("FCAPI {id}: unexpected reply {reply:?}")),
            },
        }
    }

    /// One HTTP/1.1 request on a relayed (`FCAPI`) connection; the connection stays open for the next one.
    pub fn http(&mut self, method: &str, path: &str, body: Option<&str>) -> Result<HttpResponse, String> {
        let body = body.unwrap_or("");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        self.writer
            .write_all(request.as_bytes())
            .and_then(|_| self.writer.flush())
            .map_err(|err| format!("{method} {path}: write: {err}"))?;
        let status_line = self.read_bounded_line().map_err(|err| format!("{method} {path}: {err}"))?;
        let status: u16 = status_line
            .strip_prefix("HTTP/1.1 ")
            .or_else(|| status_line.strip_prefix("HTTP/1.0 "))
            .and_then(|rest| rest.get(..3))
            .and_then(|code| code.parse().ok())
            .ok_or_else(|| format!("{method} {path}: bad status line {status_line:?}"))?;
        let mut length = 0usize;
        loop {
            let header = self.read_bounded_line().map_err(|err| format!("{method} {path}: {err}"))?;
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse().map_err(|_| format!("{method} {path}: bad {header:?}"))?;
                }
            }
        }
        if length > MAX_BODY {
            return Err(format!("{method} {path}: body of {length} bytes exceeds {MAX_BODY}"));
        }
        let mut buf = vec![0u8; length];
        self.reader.read_exact(&mut buf).map_err(|err| format!("{method} {path}: body: {err}"))?;
        Ok(HttpResponse { status, body: String::from_utf8_lossy(&buf).into_owned() })
    }

    /// Waits up to `timeout` for the agent to close this connection (end of file); bytes are discarded.
    pub fn wait_closed(&mut self, timeout: Duration) -> Result<Duration, String> {
        let started = Instant::now();
        let deadline = started + timeout;
        let mut buf = [0u8; 512];
        // macOS refuses SO_RCVTIMEO (EINVAL) on a socket the peer has already shut down; the read below then
        // returns end of file at once, and otherwise the connection's own I/O timeout still bounds it.
        let previous = self.reader.get_ref().read_timeout().ok().flatten();
        let _ = self.reader.get_ref().set_read_timeout(Some(Duration::from_secs(1)));
        let result = loop {
            if Instant::now() >= deadline {
                break Err(format!("connection still open after {} s", timeout.as_secs()));
            }
            match self.reader.read(&mut buf) {
                Ok(0) => break Ok(started.elapsed()),
                Ok(_) => continue,
                Err(err) if matches!(err.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Err(_) => break Ok(started.elapsed()), // a reset is a close too
            }
        };
        let _ = self.reader.get_ref().set_read_timeout(previous);
        result
    }

    /// Runs `vesper-vesperd <args>` in the guest (status, isolation, reap, list, log and the test hooks).
    pub fn vesperd(&mut self, args: &[&str]) -> Result<(i32, Vec<String>), String> {
        self.guest_tool("VESPERD", args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{connect, ControlEndpoint, DEFAULT_AGENT_PORT};
    use std::io::BufReader;
    use std::os::unix::net::UnixListener;
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn ready_is_parsed_against_its_fixed_form() {
        let ok = parse_ready("READY vm-1 pid=42 firecracker_version=1.16.0", "vm-1").unwrap();
        assert_eq!(ok, Ready { id: "vm-1".into(), pid: 42, firecracker_version: "1.16.0".into() });
        let err = parse_ready("ERR busy vm-1", "vm-1").unwrap_err();
        assert!(err.ends_with("busy vm-1"), "{err}");
        for bad in [
            "READY vm-2 pid=42 firecracker_version=1.16.0",
            "READY vm-1 pid=0 firecracker_version=1.16.0",
            "READY vm-1 pid=x firecracker_version=1.16.0",
            "READY vm-1 pid=42 firecracker_version=",
            "READY vm-1 pid=42 firecracker_version=1.16.0 extra",
            "READY vm-1 pid=42 firecracker_version=1.16;rm",
            "OK",
            "",
        ] {
            assert!(parse_ready(bad, "vm-1").is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn ids_follow_the_jailer_rule() {
        assert!(valid_microvm_id("a-B-9"));
        assert!(valid_microvm_id(&"x".repeat(64)));
        for bad in ["", "../x", "a b", "a_b", "a*", &"x".repeat(65)] {
            assert!(!valid_microvm_id(bad), "accepted {bad:?}");
        }
    }

    /// A bridge plus an agent that knows LAUNCHAPI and FCAPI; FCAPI answers one HTTP exchange per request.
    fn fake(path: &std::path::Path, launch_reply: &'static str) {
        let listener = UnixListener::bind(path).unwrap();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut writer = stream;
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    writer.write_all(b"VESPER-AGENT 1 vesper\n").unwrap();
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line.starts_with("LAUNCHAPI") {
                        writer.write_all(format!("{launch_reply}\n").as_bytes()).unwrap();
                        // Hold the lease until the client closes, then close too.
                        let mut sink = Vec::new();
                        let _ = reader.read_to_end(&mut sink);
                    } else if line == "FCAPI vm-1\n" {
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
                                let body = "{\"id\":\"vm-1\",\"state\":\"Not started\"}";
                                format!("HTTP/1.1 200 \r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())
                            } else {
                                "HTTP/1.1 204 \r\n\r\n".to_string()
                            };
                            writer.write_all(reply.as_bytes()).unwrap();
                        }
                    } else {
                        writer.write_all(b"ERR no vesperd microVM\n").unwrap();
                    }
                });
            }
        });
    }

    #[test]
    fn launch_holds_a_lease_and_fcapi_relays_http() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake(&path, "READY vm-1 pid=77 firecracker_version=1.16.0");
        let endpoint = ControlEndpoint::new(&path, DEFAULT_AGENT_PORT);
        let mut lease = connect(&endpoint).unwrap();
        let ready = lease.launch_api("vm-1", None, Duration::from_secs(5)).unwrap();
        assert_eq!((ready.pid, ready.firecracker_version.as_str()), (77, "1.16.0"));
        let mut api = connect(&endpoint).unwrap();
        api.fcapi("vm-1").unwrap();
        let root = api.http("GET", "/", None).unwrap();
        assert_eq!(root.status, 200);
        assert!(root.body.contains("\"state\":\"Not started\""), "{}", root.body);
        let put = api.http("PUT", "/actions", Some("{\"action_type\":\"InstanceStart\"}")).unwrap();
        assert_eq!((put.status, put.body.as_str()), (204, ""));
        let mut other = connect(&endpoint).unwrap();
        assert!(other.fcapi("vm-2").unwrap_err().contains("no vesperd microVM"));
        assert!(lease.launch_api("../x", None, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn launch_reports_err_and_a_server_close_is_seen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3.sock");
        fake(&path, "ERR busy vm-1");
        let endpoint = ControlEndpoint::new(&path, DEFAULT_AGENT_PORT);
        let mut lease = connect(&endpoint).unwrap();
        let err = lease.launch_api("vm-1", Some(134217728), Duration::from_secs(5)).unwrap_err();
        assert!(err.contains("busy"), "{err}");
        // The fake keeps reading until the client closes; shut our side and the fake closes too.
        lease.writer.shutdown(std::net::Shutdown::Write).unwrap();
        lease.wait_closed(Duration::from_secs(5)).unwrap();
    }
}
