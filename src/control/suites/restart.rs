//! GH #25 checklist: the shim stays authoritative across a containerd restart, a dead shim leaves no
//! microVM behind, in-flight `ctr run` fails cleanly, and the durable task record tracks the phases.

use super::{connect, ctr_step, field, rc_out, report, step, ControlEndpoint, SuiteReport};
use std::thread;
use std::time::{Duration, Instant};

fn wait_containerd(session: &mut super::Session, timeout: Duration) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(h) = session.health() {
            if h["containerd_ok"] == serde_json::Value::Bool(true) && h["services"]["containerd"] == "started" {
                return Ok("containerd respawned and healthy".to_string());
            }
        }
        if Instant::now() >= deadline {
            return Err("containerd did not come back within the timeout".to_string());
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Runs the GH #25 checklist through `ctr` with the Vesper shim.
pub fn run_restart_suite(endpoint: &ControlEndpoint) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return report(endpoint, steps, false);
    };
    for name in ["rs1", "rs2", "rs3"] {
        let _ = session.ctr(&["try", "task", "kill", "-s", "KILL", name]);
        let _ = session.ctr(&["try", "task", "delete", "-f", name]);
        let _ = session.ctr(&["try", "container", "delete", name]);
    }
    if ctr_step(&mut session, &mut steps, "import_image", &["import"], "IMPORTED").is_none() {
        return report(endpoint, steps, false);
    }
    // 1. containerd restart with a running task.
    if let Some(lines) = ctr_step(&mut session, &mut steps, "start_rs1", &["start", "rs1", "sleep", "300"], "STARTED") {
        let pid_before = field(&lines[0], "pid=").unwrap_or("").to_string();
        step(&mut steps, "state_file_running", || {
            let (rc, lines) = session.ctr(&["state-file", "rs1"])?;
            let line = lines.first().cloned().unwrap_or_default();
            if rc != 0 || !line.contains("\"phase\":\"running\"") || !line.contains(&format!("\"pid\":{pid_before}")) {
                return Err(format!("expected a running record with pid {pid_before}: {line}"));
            }
            Ok(((), "vesper-task.json: phase running, VMM pid recorded".to_string()))
        });
        step(&mut steps, "containerd_killed_and_respawned", || {
            let pid = session.kill_service("containerd")?;
            let detail = wait_containerd(&mut session, Duration::from_secs(60))?;
            Ok(((), format!("containerd pid {pid} SIGKILLed; {detail}")))
        });
        step(&mut steps, "task_survives_restart", || {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let (_, lines) = session.ctr(&["state", "rs1"])?;
                let line = lines.first().cloned().unwrap_or_default();
                if line.contains(" RUNNING ") && field(&line, "pid=") == Some(pid_before.as_str()) {
                    return Ok(((), format!("after restart: {line} (same VMM pid)")));
                }
                if Instant::now() >= deadline {
                    return Err(format!("task not reconnected after restart: {line}"));
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        ctr_step(&mut session, &mut steps, "kill_after_restart", &["kill", "rs1"], "KILLED");
        ctr_step(&mut session, &mut steps, "delete_after_restart", &["delete", "rs1"], "DELETED");
    }
    // 2. shim crash: containerd runs the shim's delete, which cleans the microVM.
    if ctr_step(&mut session, &mut steps, "start_rs2", &["start", "rs2", "sleep", "300"], "STARTED").is_some() {
        ctr_step(&mut session, &mut steps, "kill_shim_rs2", &["kill-shim", "rs2"], "KILLED-SHIM");
        ctr_step(&mut session, &mut steps, "task_gone_after_shim_crash", &["wait-gone", "rs2", "30"], "GONE");
        step(&mut steps, "no_orphaned_microvm", || {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let (rc, lines) = session.microvm(&["status", "rs2"])?;
                let line = lines.first().cloned().unwrap_or_default();
                if rc == 0 && !line.starts_with("RUNNING") {
                    let (_, c) = session.microvm(&["cleanup", "rs2"])?;
                    return Ok(((), format!("microVM {line}; jail {}", c.first().cloned().unwrap_or_default())));
                }
                if Instant::now() >= deadline {
                    return Err(format!("microVM still running after the shim died: {line}"));
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        step(&mut steps, "container_delete_after_crash", || {
            let (_, lines) = session.ctr(&["try", "container", "delete", "rs2"])?;
            let (rc, out) = rc_out(&lines);
            if rc != 0 {
                return Err(format!("container delete failed: {}", out.join(" ")));
            }
            Ok(((), "container record removed".to_string()))
        });
    }
    // 3. in-flight `ctr run --rm` across a containerd restart fails cleanly and leaves nothing.
    if ctr_step(&mut session, &mut steps, "inflight_start_rs3", &["inflight-start", "rs3", "sleep", "20"], "INFLIGHT").is_some() {
        step(&mut steps, "containerd_killed_during_inflight", || {
            let pid = session.kill_service("containerd")?;
            let detail = wait_containerd(&mut session, Duration::from_secs(60))?;
            Ok(((), format!("containerd pid {pid} SIGKILLed; {detail}")))
        });
        step(&mut steps, "inflight_fails_gracefully", || {
            let (_, lines) = session.ctr(&["inflight-result", "rs3", "60"])?;
            let (rc, out) = rc_out(&lines);
            if rc == 0 {
                return Err("ctr run across a containerd restart must not report success".to_string());
            }
            Ok(((), format!("ctr exited {rc}: {}", out.last().cloned().unwrap_or_default())))
        });
        step(&mut steps, "inflight_task_reconciled", || {
            let (_, lines) = session.ctr(&["state", "rs3"])?;
            let line = lines.first().cloned().unwrap_or_default();
            if line.contains(" RUNNING ") {
                let _ = session.ctr(&["kill", "rs3"]);
            }
            let _ = session.ctr(&["try", "task", "delete", "-f", "rs3"]);
            let _ = session.ctr(&["try", "container", "delete", "rs3"]);
            let _ = session.microvm(&["cleanup", "rs3"]);
            Ok(((), format!("after restart the task was {}; killed and deleted", line.split_whitespace().nth(2).unwrap_or("absent"))))
        });
    }
    ctr_step(&mut session, &mut steps, "no_leftovers", &["leftovers"], "CLEAN");
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 16;
    report(endpoint, steps, passed)
}
