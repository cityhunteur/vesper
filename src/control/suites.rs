//! Host-run checklists over the control channel: GH #13 (channel), #14 (services), #18 (microVM
//! lifecycle) and #19 (rootfs pipeline). Each produces a `SuiteReport` with actionable step details.

use super::{ControlEndpoint, Session, connect, connect_with_retry};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

mod bounds;
pub use bounds::run_bounds_suite;
mod bundle;
mod egress;
pub use egress::run_egress_suite;
mod exec;
pub use exec::run_exec_suite;
mod launchapi;
pub use launchapi::run_launchapi_suite;
mod replay;
mod restart;
pub use replay::{ReplayResponse, ReplayRun, ReplaySession, ReplayStep, fixture_names};
mod sharenet;
pub use sharenet::run_sharenet_suite;
mod snapshot;
mod snapshot_bounds;
pub use snapshot::{
    DEFAULT_CEILING_MIB, MEASURE_FILL_MIB, MEASURE_GUEST_MIB, SnapshotRun, prepare_workdir,
    run_snapshot_suite,
};
mod rootfs;
pub use rootfs::run_rootfs_suite;
mod shim;
pub use shim::run_shim_suite;
mod vesperd;
mod workload;
pub use bundle::run_bundle_suite;
pub use restart::run_restart_suite;
pub use vesperd::{VESPERD_IDLE_TIMEOUT, VESPERD_REQUEST_CEILING, VesperdRun, run_vesperd_suite};
pub use workload::run_workload_suite;

/// Result of one checklist step of the GH #13 control suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteStep {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteReport {
    pub bridge_path: String,
    pub port: u32,
    pub steps: Vec<SuiteStep>,
    pub passed: bool,
}

fn step<T>(
    steps: &mut Vec<SuiteStep>,
    name: &str,
    run: impl FnOnce() -> Result<(T, String), String>,
) -> Option<T> {
    let started = Instant::now();
    match run() {
        Ok((value, detail)) => {
            steps.push(SuiteStep {
                name: name.to_string(),
                ok: true,
                detail,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
            Some(value)
        }
        Err(err) => {
            steps.push(SuiteStep {
                name: name.to_string(),
                ok: false,
                detail: err,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
            None
        }
    }
}

/// Runs the GH #13 checklist: handshake, keepalive under idle, reconnect after a drop, >1 MB echo.
pub fn run_suite(
    endpoint: &ControlEndpoint,
    keepalive_interval: Duration,
    keepalive_total: Duration,
    payload_bytes: usize,
) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!(
            "protocol {} host {} in {} ms",
            session.handshake.protocol_version,
            session.handshake.hostname,
            session.handshake.connect_ms
        );
        Ok((session, detail))
    });
    let session = session.and_then(|mut session| {
        step(&mut steps, "keepalive", || {
            let pings = session.keepalive(keepalive_interval, keepalive_total)?;
            Ok((
                session,
                format!("{pings} pings over {} s", keepalive_total.as_secs()),
            ))
        })
    });
    let session = session.and_then(|mut session| {
        step(&mut steps, "reconnect", || {
            session.drop_connection()?;
            let dropped = session.ping().is_err();
            if !dropped {
                return Err("connection survived DROP".to_string());
            }
            let (fresh, attempts) = connect_with_retry(endpoint, 5, Duration::from_millis(500))?;
            Ok((
                fresh,
                format!("reconnected after a server-side drop in {attempts} attempt(s)"),
            ))
        })
    });
    let session = session.and_then(|mut session| {
        step(&mut steps, "large_payload", || {
            let payload: Vec<u8> = (0..payload_bytes).map(|i| (i % 251) as u8).collect();
            let elapsed = session.echo(&payload)?;
            Ok((
                session,
                format!(
                    "{payload_bytes} bytes echoed intact in {} ms",
                    elapsed.as_millis()
                ),
            ))
        })
    });
    session.and_then(|mut session| {
        step(&mut steps, "control_slots_reserved", || {
            control_slots_reserved(endpoint, &mut session)
        })
    });
    let passed = steps.len() == 5 && steps.iter().all(|s| s.ok);
    SuiteReport {
        bridge_path: endpoint.bridge_path.display().to_string(),
        port: endpoint.port,
        steps,
        passed,
    }
}

/// TD-026: with the agent's handlers filled to the lease cap by idle connections, `LAUNCHAPI` is refused as full and
/// a control command on one more connection is still answered. The probe's id is not valid, so an accepted request
/// fails at once instead of starting a microVM. Only a run against the real agent proves the agent: `cargo test`'s
/// fake bridge has its own copy of the rule.
fn control_slots_reserved(
    endpoint: &ControlEndpoint,
    session: &mut Session,
) -> Result<((), String), String> {
    let health = session.health()?;
    let open = health["handlers"]
        .as_u64()
        .ok_or("HEALTH has no handlers count")? as usize;
    let max = health["handlers_max"]
        .as_u64()
        .ok_or("HEALTH has no handlers_max")? as usize;
    let cap = max
        .checked_sub(8)
        .ok_or("handlers_max is under the reserve")?;
    let mut held = Vec::new();
    while open + held.len() < cap {
        held.push(connect(endpoint)?);
    }
    let mut lease = connect(endpoint)?;
    let refused = lease.request_line("LAUNCHAPI bad/id")?;
    let mut control = connect(endpoint)?;
    let answered = control.health()?;
    let seen = answered["handlers"].as_u64().unwrap_or(0);
    drop(held);
    if !refused.starts_with("ERR full:") {
        return Err(format!(
            "LAUNCHAPI at the cap ({cap} of {max}) was not refused as full: {refused:?}"
        ));
    }
    if (seen as usize) < cap + 1 {
        return Err(format!(
            "HEALTH counted {seen} handlers with at least {} open",
            cap + 1
        ));
    }
    Ok((
        (),
        format!(
            "{open} open, {} more held: LAUNCHAPI refused ({refused}); HEALTH answered with {seen} handlers open",
            cap - open
        ),
    ))
}

/// Runs the GH #14 checklist per service: healthy at boot, SIGKILL, supervised restart within
/// `recovery_timeout`, and log lines that record the startup and the recovery.
pub fn run_services_suite(
    endpoint: &ControlEndpoint,
    services: &[&str],
    recovery_timeout: Duration,
) -> SuiteReport {
    let mut steps = Vec::new();
    let healthy = |session: &mut Session, service: &str| -> Result<serde_json::Value, String> {
        let health = session.health()?;
        let ok = health
            .get(format!("{service}_ok"))
            .and_then(|v| v.as_bool())
            == Some(true);
        let state = health["services"][service]
            .as_str()
            .unwrap_or("?")
            .to_string();
        if !ok || state != "started" {
            return Err(format!(
                "{service} not healthy: state {state}, health {health}"
            ));
        }
        Ok(health)
    };
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!(
            "protocol {} host {}",
            session.handshake.protocol_version, session.handshake.hostname
        );
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return SuiteReport {
            bridge_path: endpoint.bridge_path.display().to_string(),
            port: endpoint.port,
            steps,
            passed: false,
        };
    };
    // TAP devices for microVM networks (TD-032) and the upstream suite (GH #173) need the tun module at boot.
    step(&mut steps, "tun_loaded_at_boot", || {
        let health = session.health()?;
        match health.get("tun").and_then(|v| v.as_bool()) {
            Some(true) => Ok(((), "/dev/net/tun exists after boot".to_string())),
            _ => Err(format!("/dev/net/tun missing after boot; health {health}")),
        }
    });
    for service in services {
        let service = *service;
        let before = step(&mut steps, &format!("{service}_healthy_at_boot"), || {
            let health = healthy(&mut session, service)?;
            Ok((
                (),
                format!("state started, {service}_ok true, health {health}"),
            ))
        });
        if before.is_none() {
            continue;
        }
        let killed = step(&mut steps, &format!("{service}_simulated_crash"), || {
            let pid = session.kill_service(service)?;
            Ok((pid, format!("SIGKILL sent to pid {pid}")))
        });
        let Some(old_pid) = killed else { continue };
        step(&mut steps, &format!("{service}_recovers"), || {
            let deadline = Instant::now() + recovery_timeout;
            let started = Instant::now();
            loop {
                if let Ok(health) = healthy(&mut session, service) {
                    return Ok((
                        (),
                        format!(
                            "healthy again after {} ms (old pid {old_pid}); health {health}",
                            started.elapsed().as_millis()
                        ),
                    ));
                }
                if Instant::now() >= deadline {
                    return Err(format!(
                        "{service} did not recover within {} s",
                        recovery_timeout.as_secs()
                    ));
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        step(
            &mut steps,
            &format!("{service}_logs_capture_startup_and_recovery"),
            || {
                // The whole log since boot: other suites on the same boot restart the daemons and push the boot-time
                // startup line out of a short tail.
                let lines = session.logs(service, 20_000)?;
                let startups = lines
                    .iter()
                    .filter(|l| l.contains("starting containerd") || l.contains("Starting up"))
                    .count();
                let messages = session.logs("messages", 200)?;
                let respawns = messages
                    .iter()
                    .filter(|l| l.contains("supervise-daemon") && l.contains(service))
                    .count();
                if startups < 2 {
                    return Err(format!(
                        "expected at least two startups in /var/log/{service}.log, found {startups}"
                    ));
                }
                Ok((
                    (),
                    format!(
                        "{startups} startup lines in {service}.log, {respawns} supervise-daemon lines for {service} in messages"
                    ),
                ))
            },
        );
    }
    let passed = steps.iter().all(|s| s.ok) && steps.len() == 2 + services.len() * 4;
    SuiteReport {
        bridge_path: endpoint.bridge_path.display().to_string(),
        port: endpoint.port,
        steps,
        passed,
    }
}

fn microvm_step(
    session: &mut Session,
    steps: &mut Vec<SuiteStep>,
    name: &str,
    args: &[&str],
    expect: &str,
) -> Option<Vec<String>> {
    step(steps, name, || {
        let (rc, lines) = session.microvm(args)?;
        let first = lines.first().cloned().unwrap_or_default();
        if rc != 0 || !first.starts_with(expect) {
            return Err(format!(
                "`vesper-microvm {}` rc={rc}: {}",
                args.join(" "),
                lines.join(" | ")
            ));
        }
        Ok((lines.clone(), first))
    })
}

/// Runs the GH #18 checklist: launch under the jailer, isolation (uid, chroot, cgroup, seccomp), clean
/// stop and cleanup, cleanup after SIGKILL, and a guest-initiated power-off.
pub fn run_microvm_suite(endpoint: &ControlEndpoint, id: &str) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!(
            "protocol {} host {}",
            session.handshake.protocol_version, session.handshake.hostname
        );
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return SuiteReport {
            bridge_path: endpoint.bridge_path.display().to_string(),
            port: endpoint.port,
            steps,
            passed: false,
        };
    };
    let _ = microvm_step(
        &mut session,
        &mut steps,
        "cleanup_before",
        &["cleanup", id],
        "CLEAN",
    );
    if steps.last().map(|s| !s.ok).unwrap_or(true) {
        steps.pop(); // a dirty pre-state is cleaned, not a failure
    }
    let prepared = microvm_step(
        &mut session,
        &mut steps,
        "prepare",
        &["prepare"],
        "PREPARED",
    );
    if prepared.is_none() {
        return SuiteReport {
            bridge_path: endpoint.bridge_path.display().to_string(),
            port: endpoint.port,
            steps,
            passed: false,
        };
    }
    if microvm_step(
        &mut session,
        &mut steps,
        "launch",
        &["launch", id],
        "LAUNCHED",
    )
    .is_some()
    {
        let isolation = microvm_step(
            &mut session,
            &mut steps,
            "isolation_output",
            &["isolation", id],
            "{",
        );
        steps.pop();
        if let Some(lines) = isolation {
            step(&mut steps, "jailer_isolation", || {
                let json: serde_json::Value = serde_json::from_str(&lines[0])
                    .map_err(|e| format!("isolation JSON: {e}: {}", lines[0]))?;
                let checks = ["uid_ok", "chroot_ok", "cgroup_ok", "seccomp_ok"];
                let failed: Vec<&str> = checks
                    .iter()
                    .copied()
                    .filter(|k| json[k] != serde_json::Value::Bool(true))
                    .collect();
                if !failed.is_empty() {
                    return Err(format!(
                        "isolation checks failed: {}; {}",
                        failed.join(", "),
                        lines[0]
                    ));
                }
                Ok((
                    (),
                    format!(
                        "uid {}, jail files {}, cgroup {}, seccomp mode {}",
                        json["uid"], json["jail_files"], json["cgroup"], json["seccomp_mode"]
                    ),
                ))
            });
        } else {
            steps.push(SuiteStep {
                name: "jailer_isolation".into(),
                ok: false,
                detail: "no isolation output".into(),
                elapsed_ms: 0,
            });
        }
        microvm_step(&mut session, &mut steps, "stop", &["stop", id], "STOPPED");
        microvm_step(
            &mut session,
            &mut steps,
            "cleanup_after_stop",
            &["cleanup", id],
            "CLEAN",
        );
        if microvm_step(
            &mut session,
            &mut steps,
            "relaunch",
            &["launch", id],
            "LAUNCHED",
        )
        .is_some()
        {
            microvm_step(
                &mut session,
                &mut steps,
                "abnormal_termination",
                &["crash", id],
                "CRASHED",
            );
            microvm_step(
                &mut session,
                &mut steps,
                "cleanup_after_crash",
                &["cleanup", id],
                "CLEAN",
            );
        }
        if microvm_step(
            &mut session,
            &mut steps,
            "guest_poweroff_launch",
            &["launch", id, "--exit"],
            "LAUNCHED",
        )
        .is_some()
        {
            step(&mut steps, "guest_poweroff_exits", || {
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    let (_, lines) = session.microvm(&["status", id])?;
                    let first = lines.first().cloned().unwrap_or_default();
                    if first.starts_with("EXITED") {
                        return Ok((
                            (),
                            "firecracker exited after the guest powered off".to_string(),
                        ));
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("still {first} after 15 s"));
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            });
            microvm_step(
                &mut session,
                &mut steps,
                "cleanup_after_poweroff",
                &["cleanup", id],
                "CLEAN",
            );
        }
    }
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 11;
    SuiteReport {
        bridge_path: endpoint.bridge_path.display().to_string(),
        port: endpoint.port,
        steps,
        passed,
    }
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|p| p.strip_prefix(key))
}

fn report(endpoint: &ControlEndpoint, steps: Vec<SuiteStep>, passed: bool) -> SuiteReport {
    SuiteReport {
        bridge_path: endpoint.bridge_path.display().to_string(),
        port: endpoint.port,
        steps,
        passed,
    }
}

fn token<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    field(line, &format!("{key}="))
}

/// Runs the GH #21 baseline: the API conformance checklist of the stock Firecracker inside the System VM
/// (one `conformance:<check>` step per `CHECK` line plus `conformance_summary`) and two performance runs
/// of `perf_cycles` launch/exit/cleanup cycles each (`perf_run_1`, `perf_run_2`, then `perf_comparable`:
/// both runs without failures and boot p50 within 20 %).
pub fn run_parity_suite(endpoint: &ControlEndpoint, id: &str, perf_cycles: u32) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!(
            "protocol {} host {}",
            session.handshake.protocol_version, session.handshake.hostname
        );
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return report(endpoint, steps, false);
    };
    let started = Instant::now();
    match session.fcparity(&["conformance", id]) {
        Ok((_, lines)) => {
            let mut summary = None;
            for line in &lines {
                let mut parts = line.splitn(4, ' ');
                match (parts.next(), parts.next(), parts.next(), parts.next()) {
                    (Some("CHECK"), Some(name), Some(verdict), detail) => steps.push(SuiteStep {
                        name: format!("conformance:{name}"),
                        ok: verdict == "ok",
                        detail: detail.unwrap_or("").to_string(),
                        elapsed_ms: 0,
                    }),
                    (Some("CONFORMANCE"), Some(a), b, c) => {
                        summary = Some(
                            format!("{a} {} {}", b.unwrap_or(""), c.unwrap_or(""))
                                .trim()
                                .to_string(),
                        )
                    }
                    _ => {}
                }
            }
            let summary =
                summary.unwrap_or_else(|| format!("no summary line: {}", lines.join(" | ")));
            let ok = token(&summary, "failed") == Some("0") && token(&summary, "passed").is_some();
            steps.push(SuiteStep {
                name: "conformance_summary".into(),
                ok,
                detail: summary,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
        Err(err) => steps.push(SuiteStep {
            name: "conformance_summary".into(),
            ok: false,
            detail: err,
            elapsed_ms: started.elapsed().as_millis() as u64,
        }),
    }
    let mut p50s = Vec::new();
    for run in 1..=2 {
        let cycles = perf_cycles.to_string();
        let name = format!("perf_run_{run}");
        let line = step(&mut steps, &name, || {
            let (rc, lines) = session.fcparity(&["perf", &cycles])?;
            let perf = lines
                .iter()
                .find(|l| l.starts_with("PERF "))
                .cloned()
                .unwrap_or_default();
            if rc != 0 || perf.is_empty() || token(&perf, "failures") != Some("0") {
                return Err(format!(
                    "perf rc={rc}: {}",
                    lines
                        .iter()
                        .rev()
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" | ")
                ));
            }
            let detail = perf.trim_start_matches("PERF ").to_string();
            Ok((detail.clone(), detail))
        });
        if let Some(line) = line {
            p50s.push(
                token(&line, "boot_p50_ms")
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0),
            );
        }
    }
    step(&mut steps, "perf_comparable", || {
        if p50s.len() < 2 || p50s.iter().any(|v| *v <= 0.0) {
            return Err("fewer than two successful performance runs".to_string());
        }
        let (a, b) = (p50s[0], p50s[1]);
        let drift = (a - b).abs() * 100.0 / a.min(b);
        if drift > 20.0 {
            return Err(format!(
                "boot p50 drift {drift:.1}% between runs ({a} ms vs {b} ms) exceeds 20%"
            ));
        }
        Ok((
            (),
            format!("boot p50 {a} ms vs {b} ms, drift {drift:.1}% (limit 20%)"),
        ))
    });
    let passed =
        steps.iter().all(|s| s.ok) && steps.iter().any(|s| s.name.starts_with("conformance:"));
    report(endpoint, steps, passed)
}

fn ctr_step(
    session: &mut Session,
    steps: &mut Vec<SuiteStep>,
    name: &str,
    args: &[&str],
    expect: &str,
) -> Option<Vec<String>> {
    step(steps, name, || {
        let (rc, lines) = session.ctr(args)?;
        let first = lines.first().cloned().unwrap_or_default();
        if rc != 0 || !first.starts_with(expect) {
            return Err(format!(
                "`vesper-ctr {}` rc={rc}: {}",
                args.join(" "),
                lines.join(" | ")
            ));
        }
        Ok((lines.clone(), first))
    })
}

/// `RC <code>` then `OUT <line>` lines from `vesper-ctr run|try`.
fn rc_out(lines: &[String]) -> (i32, Vec<String>) {
    let rc = lines
        .first()
        .and_then(|l| l.strip_prefix("RC "))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(-1);
    let out = lines
        .iter()
        .filter_map(|l| l.strip_prefix("OUT ").map(str::to_string))
        .collect();
    (rc, out)
}

pub fn write_report(path: impl AsRef<Path>, report: &SuiteReport) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    let mut json =
        serde_json::to_string_pretty(report).map_err(|err| format!("serialize report: {err}"))?;
    json.push('\n');
    std::fs::write(path, json).map_err(|err| format!("write {}: {err}", path.display()))
}
