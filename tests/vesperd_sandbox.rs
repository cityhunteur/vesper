//! ADR 0012 slice 2: on macOS `vesperd` runs under its own seatbelt profile, on both backends. Through its
//! `--sandbox-probe` hook it tries what the profile must refuse; each attempt must be denied, and the same attempts
//! without the profile must work, so the probe itself is shown able to see a difference. The model backend opens
//! host paths only under its `--host-root` directories (CE-0013).
#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn short_dir() -> tempfile::TempDir {
    // Short paths: a Unix socket path is at most 104 bytes on macOS.
    tempfile::Builder::new()
        .prefix("vsd")
        .tempdir_in("/tmp")
        .unwrap()
}

fn probe(backend: &[&str], dir: &Path, outside: &Path, no_sandbox: bool) -> Vec<String> {
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let unix = std::os::unix::net::UnixListener::bind(dir.join("o.sock")).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vesperd"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(backend)
        .arg("--socket")
        .arg(dir.join("d.sock"))
        .args(["--sandbox-probe", "--probe-tcp"])
        .arg(tcp.local_addr().unwrap().port().to_string())
        .arg("--probe-unix")
        .arg(dir.join("o.sock"))
        .arg("--probe-file")
        .arg(outside);
    if no_sandbox {
        cmd.arg("--no-sandbox");
    }
    let out = cmd.output().unwrap();
    drop((tcp, unix));
    let _ = std::fs::remove_file(dir.join("o.sock"));
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

fn assert_denied_then_allowed(backend: &[&str]) {
    let dir = short_dir();
    // A file outside every host root: the model backend below has only `dir` as its root.
    let other = short_dir();
    let outside = other.path().join("vmlinux");
    std::fs::write(&outside, b"kernel").unwrap();
    let confined = probe(backend, dir.path(), &outside, false);
    assert_eq!(confined.len(), 7, "{confined:?}");
    for line in &confined {
        assert!(line.ends_with(" denied"), "under the profile: {line}");
    }
    for line in probe(backend, dir.path(), &outside, true) {
        assert!(
            line.ends_with(" ALLOWED"),
            "without the profile the probe must succeed, or it shows nothing: {line}"
        );
    }
}

#[test]
fn the_model_backend_profile_refuses_what_vesperd_never_needs() {
    let root = short_dir();
    let root = root.path().to_str().unwrap();
    assert_denied_then_allowed(&["--backend", "model", "--host-root", root]);
}

#[test]
fn the_systemvm_backend_profile_refuses_what_vesperd_never_needs() {
    // The bridge need not exist: the probe exits before vesperd connects to it.
    assert_denied_then_allowed(&["--id", "probe", "--bridge", "/tmp/vesper-no-bridge.sock"]);
}

struct Running(std::process::Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `PUT /boot-source` with `kernel` through a sandboxed model `vesperd` with `--host-root root`.
fn put_kernel(root: &Path, kernel: &Path) -> String {
    use std::io::{Read, Write};
    let dir = short_dir();
    let socket = dir.path().join("v.sock");
    let _child = Running(
        Command::new(env!("CARGO_BIN_EXE_vesperd"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["--backend", "model", "--host-root"])
            .arg(root)
            .arg("--socket")
            .arg(&socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    let mut stream = loop {
        if let Ok(s) = std::os::unix::net::UnixStream::connect(&socket) {
            break s;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "vesperd did not listen"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let body = format!("{{\"kernel_image_path\":\"{}\"}}", kernel.display());
    write!(
        stream,
        "PUT /boot-source HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply
}

#[test]
fn the_model_backend_opens_host_paths_only_under_its_host_roots() {
    let inside = short_dir();
    let outside = short_dir();
    for d in [&inside, &outside] {
        std::fs::write(d.path().join("vmlinux"), b"kernel").unwrap();
    }
    let ok = put_kernel(inside.path(), &inside.path().join("vmlinux"));
    assert!(ok.starts_with("HTTP/1.1 204"), "{ok}");
    let refused = put_kernel(inside.path(), &outside.path().join("vmlinux"));
    assert!(refused.starts_with("HTTP/1.1 400"), "{refused}");
    assert!(
        refused.contains("The kernel file cannot be opened: Operation not permitted"),
        "{refused}"
    );
}

/// Runs `cmd` to its end, or kills it after 20 s: a `vesperd` that does not refuse would serve for ever.
fn output_within(mut cmd: Command) -> std::process::Output {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn a_host_root_that_covers_home_is_refused() {
    let dir = short_dir();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vesperd"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["--backend", "model", "--host-root", "/Users", "--socket"])
        .arg(dir.path().join("v.sock"));
    let out = output_within(cmd);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("would cover"), "{err}");
}

#[test]
fn host_root_is_refused_on_the_systemvm_backend() {
    let dir = short_dir();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vesperd"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "--id",
            "x",
            "--bridge",
            "/tmp/nb.sock",
            "--host-root",
            "/tmp",
        ])
        .arg("--socket")
        .arg(dir.path().join("v.sock"));
    let out = output_within(cmd);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        err.contains("--host-root applies to --backend model only"),
        "{err}"
    );
}
