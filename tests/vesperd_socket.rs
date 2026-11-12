//! The `vesperd` socket is owner-only by default and is never taken from another server (TD-050, ADR 0007 slice 1).
//! These tests start the real binary.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn vesperd(socket: Option<&Path>, runtime_dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vesperd"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("TMPDIR")
        .env_remove("HOME")
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = socket {
        cmd.arg("--socket").arg(path);
    }
    cmd
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(socket: Option<&Path>, runtime_dir: &Path, expect: &Path) -> Running {
    let child = vesperd(socket, runtime_dir).spawn().expect("spawn vesperd");
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if std::os::unix::net::UnixStream::connect(expect).is_ok() {
            return Running(child);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("vesperd did not listen on {}", expect.display());
}

/// Runs vesperd expecting it to fail; returns its stderr.
fn refused(socket: Option<&Path>, runtime_dir: &Path) -> String {
    let out = vesperd(socket, runtime_dir).output().expect("run vesperd");
    assert!(!out.status.success(), "vesperd should have refused to start");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

#[test]
fn default_socket_is_owner_only() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir: PathBuf = tmp.path().join("vesper");
    let socket = dir.join("vesperd.sock");
    let _server = start(None, tmp.path(), &socket);
    assert_eq!(mode(&dir), 0o700, "runtime directory");
    assert_eq!(mode(&socket), 0o600, "socket");
}

#[test]
fn a_second_vesperd_does_not_take_the_socket() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let socket = tmp.path().join("vesper").join("vesperd.sock");
    let _first = start(None, tmp.path(), &socket);
    let err = refused(None, tmp.path());
    assert!(err.contains("another vesperd holds"), "stderr: {err}");
    assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok(), "the first server must keep its socket");
}

#[test]
fn a_live_socket_of_another_process_is_not_removed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let socket = tmp.path().join("other.sock");
    let _other = std::os::unix::net::UnixListener::bind(&socket).expect("bind");
    let err = refused(Some(&socket), tmp.path());
    assert!(err.contains("another process is serving"), "stderr: {err}");
    assert!(socket.exists());
}

#[test]
fn a_stale_socket_is_replaced() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let socket = tmp.path().join("stale.sock");
    drop(std::os::unix::net::UnixListener::bind(&socket).expect("bind"));
    assert!(socket.exists(), "a dropped listener leaves its socket file");
    let _server = start(Some(&socket), tmp.path(), &socket);
    assert_eq!(mode(&socket), 0o600);
}

#[test]
fn a_shared_directory_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o777)).expect("chmod");
    let err = refused(Some(&tmp.path().join("vesperd.sock")), tmp.path());
    assert!(err.contains("writable by group or others"), "stderr: {err}");
}

#[test]
fn a_regular_file_at_the_path_is_not_removed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let path = tmp.path().join("notes.txt");
    std::fs::write(&path, "keep me").expect("write");
    let err = refused(Some(&path), tmp.path());
    assert!(err.contains("is not a socket"), "stderr: {err}");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "keep me");
}
