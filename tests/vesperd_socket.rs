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
    // The model: the default backend is the System VM's (D-063), and these tests need no System VM.
    cmd.args(["--backend", "model"]);
    if let Some(path) = socket {
        cmd.arg("--socket").arg(path);
    }
    cmd
}

/// Held while a test starts a `vesperd`, and while a test has a listening socket that must be gone before the
/// next start. On macOS a new socket is marked close-on-exec a moment after it is made, so a `vesperd` that
/// another test starts in that moment inherits it: the "stale" socket of `a_stale_socket_is_replaced` then had a
/// listener in another process, a connect to it worked, and its own `vesperd` refused to start (about one run in
/// six on a Mac when the tests ran together).
static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn spawn_lock() -> std::sync::MutexGuard<'static, ()> {
    SPAWN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(socket: Option<&Path>, runtime_dir: &Path, expect: &Path) -> Running {
    let child = {
        let _spawning = spawn_lock();
        vesperd(socket, runtime_dir).spawn().expect("spawn vesperd")
    };
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
    let child = {
        let _spawning = spawn_lock();
        vesperd(socket, runtime_dir).spawn().expect("spawn vesperd")
    };
    let out = child.wait_with_output().expect("run vesperd");
    assert!(
        !out.status.success(),
        "vesperd should have refused to start"
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
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
    assert!(
        std::os::unix::net::UnixStream::connect(&socket).is_ok(),
        "the first server must keep its socket"
    );
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
    {
        let _no_spawn_meanwhile = spawn_lock();
        drop(std::os::unix::net::UnixListener::bind(&socket).expect("bind"));
    }
    assert!(socket.exists(), "a dropped listener leaves its socket file");
    let _server = start(Some(&socket), tmp.path(), &socket);
    assert_eq!(mode(&socket), 0o600);
}

/// A socket that takes a connect is owner-only already: `vesperd` binds it, sets the mode, and listens only then
/// (a socket that listened from its bind took connects for a moment with the umask's mode). The test connects as
/// fast as it can and reads the mode at the first connect that works.
#[test]
fn a_socket_that_takes_a_connect_is_owner_only() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let socket = tmp.path().join("early.sock");
    let mut server = {
        let _spawning = spawn_lock();
        Running(
            vesperd(Some(&socket), tmp.path())
                .spawn()
                .expect("spawn vesperd"),
        )
    };
    let started = Instant::now();
    let mut seen = None;
    while started.elapsed() < Duration::from_secs(20) && seen.is_none() {
        // No sleep: the first connect that works is the earliest look this test can take.
        if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            seen = Some(mode(&socket));
        } else if let Ok(Some(status)) = server.0.try_wait() {
            panic!("vesperd ended before it listened: {status}");
        }
    }
    assert_eq!(
        seen,
        Some(0o600),
        "the mode of the socket at the first connect that worked"
    );
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

/// TD-007: each request writes one structured line to stderr, with an id, the method, the path and the status.
#[test]
fn each_request_writes_a_log_line() {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("rt");
    std::fs::create_dir_all(&runtime).unwrap();
    let socket = dir.path().join("api.sock");
    let mut running = start(Some(&socket), &runtime, &socket);
    let mut conn = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    conn.write_all(b"GET /version HTTP/1.1\r\nHost: v\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut reply = String::new();
    let _ = conn.read_to_string(&mut reply);
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    let _ = running.0.kill();
    let _ = running.0.wait();
    let mut err = String::new();
    running
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    let line = err
        .lines()
        .find(|l| l.contains(" request "))
        .unwrap_or_else(|| panic!("no request line in: {err}"));
    for want in ["id=1", "method=GET", "path=/version", "status=200", "ms="] {
        assert!(line.contains(want), "{want} missing from {line}");
    }
}

/// TD-019: a chunked body (no Content-Length) that passes 2 MiB while it streams gets a 413 envelope. This also
/// catches a change in the wording of the body library's limit error, which `handle_all` matches.
#[test]
fn a_chunked_body_over_the_limit_gets_a_413_envelope() {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("rt");
    std::fs::create_dir_all(&runtime).unwrap();
    let socket = dir.path().join("api.sock");
    let _running = start(Some(&socket), &runtime, &socket);
    let mut conn = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    conn.write_all(b"PUT /mmds HTTP/1.1\r\nHost: v\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
    let chunk = vec![b' '; 64 * 1024];
    for _ in 0..40 {
        // 40 x 64 KiB = 2.5 MiB; the server may answer and close before the last chunk.
        if conn
            .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
            .is_err()
            || conn.write_all(&chunk).is_err()
            || conn.write_all(b"\r\n").is_err()
        {
            break;
        }
    }
    let _ = conn.write_all(b"0\r\n\r\n");
    let mut reply = String::new();
    let _ = conn.read_to_string(&mut reply);
    assert!(reply.starts_with("HTTP/1.1 413"), "{reply}");
    assert!(reply.contains("\"PayloadTooLarge\""), "{reply}");
}

/// TD-024: a request blocked on a host path (a FIFO with no writer) does not keep vesperd from stopping: after
/// SIGTERM it exits within its 5 s grace.
#[test]
fn a_blocked_request_does_not_stop_the_shutdown() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("rt");
    std::fs::create_dir_all(&runtime).unwrap();
    let socket = dir.path().join("api.sock");
    let fifo = dir.path().join("kernel.fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let mut running = start(Some(&socket), &runtime, &socket);
    let mut conn = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    let body = format!("{{\"kernel_image_path\":\"{}\"}}", fifo.display());
    conn.write_all(format!("PUT /boot-source HTTP/1.1\r\nHost: v\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        Command::new("kill")
            .args(["-TERM", &running.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let stopped = Instant::now();
    let exited = loop {
        if let Some(status) = running.0.try_wait().unwrap() {
            break Some(status);
        }
        if stopped.elapsed() > Duration::from_secs(15) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    // A vesperd that did not exit is killed when `running` is dropped.
    assert!(
        exited.is_some(),
        "vesperd did not exit within 15 s of SIGTERM with a blocked request"
    );
}
