//! The vsock handlers in the System VM serve the macOS host only (ADR 0010): `vsock_host_only` in
//! `systemvm/overlay/usr/local/lib/vesper/vsock-peer.sh`, as `vesper-port-relay-conn` uses it. Hermetic: the handler
//! is run with the environment socat gives it and an input that makes it stop before it would connect anywhere.

use std::io::Write;
use std::process::{Command, Stdio};

/// Runs the port-relay handler with `env`, feeding it a line that is not a port; returns (exit code, stdout).
fn handler(env: &[(&str, &str)]) -> (i32, String) {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/systemvm/overlay/usr/local/bin/vesper-port-relay-conn");
    let mut cmd = Command::new("sh");
    cmd.arg(script).env_remove("SOCAT_PEERADDR").env_remove("SOCAT_PID");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("sh");
    child.stdin.take().expect("stdin").write_all(b"PORT not-a-port\n").expect("write");
    let out = child.wait_with_output().expect("wait");
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn a_peer_that_is_not_the_host_is_turned_away() {
    for peer in ["1", "3", "42", "", "2 ", "02"] {
        let (rc, out) = handler(&[("SOCAT_PID", "7"), ("SOCAT_PEERADDR", peer)]);
        assert_eq!(rc, 1, "peer {peer:?}");
        assert!(out.contains("ERR this endpoint serves the macOS host only"), "peer {peer:?}: {out}");
    }
}

#[test]
fn a_socat_handler_without_a_peer_address_is_turned_away() {
    let (rc, out) = handler(&[("SOCAT_PID", "7")]);
    assert_eq!(rc, 1);
    assert!(out.contains("ERR this endpoint serves the macOS host only"), "{out}");
}

#[test]
fn the_host_and_a_handler_run_by_hand_get_past_the_check() {
    // Past the check the handler reads its line and exits 1 on a bad port, without the refusal message.
    for env in [vec![("SOCAT_PID", "7"), ("SOCAT_PEERADDR", "2")], vec![]] {
        let (rc, out) = handler(&env);
        assert_eq!(rc, 1, "{env:?}");
        assert!(!out.contains("ERR this endpoint"), "{env:?}: {out}");
    }
}
