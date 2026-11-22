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
