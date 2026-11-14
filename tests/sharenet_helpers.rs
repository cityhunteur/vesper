//! The share-network helpers (GH #160 M2, ADR 0009) as far as they run without root: both managers find the shared
//! namespace file from the source tree, and the namespace code has one home.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn overlay(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("systemvm/overlay").join(rel)
}

/// Each script sources `../lib/vesper/netns.sh` beside its own directory before it reads its command, so an unknown
/// command reaching the usage line shows the file was found.
#[test]
fn both_managers_source_the_shared_namespace_helpers() {
    for (script, usage) in [("usr/local/bin/vesper-microvm", "sharenet|reap"), ("usr/local/bin/vesper-vesperd", "usage")] {
        let out = Command::new("sh").arg(overlay(script)).arg("no-such-command").output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{script}: {text}");
        assert!(text.contains(usage) && !text.contains("netns.sh"), "{script}: {text}");
    }
}

/// The namespace is made in one place: neither manager calls `unshare --net` or `tunctl` itself, and the reaper's
/// runlevel link and the service exist.
#[test]
fn the_namespace_code_has_one_home() {
    let lib = fs::read_to_string(overlay("usr/local/lib/vesper/netns.sh")).unwrap();
    for f in ["vnetns_mounted()", "vnetns_down()", "vnetns_up()"] {
        assert!(lib.contains(f), "{f}");
    }
    for script in ["usr/local/bin/vesper-microvm", "usr/local/bin/vesper-vesperd"] {
        let text = fs::read_to_string(overlay(script)).unwrap();
        assert!(!text.contains("unshare --net") && !text.contains("tunctl "), "{script} makes a namespace itself");
        assert!(text.contains("netns.sh"), "{script}");
    }
    assert!(overlay("etc/init.d/vesper-microvm-reaper").is_file());
    let link = fs::read_link(overlay("etc/runlevels/default/vesper-microvm-reaper")).unwrap();
    assert_eq!(link, PathBuf::from("/etc/init.d/vesper-microvm-reaper"));
}
