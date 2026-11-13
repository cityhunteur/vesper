//! Client side of the agent's `LAUNCHAPI` and `FCAPI` verbs (ADR 0007, slice 3).
//!
//! `LAUNCHAPI <id> [ceiling]` starts one jailed Firecracker built from API requests and answers
//! `READY <id> pid=<n> firecracker_version=<v>` or `ERR <reason>`; after `READY` the connection is the microVM's
//! lease. `FCAPI <id>` answers `OK` and then relays bytes to that microVM's API socket, or `ERR <reason>`.
//! `LAUNCHAPI` also takes `stage=SRC:DEST` and `tap=NAME` tokens; each `tap=NAME` is a tap device in the microVM's
//! own network namespace, owned by the jailer's uid (CE-0010).
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

/// `tap=` tokens per `LAUNCHAPI` (CE-0010).
pub const MAX_TAPS: usize = 8;

/// A tap device name the guest accepts: a Linux interface name (at most 15 bytes) of ASCII letters, digits, `_` and
/// `-`, not starting with `-` (it would read as an option), and never `lo`. The guest checks it again
/// (`vesper-vesperd`, `valid_tap`).
pub fn valid_tap_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 15
        && name != "lo"
        && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// One `vesperd --tap NAME[:ADDR/PREFIX]`: a tap device, and under `--network nat` the gateway address its guest
/// routes through (ADR 0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapSpec {
    pub name: String,
    pub gateway: Option<(std::net::Ipv4Addr, u8)>,
}

/// Networks a tap gateway may not touch: Docker's bridge and the veth pool (`198.18.0.0/15`). The guest also checks
/// the System VM's `eth0` subnet and resolver, which only it knows.
const RESERVED_NETS: [(std::net::Ipv4Addr, u8); 2] =
    [(std::net::Ipv4Addr::new(172, 17, 0, 0), 16), (std::net::Ipv4Addr::new(198, 18, 0, 0), 15)];

fn mask(prefix: u8) -> u32 {
    if prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(prefix)) }
}

/// Whether two IPv4 networks share an address.
pub fn nets_overlap(a: (std::net::Ipv4Addr, u8), b: (std::net::Ipv4Addr, u8)) -> bool {
    let m = mask(a.1.min(b.1));
    u32::from(a.0) & m == u32::from(b.0) & m
}

impl TapSpec {
    /// Parses `NAME` or `NAME:ADDR/PREFIX`. A gateway is a host address inside RFC 1918 with a prefix length of 16
    /// to 30, clear of `172.17.0.0/16` and `198.18.0.0/15`.
    pub fn parse(text: &str) -> Result<TapSpec, String> {
        let (name, gateway) = match text.split_once(':') {
            None => (text, None),
            Some((name, cidr)) => (name, Some(parse_gateway(cidr).map_err(|err| format!("--tap {text:?}: {err}"))?)),
        };
        if !valid_tap_name(name) {
            return Err(format!("--tap {text:?}: use 1 to 15 letters, digits, _ and -, not starting with -, not lo"));
        }
        Ok(TapSpec { name: name.to_string(), gateway })
    }

    /// The `LAUNCHAPI` tokens: `tap=NAME`, and `tapaddr=NAME:ADDR/PREFIX` when it has a gateway.
    pub fn tokens(&self) -> Vec<String> {
        let mut out = vec![format!("tap={}", self.name)];
        if let Some((addr, prefix)) = self.gateway {
            out.push(format!("tapaddr={}:{addr}/{prefix}", self.name));
        }
        out
    }
}

fn parse_gateway(cidr: &str) -> Result<(std::net::Ipv4Addr, u8), String> {
    let (addr, prefix) = cidr.split_once('/').ok_or("give the gateway as ADDR/PREFIX")?;
    let addr: std::net::Ipv4Addr = addr.parse().map_err(|_| format!("{addr:?} is not an IPv4 address"))?;
    let prefix: u8 = prefix.parse().ok().filter(|p| (16..=30).contains(p)).ok_or("the prefix length must be 16 to 30")?;
    let host = u32::from(addr) & !mask(prefix);
    if host == 0 || host == !mask(prefix) {
        return Err(format!("{addr}/{prefix}: the gateway must be a host address"));
    }
    if !addr.is_private() {
        return Err(format!("{addr}/{prefix}: not inside RFC 1918"));
    }
    if let Some((net, p)) = RESERVED_NETS.iter().find(|n| nets_overlap((addr, prefix), **n)) {
        return Err(format!("{addr}/{prefix} overlaps {net}/{p}"));
    }
    Ok((addr, prefix))
}

/// Checks `--tap` flags against `--network nat`: at most `MAX_TAPS`, no name twice, gateways only under `nat`, at
/// least one gateway under `nat`, and no two gateway subnets overlapping.
pub fn check_network(nat: bool, taps: &[TapSpec]) -> Result<(), String> {
    if taps.len() > MAX_TAPS {
        return Err(format!("at most {MAX_TAPS} --tap flags"));
    }
    for (i, tap) in taps.iter().enumerate() {
        if taps[..i].iter().any(|t| t.name == tap.name) {
            return Err(format!("--tap {:?} given twice", tap.name));
        }
        if let Some(g) = tap.gateway {
            if !nat {
                return Err(format!("--tap {}:{}/{}: a gateway address needs --network nat", tap.name, g.0, g.1));
            }
            if let Some(other) = taps[..i].iter().find(|t| t.gateway.is_some_and(|o| nets_overlap(g, o))) {
                return Err(format!("--tap {}: its subnet overlaps the one of --tap {}", tap.name, other.name));
            }
        }
    }
    if nat && !taps.iter().any(|t| t.gateway.is_some()) {
        return Err("--network nat needs a --tap NAME:ADDR/PREFIX".to_string());
    }
    Ok(())
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

/// An HTTP/1.1 request with a JSON body (empty when `None`), as the System VM suites send it.
pub fn http_request(method: &str, path: &str, body: Option<&str>) -> Vec<u8> {
    let body = body.unwrap_or("");
    format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn read_raw_line<R: BufRead>(reader: &mut R, out: &mut Vec<u8>) -> Result<String, String> {
    let mut buf = Vec::new();
    let read = reader
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut buf)
        .map_err(|err| format!("read response: {err}"))?;
    if read == 0 {
        return Err("closed before a response".to_string());
    }
    if buf.last() != Some(&b'\n') {
        return Err(format!("response line longer than {MAX_LINE} bytes or cut short"));
    }
    out.extend_from_slice(&buf);
    Ok(String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string())
}

/// Reads one HTTP response (status line, headers and a `Content-Length` body of at most `MAX_BODY` bytes) and
/// returns its bytes unchanged.
pub fn read_http_response<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    read_raw_line(reader, &mut raw)?;
    let mut length = 0usize;
    loop {
        let header = read_raw_line(reader, &mut raw)?;
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().map_err(|_| format!("bad {header:?}"))?;
            }
        }
    }
    if length > MAX_BODY {
        return Err(format!("body of {length} bytes exceeds {MAX_BODY}"));
    }
    let start = raw.len();
    raw.resize(start + length, 0);
    reader.read_exact(&mut raw[start..]).map_err(|err| format!("body: {err}"))?;
    Ok(raw)
}

/// Status and body of a response read by `read_http_response`.
pub fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("no end of headers")?;
    let status_line = head.lines().next().unwrap_or("");
    let status: u16 = status_line
        .strip_prefix("HTTP/1.1 ")
        .or_else(|| status_line.strip_prefix("HTTP/1.0 "))
        .and_then(|rest| rest.get(..3))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("bad status line {status_line:?}"))?;
    Ok(HttpResponse { status, body: body.to_string() })
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
        let raw = self.http_raw(&http_request(method, path, body))?;
        parse_http_response(&raw).map_err(|err| format!("{method} {path}: {err}"))
    }

    /// Writes `request` on a relayed (`FCAPI`) connection and returns the response exactly as received.
    pub fn http_raw(&mut self, request: &[u8]) -> Result<Vec<u8>, String> {
        self.writer
            .write_all(request)
            .and_then(|_| self.writer.flush())
            .map_err(|err| format!("write request: {err}"))?;
        read_http_response(&mut self.reader)
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

    #[test]
    fn tap_specs_check_gateways() {
        let t = TapSpec::parse("tap0:172.16.0.1/30").unwrap();
        assert_eq!(t.tokens(), vec!["tap=tap0".to_string(), "tapaddr=tap0:172.16.0.1/30".to_string()]);
        assert_eq!(TapSpec::parse("tap1").unwrap().tokens(), vec!["tap=tap1".to_string()]);
        for (bad, why) in [
            ("tap0:172.16.0.1", "ADDR/PREFIX"),
            ("tap0:172.16.0.0/30", "host address"),
            ("tap0:172.16.0.3/30", "host address"),
            ("tap0:172.16.0.1/31", "16 to 30"),
            ("tap0:10.0.0.1/8", "16 to 30"),
            ("tap0:8.8.8.1/24", "RFC 1918"),
            ("tap0:172.17.0.1/24", "overlaps 172.17.0.0/16"),
            ("tap0:300.1.1.1/24", "not an IPv4"),
            ("lo:172.16.0.1/30", "not lo"),
        ] {
            let err = TapSpec::parse(bad).unwrap_err();
            assert!(err.contains(why), "{bad}: {err}");
        }
        let a = TapSpec::parse("a:10.1.0.1/16").unwrap();
        let b = TapSpec::parse("b:10.1.2.1/24").unwrap();
        assert!(check_network(true, &[a.clone(), b]).unwrap_err().contains("overlaps"));
        assert!(check_network(false, &[a.clone()]).unwrap_err().contains("needs --network nat"));
        assert!(check_network(true, &[TapSpec::parse("c").unwrap()]).unwrap_err().contains("needs a --tap NAME:ADDR"));
        assert!(check_network(true, &[a]).is_ok());
    }

    #[test]
    fn tap_names_are_short_interface_names() {
        assert!(valid_tap_name("tap0"));
        assert!(valid_tap_name("vm_1-tap"));
        assert!(valid_tap_name(&"t".repeat(15)));
        // ip keywords are names like any other: the guest always passes them after `dev`.
        assert!(valid_tap_name("up") && valid_tap_name("dev") && valid_tap_name("t-"));
        for bad in ["", "lo", "-", "-tap0", "tap 0", "tap/0", "tap.0", "tap:0", "tap=0", "../x", &"t".repeat(16)] {
            assert!(!valid_tap_name(bad), "accepted {bad:?}");
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
