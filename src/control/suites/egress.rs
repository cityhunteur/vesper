//! The egress suite for `vesperd --network nat` (ADR 0008, TD-058): every verdict comes from inside a guest.
//!
//! Each microVM boots with `vesper_net=` and `vesper_probe=` on its kernel command line; the guest init configures
//! `eth0`, runs each probe once and prints `VESPER-PROBE <probe> open|closed`. The suite starts TCP and UDP
//! listeners on every System VM address (`VESPERD test-listen`), so a `closed` to the System VM means the policy
//! dropped it, not that nothing listened, and it compares both layers' drop counters (`VESPERD net`) before and after.
//! The System VM's own addresses come from the helper (`POLICY ... eth0= gw=`), not from this host's vmnet defaults.
//! Two microVMs run with their namespace layer opened (`VESPERD test-open-ns`): one after dockerd was killed and
//! respawned while it ran (no launch in between, so nothing reinstalled the root rules), to show the root `mangle`
//! layer holds alone; one with its masquerade and reverse-path check also removed, to show the root `raw` rule drops
//! sources outside the pool.

use super::launchapi::{first, wait_absent};
use super::vesperd::{signal, wait_exit, Client, BOOT_ARGS, TEARDOWN_TIMEOUT};
use super::super::Session;
use super::{connect, report, step, ControlEndpoint, SuiteReport, SuiteStep, VesperdRun};
use std::thread;
use std::time::{Duration, Instant};

/// The listeners' port in the System VM.
const LISTEN_PORT: &str = "7777";
/// A public address that answers TCP 443, used only when the System VM itself reaches it.
const PUBLIC: &str = "1.1.1.1:443";
/// How long the guest's probes may take (each waits up to 2 s).
const PROBE_WAIT: Duration = Duration::from_secs(90);
/// Steps a full run records.
const STEPS: usize = 10;

/// The System VM's own addresses, as the helper reports them.
#[derive(Debug, Clone, Default)]
struct SystemVm {
    eth0: String,
    gateway: String,
}

/// Probes that must be `closed` from any `nat` guest. `peer` is another lease's root and namespace veth addresses.
fn denied(sys: &SystemVm, peer: &[String]) -> Vec<String> {
    let mut out: Vec<String> = [
        format!("tcp:{}:{LISTEN_PORT}", sys.eth0), // the System VM's eth0
        format!("udp:{}:{LISTEN_PORT}", sys.eth0),
        format!("tcp:172.17.0.1:{LISTEN_PORT}"), // docker0
        format!("tcp:{}:80", sys.gateway),       // the macOS vmnet gateway, other than DNS
        format!("tcp:{}:22", sys.gateway),
        format!("tcp:198.18.0.1:{LISTEN_PORT}"), // the veth pool
        "tcp:10.0.0.1:443".to_string(),          // RFC 1918, CGNAT, link-local, metadata, multicast, reserved
        "tcp:172.16.0.1:443".to_string(),
        "tcp:100.64.0.1:443".to_string(),
        "tcp:169.254.169.254:80".to_string(),
        "tcp:224.0.0.1:80".to_string(),
        "tcp:240.0.0.1:80".to_string(),
        "tcp:127.0.0.1:80".to_string(),
        "spoof:9.9.9.9:1.1.1.1:443".to_string(), // a source outside the tap subnet, to a public address
    ]
    .to_vec();
    out.extend(peer.iter().map(|addr| format!("tcp:{addr}:{LISTEN_PORT}")));
    out
}

/// One guest's probe results: `(probe, open)`.
fn probe_results(lines: &[String]) -> Vec<(String, bool)> {
    lines
        .iter()
        .filter_map(|l| l.split("VESPER-PROBE ").nth(1))
        .filter_map(|rest| {
            let mut words = rest.split_whitespace();
            Some((words.next()?.to_string(), words.next()? == "open"))
        })
        .collect()
}

struct Lease {
    id: String,
    v: super::vesperd::Vesperd,
}

/// Starts `vesperd --network nat --tap tap0:172.16.0.1/30`, attaches eth0 and boots it with `probes`, unless `before`
/// (run after launch, before boot) fails.
fn boot(
    run: &VesperdRun,
    control: &mut Session,
    id: &str,
    probes: &[String],
    before: &mut dyn FnMut(&mut Session) -> Result<(), String>,
) -> Result<Lease, String> {
    let flags: Vec<String> = ["--network", "nat", "--tap", "tap0:172.16.0.1/30"].map(String::from).to_vec();
    let (v, _, _) = run.start_with(id, &format!("{id}.sock"), &flags)?;
    let lease = Lease { id: id.to_string(), v };
    before(control)?;
    let mut api = Client::connect(&lease.v.socket)?;
    let eth0 = r#"{"iface_id":"eth0","host_dev_name":"tap0","guest_mac":"06:00:ac:10:00:02"}"#;
    api.expect("PUT", "/network-interfaces/eth0", Some(eth0), 204)?;
    api.expect("PUT", "/machine-config", Some(r#"{"vcpu_count":1,"mem_size_mib":256}"#), 204)?;
    let args = format!("{BOOT_ARGS} vesper_net=172.16.0.2/30,172.16.0.1 vesper_probe={}", probes.join(","));
    let boot = format!(r#"{{"kernel_image_path":"/Image","initrd_path":"/initrd.gz","boot_args":"{args}"}}"#);
    api.expect("PUT", "/boot-source", Some(&boot), 204)?;
    api.expect("PUT", "/actions", Some(r#"{"action_type":"InstanceStart"}"#), 204)?;
    Ok(lease)
}

/// Waits for `VESPER-PROBES-DONE` and returns the results.
fn results(control: &mut Session, id: &str) -> Result<Vec<(String, bool)>, String> {
    let deadline = Instant::now() + PROBE_WAIT;
    loop {
        let (_, lines) = control.vesperd(&["log", id, "400"])?;
        if lines.iter().any(|l| l.contains("VESPER-NET-FAILED")) {
            return Err(format!("{id}: the guest could not configure eth0"));
        }
        if lines.iter().any(|l| l.contains("VESPER-PROBES-DONE")) {
            return Ok(probe_results(&lines));
        }
        if Instant::now() >= deadline {
            return Err(format!("{id}: no VESPER-PROBES-DONE within {} s", PROBE_WAIT.as_secs()));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// The `DROPS <id> ns=N root=N raw=N reply=N` counters (root, raw and reply count every lease together).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Drops {
    ns: u64,
    root: u64,
    raw: u64,
}

fn drops(control: &mut Session, id: &str) -> Result<Drops, String> {
    let (_, lines) = control.vesperd(&["net", id])?;
    let line = lines.iter().find(|l| l.starts_with("DROPS ")).ok_or_else(|| format!("net {id}: {}", lines.join(" | ")))?;
    let field = |k: &str| line.split_whitespace().find_map(|w| w.strip_prefix(k)).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    Ok(Drops { ns: field("ns="), root: field("root="), raw: field("raw=") })
}

/// A `POLICY nat` field of `VESPERD net <id>`.
fn policy_field(control: &mut Session, id: &str, key: &str) -> Result<String, String> {
    let (_, lines) = control.vesperd(&["net", id])?;
    let line = lines.iter().find(|l| l.starts_with("POLICY nat")).ok_or_else(|| format!("net {id}: {}", lines.join(" | ")))?;
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(key))
        .map(str::to_string)
        .ok_or_else(|| format!("net {id}: no {key} in {line}"))
}

/// The lease's veth addresses (root end, namespace end).
fn veth_addrs(control: &mut Session, id: &str) -> Result<Vec<String>, String> {
    Ok(vec![policy_field(control, id, "root=")?, policy_field(control, id, "ns=")?])
}

/// The System VM's eth0 address and gateway, from a running nat lease.
fn system_vm(control: &mut Session, id: &str) -> Result<SystemVm, String> {
    let eth0 = policy_field(control, id, "eth0=")?;
    let eth0 = eth0.split('/').next().unwrap_or_default().to_string();
    let gateway = policy_field(control, id, "gw=")?;
    if eth0.parse::<std::net::Ipv4Addr>().is_err() || gateway.parse::<std::net::Ipv4Addr>().is_err() {
        return Err(format!("System VM addresses eth0={eth0} gw={gateway}"));
    }
    Ok(SystemVm { eth0, gateway })
}

/// Waits until the agent's HEALTH reports dockerd up again.
fn docker_back(control: &mut Session, within: Duration) -> Result<Duration, String> {
    let start = Instant::now();
    thread::sleep(Duration::from_secs(2));
    loop {
        let health = control.health()?;
        if health["docker_ok"] == serde_json::Value::Bool(true) {
            return Ok(start.elapsed());
        }
        if start.elapsed() > within {
            return Err(format!("dockerd not back within {} s: {health}", within.as_secs()));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Judges one guest: every `deny` probe closed; `allow` probes open when the System VM reaches the internet.
fn judge(got: &[(String, bool)], deny: &[String], allow: &[String]) -> Result<String, String> {
    let state = |p: &str| got.iter().find(|(q, _)| q == p).map(|(_, open)| *open);
    let mut wrong = Vec::new();
    for p in deny {
        match state(p) {
            Some(false) => {}
            Some(true) => wrong.push(format!("{p} open")),
            None => wrong.push(format!("{p} not run")),
        }
    }
    for p in allow {
        match state(p) {
            Some(true) => {}
            Some(false) => wrong.push(format!("{p} closed")),
            None => wrong.push(format!("{p} not run")),
        }
    }
    if wrong.is_empty() {
        Ok(format!("{} denied probes closed; {} allowed probes open", deny.len(), allow.len()))
    } else {
        Err(wrong.join(", "))
    }
}

fn stop(control: &mut Session, lease: &mut Lease) -> Result<(), String> {
    signal(&lease.v, "TERM")?;
    let _ = wait_exit(&mut lease.v, TEARDOWN_TIMEOUT)?;
    wait_absent(control, &lease.id, Duration::from_secs(10)).map(|_| ())
}

fn no_veth_left(control: &mut Session) -> Result<(), String> {
    let (_, lines) = control.vesperd(&["list"])?;
    let left: Vec<&String> = lines.iter().filter(|l| l.starts_with("VETH ") || l.starts_with("LINK vpd")).collect();
    if left.is_empty() { Ok(()) } else { Err(format!("left: {left:?}")) }
}

pub fn run_egress_suite(endpoint: &ControlEndpoint, run: &VesperdRun, id: &str) -> SuiteReport {
    let mut steps: Vec<SuiteStep> = Vec::new();
    let [a, b, c, d] = [format!("{id}-a"), format!("{id}-b"), format!("{id}-root"), format!("{id}-none")];
    let control = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {}", session.handshake.protocol_version);
        Ok((session, detail))
    });
    let Some(mut control) = control else {
        return report(endpoint, steps, false);
    };
    let _ = std::fs::create_dir_all(&run.workdir);
    for each in [&a, &b, &c, &d] {
        let _ = control.vesperd(&["test-age", each, "3600"]);
    }
    let _ = control.vesperd(&["reap"]);
    let (ip, port) = PUBLIC.split_once(':').unwrap_or((PUBLIC, "443"));
    // The allowed probes need the internet; without it the suite cannot judge them, so it fails here.
    step(&mut steps, "listeners_and_internet", || {
        let (rc, l) = control.vesperd(&["test-listen", LISTEN_PORT])?;
        if rc != 0 {
            return Err(format!("test-listen: {}", l.join(" | ")));
        }
        let (_, reach) = control.vesperd(&["test-reach", ip, port])?;
        if !first(&reach).ends_with(" open") {
            return Err(format!("the System VM itself does not reach {PUBLIC} ({}); the allowed probes cannot be judged", first(&reach)));
        }
        Ok(((), format!("{}; System VM to {PUBLIC}: {}", first(&l), first(&reach))))
    });
    let mut sys = SystemVm::default();
    let allow = vec![format!("tcp:{PUBLIC}"), "dns:example.com".to_string()];

    // Two nat leases at once: distinct veths, each policed, neither reaching the other.
    let mut leases: Vec<Lease> = Vec::new();
    step(&mut steps, "nat_guest_reaches_public_only", || {
        let la = boot(run, &mut control, &a, &[], &mut |_| Ok(()))?;
        leases.push(la);
        let peer = veth_addrs(&mut control, &a)?;
        sys = system_vm(&mut control, &a)?;
        let deny = denied(&sys, &peer);
        let probes: Vec<String> = allow.iter().cloned().chain(deny.iter().cloned()).collect();
        let lb = boot(run, &mut control, &b, &probes, &mut |_| Ok(()))?;
        leases.push(lb);
        let got = results(&mut control, &b)?;
        let verdict = judge(&got, &deny, &allow)?;
        let ns1 = drops(&mut control, &b)?.ns;
        if ns1 == 0 {
            return Err(format!("{verdict}; but the namespace layer counted no drops"));
        }
        let pb = veth_addrs(&mut control, &b)?;
        if pb == peer {
            return Err(format!("both leases have the veth {peer:?}"));
        }
        Ok(((), format!("{verdict} (System VM {} via {}); namespace layer dropped {ns1}; {b} and {a} on distinct veths {pb:?} / {peer:?} and {b} reached neither of {a}'s", sys.eth0, sys.gateway)))
    });
    step(&mut steps, "isolation_reports_egress", || {
        let mut out = Vec::new();
        for l in &leases {
            let (_, iso) = control.vesperd(&["isolation", &l.id])?;
            let j = first(&iso);
            for want in [r#""netns_ok":true"#, r#""ipv6_off":true"#, r#""egress":"nat""#, r#""egress_ok":true"#] {
                if !j.contains(want) {
                    return Err(format!("{}: {j}", l.id));
                }
            }
            out.push(l.id.clone());
        }
        Ok(((), format!("{}: netns_ok, ipv6_off, egress nat, egress_ok", out.join(", "))))
    });
    step(&mut steps, "teardown_removes_veths", || {
        for l in leases.iter_mut() {
            stop(&mut control, l)?;
        }
        no_veth_left(&mut control)?;
        Ok(((), "SIGTERM: both leases ABSENT, no VETH record or vpd link left".to_string()))
    });
    leases.clear();

    // The root mangle layer alone: the lease is launched, then dockerd is killed and respawns (Docker rebuilds its
    // chains) with no launch in between, then the namespace layer is opened.
    step(&mut steps, "root_mangle_alone_after_docker_restart", || {
        let deny = denied(&sys, &[]);
        let probes: Vec<String> = allow.iter().cloned().chain(deny.iter().cloned()).collect();
        let mut note = String::new();
        let mut before = Drops::default();
        let mut lease = boot(run, &mut control, &c, &probes, &mut |s| {
            let pid = s.kill_service("docker")?;
            let back = docker_back(s, Duration::from_secs(60))?;
            let (rc, l) = s.vesperd(&["test-open-ns", &c, "filter"])?;
            if rc != 0 {
                return Err(format!("test-open-ns: {}", first(&l)));
            }
            before = drops(s, &c)?;
            note = format!("dockerd (pid {pid}) killed after LAUNCHAPI, healthy again after {} ms; {}", back.as_millis(), first(&l));
            Ok(())
        })?;
        let got = results(&mut control, &c);
        let after = drops(&mut control, &c);
        let _ = stop(&mut control, &mut lease);
        let verdict = judge(&got?, &deny, &allow)?;
        let after = after?;
        if after.ns != before.ns || after.root <= before.root {
            return Err(format!("{verdict}; drops {before:?} -> {after:?}: the root layer did not do the dropping"));
        }
        Ok(((), format!("{note}; {verdict}; root drops +{}, namespace +0", after.root - before.root)))
    });

    // The root raw layer: no namespace filter, masquerade or reverse-path check, so guest sources arrive unchanged.
    step(&mut steps, "root_raw_drops_sources_outside_the_pool", || {
        let deny = denied(&sys, &[]);
        let probes: Vec<String> = allow.iter().cloned().chain(deny.iter().cloned()).collect();
        let mut before = Drops::default();
        let mut opened = String::new();
        let mut lease = boot(run, &mut control, &c, &probes, &mut |s| {
            let (rc, l) = s.vesperd(&["test-open-ns", &c, "all"])?;
            opened = first(&l);
            before = drops(s, &c)?;
            if rc == 0 { Ok(()) } else { Err(format!("test-open-ns: {opened}")) }
        })?;
        let got = results(&mut control, &c);
        let after = drops(&mut control, &c);
        let _ = stop(&mut control, &mut lease);
        let every: Vec<String> = probes.clone();
        let verdict = judge(&got?, &every, &[])?;
        let after = after?;
        if after.raw <= before.raw {
            return Err(format!("{verdict}; raw drops {} -> {}", before.raw, after.raw));
        }
        Ok(((), format!("{opened}; every probe closed, the allowed ones too ({verdict}); raw drops +{}", after.raw - before.raw)))
    });

    step(&mut steps, "none_lease_has_no_uplink", || {
        let flags: Vec<String> = ["--tap", "tap0"].map(String::from).to_vec();
        let (mut v, _, _) = run.start_with(&d, &format!("{d}.sock"), &flags)?;
        let checked = (|| -> Result<String, String> {
            let (_, iso) = control.vesperd(&["isolation", &d])?;
            let (_, net) = control.vesperd(&["net", &d])?;
            if !first(&iso).contains(r#""egress":"none""#) || !net.iter().any(|l| l == "POLICY none") {
                return Err(format!("{} / {}", first(&iso), net.join(" | ")));
            }
            if net.iter().any(|l| l.starts_with("LINK up.")) {
                return Err(format!("uplink in a none namespace: {}", net.join(" | ")));
            }
            Ok("egress none; no uplink in its namespace".to_string())
        })();
        signal(&v, "TERM")?;
        let _ = wait_exit(&mut v, TEARDOWN_TIMEOUT);
        wait_absent(&mut control, &d, Duration::from_secs(10))?;
        Ok(((), checked?))
    });

    step(&mut steps, "reaper_removes_veth_of_killed_holder", || {
        let lease = boot(run, &mut control, &a, &[], &mut |_| Ok(()))?;
        let (rc, l) = control.vesperd(&["kill-holder", &a])?;
        if rc != 0 {
            return Err(format!("kill-holder: {}", l.join(" | ")));
        }
        drop(lease);
        let (_, reap) = control.vesperd(&["reap"])?;
        wait_absent(&mut control, &a, TEARDOWN_TIMEOUT)?;
        no_veth_left(&mut control)?;
        Ok(((), format!("{}; {}; no veth left", first(&l), reap.iter().find(|r| r.contains(&a)).cloned().unwrap_or_default())))
    });

    step(&mut steps, "listeners_stopped", || {
        let (_, l) = control.vesperd(&["test-listen", "stop"])?;
        Ok(((), first(&l)))
    });
    let passed = steps.iter().all(|s| s.ok) && steps.len() == STEPS;
    report(endpoint, steps, passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_lines_and_verdicts() {
        let lines = vec![
            "[ 1.0] VESPER-PROBE tcp:1.1.1.1:443 open".to_string(),
            "VESPER-PROBE tcp:10.0.0.1:443 closed".to_string(),
            "VESPER-PROBES-DONE".to_string(),
        ];
        let got = probe_results(&lines);
        assert_eq!(got, vec![("tcp:1.1.1.1:443".to_string(), true), ("tcp:10.0.0.1:443".to_string(), false)]);
        let allow = vec!["tcp:1.1.1.1:443".to_string()];
        assert!(judge(&got, &["tcp:10.0.0.1:443".to_string()], &allow).is_ok());
        assert!(judge(&got, &["tcp:1.1.1.1:443".to_string()], &[]).unwrap_err().contains("open"));
        assert!(judge(&got, &["tcp:9.9.9.9:1".to_string()], &[]).unwrap_err().contains("not run"));
        let closed = vec![("tcp:1.1.1.1:443".to_string(), false)];
        assert!(judge(&closed, &[], &allow).unwrap_err().contains("closed"));
        let sys = SystemVm { eth0: "192.168.65.3".into(), gateway: "192.168.65.1".into() };
        let d = denied(&sys, &["198.18.0.5".to_string()]);
        assert!(d.iter().any(|p| p == "tcp:198.18.0.5:7777") && d.iter().any(|p| p == "tcp:192.168.65.3:7777"));
        assert!(d.iter().any(|p| p == "tcp:192.168.65.1:80") && !d.iter().any(|p| p.contains("192.168.64.")));
    }
}
