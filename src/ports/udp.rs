//! UDP published ports on the host loopback (TD-037, D-077). One bridge stream carries every flow of a mapping: the
//! preface is `CONNECT <relay>\nUDP <port>\n`, then frames of `kind (1) | flow id (4, BE) | length (2, BE) | payload`
//! in both directions (`shim/internal/udpframe` is the guest side; the two must agree). The frame has no address:
//! the guest relay sends every flow to `127.0.0.1:<port>`. A flow is one host client address; replies go back from
//! the published socket, so a client with a connected socket accepts them. The table holds at most `MAX_FLOWS`
//! flows, drops the least recently used one past that, and closes a flow after `IDLE` without a datagram. A datagram
//! that finds the stream busy is dropped, as UDP allows; nothing waits on the bridge.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{UdpSocket, UnixStream};
use tokio::sync::mpsc;

pub const DATA: u8 = 0;
pub const CLOSE: u8 = 1;
/// The largest UDP payload over IPv4.
pub const MAX_PAYLOAD: usize = 65507;
pub const MAX_FLOWS: usize = 1024;
pub const IDLE: Duration = Duration::from_secs(90);
/// Frames waiting for the bridge; a datagram that finds the queue full is dropped.
const QUEUE: usize = 256;

/// One frame, ready to write.
pub fn encode(kind: u8, flow: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(7 + payload.len());
    out.push(kind);
    out.extend_from_slice(&flow.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Reads one frame; an unknown kind, a `CLOSE` with a payload or a length over `MAX_PAYLOAD` ends the stream.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<(u8, u32, Vec<u8>), String> {
    let mut head = [0u8; 7];
    r.read_exact(&mut head)
        .await
        .map_err(|e| format!("frame: {e}"))?;
    let kind = head[0];
    let flow = u32::from_be_bytes([head[1], head[2], head[3], head[4]]);
    let len = u16::from_be_bytes([head[5], head[6]]) as usize;
    if (kind != DATA && kind != CLOSE) || len > MAX_PAYLOAD || (kind == CLOSE && len != 0) {
        return Err(format!("bad frame: kind {kind}, length {len}"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)
        .await
        .map_err(|e| format!("frame payload: {e}"))?;
    Ok((kind, flow, payload))
}

/// What a UDP mapping has done, for its log lines.
#[derive(Debug, Default)]
pub struct Counters {
    pub flows_opened: AtomicU64,
    pub evictions: AtomicU64,
    pub expired: AtomicU64,
    pub drops: AtomicU64,
    pub redials: AtomicU64,
}

/// The flow table: one id for each client address, most recent use kept.
#[derive(Debug, Default)]
pub struct Flows {
    by_addr: HashMap<SocketAddr, (u32, Instant)>,
    by_id: HashMap<u32, SocketAddr>,
    next: u32,
}

impl Flows {
    /// The flow id of `addr`, made if new; also the id dropped to make room, if any.
    pub fn id_for(&mut self, addr: SocketAddr, now: Instant, cap: usize) -> (u32, Option<u32>) {
        if let Some((id, last)) = self.by_addr.get_mut(&addr) {
            *last = now;
            return (*id, None);
        }
        let mut evicted = None;
        if self.by_addr.len() >= cap
            && let Some((&old, &(id, _))) = self.by_addr.iter().min_by_key(|(_, (_, last))| *last)
        {
            self.by_addr.remove(&old);
            self.by_id.remove(&id);
            evicted = Some(id);
        }
        while self.by_id.contains_key(&self.next) {
            self.next = self.next.wrapping_add(1);
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.by_addr.insert(addr, (id, now));
        self.by_id.insert(id, addr);
        (id, evicted)
    }

    /// The client of flow `id`, marked as used now.
    pub fn addr_of(&mut self, id: u32, now: Instant) -> Option<SocketAddr> {
        let addr = *self.by_id.get(&id)?;
        if let Some(entry) = self.by_addr.get_mut(&addr) {
            entry.1 = now;
        }
        Some(addr)
    }

    pub fn remove(&mut self, id: u32) {
        if let Some(addr) = self.by_id.remove(&id) {
            self.by_addr.remove(&addr);
        }
    }

    /// Removes and returns the flows idle for `idle` or more.
    pub fn expire(&mut self, now: Instant, idle: Duration) -> Vec<u32> {
        let old: Vec<u32> = self
            .by_addr
            .values()
            .filter(|(_, last)| now.duration_since(*last) >= idle)
            .map(|(id, _)| *id)
            .collect();
        for id in &old {
            self.remove(*id);
        }
        old
    }

    pub fn len(&self) -> usize {
        self.by_addr.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_addr.is_empty()
    }
}

/// The published socket. macOS refuses a datagram larger than the socket's send buffer, 9,216 bytes by default
/// (`net.inet.udp.maxdgram`), so replies up to `MAX_PAYLOAD` need a larger one.
pub fn bind(addr: SocketAddr) -> std::io::Result<UdpSocket> {
    let sock = socket2::Socket::new(
        socket2::Domain::for_address(addr),
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    sock.set_send_buffer_size(MAX_PAYLOAD + 1024)?;
    sock.set_recv_buffer_size(4 * (MAX_PAYLOAD + 1024))?;
    sock.set_nonblocking(true)?;
    sock.bind(&addr.into())?;
    UdpSocket::from_std(sock.into())
}

async fn dial(bridge: &Path, relay_port: u32, host_port: u16) -> Result<UnixStream, String> {
    let mut stream = UnixStream::connect(bridge)
        .await
        .map_err(|e| format!("bridge {}: {e}", bridge.display()))?;
    stream
        .write_all(format!("CONNECT {relay_port}\nUDP {host_port}\n").as_bytes())
        .await
        .map_err(|e| format!("bridge and gateway preface: {e}"))?;
    Ok(stream)
}

/// Serves one UDP mapping until the task is aborted: dials the bridge, relays, and dials again with a backoff when
/// the stream ends (the flows of the ended stream are lost).
pub async fn forward(
    socket: UdpSocket,
    bridge: PathBuf,
    relay_port: u32,
    host_port: u16,
    counters: Arc<Counters>,
) {
    let socket = Arc::new(socket);
    let mut backoff = Duration::from_secs(1);
    loop {
        match tokio::time::timeout(Duration::from_secs(5), dial(&bridge, relay_port, host_port))
            .await
        {
            Ok(Ok(stream)) => {
                let began = Instant::now();
                session(&socket, stream, &counters, IDLE).await;
                // A bridge that accepts and closes at once (its cap, no relay) keeps the backoff growing.
                if began.elapsed() >= Duration::from_secs(10) {
                    backoff = Duration::from_secs(1);
                }
                eprintln!("ports: udp {host_port}: bridge stream ended; dialling again");
            }
            Ok(Err(err)) => eprintln!("ports: udp {host_port}: {err}"),
            Err(_) => eprintln!("ports: udp {host_port}: gateway dial timed out"),
        }
        counters.redials.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

/// Relays one bridge stream until it ends.
pub async fn session(
    socket: &Arc<UdpSocket>,
    stream: UnixStream,
    counters: &Arc<Counters>,
    idle: Duration,
) {
    let (mut rd, mut wr) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(QUEUE);
    // Close frames have their own queue, served first, so a busy data queue never loses one: a lost close would
    // leave the guest holding a flow the host has forgotten. At most one close per flow is ever queued.
    let (ctl, mut ctl_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let _writer = AbortOnDrop(tokio::spawn(async move {
        loop {
            let frame = tokio::select! {
                biased;
                f = ctl_rx.recv() => f,
                f = rx.recv() => f,
            };
            let Some(frame) = frame else { break };
            if wr.write_all(&frame).await.is_err() {
                break;
            }
        }
    }));
    let flows = Arc::new(Mutex::new(Flows::default()));
    let mut reader = {
        let (socket, flows) = (socket.clone(), flows.clone());
        AbortOnDrop(tokio::spawn(async move {
            while let Ok((kind, id, payload)) = read_frame(&mut rd).await {
                if kind == CLOSE {
                    flows.lock().unwrap().remove(id);
                    continue;
                }
                let addr = flows.lock().unwrap().addr_of(id, Instant::now());
                if let Some(addr) = addr {
                    let _ = socket.send_to(&payload, addr).await;
                }
            }
        }))
    };
    let mut ticker = tokio::time::interval(Duration::from_secs(5).min(idle));
    // Room for one byte more than the largest IPv4 payload, so a larger IPv6 datagram is seen and dropped, not cut.
    let mut buf = vec![0u8; MAX_PAYLOAD + 1];
    loop {
        tokio::select! {
            got = socket.recv_from(&mut buf) => {
                let Ok((n, addr)) = got else { continue };
                if n > MAX_PAYLOAD {
                    counters.drops.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                let (id, evicted) = {
                    let mut f = flows.lock().unwrap();
                    let before = f.len();
                    let out = f.id_for(addr, Instant::now(), MAX_FLOWS);
                    if f.len() > before || out.1.is_some() {
                        counters.flows_opened.fetch_add(1, Ordering::Relaxed);
                    }
                    out
                };
                if let Some(old) = evicted {
                    counters.evictions.fetch_add(1, Ordering::Relaxed);
                    let _ = ctl.send(encode(CLOSE, old, &[]));
                }
                if tx.try_send(encode(DATA, id, &buf[..n])).is_err() {
                    counters.drops.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ = ticker.tick() => {
                let old = flows.lock().unwrap().expire(Instant::now(), idle);
                counters.expired.fetch_add(old.len() as u64, Ordering::Relaxed);
                for id in old {
                    let _ = ctl.send(encode(CLOSE, id, &[]));
                }
            }
            _ = &mut reader.0 => break,
        }
    }
}

/// Aborts its task when dropped, so a session's reader and writer end with the session, also when the mapping's
/// task is aborted (council R1: an orphaned reader kept the published socket bound).
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    #[tokio::test]
    async fn frames_round_trip_and_bad_frames_end_the_stream() {
        let big = vec![7u8; MAX_PAYLOAD];
        let mut raw = Vec::new();
        for p in [&b""[..], b"x", &big] {
            raw.extend(encode(DATA, 42, p));
        }
        raw.extend(encode(CLOSE, 42, &[]));
        let mut r = &raw[..];
        for want in [&b""[..], b"x", &big] {
            assert_eq!(read_frame(&mut r).await.unwrap(), (DATA, 42, want.to_vec()));
        }
        assert_eq!(read_frame(&mut r).await.unwrap(), (CLOSE, 42, vec![]));
        for bad in [
            &[9u8, 0, 0, 0, 1, 0, 0][..],
            &[1, 0, 0, 0, 1, 0, 1, b'x'],
            &[0, 0, 0, 0, 1, 0xff, 0xff],
            &[0, 0, 0, 0, 1, 0, 4, b'a'],
        ] {
            let mut r = bad;
            assert!(read_frame(&mut r).await.is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_flow_table_drops_the_oldest_and_expires_idle_flows() {
        let mut f = Flows::default();
        let t0 = Instant::now();
        let a: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let b: SocketAddr = "127.0.0.1:5001".parse().unwrap();
        let c: SocketAddr = "127.0.0.1:5002".parse().unwrap();
        let (ia, _) = f.id_for(a, t0, 2);
        let (ib, _) = f.id_for(b, t0 + Duration::from_secs(1), 2);
        assert_eq!(
            f.id_for(a, t0 + Duration::from_secs(2), 2),
            (ia, None),
            "a known client keeps its id"
        );
        // Full: c takes the place of b, now the least recently used.
        let (ic, evicted) = f.id_for(c, t0 + Duration::from_secs(3), 2);
        assert_eq!(evicted, Some(ib));
        assert!(ic != ia && f.addr_of(ib, t0).is_none() && f.len() == 2);
        assert_eq!(
            f.expire(t0 + Duration::from_secs(4), Duration::from_secs(2)),
            vec![ia]
        );
        assert_eq!(f.addr_of(ic, t0 + Duration::from_secs(4)), Some(c));
    }

    /// A stand-in guest relay that echoes each DATA frame back on its flow: datagrams from two clients come back to
    /// their own senders through one bridge stream.
    #[tokio::test]
    async fn datagrams_cross_one_stream_and_come_back_to_their_senders() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = dir.path().join("3.sock");
        let listener = UnixListener::bind(&bridge).unwrap();
        let guest = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut rd, mut wr) = stream.into_split();
            let mut preface = Vec::new();
            while preface.iter().filter(|b| **b == b'\n').count() < 2 {
                let mut one = [0u8; 1];
                rd.read_exact(&mut one).await.unwrap();
                preface.push(one[0]);
            }
            assert_eq!(preface, b"CONNECT 2400\nUDP 5353\n");
            while let Ok((kind, id, payload)) = read_frame(&mut rd).await {
                if kind == DATA {
                    let mut echo = b"echo:".to_vec();
                    echo.extend(payload);
                    wr.write_all(&encode(DATA, id, &echo)).await.unwrap();
                }
            }
            // A wedged guest: it keeps its end of the stream open after the host stops writing.
            std::future::pending::<()>().await;
        });
        let published = bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let at = published.local_addr().unwrap();
        let counters = Arc::new(Counters::default());
        let relay = tokio::spawn(forward(published, bridge, 2400, 5353, counters.clone()));
        let (one, two) = (
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
        );
        one.connect(at).await.unwrap();
        two.connect(at).await.unwrap();
        let mut buf = [0u8; 64];
        for (sock, msg) in [(&one, &b"one"[..]), (&two, b"two"), (&one, b"again")] {
            sock.send(msg).await.unwrap();
            let n = tokio::time::timeout(Duration::from_secs(5), sock.recv(&mut buf))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&buf[..n], [&b"echo:"[..], msg].concat().as_slice());
        }
        assert_eq!(counters.flows_opened.load(Ordering::Relaxed), 2);
        relay.abort();
        // The published socket goes with the mapping's task, though the guest still holds the stream open: the port
        // can be bound again (council R1, T1).
        let mut rebound = false;
        for _ in 0..50 {
            if bind(at).is_ok() {
                rebound = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            rebound,
            "{at} stayed bound after the mapping's task was aborted"
        );
        guest.abort();
    }
}
