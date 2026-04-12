use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio::time::timeout;
use url::Url;

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

    if let Some(parent) = Path::new(&config.listen_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| format!("create listen dir: {err}"))?;
    }

    let _ = tokio::fs::remove_file(&config.listen_path).await;
    let listener = UnixListener::bind(&config.listen_path)
        .map_err(|err| format!("listen on unix socket {}: {err}", config.listen_path))?;

    let accept_loop = async {
        loop {
            let (client, _) = listener
                .accept()
                .await
                .map_err(|err| format!("accept client connection: {err}"))?;
            let target = target.clone();
            let dial_timeout = config.dial_timeout;
            let vsock_socket_dir = config.vsock_socket_dir.clone();

            tokio::spawn(async move {
                let _ = handle_client(client, target, dial_timeout, &vsock_socket_dir).await;
            });
        }
    };

    tokio::select! {
        result = accept_loop => result,
        _ = shutdown => Ok(()),
    }
}

async fn handle_client(
    client: UnixStream,
    target: Target,
    dial_timeout: Duration,
    vsock_socket_dir: &str,
) -> Result<(), String> {
    match target.scheme.as_str() {
        "unix" => {
            let upstream = timeout(dial_timeout, UnixStream::connect(target.address))
                .await
                .map_err(|_| "upstream dial timeout".to_string())?
                .map_err(|err| format!("dial unix upstream: {err}"))?;
            relay(client, upstream).await
        }
        "tcp" => {
            let upstream = timeout(dial_timeout, TcpStream::connect(target.address))
                .await
                .map_err(|_| "upstream dial timeout".to_string())?
                .map_err(|err| format!("dial tcp upstream: {err}"))?;
            relay(client, upstream).await
        }
        "vsock" => {
            let socket_path = vsock_socket_path(vsock_socket_dir, target.cid)?;
            let mut upstream = timeout(dial_timeout, UnixStream::connect(&socket_path))
                .await
                .map_err(|_| "upstream dial timeout".to_string())?
                .map_err(|err| format!("dial vsock bridge {socket_path}: {err}"))?;
            upstream
                .write_all(format!("CONNECT {}\n", target.port).as_bytes())
                .await
                .map_err(|err| format!("write vsock CONNECT preface: {err}"))?;
            upstream
                .flush()
                .await
                .map_err(|err| format!("flush vsock CONNECT preface: {err}"))?;
            relay(client, upstream).await
        }
        _ => Err("unsupported target scheme".to_string()),
    }
}

fn vsock_socket_path(vsock_socket_dir: &str, cid: u32) -> Result<String, String> {
    if vsock_socket_dir.trim().is_empty() {
        return Err("vsock socket dir is required".to_string());
    }

    let mut path = PathBuf::from(vsock_socket_dir);
    path.push(format!("{cid}.sock"));
    Ok(path.to_string_lossy().into_owned())
}

async fn relay<Upstream>(mut client: UnixStream, mut upstream: Upstream) -> Result<(), String>
where
    Upstream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    copy_bidirectional(&mut client, &mut upstream)
        .await
        .map_err(|err| format!("relay stream error: {err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::AsyncReadExt;
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
}
