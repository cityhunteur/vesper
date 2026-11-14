//! The share-network suite (GH #160 M2, ADR 0009): a shim microVM launched with `--share-net` gets a network
//! namespace of its own that holds `lo` and one tap, and nothing outside that microVM reaches an address in it.
//!
//! A test listener stands in for the share server (M3) on the server address. The microVM's own guest reaches it on
//! the NFS port and on no other port; the System VM's root namespace, a privileged container on the Docker bridge,
//! a privileged container on the host network, another microVM's namespace and another microVM's guest do not.
//! A launch killed after its namespace exists is left alone during its grace period and reaped after it.

use super::launchapi::first;
use super::super::Session;
use super::{connect, report, step, ControlEndpoint, SuiteReport, SuiteStep};
use std::thread;
use std::time::{Duration, Instant};

/// Steps a full run records.
const STEPS: usize = 10;
const SERVER: &str = "169.254.77.1";
/// The NFS port, which the namespace's INPUT rule accepts from the tap, and one it does not.
const NFS_PORT: &str = "2049";
const OTHER_PORT: &str = "2051";
/// Seconds each guest waits before it probes, so the listeners are up first.
const PROBE_DELAY: u64 = 10;
const PROBE_WAIT: Duration = Duration::from_secs(60);

/// `KEY=VALUE` fields of a helper line.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|w| w.strip_prefix(key)?.strip_prefix('='))
}

/// What a `SHARENET` line must say: `lo` and the tap only, the server address, the shim's jail uid as the tap's
/// owner, no default route, both policies DROP with the two port rules, and Firecracker inside the namespace.
fn check_sharenet(line: &str) -> Result<(), String> {
    let want = [
        ("links", "lo,share0"),
        ("addr", "169.254.77.1/30"),
        ("owner", "10000"),
        ("routes", "1"),
        ("default", "no"),
        ("input", "DROP"),
        ("forward", "DROP"),
        ("rules", "2"),
        ("fc_in_ns", "yes"),
    ];
    if !line.starts_with("SHARENET ") {
        return Err(format!("not a SHARENET line: {line}"));
    }
    for (key, value) in want {
        if field(line, key) != Some(value) {
            return Err(format!("{key} is {:?}, expected {value}: {line}", field(line, key)));
        }
    }
    Ok(())
}

/// The guest's probe results once it has printed `VESPER-PROBES-DONE`: `(probe, open)` pairs.
fn probes(control: &mut Session, id: &str) -> Result<Vec<(String, bool)>, String> {
    let deadline = Instant::now() + PROBE_WAIT;
    loop {
        let (_, log) = control.microvm(&["log", id, "200"])?;
        if log.iter().any(|l| l.contains("VESPER-PROBES-DONE")) {
            if !log.iter().any(|l| l.contains("VESPER-SHARE-NET-OK 169.254.77.2/30")) {
                return Err(format!("{id}: the guest did not configure its share interface"));
            }
            return Ok(log
                .iter()
                .filter_map(|l| l.split("VESPER-PROBE ").nth(1))
                .filter_map(|p| p.trim().rsplit_once(' ').map(|(probe, r)| (probe.to_string(), r == "open")))
                .collect());
        }
        if Instant::now() >= deadline {
            return Err(format!("{id}: no VESPER-PROBES-DONE within {} s", PROBE_WAIT.as_secs()));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn probe(results: &[(String, bool)], port: &str) -> Option<bool> {
    results.iter().find(|(p, _)| *p == format!("tcp:{SERVER}:{port}")).map(|(_, open)| *open)
}

pub fn run_sharenet_suite(endpoint: &ControlEndpoint, id: &str) -> SuiteReport {
    let mut steps: Vec<SuiteStep> = Vec::new();
    let [a, b, plain, aborted] = [format!("{id}-a"), format!("{id}-b"), format!("{id}-plain"), format!("{id}-aborted")];
    let control = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        Ok((session, "connected".to_string()))
    });
    let Some(mut control) = control else {
        return report(endpoint, steps, false);
    };
    for each in [&a, &b, &plain, &aborted] {
        let _ = control.microvm(&["cleanup", each, "--logs"]);
    }

    step(&mut steps, "namespace_holds_lo_and_one_tap", || {
        let probe = format!("{PROBE_DELAY}:tcp:{SERVER}:{NFS_PORT},tcp:{SERVER}:{OTHER_PORT}");
        let (rc, launched) = control.microvm(&["launch", &a, "--share-net", "--share-probe", &probe])?;
        if rc != 0 {
            return Err(format!("launch: {}", launched.join(" | ")));
        }
        let (_, status) = control.microvm(&["status", &a])?;
        if !first(&status).contains("sharenet=present") {
            return Err(format!("status: {}", first(&status)));
        }
        let (_, net) = control.microvm(&["sharenet", &a])?;
        check_sharenet(&first(&net))?;
        Ok(((), first(&net)))
    });

    step(&mut steps, "second_microvm_gets_its_own", || {
        let probe = format!("{PROBE_DELAY}:tcp:{SERVER}:{NFS_PORT}");
        let (rc, launched) = control.microvm(&["launch", &b, "--share-net", "--share-probe", &probe])?;
        if rc != 0 {
            return Err(format!("launch: {}", launched.join(" | ")));
        }
        let (_, net) = control.microvm(&["sharenet", &b])?;
        check_sharenet(&first(&net))?;
        Ok(((), format!("the same address pair in a second namespace: {}", first(&net))))
    });

    step(&mut steps, "nothing_outside_reaches_the_server_address", || {
        for port in [NFS_PORT, OTHER_PORT] {
            let (rc, l) = control.microvm(&["test-share-listen", &a, port])?;
            if rc != 0 {
                return Err(format!("listener on {port}: {}", first(&l)));
            }
        }
        let (rc, reach) = control.microvm(&["test-share-reach", NFS_PORT, &b])?;
        if rc != 0 {
            return Err(format!("test-share-reach: {}", reach.join(" | ")));
        }
        let from = ["root", &format!("namespace:{b}"), "container-bridge", "container-host"].map(String::from);
        for who in &from {
            if !reach.iter().any(|l| *l == format!("REACH {who} closed")) {
                return Err(format!("{who} is not closed: {}", reach.join(" | ")));
            }
        }
        Ok(((), format!("a listener on {SERVER}:{NFS_PORT} in {a}'s namespace: {}", reach.join("; "))))
    });

    step(&mut steps, "its_own_guest_reaches_the_nfs_port_only", || {
        let r = probes(&mut control, &a)?;
        match (probe(&r, NFS_PORT), probe(&r, OTHER_PORT)) {
            (Some(true), Some(false)) => Ok(((), format!("{a}'s guest: {NFS_PORT} open, {OTHER_PORT} closed by the INPUT rule (a listener is on both)"))),
            other => Err(format!("{a}'s guest: {other:?} from {r:?}")),
        }
    });

    step(&mut steps, "another_guest_does_not_reach_it", || {
        let r = probes(&mut control, &b)?;
        match probe(&r, NFS_PORT) {
            Some(false) => Ok(((), format!("{b}'s guest: {SERVER}:{NFS_PORT} closed (its own namespace has no listener)"))),
            other => Err(format!("{b}'s guest: {other:?} from {r:?}")),
        }
    });

    step(&mut steps, "a_plain_launch_has_no_network", || {
        let (rc, launched) = control.microvm(&["launch", &plain])?;
        if rc != 0 {
            let _ = control.microvm(&["cleanup", &plain, "--logs"]);
            return Err(format!("launch: {}", launched.join(" | ")));
        }
        let (_, status) = control.microvm(&["status", &plain])?;
        let (net_rc, net) = control.microvm(&["sharenet", &plain])?;
        let (_, clean) = control.microvm(&["cleanup", &plain, "--logs"])?;
        if first(&status).contains("sharenet=") || net_rc == 0 || first(&net) != format!("ABSENT {plain}") {
            return Err(format!("status: {}; sharenet: {}", first(&status), first(&net)));
        }
        Ok(((), format!("{}; {}; {}", first(&status), first(&net), first(&clean))))
    });

    step(&mut steps, "cleanup_removes_the_namespace", || {
        let mut detail = Vec::new();
        for each in [&a, &b] {
            let (rc, clean) = control.microvm(&["cleanup", each, "--logs"])?;
            let (net_rc, net) = control.microvm(&["sharenet", each])?;
            if rc != 0 || first(&clean) != format!("CLEAN {each}") || net_rc == 0 {
                return Err(format!("{each}: {}; sharenet: {}", clean.join(" | "), first(&net)));
            }
            detail.push(format!("{}, {}", first(&clean), first(&net)));
        }
        Ok(((), detail.join("; ")))
    });

    step(&mut steps, "aborted_launch_is_reaped_after_its_grace_period", || {
        let (rc, made) = control.microvm(&["test-abort-launch", &aborted])?;
        if rc != 0 || !first(&made).ends_with("namespace=bound") {
            return Err(format!("test-abort-launch: {}", made.join(" | ")));
        }
        let (_, early) = control.microvm(&["reap"])?;
        if early.iter().any(|l| l.starts_with(&format!("REAPED {aborted} "))) {
            return Err(format!("reaped inside the grace period: {}", early.join(" | ")));
        }
        let (_, aged) = control.microvm(&["test-age", &aborted, "60"])?;
        let (_, late) = control.microvm(&["reap"])?;
        let (net_rc, net) = control.microvm(&["sharenet", &aborted])?;
        if !late.iter().any(|l| *l == format!("REAPED {aborted} CLEAN {aborted}")) || net_rc == 0 {
            return Err(format!("reap after {}: {}; sharenet: {}", first(&aged), late.join(" | "), first(&net)));
        }
        Ok(((), format!("{}; left alone while recent; after 60 s: {}", first(&made), late.join(" | "))))
    });

    step(&mut steps, "nothing_left", || {
        let (rc, left) = control.ctr(&["leftovers"])?;
        if rc == 0 && first(&left) == "CLEAN" {
            Ok(((), "no Firecracker, jail, share namespace or record".to_string()))
        } else {
            Err(format!("leftovers: {}", left.join(" | ")))
        }
    });
    let passed = steps.iter().all(|s| s.ok) && steps.len() == STEPS;
    report(endpoint, steps, passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sharenet_line_is_checked_field_by_field() {
        let good = "SHARENET a links=lo,share0 addr=169.254.77.1/30 owner=10000 routes=1 default=no input=DROP forward=DROP rules=2 fc_in_ns=yes";
        check_sharenet(good).unwrap();
        for (from, to) in [("links=lo,share0", "links=eth0,lo,share0"), ("owner=10000", "owner=20000"), ("default=no", "default=yes"),
            ("input=DROP", "input=ACCEPT"), ("fc_in_ns=yes", "fc_in_ns=no")] {
            assert!(check_sharenet(&good.replace(from, to)).is_err(), "{to}");
        }
        assert!(check_sharenet("ABSENT a").is_err());
        let r = vec![("tcp:169.254.77.1:2049".to_string(), true), ("tcp:169.254.77.1:2051".to_string(), false)];
        assert_eq!((probe(&r, "2049"), probe(&r, "2051"), probe(&r, "80")), (Some(true), Some(false), None));
    }
}
