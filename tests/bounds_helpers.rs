//! The W4 bound helpers run hermetically under `sh` (TD-053, TD-055, TD-056): the console-log bounder, the shared
//! memory admission ledger and the snapshot store's default quota. The System VM suites (`make systemvm-bounds-suite`)
//! run them for real; these catch regressions in CI.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn helper(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("systemvm/overlay/usr/local/bin").join(name)
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vesper-bounds-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A line reaches the log while the writer is still open: no stage may buffer it (a `fold` stage once held the last
/// few KB, so launch never saw VESPER-MICROVM-OK).
#[test]
fn the_bounder_writes_each_line_at_once() {
    let d = scratch("flush");
    let log = d.join("c.log");
    let mut child = Command::new("sh").arg(helper("vesper-boundlog")).arg(&log).arg("1048576").stdin(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"boot\nVESPER-MICROVM-OK\n").unwrap();
    stdin.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let seen = loop {
        if fs::read_to_string(&log).unwrap_or_default().contains("VESPER-MICROVM-OK") {
            break true;
        }
        if Instant::now() > deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(stdin);
    child.wait().unwrap();
    assert!(seen, "the marker did not reach the log while the writer was open");
}

/// More than both files hold: the log rotates, each file stays within its bound (plus one line), and a stream with
/// no newline is split at 4096 bytes.
#[test]
fn the_bounder_rotates_and_splits() {
    let d = scratch("rotate");
    let log = d.join("c.log");
    let line = "x".repeat(99) + "\n";
    let mut input = line.repeat(1200).into_bytes(); // 120,000 bytes against a 50,000-byte bound
    input.extend(std::iter::repeat_n(b'y', 10_000));
    let mut child = Command::new("sh").arg(helper("vesper-boundlog")).arg(&log).arg("50000").stdin(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    assert!(child.wait().unwrap().success());
    let cur = fs::read(&log).unwrap();
    let prev = fs::read(log.with_extension("log.1")).unwrap();
    assert!(cur.len() <= 50_000 + 4097 && prev.len() <= 50_000 + 4097 && !prev.is_empty(), "{} {}", cur.len(), prev.len());
    let tail: Vec<usize> = String::from_utf8_lossy(&cur).lines().rev().take(3).map(str::len).collect();
    assert_eq!(tail, vec![10_000 - 2 * 4096, 4096, 4096], "a newline-free stream splits at 4096");
}

struct Ledger {
    root: PathBuf,
}

impl Ledger {
    fn new(name: &str, mem_total_kib: u64) -> Ledger {
        let root = scratch(name);
        fs::create_dir_all(root.join("proc")).unwrap();
        fs::write(root.join("proc/meminfo"), format!("MemTotal:       {mem_total_kib} kB\n")).unwrap();
        Ledger { root }
    }
    fn run(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new("sh").arg(helper("vesper-memadmit")).args(args).env("VESPER_MEMADMIT_ROOT", &self.root).output().unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
    fn committed(&self) -> (u64, u64) {
        let (_, s) = self.run(&["status"]);
        let f = |k: &str| s.split_whitespace().find_map(|w| w.strip_prefix(k)).unwrap().parse().unwrap();
        (f("committed="), f("entries="))
    }
    fn jail(&self, id: &str) -> PathBuf {
        self.root.join("srv/jailer/firecracker").join(id)
    }
    fn age(&self, owner: &str, id: &str) {
        let e = self.root.join("run/vesper/mem").join(format!("{owner}.{id}"));
        assert!(Command::new("touch").args(["-d", "2000-01-01 00:00:00"]).arg(&e).status().unwrap().success());
    }
}

const MIB: u64 = 1 << 20;

/// Acquire, refuse, release and stale expiry, a refused acquire that leaves the live entry alone, and a launch in
/// progress for the same id. `flock` is Linux's; CI runs this on Linux.
#[cfg(target_os = "linux")]
#[test]
fn the_ledger_admits_refuses_releases_and_never_loses_a_live_entry() {
    // 2 GiB, less the 256 MiB reserve: room for 1792 MiB.
    let l = Ledger::new("ledger", 2 * 1024 * 1024);
    let b = (384 * MIB).to_string();
    assert!(l.run(&["acquire", "shim", "a", &b]).1.starts_with("ADMIT shim a"));
    fs::create_dir_all(l.jail("a")).unwrap();
    for id in ["b", "c", "d"] {
        assert_eq!(l.run(&["acquire", "shim", id, &b]).0, 0, "{id}");
        fs::create_dir_all(l.jail(id)).unwrap();
    }
    assert_eq!(l.committed(), (4 * 384 * MIB, 4));
    let (rc, out) = l.run(&["acquire", "shim", "e", &b]);
    assert!(rc == 1 && out.starts_with("ERR memory:"), "{out}");

    // A duplicate id while the first launch may be in progress is refused, and the live entry stays counted.
    let (rc, out) = l.run(&["acquire", "shim", "a", &(999 * MIB * 1024).to_string()]);
    assert!(rc == 1 && out.starts_with("ERR busy"), "{out}");
    assert_eq!(l.committed(), (4 * 384 * MIB, 4));
    // After the pending window, a relaunch of the same (dead) id replaces its entry rather than adding to it.
    l.age("shim", "a");
    assert_eq!(l.run(&["acquire", "shim", "a", &b]).0, 0);
    assert_eq!(l.committed(), (4 * 384 * MIB, 4));

    // Release frees it; an entry whose jail and cgroup are gone and whose pending window passed is dropped.
    l.run(&["release", "shim", "b"]);
    assert_eq!(l.committed(), (3 * 384 * MIB, 3));
    fs::remove_dir_all(l.jail("c")).unwrap();
    l.age("shim", "c");
    assert_eq!(l.committed(), (2 * 384 * MIB, 2));
    // The owners are separate: a vesperd entry with the same id is its own.
    assert_eq!(l.run(&["acquire", "vesperd", "a", &(640 * MIB).to_string()]).0, 0);
    assert_eq!(l.committed(), (2 * 384 * MIB + 640 * MIB, 3));
}

/// The store's default quota is 25% of the data disk in 64-bit arithmetic: BusyBox awk's printf "%d" once clamped it
/// to 2 GiB. A fake `df` reports a 64 GiB disk.
#[test]
fn the_default_store_quota_is_a_quarter_of_a_large_disk() {
    let d = scratch("quota");
    let bin = d.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let df = bin.join("df");
    fs::write(&df, "#!/bin/sh\necho 'Filesystem 1K-blocks Used Available Use% Mounted on'\necho '/dev/vdb 67108864 0 67108864 0% /data'\n").unwrap();
    make_executable(&df);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = Command::new("sh").arg(helper("vesper-vesperd")).args(["test-store-quota", "clear"]).env("PATH", path).output().unwrap();
    let out = String::from_utf8_lossy(&out.stdout);
    let want = 67_108_864u64 * 1024 / 100 * 25;
    assert!(out.contains(&format!("QUOTA bytes={want} ")), "{out}");
}

fn make_executable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
}
