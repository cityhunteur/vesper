//! Published-port forwarding for the Docker ingress (GH #28): watches container events through the
//! Docker API, binds each `-p HOST:CONTAINER` port on the host loopback, and forwards TCP through the
//! launcher's vsock bridge to the System VM gateway (`vesper-port-relay`, vsock port 2400), which hands the
//! stream to the guest loopback port dockerd published. Ownership of the path is
//! `host loopback -> vesper-proxy -> bridge -> System VM gateway -> container`, recorded in a status file.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixStream};
use tokio::task::JoinHandle;

/// Publisher settings.
#[derive(Debug, Clone)]
pub struct PortsConfig {
    /// The Docker ingress socket (through `vesper-proxy` itself or dockerd directly).
    pub docker_socket: PathBuf,
    /// The launcher's vsock bridge socket (`CONNECT <port>` preface).
    pub bridge_path: PathBuf,
    /// The guest vsock port of `vesper-port-relay`.
    pub relay_port: u32,
    /// Where the current mappings are written after every change.
    pub status_path: PathBuf,
    /// Host address to bind; loopback only (binding anything else widens the host surface).
    pub bind_ip: IpAddr,
}

impl Default for PortsConfig {
    fn default() -> Self {
        Self {
            docker_socket: PathBuf::from("run/vesper/docker.sock"),
            bridge_path: PathBuf::from("run/systemvm/vsock/3.sock"),
            relay_port: 2400,
            status_path: PathBuf::from("run/vesper/ports.json"),
            bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }
}

/// One published port as dockerd reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub proto: String,
    pub container_port: u16,
    pub host_ip: String,
    pub host_port: u16,
}

/// One host-side mapping and its state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
    pub container: String,
    pub name: String,
    pub proto: String,
    pub host: String,
    pub host_port: u16,
    pub container_port: u16,
    pub target: String,
    /// `listening`, `conflict` (the host port is taken) or `unsupported` (UDP, non-loopback host ip).
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The status file: ownership path, gateway and every mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub path: String,
    pub gateway: String,
    pub updated_at: String,
    pub mappings: Vec<Mapping>,
}

/// Parses dockerd's `NetworkSettings.Ports` object from a container inspect document.
pub fn parse_bindings(inspect: &serde_json::Value) -> Vec<Binding> {
    let mut out = Vec::new();
    let Some(ports) = inspect["NetworkSettings"]["Ports"].as_object() else {
        return out;
    };
    for (key, hosts) in ports {
        let (port, proto) = key.split_once('/').unwrap_or((key.as_str(), "tcp"));
        let Ok(container_port) = port.parse::<u16>() else { continue };
        let Some(list) = hosts.as_array() else { continue };
        for h in list {
            let host_ip = h["HostIp"].as_str().unwrap_or("").to_string();
            let Some(host_port) = h["HostPort"].as_str().and_then(|p| p.parse::<u16>().ok()) else { continue };
            // dockerd lists the same host port once for IPv4 and once for IPv6; one loopback listener covers both.
            if host_ip == "::" && list.iter().any(|o| o["HostIp"] == "0.0.0.0" && o["HostPort"] == h["HostPort"]) {
                continue;
            }
            out.push(Binding { proto: proto.to_string(), container_port, host_ip, host_port });
        }
    }
    out.sort_by_key(|b| (b.host_port, b.proto.clone()));
    out
}

/// Extracts `(action, container id, name)` from a Docker container event.
pub fn parse_event(event: &serde_json::Value) -> Option<(String, String, String)> {
    if event["Type"].as_str()? != "container" {
        return None;
    }
    let action = event["Action"].as_str()?.split(':').next()?.to_string();
    let id = event["Actor"]["ID"].as_str()?.to_string();
    let name = event["Actor"]["Attributes"]["name"].as_str().unwrap_or("").to_string();
    Some((action, id, name))
}

/// Decides how a binding is published: `Ok(addr)` to listen on, or the unsupported reason.
pub fn plan_binding(bind_ip: IpAddr, b: &Binding) -> Result<SocketAddr, String> {
    if b.proto != "tcp" {
        return Err(format!("{} is not forwarded (only TCP is published to the host)", b.proto));
    }
    match b.host_ip.as_str() {
        "" | "0.0.0.0" | "::" => Ok(SocketAddr::new(bind_ip, b.host_port)),
        ip => match ip.parse::<IpAddr>() {
            Ok(addr) if addr.is_loopback() => Ok(SocketAddr::new(addr, b.host_port)),
            Ok(_) => Err(format!("host ip {ip} is not loopback; only localhost publishing is supported")),
            Err(_) => Err(format!("host ip {ip:?} is not an address")),
        },
    }
}

/// Minimal HTTP/1.1 client over the Docker unix socket: returns `(status, body)` for a
/// Content-Length response.
pub async fn docker_get(socket: &PathBuf, path: &str) -> Result<(u16, Vec<u8>), String> {
    let mut stream = UnixStream::connect(socket).await.map_err(|e| format!("connect {}: {e}", socket.display()))?;
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .map_err(|e| format!("write request: {e}"))?;
    let mut reader = BufReader::new(stream);
    let (status, headers) = read_head(&mut reader).await?;
    let mut body = Vec::new();
    if let Some(len) = header(&headers, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.resize(len, 0);
        reader.read_exact(&mut body).await.map_err(|e| format!("read body: {e}"))?;
    } else if header(&headers, "transfer-encoding").map(|v| v.contains("chunked")).unwrap_or(false) {
        while let Some(chunk) = read_chunk(&mut reader).await? {
            body.extend_from_slice(&chunk);
        }
    } else {
        reader.read_to_end(&mut body).await.map_err(|e| format!("read body: {e}"))?;
    }
    Ok((status, body))
}

async fn read_head<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Result<(u16, Vec<(String, String)>), String> {
    let mut line = String::new();
    reader.read_line(&mut line).await.map_err(|e| format!("read status line: {e}"))?;
    let status = line.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).ok_or_else(|| format!("bad status line {line:?}"))?;
    let mut headers = Vec::new();
    loop {
        line.clear();
        reader.read_line(&mut line).await.map_err(|e| format!("read header: {e}"))?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    Ok((status, headers))
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Reads one HTTP chunk; `None` at the terminating zero-length chunk.
async fn read_chunk<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Result<Option<Vec<u8>>, String> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await.map_err(|e| format!("read chunk size: {e}"))?;
        if n == 0 {
            return Err("event stream closed".to_string());
        }
        if !line.trim().is_empty() {
            break;
        }
    }
    let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("0"), 16).map_err(|_| format!("bad chunk size {line:?}"))?;
    if size == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; size];
    reader.read_exact(&mut buf).await.map_err(|e| format!("read chunk: {e}"))?;
    Ok(Some(buf))
}

struct Live {
    mapping: Mapping,
    task: Option<JoinHandle<()>>,
}

/// The publisher: reconciles running containers at start, then follows `start` and `die` events.
/// Reconnects to the event stream with a backoff when it drops (dockerd or the ingress restarting).
pub struct Publisher {
    config: PortsConfig,
    live: HashMap<String, Vec<Live>>,
}

impl Publisher {
    pub fn new(config: PortsConfig) -> Self {
        Self { config, live: HashMap::new() }
    }

    pub async fn run(&mut self) -> Result<(), String> {
        loop {
            if let Err(err) = self.reconcile().await {
                eprintln!("ports: reconcile failed: {err}; retrying in 2 s");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            match self.follow_events().await {
                Ok(()) => eprintln!("ports: event stream ended; reconnecting"),
                Err(err) => eprintln!("ports: event stream error: {err}; reconnecting in 2 s"),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn reconcile(&mut self) -> Result<(), String> {
        let (status, body) = docker_get(&self.config.docker_socket, "/containers/json").await?;
        if status != 200 {
            return Err(format!("GET /containers/json -> {status}"));
        }
        let list: serde_json::Value = serde_json::from_slice(&body).map_err(|e| format!("containers json: {e}"))?;
        let running: Vec<(String, String)> = list
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| Some((c["Id"].as_str()?.to_string(), c["Names"][0].as_str().unwrap_or("").trim_start_matches('/').to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let gone: Vec<String> = self.live.keys().filter(|id| !running.iter().any(|(r, _)| r == *id)).cloned().collect();
        for id in gone {
            self.unpublish(&id);
        }
        for (id, name) in running {
            if !self.live.contains_key(&id) {
                self.publish(&id, &name).await;
            }
        }
        self.write_status();
        Ok(())
    }

    async fn follow_events(&mut self) -> Result<(), String> {
        let path = "/events?filters=%7B%22type%22%3A%5B%22container%22%5D%7D";
        let mut stream = UnixStream::connect(&self.config.docker_socket).await.map_err(|e| format!("connect: {e}"))?;
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\n\r\n").as_bytes())
            .await
            .map_err(|e| format!("write request: {e}"))?;
        let mut reader = BufReader::new(stream);
        let (status, headers) = read_head(&mut reader).await?;
        if status != 200 {
            return Err(format!("GET /events -> {status}"));
        }
        let chunked = header(&headers, "transfer-encoding").map(|v| v.contains("chunked")).unwrap_or(false);
        loop {
            let payload = if chunked {
                match read_chunk(&mut reader).await? {
                    Some(c) => c,
                    None => return Ok(()),
                }
            } else {
                let mut line = String::new();
                if reader.read_line(&mut line).await.map_err(|e| format!("read event: {e}"))? == 0 {
                    return Ok(());
                }
                line.into_bytes()
            };
            for doc in payload.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
                let Ok(event) = serde_json::from_slice::<serde_json::Value>(doc) else { continue };
                let Some((action, id, name)) = parse_event(&event) else { continue };
                match action.as_str() {
                    "start" | "unpause" => {
                        self.unpublish(&id);
                        self.publish(&id, &name).await;
                        self.write_status();
                    }
                    "die" | "stop" | "kill" | "destroy" | "pause" => {
                        if self.live.contains_key(&id) {
                            self.unpublish(&id);
                            self.write_status();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    async fn publish(&mut self, id: &str, name: &str) {
        let (status, body) = match docker_get(&self.config.docker_socket, &format!("/containers/{id}/json")).await {
            Ok(r) => r,
            Err(err) => {
                eprintln!("ports: inspect {name}: {err}");
                return;
            }
        };
        if status != 200 {
            return;
        }
        let Ok(inspect) = serde_json::from_slice::<serde_json::Value>(&body) else { return };
        let mut entries = Vec::new();
        for b in parse_bindings(&inspect) {
            let target = format!("vsock:{}:{} -> 127.0.0.1:{}", self.config.bridge_path.display(), self.config.relay_port, b.host_port);
            let mut mapping = Mapping {
                container: id.chars().take(12).collect(),
                name: name.to_string(),
                proto: b.proto.clone(),
                host: String::new(),
                host_port: b.host_port,
                container_port: b.container_port,
                target: target.clone(),
                state: "listening".to_string(),
                error: None,
            };
            let task = match plan_binding(self.config.bind_ip, &b) {
                Err(reason) => {
                    mapping.state = "unsupported".into();
                    mapping.error = Some(reason);
                    None
                }
                Ok(addr) => {
                    mapping.host = addr.to_string();
                    match TcpListener::bind(addr).await {
                        Ok(listener) => {
                            eprintln!("ports: {name} {addr} -> {target} ({}/{})", b.container_port, b.proto);
                            Some(tokio::spawn(forward(listener, self.config.bridge_path.clone(), self.config.relay_port, b.host_port)))
                        }
                        Err(err) => {
                            mapping.state = "conflict".into();
                            mapping.error = Some(format!("cannot listen on {addr}: {err}; the container port is reachable only inside the System VM on 127.0.0.1:{}", b.host_port));
                            eprintln!("ports: {name}: {}", mapping.error.as_deref().unwrap_or(""));
                            None
                        }
                    }
                }
            };
            entries.push(Live { mapping, task });
        }
        if !entries.is_empty() {
            self.live.insert(id.to_string(), entries);
        }
    }

    fn unpublish(&mut self, id: &str) {
        if let Some(entries) = self.live.remove(id) {
            for e in entries {
                if let Some(t) = e.task {
                    t.abort();
                }
                eprintln!("ports: {} {} released", e.mapping.name, e.mapping.host);
            }
        }
    }

    /// The current status document.
    pub fn status(&self) -> Status {
        let mut mappings: Vec<Mapping> = self.live.values().flat_map(|v| v.iter().map(|e| e.mapping.clone())).collect();
        mappings.sort_by_key(|m| (m.host_port, m.name.clone()));
        Status {
            path: "host loopback -> vesper-proxy -> bridge -> System VM gateway (vesper-port-relay) -> container".to_string(),
            gateway: format!("{} port {}", self.config.bridge_path.display(), self.config.relay_port),
            updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            mappings,
        }
    }

    fn write_status(&self) {
        let status = self.status();
        if let Some(parent) = self.config.status_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = self.config.status_path.with_extension("json.tmp");
        if let Ok(raw) = serde_json::to_vec_pretty(&status) {
            if std::fs::write(&tmp, raw).is_ok() {
                let _ = std::fs::rename(&tmp, &self.config.status_path);
            }
        }
    }
}

/// Opens one relayed connection: the bridge takes the `CONNECT <relay>` preface silently (no reply, as
/// for every bridge client), then the gateway reads `PORT <n>` and connects to the guest loopback port.
pub async fn dial_gateway(bridge: &PathBuf, relay_port: u32, host_port: u16) -> Result<UnixStream, String> {
    let mut stream = UnixStream::connect(bridge).await.map_err(|e| format!("bridge {}: {e}", bridge.display()))?;
    stream
        .write_all(format!("CONNECT {relay_port}\nPORT {host_port}\n").as_bytes())
        .await
        .map_err(|e| format!("bridge and gateway preface: {e}"))?;
    stream.flush().await.map_err(|e| format!("flush preface: {e}"))?;
    Ok(stream)
}

async fn forward(listener: TcpListener, bridge: PathBuf, relay_port: u32, host_port: u16) {
    loop {
        let Ok((mut client, _)) = listener.accept().await else { continue };
        let bridge = bridge.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(5), dial_gateway(&bridge, relay_port, host_port)).await {
                Ok(Ok(mut upstream)) => {
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                }
                Ok(Err(err)) => eprintln!("ports: {host_port}: {err}"),
                Err(_) => eprintln!("ports: {host_port}: gateway dial timed out"),
            }
            let _ = client.shutdown().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inspect(ports: &str) -> serde_json::Value {
        serde_json::from_str(&format!(r#"{{"NetworkSettings":{{"Ports":{ports}}}}}"#)).unwrap()
    }

    #[test]
    fn bindings_dedupe_ipv6_twins_and_sort() {
        let b = parse_bindings(&inspect(
            r#"{"81/tcp":[{"HostIp":"0.0.0.0","HostPort":"18081"},{"HostIp":"::","HostPort":"18081"}],"80/tcp":[{"HostIp":"0.0.0.0","HostPort":"18080"},{"HostIp":"::","HostPort":"18080"}],"53/udp":[{"HostIp":"0.0.0.0","HostPort":"1053"}],"9/tcp":null}"#,
        ));
        assert_eq!(b.len(), 3);
        assert_eq!((b[0].host_port, b[0].proto.as_str()), (1053, "udp"));
        assert_eq!((b[1].host_port, b[1].container_port), (18080, 80));
        assert_eq!((b[2].host_port, b[2].container_port), (18081, 81));
        assert!(parse_bindings(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn plan_accepts_loopback_only_tcp() {
        let tcp = |ip: &str| Binding { proto: "tcp".into(), container_port: 80, host_ip: ip.into(), host_port: 8080 };
        let lo = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert_eq!(plan_binding(lo, &tcp("0.0.0.0")).unwrap().to_string(), "127.0.0.1:8080");
        assert_eq!(plan_binding(lo, &tcp("::")).unwrap().to_string(), "127.0.0.1:8080");
        assert_eq!(plan_binding(lo, &tcp("127.0.0.1")).unwrap().to_string(), "127.0.0.1:8080");
        assert!(plan_binding(lo, &tcp("10.0.0.5")).unwrap_err().contains("not loopback"));
        let udp = Binding { proto: "udp".into(), container_port: 53, host_ip: "0.0.0.0".into(), host_port: 1053 };
        assert!(plan_binding(lo, &udp).unwrap_err().contains("only TCP"));
    }

    #[test]
    fn events_are_filtered_to_containers() {
        let e = serde_json::json!({"Type":"container","Action":"start","Actor":{"ID":"abc","Attributes":{"name":"web"}}});
        assert_eq!(parse_event(&e), Some(("start".into(), "abc".into(), "web".into())));
        let e = serde_json::json!({"Type":"container","Action":"exec_start: sh","Actor":{"ID":"abc","Attributes":{}}});
        assert_eq!(parse_event(&e).unwrap().0, "exec_start");
        let e = serde_json::json!({"Type":"image","Action":"pull","Actor":{"ID":"x"}});
        assert!(parse_event(&e).is_none());
    }

    #[tokio::test]
    async fn head_and_chunks_are_parsed() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let mut reader = BufReader::new(&raw[..]);
        let (status, headers) = read_head(&mut reader).await.unwrap();
        assert_eq!(status, 200);
        assert_eq!(header(&headers, "transfer-encoding"), Some("chunked"));
        assert_eq!(read_chunk(&mut reader).await.unwrap(), Some(b"hello".to_vec()));
        assert_eq!(read_chunk(&mut reader).await.unwrap(), None);
    }

    #[tokio::test]
    async fn conflict_is_reported_not_fatal() {
        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = taken.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("vesper-ports-{port}"));
        let mut p = Publisher::new(PortsConfig { status_path: dir.join("ports.json"), ..PortsConfig::default() });
        let b = Binding { proto: "tcp".into(), container_port: 80, host_ip: "0.0.0.0".into(), host_port: port };
        let addr = plan_binding(p.config.bind_ip, &b).unwrap();
        let mut mapping = Mapping { container: "c".into(), name: "n".into(), proto: "tcp".into(), host: addr.to_string(), host_port: port, container_port: 80, target: "t".into(), state: "listening".into(), error: None };
        match TcpListener::bind(addr).await {
            Ok(_) => panic!("port should be taken"),
            Err(err) => {
                mapping.state = "conflict".into();
                mapping.error = Some(err.to_string());
            }
        }
        p.live.insert("c".into(), vec![Live { mapping, task: None }]);
        p.write_status();
        let status: Status = serde_json::from_slice(&std::fs::read(dir.join("ports.json")).unwrap()).unwrap();
        assert_eq!(status.mappings[0].state, "conflict");
        assert!(status.path.starts_with("host loopback"));
        p.unpublish("c");
        assert!(p.status().mappings.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
