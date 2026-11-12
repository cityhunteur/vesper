//! Host-run checklists over the control channel: GH #13 (channel), #14 (services), #18 (microVM
//! lifecycle) and #19 (rootfs pipeline). Each produces a `SuiteReport` with actionable step details.

use super::{connect, connect_with_retry, ControlEndpoint, Session};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

mod bundle;
mod exec;
pub use exec::run_exec_suite;
mod launchapi;
pub use launchapi::run_launchapi_suite;
mod restart;
mod vesperd;
pub use vesperd::{run_vesperd_suite, VesperdRun, VESPERD_IDLE_TIMEOUT, VESPERD_REQUEST_CEILING};
pub use bundle::run_bundle_suite;
pub use restart::run_restart_suite;

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
            session.handshake.protocol_version, session.handshake.hostname, session.handshake.connect_ms
        );
        Ok((session, detail))
    });
    let session = session.and_then(|mut session| {
        step(&mut steps, "keepalive", || {
            let pings = session.keepalive(keepalive_interval, keepalive_total)?;
            Ok((session, format!("{pings} pings over {} s", keepalive_total.as_secs())))
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
            Ok((fresh, format!("reconnected after a server-side drop in {attempts} attempt(s)")))
        })
    });
    session.and_then(|mut session| {
        step(&mut steps, "large_payload", || {
            let payload: Vec<u8> = (0..payload_bytes).map(|i| (i % 251) as u8).collect();
            let elapsed = session.echo(&payload)?;
            Ok(((), format!("{payload_bytes} bytes echoed intact in {} ms", elapsed.as_millis())))
        })
    });
    let passed = steps.len() == 4 && steps.iter().all(|s| s.ok);
    SuiteReport {
        bridge_path: endpoint.bridge_path.display().to_string(),
        port: endpoint.port,
        steps,
        passed,
    }
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
        let ok = health.get(format!("{service}_ok")).and_then(|v| v.as_bool()) == Some(true);
        let state = health["services"][service].as_str().unwrap_or("?").to_string();
        if !ok || state != "started" {
            return Err(format!("{service} not healthy: state {state}, health {health}"));
        }
        Ok(health)
    };
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed: false };
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
            Ok(((), format!("state started, {service}_ok true, health {health}")))
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
                    return Ok(((), format!("healthy again after {} ms (old pid {old_pid}); health {health}", started.elapsed().as_millis())));
                }
                if Instant::now() >= deadline {
                    return Err(format!("{service} did not recover within {} s", recovery_timeout.as_secs()));
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        step(&mut steps, &format!("{service}_logs_capture_startup_and_recovery"), || {
            // The whole log since boot: other suites on the same boot restart the daemons and push the boot-time
            // startup line out of a short tail.
            let lines = session.logs(service, 20_000)?;
            let startups = lines.iter().filter(|l| l.contains("starting containerd") || l.contains("Starting up")).count();
            let messages = session.logs("messages", 200)?;
            let respawns = messages.iter().filter(|l| l.contains("supervise-daemon") && l.contains(service)).count();
            if startups < 2 {
                return Err(format!("expected at least two startups in /var/log/{service}.log, found {startups}"));
            }
            Ok(((), format!("{startups} startup lines in {service}.log, {respawns} supervise-daemon lines for {service} in messages")))
        });
    }
    let passed = steps.iter().all(|s| s.ok) && steps.len() == 2 + services.len() * 4;
    SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed }
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
            return Err(format!("`vesper-microvm {}` rc={rc}: {}", args.join(" "), lines.join(" | ")));
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
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed: false };
    };
    let _ = microvm_step(&mut session, &mut steps, "cleanup_before", &["cleanup", id], "CLEAN");
    if steps.last().map(|s| !s.ok).unwrap_or(true) {
        steps.pop(); // a dirty pre-state is cleaned, not a failure
    }
    let prepared = microvm_step(&mut session, &mut steps, "prepare", &["prepare"], "PREPARED");
    if prepared.is_none() {
        return SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed: false };
    }
    if microvm_step(&mut session, &mut steps, "launch", &["launch", id], "LAUNCHED").is_some() {
        let isolation = microvm_step(&mut session, &mut steps, "isolation_output", &["isolation", id], "{");
        steps.pop();
        if let Some(lines) = isolation {
            step(&mut steps, "jailer_isolation", || {
                let json: serde_json::Value = serde_json::from_str(&lines[0]).map_err(|e| format!("isolation JSON: {e}: {}", lines[0]))?;
                let checks = ["uid_ok", "chroot_ok", "cgroup_ok", "seccomp_ok"];
                let failed: Vec<&str> = checks.iter().copied().filter(|k| json[k] != serde_json::Value::Bool(true)).collect();
                if !failed.is_empty() {
                    return Err(format!("isolation checks failed: {}; {}", failed.join(", "), lines[0]));
                }
                Ok(((), format!("uid {}, jail files {}, cgroup {}, seccomp mode {}", json["uid"], json["jail_files"], json["cgroup"], json["seccomp_mode"])))
            });
        } else {
            steps.push(SuiteStep { name: "jailer_isolation".into(), ok: false, detail: "no isolation output".into(), elapsed_ms: 0 });
        }
        microvm_step(&mut session, &mut steps, "stop", &["stop", id], "STOPPED");
        microvm_step(&mut session, &mut steps, "cleanup_after_stop", &["cleanup", id], "CLEAN");
        if microvm_step(&mut session, &mut steps, "relaunch", &["launch", id], "LAUNCHED").is_some() {
            microvm_step(&mut session, &mut steps, "abnormal_termination", &["crash", id], "CRASHED");
            microvm_step(&mut session, &mut steps, "cleanup_after_crash", &["cleanup", id], "CLEAN");
        }
        if microvm_step(&mut session, &mut steps, "guest_poweroff_launch", &["launch", id, "--exit"], "LAUNCHED").is_some() {
            step(&mut steps, "guest_poweroff_exits", || {
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    let (_, lines) = session.microvm(&["status", id])?;
                    let first = lines.first().cloned().unwrap_or_default();
                    if first.starts_with("EXITED") {
                        return Ok(((), "firecracker exited after the guest powered off".to_string()));
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("still {first} after 15 s"));
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            });
            microvm_step(&mut session, &mut steps, "cleanup_after_poweroff", &["cleanup", id], "CLEAN");
        }
    }
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 11;
    SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed }
}

fn rootfs_step(
    session: &mut Session,
    steps: &mut Vec<SuiteStep>,
    name: &str,
    args: &[&str],
    expect: &str,
) -> Option<Vec<String>> {
    step(steps, name, || {
        let (rc, lines) = session.rootfs(args)?;
        let first = lines.first().cloned().unwrap_or_default();
        if rc != 0 || !first.starts_with(expect) {
            return Err(format!("`vesper-rootfs {}` rc={rc}: {}", args.join(" "), lines.join(" | ")));
        }
        Ok((lines.clone(), first))
    })
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|p| p.strip_prefix(key))
}

/// Runs the GH #19 checklist: import a trivial image, build the rootfs twice (cached and uncached) with an
/// identical digest, check the embedded provenance, reject an unknown image, and boot the rootfs in a jailed
/// microVM.
pub fn run_rootfs_suite(endpoint: &ControlEndpoint, id: &str) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed: false };
    };
    let imported = rootfs_step(&mut session, &mut steps, "import_trivial_image", &["import-trivial"], "IMPORTED");
    let Some(imported) = imported else {
        return SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed: false };
    };
    let image_id = imported[0].split_whitespace().nth(1).unwrap_or("").to_string();
    let built = rootfs_step(&mut session, &mut steps, "build", &["build", &image_id, "--no-cache"], "BUILT");
    let (path, sha) = match &built {
        Some(lines) => (
            lines[0].split_whitespace().nth(1).unwrap_or("").to_string(),
            field(&lines[0], "sha256=").unwrap_or("").to_string(),
        ),
        None => (String::new(), String::new()),
    };
    if built.is_some() {
        step(&mut steps, "reproducible_rebuild", || {
            let (rc, lines) = session.rootfs(&["build", &image_id, "--no-cache"])?;
            let again = lines.first().cloned().unwrap_or_default();
            if rc != 0 || field(&again, "sha256=") != Some(sha.as_str()) {
                return Err(format!("second build differs: {again} (first sha256={sha})"));
            }
            Ok(((), format!("sha256={sha} twice from image {image_id}")))
        });
        step(&mut steps, "cache_hit", || {
            let (rc, lines) = session.rootfs(&["build", &image_id])?;
            let line = lines.first().cloned().unwrap_or_default();
            if rc != 0 || field(&line, "cached=") != Some("true") || field(&line, "sha256=") != Some(sha.as_str()) {
                return Err(format!("expected a cache hit with the same digest: {line}"));
            }
            Ok(((), line))
        });
        step(&mut steps, "provenance_embedded", || {
            let (rc, lines) = session.rootfs(&["provenance", &path])?;
            let text = lines.join("\n");
            if rc != 0 {
                return Err(format!("provenance failed: {text}"));
            }
            let json: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| format!("provenance JSON: {e}: {text}"))?;
            if json["source"]["image_id"].as_str() != Some(image_id.as_str()) {
                return Err(format!("source.image_id mismatch: {}", json["source"]["image_id"]));
            }
            if json["source"]["layers"].as_array().map(|l| l.is_empty()).unwrap_or(true) {
                return Err("source.layers missing".to_string());
            }
            if json["baseline"]["firecracker_revision"].as_str().map(|s| s.len()).unwrap_or(0) < 7 {
                return Err(format!("baseline revision missing: {}", json["baseline"]));
            }
            if json["output_sha256"].as_str() != Some(sha.as_str()) {
                return Err(format!("output_sha256 {} != {sha}", json["output_sha256"]));
            }
            Ok(((), format!("image {}, {} layer(s), firecracker {}, created {}", image_id, json["source"]["layers"].as_array().map(|l| l.len()).unwrap_or(0), json["baseline"]["firecracker_revision"], json["source"]["created"])))
        });
    }
    step(&mut steps, "invalid_image_rejected", || {
        let (rc, lines) = session.rootfs(&["build", "vesper/does-not-exist:nope"])?;
        let line = lines.first().cloned().unwrap_or_default();
        if rc == 0 || !line.starts_with("ERR image not found") {
            return Err(format!("expected a clear error, got rc={rc}: {line}"));
        }
        Ok(((), line))
    });
    if built.is_some() {
        let _ = microvm_step(&mut session, &mut steps, "cleanup_before", &["cleanup", id], "CLEAN");
        if microvm_step(&mut session, &mut steps, "launch_with_rootfs", &["launch", id, "--rootfs", &path], "LAUNCHED").is_some() {
            step(&mut steps, "rootfs_boots", || {
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    let (_, lines) = session.microvm(&["log", id, "200"])?;
                    let ok = lines.iter().find(|l| l.starts_with("VESPER-ROOTFS-OK"));
                    let exec_ok = lines.iter().any(|l| l.starts_with("VESPER-ROOTFS-EXEC-OK"));
                    if let (Some(ok), true) = (ok, exec_ok) {
                        if ok.contains(&image_id) {
                            return Ok(((), format!("{ok}; command executed from the rootfs")));
                        }
                        return Err(format!("provenance from the booted rootfs does not match: {ok}"));
                    }
                    if lines.iter().any(|l| l.contains("VESPER-ROOTFS-MOUNT-FAILED") || l.contains("VESPER-ROOTFS-EXEC-FAILED")) {
                        return Err(lines.iter().rev().take(12).cloned().collect::<Vec<_>>().join(" | "));
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("no rootfs marker within 30 s; tail: {}", lines.iter().rev().take(8).cloned().collect::<Vec<_>>().join(" | ")));
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            });
            microvm_step(&mut session, &mut steps, "cleanup_after_rootfs_boot", &["cleanup", id], "CLEAN");
        }
    }
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 10;
    SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed }
}

fn workload_step(
    session: &mut Session,
    steps: &mut Vec<SuiteStep>,
    name: &str,
    args: &[&str],
    expect: &str,
) -> Option<Vec<String>> {
    step(steps, name, || {
        let (rc, lines) = session.workload(args)?;
        let first = lines.first().cloned().unwrap_or_default();
        if rc != 0 || !first.starts_with(expect) {
            return Err(format!("`vesper-workload {}` rc={rc}: {}", args.join(" "), lines.join(" | ")));
        }
        Ok((lines.clone(), first))
    })
}

fn report(endpoint: &ControlEndpoint, steps: Vec<SuiteStep>, passed: bool) -> SuiteReport {
    SuiteReport { bridge_path: endpoint.bridge_path.display().to_string(), port: endpoint.port, steps, passed }
}

/// Runs the GH #20 checklist: three concurrent workloads each in its own jailed microVM (distinct pids
/// and cgroups, private rootfs copies), stop of one tears down only its microVM, a short workload's exit
/// code and output map back, and removal leaves nothing behind.
pub fn run_workload_suite(endpoint: &ControlEndpoint) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return report(endpoint, steps, false);
    };
    for name in ["wa", "wb", "wc", "wd"] {
        let _ = session.workload(&["rm", name]);
    }
    let image = match rootfs_step(&mut session, &mut steps, "import_trivial_image", &["import-trivial"], "IMPORTED") {
        Some(lines) => lines[0].split_whitespace().nth(1).unwrap_or("").to_string(),
        None => return report(endpoint, steps, false),
    };
    let a = workload_step(&mut session, &mut steps, "run_wa", &["run", "wa", &image, "--", "sh", "-c", "'echo wa-secret > /tmp/x; cat /tmp/x; sleep 300'"], "STARTED");
    let b = workload_step(&mut session, &mut steps, "run_wb_concurrent", &["run", "wb", &image, "--", "sh", "-c", "'cat /tmp/x 2>/dev/null || echo absent; sleep 300'"], "STARTED");
    let c = workload_step(&mut session, &mut steps, "run_wc_concurrent", &["run", "wc", &image, "--", "sh", "-c", "'echo wc-up; sleep 300'"], "STARTED");
    if a.is_some() && b.is_some() && c.is_some() {
        step(&mut steps, "three_microvms_running", || {
            let (_, lines) = session.workload(&["ps"])?;
            let running: Vec<&String> = lines.iter().filter(|l| l.contains(" RUNNING ")).collect();
            let pids: std::collections::HashSet<&str> = running.iter().filter_map(|l| field(l, "pid=")).collect();
            if running.len() < 3 || pids.len() < 3 {
                return Err(format!("expected 3 running workloads with distinct pids: {}", lines.join(" | ")));
            }
            Ok(((), format!("{} running, pids {:?}", running.len(), pids)))
        });
        step(&mut steps, "isolation_between_microvms", || {
            let mut cgroups = std::collections::HashSet::new();
            for name in ["wa", "wb", "wc"] {
                let (rc, lines) = session.workload(&["isolation", name])?;
                let json: serde_json::Value = serde_json::from_str(lines.first().map(String::as_str).unwrap_or("")).map_err(|e| format!("{name} isolation JSON: {e} (rc {rc}: {})", lines.join(" | ")))?;
                for key in ["uid_ok", "chroot_ok", "cgroup_ok", "seccomp_ok"] {
                    if json[key] != serde_json::Value::Bool(true) {
                        return Err(format!("{name}: {key} false: {}", lines[0]));
                    }
                }
                cgroups.insert(json["cgroup"].as_str().unwrap_or("").to_string());
            }
            let (_, wb_logs) = session.workload(&["logs", "wb"])?;
            if !wb_logs.iter().any(|l| l.trim() == "absent") {
                return Err(format!("wb could read wa's file or did not run: {}", wb_logs.join(" | ")));
            }
            if cgroups.len() != 3 {
                return Err(format!("cgroups not distinct: {cgroups:?}"));
            }
            Ok(((), format!("3 distinct cgroups {cgroups:?}; wb cannot see wa's /tmp/x (private rootfs copies)")))
        });
        workload_step(&mut session, &mut steps, "stop_wa_tears_down_its_microvm", &["stop", "wa"], "STOPPED");
        step(&mut steps, "others_unaffected_by_stop", || {
            let (_, lines) = session.workload(&["ps"])?;
            let wa = lines.iter().find(|l| l.starts_with("wa ")).cloned().unwrap_or_default();
            let others = lines.iter().filter(|l| (l.starts_with("wb ") || l.starts_with("wc ")) && l.contains(" RUNNING ")).count();
            if wa.contains(" RUNNING ") || others != 2 {
                return Err(format!("after stopping wa: {}", lines.join(" | ")));
            }
            let (rc, cleanup) = session.microvm(&["cleanup", "wl-wa"])?;
            if rc != 0 || !cleanup.first().map(|l| l.starts_with("CLEAN")).unwrap_or(false) {
                return Err(format!("wa microVM not clean: {}", cleanup.join(" | ")));
            }
            Ok(((), format!("wa {}, wb and wc still RUNNING, wl-wa CLEAN", wa.split_whitespace().nth(1).unwrap_or("?"))))
        });
    }
    if workload_step(&mut session, &mut steps, "run_wd_short", &["run", "wd", &image, "--", "sh", "-c", "'echo hello-from-microvm; exit 3'"], "STARTED").is_some() {
        step(&mut steps, "wait_maps_exit_code_and_output", || {
            let (rc, lines) = session.workload(&["wait", "wd", "60"])?;
            let first = lines.first().cloned().unwrap_or_default();
            if rc != 0 || first != "EXITED wd code=3" {
                return Err(format!("expected EXITED wd code=3, got rc={rc}: {}", lines.join(" | ")));
            }
            let (_, logs) = session.workload(&["logs", "wd"])?;
            if !logs.iter().any(|l| l.trim() == "hello-from-microvm") {
                return Err(format!("workload output missing: {}", logs.join(" | ")));
            }
            Ok(((), "EXITED wd code=3; output hello-from-microvm; microVM torn down".to_string()))
        });
    }
    for name in ["wa", "wb", "wc", "wd"] {
        workload_step(&mut session, &mut steps, &format!("rm_{name}"), &["rm", name], "REMOVED");
    }
    step(&mut steps, "nothing_left", || {
        let (_, lines) = session.workload(&["ps"])?;
        if !lines.is_empty() {
            return Err(format!("workloads remain: {}", lines.join(" | ")));
        }
        Ok(((), "no workloads, no microVMs".to_string()))
    });
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 14;
    report(endpoint, steps, passed)
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
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
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
                        summary = Some(format!("{a} {} {}", b.unwrap_or(""), c.unwrap_or("")).trim().to_string())
                    }
                    _ => {}
                }
            }
            let summary = summary.unwrap_or_else(|| format!("no summary line: {}", lines.join(" | ")));
            let ok = token(&summary, "failed") == Some("0") && token(&summary, "passed").is_some();
            steps.push(SuiteStep { name: "conformance_summary".into(), ok, detail: summary, elapsed_ms: started.elapsed().as_millis() as u64 });
        }
        Err(err) => steps.push(SuiteStep { name: "conformance_summary".into(), ok: false, detail: err, elapsed_ms: started.elapsed().as_millis() as u64 }),
    }
    let mut p50s = Vec::new();
    for run in 1..=2 {
        let cycles = perf_cycles.to_string();
        let name = format!("perf_run_{run}");
        let line = step(&mut steps, &name, || {
            let (rc, lines) = session.fcparity(&["perf", &cycles])?;
            let perf = lines.iter().find(|l| l.starts_with("PERF ")).cloned().unwrap_or_default();
            if rc != 0 || perf.is_empty() || token(&perf, "failures") != Some("0") {
                return Err(format!("perf rc={rc}: {}", lines.iter().rev().take(4).cloned().collect::<Vec<_>>().join(" | ")));
            }
            let detail = perf.trim_start_matches("PERF ").to_string();
            Ok((detail.clone(), detail))
        });
        if let Some(line) = line {
            p50s.push(token(&line, "boot_p50_ms").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0));
        }
    }
    step(&mut steps, "perf_comparable", || {
        if p50s.len() < 2 || p50s.iter().any(|v| *v <= 0.0) {
            return Err("fewer than two successful performance runs".to_string());
        }
        let (a, b) = (p50s[0], p50s[1]);
        let drift = (a - b).abs() * 100.0 / a.min(b);
        if drift > 20.0 {
            return Err(format!("boot p50 drift {drift:.1}% between runs ({a} ms vs {b} ms) exceeds 20%"));
        }
        Ok(((), format!("boot p50 {a} ms vs {b} ms, drift {drift:.1}% (limit 20%)")))
    });
    let passed = steps.iter().all(|s| s.ok) && steps.iter().any(|s| s.name.starts_with("conformance:"));
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
            return Err(format!("`vesper-ctr {}` rc={rc}: {}", args.join(" "), lines.join(" | ")));
        }
        Ok((lines.clone(), first))
    })
}

/// `RC <code>` then `OUT <line>` lines from `vesper-ctr run|try`.
fn rc_out(lines: &[String]) -> (i32, Vec<String>) {
    let rc = lines.first().and_then(|l| l.strip_prefix("RC ")).and_then(|v| v.trim().parse().ok()).unwrap_or(-1);
    let out = lines.iter().filter_map(|l| l.strip_prefix("OUT ").map(str::to_string)).collect();
    (rc, out)
}

/// Runs the GH #22 checklist through containerd's `ctr` with the Vesper shim: an end-to-end task
/// (create, start, wait, delete) with its exit code and output, state and kill of a detached task,
/// invalid RPC sequences, three concurrent tasks, and no leftovers.
pub fn run_shim_suite(endpoint: &ControlEndpoint) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return report(endpoint, steps, false);
    };
    for name in ["e2e", "det", "ca", "cb", "cc"] {
        let _ = session.ctr(&["try", "task", "kill", "-s", "KILL", name]);
        let _ = session.ctr(&["try", "task", "delete", "-f", name]);
        let _ = session.ctr(&["try", "container", "delete", name]);
    }
    if ctr_step(&mut session, &mut steps, "import_image", &["import"], "IMPORTED").is_none() {
        return report(endpoint, steps, false);
    }
    step(&mut steps, "e2e_run_exit_code_and_output", || {
        let (_, lines) = session.ctr(&["run", "e2e", "echo", "hello-from-task;", "exit", "3"])?;
        let (rc, out) = rc_out(&lines);
        if rc != 3 || !out.iter().any(|l| l == "hello-from-task") {
            return Err(format!("expected rc 3 with hello-from-task, got rc={rc}: {}", lines.join(" | ")));
        }
        Ok(((), format!("ctr run --rm exited {rc}; output {:?}", out)))
    });
    let started = ctr_step(&mut session, &mut steps, "start_detached", &["start", "det", "sleep", "300"], "STARTED");
    if started.is_some() {
        step(&mut steps, "state_running", || {
            let (_, lines) = session.ctr(&["state", "det"])?;
            let line = lines.first().cloned().unwrap_or_default();
            if !line.contains(" RUNNING ") || field(&line, "pid=").map(|p| p == "0").unwrap_or(true) {
                return Err(format!("expected RUNNING with a pid: {line}"));
            }
            Ok(((), line))
        });
        step(&mut steps, "task_metrics", || {
            let (rc, lines) = session.ctr(&["metrics", "det"])?;
            let line = lines.first().cloned().unwrap_or_default();
            let mem: u64 = field(&line, "memory_bytes=").and_then(|v| v.parse().ok()).unwrap_or(0);
            if rc != 0 || !line.starts_with("METRICS det") || mem == 0 {
                return Err(format!("expected cgroup metrics for the running microVM: {}", lines.join(" | ")));
            }
            Ok(((), format!("ctr task metrics -> {line}")))
        });
        step(&mut steps, "invalid_start_running", || {
            let (_, lines) = session.ctr(&["try", "task", "start", "det"])?;
            let (rc, out) = rc_out(&lines);
            if rc == 0 {
                return Err("starting a running task must fail".to_string());
            }
            Ok(((), format!("rc {rc}: {}", out.join(" "))))
        });
        step(&mut steps, "invalid_delete_running", || {
            let (_, lines) = session.ctr(&["try", "task", "delete", "det"])?;
            let (rc, out) = rc_out(&lines);
            if rc == 0 {
                return Err("deleting a running task must fail".to_string());
            }
            Ok(((), format!("rc {rc}: {}", out.join(" "))))
        });
        ctr_step(&mut session, &mut steps, "kill_detached", &["kill", "det"], "KILLED");
        ctr_step(&mut session, &mut steps, "delete_detached", &["delete", "det"], "DELETED");
    }
    step(&mut steps, "invalid_unknown_task", || {
        let (_, lines) = session.ctr(&["try", "task", "kill", "nope"])?;
        let (rc, out) = rc_out(&lines);
        if rc == 0 {
            return Err("killing an unknown task must fail".to_string());
        }
        Ok(((), format!("rc {rc}: {}", out.join(" "))))
    });
    let mut all = true;
    for name in ["ca", "cb", "cc"] {
        all &= ctr_step(&mut session, &mut steps, &format!("start_{name}"), &["start", name, "sleep", "300"], "STARTED").is_some();
    }
    if all {
        step(&mut steps, "three_tasks_running", || {
            let mut pids = std::collections::HashSet::new();
            for name in ["ca", "cb", "cc"] {
                let (_, lines) = session.ctr(&["state", name])?;
                let line = lines.first().cloned().unwrap_or_default();
                if !line.contains(" RUNNING ") {
                    return Err(format!("{name}: {line}"));
                }
                pids.insert(field(&line, "pid=").unwrap_or("0").to_string());
            }
            if pids.len() != 3 || pids.contains("0") {
                return Err(format!("pids not distinct: {pids:?}"));
            }
            Ok(((), format!("ca, cb, cc RUNNING with pids {pids:?}")))
        });
        for name in ["ca", "cb", "cc"] {
            ctr_step(&mut session, &mut steps, &format!("kill_{name}"), &["kill", name], "KILLED");
            ctr_step(&mut session, &mut steps, &format!("delete_{name}"), &["delete", name], "DELETED");
        }
    }
    ctr_step(&mut session, &mut steps, "no_leftovers", &["leftovers"], "CLEAN");
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 19;
    report(endpoint, steps, passed)
}

pub fn write_report(path: impl AsRef<Path>, report: &SuiteReport) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    let mut json = serde_json::to_string_pretty(report).map_err(|err| format!("serialize report: {err}"))?;
    json.push('\n');
    std::fs::write(path, json).map_err(|err| format!("write {}: {err}", path.display()))
}

