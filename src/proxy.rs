mod relay;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, copy_bidirectional};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use url::Url;

pub use relay::{Bounds, DEFAULT_MAX_LINE, DEFAULT_READ_BUFFER, Preface, RelayEnd, read_line_bounded, write_line};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub scheme: String,
    pub address: String,
    pub cid: u32,
    pub port: u32,
}

pub fn parse_target(raw: &str) -> Result<Target, String> {
    let url = Url::parse(raw.trim()).map_err(|err| format!("parse target URL: {err}"))?;

    match url.scheme() {
        "unix" => {
            let path = url.path();
            if path.is_empty() {
                return Err("unix target path is required".to_string());
            }
            Ok(Target {
                scheme: "unix".to_string(),
                address: path.to_string(),
                cid: 0,
                port: 0,
            })
        }
        "tcp" => {
            let host = url
                .host_str()
                .ok_or_else(|| "tcp target host is required".to_string())?;
            let port = url
                .port_or_known_default()
                .ok_or_else(|| "tcp target port is required".to_string())?;
            Ok(Target {
                scheme: "tcp".to_string(),
                address: format!("{host}:{port}"),
                cid: 0,
                port: 0,
            })
        }
        "vsock" => {
            let host = url
                .host_str()
                .ok_or_else(|| "vsock target cid is required".to_string())?;
            let cid: u32 = host.parse().map_err(|_| "invalid vsock cid".to_string())?;
            let port = url
                .port()
                .ok_or_else(|| "vsock target port is required".to_string())?;

            Ok(Target {
                scheme: "vsock".to_string(),
                address: String::new(),
                cid,
                port: port as u32,
            })
        }
        other => Err(format!("unsupported target scheme: {other}")),
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub listen_path: String,
    pub upstream_raw: String,
    pub dial_timeout: Duration,
    pub vsock_socket_dir: String,
    /// Permission bits applied to the listen socket after bind (owner-only by default).
    pub listen_mode: u32,
    /// Refuse to start unless a unix or vsock upstream's socket path exists.
    pub validate_upstream: bool,
    /// A line exchange with the upstream after the dial (and after `CONNECT` for vsock), before relaying.
    pub preface: Option<Preface>,
    /// Timeouts, the per-read buffer and the connection cap; the default is the plain unbounded relay.
    pub bounds: Bounds,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            listen_path: String::new(),
            upstream_raw: String::new(),
            dial_timeout: Duration::from_secs(5),
            vsock_socket_dir: String::new(),
            listen_mode: 0o600,
            validate_upstream: false,
            preface: None,
            bounds: Bounds::default(),
        }
    }
}

/// The host socket a unix or vsock upstream dials, when there is one to check.
pub fn upstream_socket_path(target: &Target, vsock_socket_dir: &str) -> Result<Option<String>, String> {
    Ok(match target.scheme.as_str() {
        "unix" => Some(target.address.clone()),
        "vsock" => Some(vsock_socket_path(vsock_socket_dir, target.cid)?),
        _ => None,
    })
}

pub async fn run_server(
    config: ServerConfig,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<(), String> {
    if config.listen_path.trim().is_empty() {
        return Err("listen path is required".to_string());
    }
    if config.upstream_raw.trim().is_empty() {
        return Err("upstream target is required".to_string());
    }

    let target = parse_target(&config.upstream_raw)?;
    if config.validate_upstream {
        if let Some(path) = upstream_socket_path(&target, &config.vsock_socket_dir)? {
            let meta = std::fs::metadata(&path)
                .map_err(|err| format!("upstream socket {path} is not available: {err} (is the System VM up? `make systemvm-up`)"))?;
            if !std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()) {
                return Err(format!("upstream path {path} is not a socket"));
            }
        }
    }

    if let Some(parent) = Path::new(&config.listen_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| format!("create listen dir: {err}"))?;
    }

    let _ = tokio::fs::remove_file(&config.listen_path).await;
    let listener = UnixListener::bind(&config.listen_path)
        .map_err(|err| format!("listen on unix socket {}: {err}", config.listen_path))?;
    std::fs::set_permissions(
        &config.listen_path,
        std::os::unix::fs::PermissionsExt::from_mode(config.listen_mode),
    )
    .map_err(|err| format!("set mode {:o} on {}: {err}", config.listen_mode, config.listen_path))?;

    serve(listener, config, shutdown).await
}

/// The accept loop of `run_server` on a listener the caller has bound, for example after `claim_socket`.
/// Returns when `shutdown` completes (dropping the listener) or when accepting fails.
pub async fn serve(
    listener: UnixListener,
    config: ServerConfig,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<(), String> {
    let target = parse_target(&config.upstream_raw)?;
    let slots = config.bounds.max_connections.map(|n| Arc::new(Semaphore::new(n)));
    let config = Arc::new(config);
    let accept_loop = async {
        loop {
            let (client, _) = listener
                .accept()
                .await
                .map_err(|err| format!("accept client connection: {err}"))?;
            let permit = match &slots {
                Some(slots) => match slots.clone().try_acquire_owned() {
                    Ok(permit) => Some(permit),
                    // At the cap: close at once, with no bytes.
                    Err(_) => {
                        drop(client);
                        continue;
                    }
                },
                None => None,
            };
            let target = target.clone();
            let config = config.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = handle_client(client, &target, &config).await;
            });
        }
    };

    tokio::select! {
        result = accept_loop => result,
        _ = shutdown => Ok(()),
    }
}

/// A dialled upstream of any scheme.
pub trait Upstream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Upstream for T {}

/// Dials `target` within `config.dial_timeout`; for vsock also writes the bridge's `CONNECT <port>` preface,
/// within `config.bounds.handshake_timeout` when it is set.
pub async fn open_upstream(target: &Target, config: &ServerConfig) -> Result<Box<dyn Upstream>, String> {
    let dial_timeout = config.dial_timeout;
    match target.scheme.as_str() {
        "unix" => {
            let upstream = timeout(dial_timeout, UnixStream::connect(&target.address))
                .await
                .map_err(|_| "upstream dial timeout".to_string())?
                .map_err(|err| format!("dial unix upstream: {err}"))?;
            Ok(Box::new(upstream))
        }
        "tcp" => {
            let upstream = timeout(dial_timeout, TcpStream::connect(&target.address))
                .await
                .map_err(|_| "upstream dial timeout".to_string())?
                .map_err(|err| format!("dial tcp upstream: {err}"))?;
            Ok(Box::new(upstream))
        }
        "vsock" => {
            let socket_path = vsock_socket_path(&config.vsock_socket_dir, target.cid)?;
            let mut upstream = timeout(dial_timeout, UnixStream::connect(&socket_path))
                .await
                .map_err(|_| "upstream dial timeout".to_string())?
                .map_err(|err| format!("dial vsock bridge {socket_path}: {err}"))?;
            write_line(&mut upstream, &format!("CONNECT {}", target.port), config.bounds.handshake_timeout)
                .await
                .map_err(|err| format!("vsock CONNECT preface: {err}"))?;
            Ok(Box::new(upstream))
        }
        _ => Err("unsupported target scheme".to_string()),
    }
}

async fn handle_client(client: UnixStream, target: &Target, config: &ServerConfig) -> Result<(), String> {
    let mut upstream = open_upstream(target, config).await?;
    if let Some(preface) = &config.preface {
        // On a refusal or a timeout the client connection is dropped here: closed, with no bytes.
        relay::run_preface(&mut upstream, preface, &config.bounds).await?;
    }
    if config.bounds.plain() {
        return relay(client, upstream).await;
    }
    relay::bounded(client, upstream, &config.bounds).await;
    Ok(())
}

/// Checks the socket's directory, takes `<socket>.lock` and removes a stale socket (TD-050, ADR 0007 §2). Refuses
/// when the directory is writable by group or others, when another holder has the lock, when a process answers on
/// the path, or when the path is not a socket. The returned lock is held for the life of the server; the live-socket
/// check, the unlink and the caller's bind all happen while it is held.
pub fn claim_socket(socket: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let parent = socket.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if !parent.exists() {
        std::fs::create_dir_all(parent).map_err(|err| format!("create socket dir {}: {err}", parent.display()))?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("set socket dir permissions {}: {err}", parent.display()))?;
    }
    let mode = std::fs::metadata(parent)
        .map_err(|err| format!("socket dir {}: {err}", parent.display()))?
        .permissions()
        .mode();
    if mode & 0o022 != 0 {
        return Err(format!(
            "socket dir {} is writable by group or others (mode {:o}); use a private directory",
            parent.display(),
            mode & 0o7777
        ));
    }
    let mut lock_path = socket.as_os_str().to_owned();
    lock_path.push(".lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|err| format!("open {}: {err}", Path::new(&lock_path).display()))?;
    lock.try_lock()
        .map_err(|_| format!("another vesperd holds {}", Path::new(&lock_path).display()))?;
    if let Ok(meta) = std::fs::symlink_metadata(socket) {
        if !meta.file_type().is_socket() {
            return Err(format!("{} exists and is not a socket; refusing to remove it", socket.display()));
        }
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Err(format!("another process is serving {}", socket.display()));
        }
        std::fs::remove_file(socket).map_err(|err| format!("remove stale socket {}: {err}", socket.display()))?;
    }
    Ok(lock)
}

fn vsock_socket_path(vsock_socket_dir: &str, cid: u32) -> Result<String, String> {
    if vsock_socket_dir.trim().is_empty() {
        return Err("vsock socket dir is required".to_string());
    }

    let mut path = PathBuf::from(vsock_socket_dir);
    path.push(format!("{cid}.sock"));
    Ok(path.to_string_lossy().into_owned())
}

async fn relay<U>(mut client: UnixStream, mut upstream: U) -> Result<(), String>
where
    U: AsyncRead + AsyncWrite + Unpin,
{
    copy_bidirectional(&mut client, &mut upstream)
        .await
        .map_err(|err| format!("relay stream error: {err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_socket_path_follows_the_scheme() {
        let unix = parse_target("unix:///tmp/x.sock").unwrap();
        assert_eq!(upstream_socket_path(&unix, "/tmp/vsock").unwrap().as_deref(), Some("/tmp/x.sock"));
        let vsock = parse_target("vsock://3:2375").unwrap();
        assert_eq!(upstream_socket_path(&vsock, "/tmp/vsock").unwrap().as_deref(), Some("/tmp/vsock/3.sock"));
        let tcp = parse_target("tcp://127.0.0.1:1").unwrap();
        assert!(upstream_socket_path(&tcp, "/tmp/vsock").unwrap().is_none());
    }

    #[tokio::test]
    async fn validate_upstream_refuses_a_missing_bridge_and_sets_listen_mode() {
        let tempdir = tempfile::tempdir().unwrap();
        let listen = tempdir.path().join("docker.sock");
        let err = run_server(
            ServerConfig {
                listen_path: listen.to_string_lossy().into_owned(),
                upstream_raw: "vsock://3:2375".to_string(),
                vsock_socket_dir: tempdir.path().join("vsock").to_string_lossy().into_owned(),
                validate_upstream: true,
                ..Default::default()
            },
            async {},
        )
        .await
        .unwrap_err();
        assert!(err.contains("make systemvm-up"), "{err}");

        let bridge_dir = tempdir.path().join("vsock");
        std::fs::create_dir_all(&bridge_dir).unwrap();
        let _bridge = UnixListener::bind(bridge_dir.join("3.sock")).unwrap();
        run_server(
            ServerConfig {
                listen_path: listen.to_string_lossy().into_owned(),
                upstream_raw: "vsock://3:2375".to_string(),
                vsock_socket_dir: bridge_dir.to_string_lossy().into_owned(),
                validate_upstream: true,
                listen_mode: 0o640,
                ..Default::default()
            },
            async {},
        )
        .await
        .unwrap();
        let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&listen).unwrap().permissions()) & 0o777;
        assert_eq!(mode, 0o640);
    }
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Notify;
    use tempfile::tempdir;

    async fn wait_for_socket(path: &Path) {
        for _ in 0..100 {
            if path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn parses_vsock_target() {
        let target = parse_target("vsock://3:2375").unwrap();
        assert_eq!(target.scheme, "vsock");
        assert_eq!(target.cid, 3);
        assert_eq!(target.port, 2375);
    }

    #[test]
    fn rejects_invalid_vsock_target() {
        assert!(parse_target("vsock://cid:2375").is_err());
        assert!(parse_target("vsock://3").is_err());
    }

    #[tokio::test]
    async fn forwards_round_trip_traffic() {
        let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = echo_listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = echo_listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    loop {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        let _ = socket.write_all(&buffer[..read]).await;
                    }
                });
            }
        });

        let tempdir = tempdir().unwrap();
        let socket_path = tempdir.path().join("proxy.sock");
        let notify = Arc::new(Notify::new());
        let done = notify.clone();
        let socket_path_for_task = socket_path.clone();

        tokio::spawn(async move {
            let _ = run_server(
                ServerConfig {
                    listen_path: socket_path_for_task.to_string_lossy().into_owned(),
                    upstream_raw: format!("tcp://{}", echo_addr),
                    dial_timeout: Duration::from_secs(1),
                    vsock_socket_dir: tempdir.path().join("vsock").to_string_lossy().into_owned(),
                    ..Default::default()
                },
                async move {
                    done.notified().await;
                },
            )
            .await;
        });

        wait_for_socket(&socket_path).await;

        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        client.write_all(b"ping\n").await.unwrap();

        let mut buf = [0u8; 5];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping\n");

        notify.notify_waiters();
        let _ = tokio::fs::remove_file(&socket_path).await;
    }

    #[tokio::test]
    async fn forwards_vsock_round_trip_traffic() {
        let tempdir = tempdir().unwrap();
        let vsock_dir = tempdir.path().join("vsock");
        tokio::fs::create_dir_all(&vsock_dir).await.unwrap();
        let bridge_path = vsock_dir.join("3.sock");
        let bridge_listener = UnixListener::bind(&bridge_path).unwrap();

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = bridge_listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut preface = Vec::new();
                    loop {
                        let mut byte = [0u8; 1];
                        let Ok(read) = socket.read(&mut byte).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        preface.push(byte[0]);
                        if byte[0] == b'\n' {
                            break;
                        }
                    }

                    assert_eq!(String::from_utf8(preface).unwrap(), "CONNECT 2375\n");

                    let mut buffer = [0u8; 1024];
                    loop {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        let _ = socket.write_all(&buffer[..read]).await;
                    }
                });
            }
        });

        let socket_path = tempdir.path().join("proxy.sock");
        let notify = Arc::new(Notify::new());
        let done = notify.clone();
        let socket_path_for_task = socket_path.clone();
        let vsock_dir_for_task = vsock_dir.clone();

        tokio::spawn(async move {
            let _ = run_server(
                ServerConfig {
                    listen_path: socket_path_for_task.to_string_lossy().into_owned(),
                    upstream_raw: "vsock://3:2375".to_string(),
                    dial_timeout: Duration::from_secs(1),
                    vsock_socket_dir: vsock_dir_for_task.to_string_lossy().into_owned(),
                    ..Default::default()
                },
                async move {
                    done.notified().await;
                },
            )
            .await;
        });

        wait_for_socket(&socket_path).await;

        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        client.write_all(b"ping\n").await.unwrap();

        let mut buf = [0u8; 5];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping\n");

        notify.notify_waiters();
        let _ = tokio::fs::remove_file(&socket_path).await;
    }

    #[tokio::test]
    async fn survives_failed_upstream_dial_and_accepts_next_client() {
        let tempdir = tempdir().unwrap();
        let socket_path = tempdir.path().join("proxy.sock");
        let upstream_path = tempdir.path().join("upstream.sock");
        let notify = Arc::new(Notify::new());
        let done = notify.clone();
        let socket_path_for_task = socket_path.clone();
        let upstream_path_for_task = upstream_path.clone();

        tokio::spawn(async move {
            let _ = run_server(
                ServerConfig {
                    listen_path: socket_path_for_task.to_string_lossy().into_owned(),
                    upstream_raw: format!("unix://{}", upstream_path_for_task.to_string_lossy()),
                    dial_timeout: Duration::from_millis(200),
                    vsock_socket_dir: tempdir.path().join("vsock").to_string_lossy().into_owned(),
                    ..Default::default()
                },
                async move {
                    done.notified().await;
                },
            )
            .await;
        });

        wait_for_socket(&socket_path).await;

        let mut failed_client = UnixStream::connect(&socket_path).await.unwrap();
        failed_client.write_all(b"first\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        let mut scratch = [0u8; 8];
        let read = failed_client.read(&mut scratch).await.unwrap_or(0);
        assert_eq!(read, 0);
        drop(failed_client);

        let upstream_listener = UnixListener::bind(&upstream_path).unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = upstream_listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    loop {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        let _ = socket.write_all(&buffer[..read]).await;
                    }
                });
            }
        });

        let mut client = UnixStream::connect(&socket_path).await.unwrap();
        client.write_all(b"ping\n").await.unwrap();

        let mut buf = [0u8; 5];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping\n");

        notify.notify_waiters();
        let _ = tokio::fs::remove_file(&socket_path).await;
    }

    #[test]
    fn claim_socket_refuses_a_live_socket_and_only_one_racing_start_wins() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let live = dir.path().join("live.sock");
        let _other = std::os::unix::net::UnixListener::bind(&live).unwrap();
        let err = claim_socket(&live).unwrap_err();
        assert!(err.contains("another process is serving"), "{err}");
        assert!(live.exists());

        let path = dir.path().join("race.sock");
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (barrier, path) = (barrier.clone(), path.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    claim_socket(&path)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let won = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(won, 1, "{results:?}");
        assert!(results.iter().filter_map(|r| r.as_ref().err()).all(|e| e.contains("another vesperd holds")));
        assert!(!path.exists(), "the winner removed the stale socket");
    }
}
