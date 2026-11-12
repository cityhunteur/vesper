//! The preface step and the bounds of `run_server` (ADR 0007 §2), used by `vesperd --backend systemvm`.
//!
//! A preface is a short line exchange with the upstream after the dial and before any client byte is relayed: read
//! a greeting of a fixed form, send one request line, expect one exact reply line. The bounded relay copies bytes
//! both ways without parsing them. Each read fills at most one buffer; there is no total byte limit. It infers
//! whether a request is waiting from which side sent bytes last: while the client spoke last, the per-request
//! ceiling applies; otherwise the idle timeout does. Pipelined requests defeat that inference, because the client
//! may send the next request before the upstream answers the first one.

use std::io;
use std::sync::Mutex;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Notify;
use tokio::time::{Instant, timeout};

/// Longest preface line accepted when `Bounds::max_line` is 0.
pub const DEFAULT_MAX_LINE: usize = 1024;
/// Per-read buffer when `Bounds::read_buffer` is 0; the size `copy_bidirectional` uses.
pub const DEFAULT_READ_BUFFER: usize = 8 * 1024;

/// A line exchange with the upstream before relaying, for example the agent's greeting, `FCAPI <id>` and `OK`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preface {
    /// The greeting line must start with this.
    pub greeting_prefix: String,
    /// Sent after the greeting, without its newline.
    pub request: String,
    /// The one reply line that accepts the request; anything else closes the client connection.
    pub accept: String,
}

/// Bounds on one relayed connection and on the accept loop. The default is the plain relay with no bounds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bounds {
    /// Applies to the `CONNECT` write, the preface write and each preface line read.
    pub handshake_timeout: Option<Duration>,
    /// Longest preface line; 0 means `DEFAULT_MAX_LINE`.
    pub max_line: usize,
    /// Bytes one read may fill; 0 with no timeouts keeps `copy_bidirectional`.
    pub read_buffer: usize,
    /// Closes the connection when no byte has moved for this long while no request is waiting.
    pub idle_timeout: Option<Duration>,
    /// Closes the connection when a request has waited this long for the first byte of its reply.
    pub request_ceiling: Option<Duration>,
    /// Concurrent relayed connections; a client accepted beyond it is closed at once, with no bytes.
    pub max_connections: Option<usize>,
}

impl Bounds {
    pub(super) fn line_limit(&self) -> usize {
        if self.max_line == 0 { DEFAULT_MAX_LINE } else { self.max_line }
    }

    pub(super) fn plain(&self) -> bool {
        self.read_buffer == 0 && self.idle_timeout.is_none() && self.request_ceiling.is_none()
    }
}

/// How a bounded relay ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayEnd {
    /// Both sides closed, or one side failed.
    Closed,
    /// The idle timeout passed with no request waiting.
    Idle,
    /// A request waited longer than the per-request ceiling.
    Ceiling,
}

async fn within<T>(limit: Option<Duration>, what: &str, run: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    match limit {
        Some(d) => timeout(d, run).await.map_err(|_| format!("{what}: no answer within {} ms", d.as_millis()))?,
        None => run.await,
    }
}

/// Reads one line of at most `max` bytes, one byte at a time so that nothing after the newline is consumed.
/// The newline and a trailing carriage return are removed.
pub async fn read_line_bounded<R: AsyncRead + Unpin>(reader: &mut R, max: usize, limit: Option<Duration>) -> Result<String, String> {
    within(limit, "read line", async {
        let mut line = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            let n = reader.read(&mut byte).await.map_err(|err| format!("read line: {err}"))?;
            if n == 0 {
                return Err("read line: closed before a full line".to_string());
            }
            if byte[0] == b'\n' {
                break;
            }
            if line.len() >= max {
                return Err(format!("read line: longer than {max} bytes"));
            }
            line.push(byte[0]);
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        String::from_utf8(line).map_err(|_| "read line: not UTF-8".to_string())
    })
    .await
}

/// Writes `line` and a newline, then flushes.
pub async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, line: &str, limit: Option<Duration>) -> Result<(), String> {
    within(limit, "write line", async {
        writer.write_all(format!("{line}\n").as_bytes()).await.map_err(|err| format!("write line: {err}"))?;
        writer.flush().await.map_err(|err| format!("write line: {err}"))
    })
    .await
}

/// Runs `preface` on a freshly dialled upstream.
pub(super) async fn run_preface<S>(stream: &mut S, preface: &Preface, bounds: &Bounds) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let max = bounds.line_limit();
    let greeting = read_line_bounded(stream, max, bounds.handshake_timeout).await?;
    if !greeting.starts_with(&preface.greeting_prefix) {
        return Err(format!("unexpected greeting {greeting:?}"));
    }
    write_line(stream, &preface.request, bounds.handshake_timeout).await?;
    let reply = read_line_bounded(stream, max, bounds.handshake_timeout).await?;
    if reply != preface.accept {
        return Err(format!("{}: refused: {reply:?}", preface.request));
    }
    Ok(())
}

/// Which side sent bytes last, since when, and when any byte last moved.
struct Clock {
    client_last: bool,
    since: Instant,
    last: Instant,
}

async fn pump<R, W>(mut from: R, mut to: W, size: usize, clock: &Mutex<Clock>, turn: &Notify, client: bool) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; size];
    loop {
        let n = from.read(&mut buf).await?;
        if n == 0 {
            // Pass the half-close on; the other direction keeps running.
            let _ = to.shutdown().await;
            return Ok(());
        }
        {
            let mut c = clock.lock().expect("relay clock");
            let now = Instant::now();
            if c.client_last != client {
                c.client_last = client;
                c.since = now;
                turn.notify_one();
            }
            c.last = now;
        }
        to.write_all(&buf[..n]).await?;
    }
}

/// Copies bytes both ways under `bounds` until both sides close, one fails, or a timeout passes.
pub(super) async fn bounded<C, U>(client: C, upstream: U, bounds: &Bounds) -> RelayEnd
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let size = if bounds.read_buffer == 0 { DEFAULT_READ_BUFFER } else { bounds.read_buffer };
    let now = Instant::now();
    let clock = Mutex::new(Clock { client_last: false, since: now, last: now });
    let turn = Notify::new();
    let (client_read, client_write) = tokio::io::split(client);
    let (upstream_read, upstream_write) = tokio::io::split(upstream);
    let both = async {
        tokio::try_join!(
            pump(client_read, upstream_write, size, &clock, &turn, true),
            pump(upstream_read, client_write, size, &clock, &turn, false),
        )
    };
    let watchdog = async {
        loop {
            let (deadline, end) = {
                let c = clock.lock().expect("relay clock");
                if c.client_last {
                    (bounds.request_ceiling.map(|d| c.since + d), RelayEnd::Ceiling)
                } else {
                    (bounds.idle_timeout.map(|d| c.last + d), RelayEnd::Idle)
                }
            };
            match deadline {
                Some(at) if Instant::now() >= at => return end,
                // Wake at the deadline, or earlier when the side that spoke last changes; then look again.
                Some(at) => tokio::select! {
                    _ = tokio::time::sleep_until(at) => {}
                    _ = turn.notified() => {}
                },
                None => turn.notified().await,
            }
        }
    };
    tokio::select! {
        _ = both => RelayEnd::Closed,
        end = watchdog => end,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{run_server, ServerConfig};
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::io::duplex;
    use tokio::net::{UnixListener, UnixStream};

    async fn wait_for_socket(path: &Path) {
        for _ in 0..100 {
            if path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn bounds(idle_ms: u64, ceiling_ms: u64) -> Bounds {
        Bounds {
            read_buffer: 1024,
            idle_timeout: Some(Duration::from_millis(idle_ms)),
            request_ceiling: Some(Duration::from_millis(ceiling_ms)),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn line_reads_are_bounded_and_stop_at_the_newline() {
        let (mut a, mut b) = duplex(4096);
        b.write_all(b"VESPER-AGENT 1 vesper\r\nnext").await.unwrap();
        assert_eq!(read_line_bounded(&mut a, 64, None).await.unwrap(), "VESPER-AGENT 1 vesper");
        let mut rest = [0u8; 4];
        a.read_exact(&mut rest).await.unwrap();
        assert_eq!(&rest, b"next");
        b.write_all(&[b'x'; 65]).await.unwrap();
        assert!(read_line_bounded(&mut a, 64, None).await.unwrap_err().contains("longer than 64"));
        let err = read_line_bounded(&mut a, 64, Some(Duration::from_millis(50))).await.unwrap_err();
        assert!(err.contains("no answer within 50 ms"), "{err}");
    }

    #[tokio::test]
    async fn the_idle_timeout_applies_between_requests() {
        let (client, mut client_peer) = duplex(4096);
        let (upstream, mut upstream_peer) = duplex(4096);
        let relay = tokio::spawn(async move { bounded(client, upstream, &bounds(150, 5_000)).await });
        // One request and its reply, then silence: the relay ends after the idle timeout.
        client_peer.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
        let mut got = [0u8; 18];
        upstream_peer.read_exact(&mut got).await.unwrap();
        upstream_peer.write_all(b"HTTP/1.1 200 \r\n\r\n").await.unwrap();
        let mut reply = [0u8; 17];
        client_peer.read_exact(&mut reply).await.unwrap();
        let started = Instant::now();
        assert_eq!(relay.await.unwrap(), RelayEnd::Idle);
        assert!(started.elapsed() >= Duration::from_millis(100), "{:?}", started.elapsed());
        // The client sees end of file and no bytes.
        let mut rest = Vec::new();
        client_peer.read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty());
    }

    #[tokio::test]
    async fn a_waiting_request_gets_the_ceiling_not_the_idle_timeout() {
        let (client, mut client_peer) = duplex(4096);
        let (upstream, mut upstream_peer) = duplex(4096);
        let relay = tokio::spawn(async move { bounded(client, upstream, &bounds(100, 400)).await });
        client_peer.write_all(b"PUT /snapshot/create HTTP/1.1\r\n\r\n").await.unwrap();
        let mut got = [0u8; 8];
        upstream_peer.read_exact(&mut got).await.unwrap();
        // A slow operation that answers after three idle timeouts but inside the ceiling still gets through.
        tokio::time::sleep(Duration::from_millis(300)).await;
        upstream_peer.write_all(b"HTTP/1.1 204 \r\n\r\n").await.unwrap();
        let mut reply = [0u8; 17];
        client_peer.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"HTTP/1.1 204 \r\n\r\n");
        // The next request never gets an answer: the ceiling ends the connection.
        client_peer.write_all(b"PUT /snapshot/create HTTP/1.1\r\n\r\n").await.unwrap();
        let started = Instant::now();
        assert_eq!(relay.await.unwrap(), RelayEnd::Ceiling);
        assert!(started.elapsed() >= Duration::from_millis(350), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn large_transfers_and_half_close_pass_through() {
        let (client, mut client_peer) = duplex(64 * 1024);
        let (upstream, mut upstream_peer) = duplex(64 * 1024);
        let relay = tokio::spawn(async move { bounded(client, upstream, &bounds(2_000, 2_000)).await });
        // Four MiB from the upstream, far more than one read buffer: no total byte limit.
        let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let sent = payload.clone();
        let writer = tokio::spawn(async move {
            upstream_peer.write_all(&sent).await.unwrap();
            upstream_peer.shutdown().await.unwrap();
            let mut tail = Vec::new();
            upstream_peer.read_to_end(&mut tail).await.unwrap();
            tail
        });
        let mut back = vec![0u8; payload.len()];
        client_peer.read_exact(&mut back).await.unwrap();
        assert!(back == payload, "payload corrupted");
        // The upstream closed its side; the client can still send, and then close.
        client_peer.write_all(b"after-half-close").await.unwrap();
        client_peer.shutdown().await.unwrap();
        assert_eq!(writer.await.unwrap(), b"after-half-close");
        assert_eq!(relay.await.unwrap(), RelayEnd::Closed);
    }

    /// What the fake agent does after `CONNECT 1024`.
    #[derive(Clone, Copy)]
    enum Agent {
        /// Greets, answers `FCAPI vm-1` with `OK` and then answers every request with `ERROR_REPLY`, except
        /// `PUT /slow`, which it never answers.
        Firecracker,
        /// Greets with a line longer than the preface limit.
        LongGreeting,
        /// Never greets.
        Silent,
    }

    const ERROR_REPLY: &[u8] = b"HTTP/1.1 400 \r\nServer: Firecracker API\r\nConnection: keep-alive\r\nContent-Type: application/json\r\nContent-Length: 81\r\n\r\n{\"fault_message\":\"The requested operation is not supported after starting the microVM.\"}";

    /// A bridge plus agent: checks the `CONNECT 1024` preface, then behaves as `agent`.
    fn fake_agent(dir: &Path, agent: Agent) -> PathBuf {
        let vsock = dir.join("vsock");
        std::fs::create_dir_all(&vsock).unwrap();
        let listener = UnixListener::bind(vsock.join("3.sock")).unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    assert_eq!(read_line_bounded(&mut socket, 64, None).await.unwrap(), "CONNECT 1024");
                    match agent {
                        Agent::Silent => {
                            let mut sink = Vec::new();
                            let _ = socket.read_to_end(&mut sink).await;
                            return;
                        }
                        Agent::LongGreeting => {
                            let _ = socket.write_all(format!("VESPER-AGENT 1 {}\n", "x".repeat(2048)).as_bytes()).await;
                            let mut sink = Vec::new();
                            let _ = socket.read_to_end(&mut sink).await;
                            return;
                        }
                        Agent::Firecracker => {}
                    }
                    socket.write_all(b"VESPER-AGENT 1 vesper\n").await.unwrap();
                    let Ok(request) = read_line_bounded(&mut socket, 64, None).await else { return };
                    if request != "FCAPI vm-1" {
                        let _ = socket.write_all(b"ERR no vesperd microVM vm-2\n").await;
                        return;
                    }
                    socket.write_all(b"OK\n").await.unwrap();
                    loop {
                        // One request: a request line and headers with no body.
                        let Ok(head) = read_line_bounded(&mut socket, 1024, None).await else { return };
                        while let Ok(line) = read_line_bounded(&mut socket, 1024, None).await {
                            if line.is_empty() {
                                break;
                            }
                        }
                        if head.starts_with("PUT /slow") {
                            continue;
                        }
                        if socket.write_all(ERROR_REPLY).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        vsock
    }

    fn fcapi_config(listen: &Path, vsock: &Path, bounds: Bounds) -> ServerConfig {
        ServerConfig {
            listen_path: listen.to_string_lossy().into_owned(),
            upstream_raw: "vsock://3:1024".to_string(),
            dial_timeout: Duration::from_secs(1),
            vsock_socket_dir: vsock.to_string_lossy().into_owned(),
            preface: Some(Preface {
                greeting_prefix: "VESPER-AGENT 1 ".to_string(),
                request: "FCAPI vm-1".to_string(),
                accept: "OK".to_string(),
            }),
            bounds,
            ..Default::default()
        }
    }

    fn short_bounds() -> Bounds {
        Bounds {
            handshake_timeout: Some(Duration::from_millis(300)),
            max_line: 1024,
            read_buffer: 4096,
            idle_timeout: Some(Duration::from_millis(400)),
            request_ceiling: Some(Duration::from_millis(800)),
            max_connections: Some(2),
        }
    }

    async fn start(config: ServerConfig) -> (Arc<Notify>, PathBuf) {
        let listen = PathBuf::from(&config.listen_path);
        let notify = Arc::new(Notify::new());
        let done = notify.clone();
        tokio::spawn(async move {
            let _ = run_server(config, async move { done.notified().await }).await;
        });
        wait_for_socket(&listen).await;
        (notify, listen)
    }

    async fn exchange(client: &mut UnixStream, request: &[u8]) -> Vec<u8> {
        client.write_all(request).await.unwrap();
        let mut reply = vec![0u8; ERROR_REPLY.len()];
        client.read_exact(&mut reply).await.unwrap();
        reply
    }

    /// Reads to end of file within `limit`; returns the bytes and the time it took.
    async fn read_until_closed(client: &mut UnixStream, limit: Duration) -> (Vec<u8>, Duration) {
        let started = std::time::Instant::now();
        let mut rest = Vec::new();
        timeout(limit, client.read_to_end(&mut rest)).await.expect("connection still open").unwrap_or(0);
        (rest, started.elapsed())
    }

    #[tokio::test]
    async fn the_preface_runs_and_an_upstream_error_passes_through_byte_for_byte() {
        let dir = tempdir().unwrap();
        let vsock = fake_agent(dir.path(), Agent::Firecracker);
        let (done, listen) = start(fcapi_config(&dir.path().join("api.sock"), &vsock, short_bounds())).await;
        let mut client = UnixStream::connect(&listen).await.unwrap();
        let request = b"PUT /machine-config HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(exchange(&mut client, request).await, ERROR_REPLY);
        // Keep-alive: a second request on the same connection is relayed too.
        assert_eq!(exchange(&mut client, request).await, ERROR_REPLY);
        done.notify_waiters();
    }

    #[tokio::test]
    async fn a_refused_fcapi_or_a_bad_preface_closes_the_client_with_no_bytes() {
        for (agent, request, within) in [
            (Agent::Firecracker, "FCAPI vm-2", Duration::from_secs(2)),
            (Agent::LongGreeting, "FCAPI vm-1", Duration::from_secs(2)),
            (Agent::Silent, "FCAPI vm-1", Duration::from_secs(2)),
        ] {
            let dir = tempdir().unwrap();
            let vsock = fake_agent(dir.path(), agent);
            let mut config = fcapi_config(&dir.path().join("api.sock"), &vsock, short_bounds());
            config.preface.as_mut().unwrap().request = request.to_string();
            let (done, listen) = start(config).await;
            let mut client = UnixStream::connect(&listen).await.unwrap();
            client.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
            let (rest, took) = read_until_closed(&mut client, within).await;
            assert!(rest.is_empty(), "{request}: got {:?}", String::from_utf8_lossy(&rest));
            if matches!(agent, Agent::Silent) {
                assert!(took >= Duration::from_millis(250), "the greeting timeout ended it early: {took:?}");
            }
            done.notify_waiters();
        }
    }

    #[tokio::test]
    async fn idle_connections_and_unanswered_requests_are_closed() {
        let dir = tempdir().unwrap();
        let vsock = fake_agent(dir.path(), Agent::Firecracker);
        let (done, listen) = start(fcapi_config(&dir.path().join("api.sock"), &vsock, short_bounds())).await;

        let mut idle = UnixStream::connect(&listen).await.unwrap();
        assert_eq!(exchange(&mut idle, b"GET / HTTP/1.1\r\n\r\n").await, ERROR_REPLY);
        let (rest, took) = read_until_closed(&mut idle, Duration::from_secs(3)).await;
        assert!(rest.is_empty());
        assert!(took >= Duration::from_millis(350) && took < Duration::from_millis(800), "idle close after {took:?}");

        let mut waiting = UnixStream::connect(&listen).await.unwrap();
        waiting.write_all(b"PUT /slow HTTP/1.1\r\n\r\n").await.unwrap();
        let (rest, took) = read_until_closed(&mut waiting, Duration::from_secs(3)).await;
        assert!(rest.is_empty());
        assert!(took >= Duration::from_millis(750), "a waiting request got the idle timeout: {took:?}");
        done.notify_waiters();
    }

    #[tokio::test]
    async fn the_connection_cap_closes_extra_clients_with_no_bytes() {
        let dir = tempdir().unwrap();
        let vsock = fake_agent(dir.path(), Agent::Firecracker);
        let mut bounds = short_bounds();
        bounds.idle_timeout = Some(Duration::from_secs(10));
        let (done, listen) = start(fcapi_config(&dir.path().join("api.sock"), &vsock, bounds)).await;
        let mut first = UnixStream::connect(&listen).await.unwrap();
        let mut second = UnixStream::connect(&listen).await.unwrap();
        assert_eq!(exchange(&mut first, b"GET / HTTP/1.1\r\n\r\n").await, ERROR_REPLY);
        assert_eq!(exchange(&mut second, b"GET / HTTP/1.1\r\n\r\n").await, ERROR_REPLY);
        let mut third = UnixStream::connect(&listen).await.unwrap();
        let _ = third.write_all(b"GET / HTTP/1.1\r\n\r\n").await;
        let (rest, _) = read_until_closed(&mut third, Duration::from_secs(2)).await;
        assert!(rest.is_empty(), "a client over the cap got {:?}", String::from_utf8_lossy(&rest));
        // A slot frees when a relayed connection ends.
        drop(first);
        let mut fourth = None;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let mut next = UnixStream::connect(&listen).await.unwrap();
            next.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
            let mut reply = vec![0u8; ERROR_REPLY.len()];
            if timeout(Duration::from_millis(500), next.read_exact(&mut reply)).await.map(|r| r.is_ok()).unwrap_or(false) {
                fourth = Some(reply);
                break;
            }
        }
        assert_eq!(fourth.as_deref(), Some(ERROR_REPLY));
        done.notify_waiters();
    }
}
