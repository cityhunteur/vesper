//! TD-017: vesper-proxy checks its entitlements file before it listens, as vesperd does.

use std::process::Command;
use std::time::{Duration, Instant};

fn proxy() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vesper-proxy"))
}

#[test]
fn an_entitlements_file_without_the_sandbox_is_refused_before_listening() {
    let dir = tempfile::tempdir().unwrap();
    let plist = dir.path().join("bad.entitlements");
    std::fs::write(
        &plist,
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict></dict></plist>",
    )
    .unwrap();
    let sock = dir.path().join("docker.sock");
    let out = proxy()
        .args([
            "--listen",
            sock.to_str().unwrap(),
            "--upstream",
            "unix:///nonexistent",
            "--entitlements-file",
            plist.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("com.apple.security.app-sandbox"), "{err}");
    assert!(
        !sock.exists(),
        "the proxy listened before it checked its entitlements"
    );
}

#[test]
fn the_shipped_entitlements_file_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("docker.sock");
    let mut child = proxy()
        .args([
            "--listen",
            sock.to_str().unwrap(),
            "--upstream",
            "unix:///nonexistent",
            "--entitlements-file",
            "packaging/macos/vesper-proxy.entitlements",
        ])
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !sock.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let listened = sock.exists();
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        listened,
        "vesper-proxy did not listen with its shipped entitlements"
    );
}
