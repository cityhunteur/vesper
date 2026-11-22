//! GH #22: the `ctr` task lifecycle through the Vesper shim v2.
use super::*;

/// Runs the GH #22 checklist through containerd's `ctr` with the Vesper shim: an end-to-end task
/// (create, start, wait, delete) with its exit code and output, state and kill of a detached task,
/// invalid RPC sequences, three concurrent tasks, and no leftovers.
pub fn run_shim_suite(endpoint: &ControlEndpoint) -> SuiteReport {
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
    for name in ["e2e", "det", "ca", "cb", "cc"] {
        let _ = session.ctr(&["try", "task", "kill", "-s", "KILL", name]);
        let _ = session.ctr(&["try", "task", "delete", "-f", name]);
        let _ = session.ctr(&["try", "container", "delete", name]);
    }
    if ctr_step(
        &mut session,
        &mut steps,
        "import_image",
        &["import"],
        "IMPORTED",
    )
    .is_none()
    {
        return report(endpoint, steps, false);
    }
    step(&mut steps, "e2e_run_exit_code_and_output", || {
        let (_, lines) = session.ctr(&["run", "e2e", "echo", "hello-from-task;", "exit", "3"])?;
        let (rc, out) = rc_out(&lines);
        if rc != 3 || !out.iter().any(|l| l == "hello-from-task") {
            return Err(format!(
                "expected rc 3 with hello-from-task, got rc={rc}: {}",
                lines.join(" | ")
            ));
        }
        Ok(((), format!("ctr run --rm exited {rc}; output {:?}", out)))
    });
    let started = ctr_step(
        &mut session,
        &mut steps,
        "start_detached",
        &["start", "det", "sleep", "300"],
        "STARTED",
    );
    if started.is_some() {
        step(&mut steps, "state_running", || {
            let (_, lines) = session.ctr(&["state", "det"])?;
            let line = lines.first().cloned().unwrap_or_default();
            if !line.contains(" RUNNING ") || field(&line, "pid=").map(|p| p == "0").unwrap_or(true)
            {
                return Err(format!("expected RUNNING with a pid: {line}"));
            }
            Ok(((), line))
        });
        step(&mut steps, "task_metrics", || {
            let (rc, lines) = session.ctr(&["metrics", "det"])?;
            let line = lines.first().cloned().unwrap_or_default();
            let mem: u64 = field(&line, "memory_bytes=")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if rc != 0 || !line.starts_with("METRICS det") || mem == 0 {
                return Err(format!(
                    "expected cgroup metrics for the running microVM: {}",
                    lines.join(" | ")
                ));
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
        ctr_step(
            &mut session,
            &mut steps,
            "kill_detached",
            &["kill", "det"],
            "KILLED",
        );
        ctr_step(
            &mut session,
            &mut steps,
            "delete_detached",
            &["delete", "det"],
            "DELETED",
        );
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
        all &= ctr_step(
            &mut session,
            &mut steps,
            &format!("start_{name}"),
            &["start", name, "sleep", "300"],
            "STARTED",
        )
        .is_some();
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
            ctr_step(
                &mut session,
                &mut steps,
                &format!("kill_{name}"),
                &["kill", name],
                "KILLED",
            );
            ctr_step(
                &mut session,
                &mut steps,
                &format!("delete_{name}"),
                &["delete", name],
                "DELETED",
            );
        }
    }
    ctr_step(
        &mut session,
        &mut steps,
        "no_leftovers",
        &["leftovers"],
        "CLEAN",
    );
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 19;
    report(endpoint, steps, passed)
}
