//! ADR 0012: on macOS `vesper-proxy` runs under its own seatbelt profile. Through its `--sandbox-probe` hook it tries
//! what the profile must refuse; each attempt must be denied, and the same attempts without the profile must work,
//! so the probe itself is shown able to see a difference.
#![cfg(target_os = "macos")]

use std::process::Command;

fn probe(no_sandbox: bool) -> Vec<String> {
    // Short paths: a Unix socket path is at most 104 bytes on macOS.
    let dir = tempfile::Builder::new()
        .prefix("vsp")
        .tempdir_in("/tmp")
        .unwrap();
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let unix = std::os::unix::net::UnixListener::bind(dir.path().join("o.sock")).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vesper-proxy"));
    cmd.args(["--sandbox-probe", "--listen"])
        .arg(dir.path().join("d.sock"));
    cmd.arg("--upstream")
        .arg(format!("unix://{}", dir.path().join("up.sock").display()));
    cmd.args([
        "--probe-tcp",
        &tcp.local_addr().unwrap().port().to_string(),
        "--probe-unix",
    ])
    .arg(dir.path().join("o.sock"));
    if no_sandbox {
        cmd.arg("--no-sandbox");
    }
    let out = cmd.output().unwrap();
    drop((tcp, unix));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn the_profile_refuses_what_the_proxy_never_needs() {
    let confined = probe(false);
    let open = probe(true);
    assert_eq!(confined.len(), 6, "{confined:?}");
    for line in &confined {
        assert!(line.ends_with(" denied"), "under the profile: {line}");
    }
    for line in &open {
        assert!(
            line.ends_with(" ALLOWED"),
            "without the profile the probe must succeed, or it shows nothing: {line}"
        );
    }
}

/// Starts a sandboxed proxy on `listen` to `upstream`, sends `ping` through it and returns the echo.
fn relay_once(listen: &std::path::Path, upstream: &str) -> String {
    use std::io::{Read, Write};
    let mut child = Command::new(env!("CARGO_BIN_EXE_vesper-proxy"))
        .arg("--listen")
        .arg(listen)
        .args(["--upstream", upstream])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !listen.exists() && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let got = (|| {
        let mut c = std::os::unix::net::UnixStream::connect(listen).ok()?;
        c.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok()?;
        c.write_all(b"ping").ok()?;
        let mut buf = [0u8; 4];
        c.read_exact(&mut buf).ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    })();
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    got.unwrap_or_else(|| format!("no echo; stderr: {}", String::from_utf8_lossy(&out.stderr)))
}

fn echo_server<L: Send + 'static, S: std::io::Read + std::io::Write + 'static>(
    listener: L,
    accept: fn(&L) -> S,
) {
    std::thread::spawn(move || {
        let mut s = accept(&listener);
        let mut buf = [0u8; 4];
        if s.read_exact(&mut buf).is_ok() {
            let _ = s.write_all(&buf);
        }
    });
}

#[test]
fn the_confined_proxy_relays_to_unix_and_loopback_tcp_upstreams() {
    let dir = tempfile::Builder::new()
        .prefix("vsp")
        .tempdir_in("/tmp")
        .unwrap();
    let unix = std::os::unix::net::UnixListener::bind(dir.path().join("up.sock")).unwrap();
    echo_server(unix, |l| l.accept().unwrap().0);
    let upstream = format!("unix://{}", dir.path().join("up.sock").display());
    assert_eq!(relay_once(&dir.path().join("a.sock"), &upstream), "ping");
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = tcp.local_addr().unwrap().port();
    echo_server(tcp, |l| l.accept().unwrap().0);
    assert_eq!(
        relay_once(
            &dir.path().join("b.sock"),
            &format!("tcp://127.0.0.1:{port}")
        ),
        "ping"
    );
}

#[test]
fn a_socket_directly_in_home_is_refused_with_its_reason() {
    let home = std::env::var("HOME").unwrap();
    let sock =
        std::path::Path::new(&home).join(format!("vesper-probe-{}.sock", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_vesper-proxy"))
        .arg("--listen")
        .arg(&sock)
        .args(["--upstream", "unix:///nonexistent"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("would cover"), "{err}");
    assert!(!sock.exists());
}
