//! GH #20: workloads in microVMs (concurrent runs, isolation, stop teardown, exit mapping).
use super::rootfs::rootfs_step;
use super::*;

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
            return Err(format!(
                "`vesper-workload {}` rc={rc}: {}",
                args.join(" "),
                lines.join(" | ")
            ));
        }
        Ok((lines.clone(), first))
    })
}

/// Runs the GH #20 checklist: three concurrent workloads each in its own jailed microVM (distinct pids
/// and cgroups, private rootfs copies), stop of one tears down only its microVM, a short workload's exit
/// code and output map back, and removal leaves nothing behind.
pub fn run_workload_suite(endpoint: &ControlEndpoint) -> SuiteReport {
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
    for name in ["wa", "wb", "wc", "wd"] {
        let _ = session.workload(&["rm", name]);
    }
    let image = match rootfs_step(
        &mut session,
        &mut steps,
        "import_trivial_image",
        &["import-trivial"],
        "IMPORTED",
    ) {
        Some(lines) => lines[0].split_whitespace().nth(1).unwrap_or("").to_string(),
        None => return report(endpoint, steps, false),
    };
    let a = workload_step(
        &mut session,
        &mut steps,
        "run_wa",
        &[
            "run",
            "wa",
            &image,
            "--",
            "sh",
            "-c",
            "'echo wa-secret > /tmp/x; cat /tmp/x; sleep 300'",
        ],
        "STARTED",
    );
    let b = workload_step(
        &mut session,
        &mut steps,
        "run_wb_concurrent",
        &[
            "run",
            "wb",
            &image,
            "--",
            "sh",
            "-c",
            "'cat /tmp/x 2>/dev/null || echo absent; sleep 300'",
        ],
        "STARTED",
    );
    let c = workload_step(
        &mut session,
        &mut steps,
        "run_wc_concurrent",
        &[
            "run",
            "wc",
            &image,
            "--",
            "sh",
            "-c",
            "'echo wc-up; sleep 300'",
        ],
        "STARTED",
    );
    if a.is_some() && b.is_some() && c.is_some() {
        step(&mut steps, "three_microvms_running", || {
            let (_, lines) = session.workload(&["ps"])?;
            let running: Vec<&String> = lines.iter().filter(|l| l.contains(" RUNNING ")).collect();
            let pids: std::collections::HashSet<&str> =
                running.iter().filter_map(|l| field(l, "pid=")).collect();
            if running.len() < 3 || pids.len() < 3 {
                return Err(format!(
                    "expected 3 running workloads with distinct pids: {}",
                    lines.join(" | ")
                ));
            }
            Ok(((), format!("{} running, pids {:?}", running.len(), pids)))
        });
        step(&mut steps, "isolation_between_microvms", || {
            let mut cgroups = std::collections::HashSet::new();
            for name in ["wa", "wb", "wc"] {
                let (rc, lines) = session.workload(&["isolation", name])?;
                let json: serde_json::Value = serde_json::from_str(
                    lines.first().map(String::as_str).unwrap_or(""),
                )
                .map_err(|e| {
                    format!(
                        "{name} isolation JSON: {e} (rc {rc}: {})",
                        lines.join(" | ")
                    )
                })?;
                for key in ["uid_ok", "chroot_ok", "cgroup_ok", "seccomp_ok"] {
                    if json[key] != serde_json::Value::Bool(true) {
                        return Err(format!("{name}: {key} false: {}", lines[0]));
                    }
                }
                cgroups.insert(json["cgroup"].as_str().unwrap_or("").to_string());
            }
            let (_, wb_logs) = session.workload(&["logs", "wb"])?;
            if !wb_logs.iter().any(|l| l.trim() == "absent") {
                return Err(format!(
                    "wb could read wa's file or did not run: {}",
                    wb_logs.join(" | ")
                ));
            }
            if cgroups.len() != 3 {
                return Err(format!("cgroups not distinct: {cgroups:?}"));
            }
            Ok((
                (),
                format!(
                    "3 distinct cgroups {cgroups:?}; wb cannot see wa's /tmp/x (private rootfs copies)"
                ),
            ))
        });
        workload_step(
            &mut session,
            &mut steps,
            "stop_wa_tears_down_its_microvm",
            &["stop", "wa"],
            "STOPPED",
        );
        step(&mut steps, "others_unaffected_by_stop", || {
            let (_, lines) = session.workload(&["ps"])?;
            let wa = lines
                .iter()
                .find(|l| l.starts_with("wa "))
                .cloned()
                .unwrap_or_default();
            let others = lines
                .iter()
                .filter(|l| {
                    (l.starts_with("wb ") || l.starts_with("wc ")) && l.contains(" RUNNING ")
                })
                .count();
            if wa.contains(" RUNNING ") || others != 2 {
                return Err(format!("after stopping wa: {}", lines.join(" | ")));
            }
            let (rc, cleanup) = session.microvm(&["cleanup", "wl-wa"])?;
            if rc != 0
                || !cleanup
                    .first()
                    .map(|l| l.starts_with("CLEAN"))
                    .unwrap_or(false)
            {
                return Err(format!("wa microVM not clean: {}", cleanup.join(" | ")));
            }
            Ok((
                (),
                format!(
                    "wa {}, wb and wc still RUNNING, wl-wa CLEAN",
                    wa.split_whitespace().nth(1).unwrap_or("?")
                ),
            ))
        });
    }
    if workload_step(
        &mut session,
        &mut steps,
        "run_wd_short",
        &[
            "run",
            "wd",
            &image,
            "--",
            "sh",
            "-c",
            "'echo hello-from-microvm; exit 3'",
        ],
        "STARTED",
    )
    .is_some()
    {
        step(&mut steps, "wait_maps_exit_code_and_output", || {
            let (rc, lines) = session.workload(&["wait", "wd", "60"])?;
            let first = lines.first().cloned().unwrap_or_default();
            if rc != 0 || first != "EXITED wd code=3" {
                return Err(format!(
                    "expected EXITED wd code=3, got rc={rc}: {}",
                    lines.join(" | ")
                ));
            }
            let (_, logs) = session.workload(&["logs", "wd"])?;
            if !logs.iter().any(|l| l.trim() == "hello-from-microvm") {
                return Err(format!("workload output missing: {}", logs.join(" | ")));
            }
            Ok((
                (),
                "EXITED wd code=3; output hello-from-microvm; microVM torn down".to_string(),
            ))
        });
    }
    for name in ["wa", "wb", "wc", "wd"] {
        workload_step(
            &mut session,
            &mut steps,
            &format!("rm_{name}"),
            &["rm", name],
            "REMOVED",
        );
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
