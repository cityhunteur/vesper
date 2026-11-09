//! GH #23: runc execution, separate streams, terminals, resize and signals on real hardware.
use super::*;

pub fn run_exec_suite(endpoint: &ControlEndpoint) -> SuiteReport {
    let mut steps = Vec::new();
    let Some(mut session) = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        Ok((session, "connected".into()))
    }) else {
        return report(endpoint, steps, false);
    };
    let names = [
        "ex-stdio",
        "ex-tty",
        "ex-host",
        "ex-term",
        "ex-trap",
        "ex-kill",
        "ex-bad",
        "ex-interactive",
        "ex-orphan",
        "ex-orphan-tty",
        "ex-exit",
    ];
    for name in names {
        cleanup(&mut session, name);
    }
    if ctr_step(&mut session, &mut steps, "import", &["import"], "IMPORTED").is_none() {
        return report(endpoint, steps, false);
    }
    step(&mut steps, "stdin_stdout_stderr_exit7", || {
        let (rc, lines) = session.ctr(&["run-stdio", "ex-stdio"])?;
        let detail = lines.join(" | ");
        if rc != 0
            || !lines.iter().any(|l| l == "RC 7")
            || !lines.iter().any(|l| l == "OUT stdin-roundtrip")
            || !lines.iter().any(|l| l == "ERR stderr-only")
            || lines.iter().any(|l| l == "OUT stderr-only")
        {
            return Err(detail);
        }
        Ok(((), detail))
    });
    step(&mut steps, "interactive_shell", || {
        let (_, lines) = session.ctr(&["interactive", "ex-interactive"])?;
        let (rc, out) = rc_out(&lines);
        if rc != 7 || !out.iter().any(|l| l.trim() == "interactive-ok") {
            return Err(lines.join(" | "));
        }
        Ok(((), lines.join(" | ")))
    });
    step(&mut steps, "run_tty_resize_exit7", || {
        tty_check(&mut session, &["run-tty", "ex-tty"])
    });
    if ctr_step(
        &mut session,
        &mut steps,
        "start_exec_host",
        &[
            "start",
            "ex-host",
            "rm",
            "/bin/sh;",
            "exec",
            "/bin/busybox",
            "sleep",
            "300",
        ],
        "STARTED",
    )
    .is_some()
    {
        step(&mut steps, "exec_no_shell_exit7", || {
            let (_, lines) = session.ctr(&["exec", "ex-host", "probe"])?;
            let (rc, out) = rc_out(&lines);
            if rc != 7
                || !out.iter().any(|l| l == "exec-output")
                || !out.iter().any(|l| l == "exec-stderr")
            {
                return Err(lines.join(" | "));
            }
            Ok(((), lines.join(" | ")))
        });
        step(&mut steps, "exec_tty_resize_exit7", || {
            tty_check(&mut session, &["exec-tty", "ex-host"])
        });
        step(&mut steps, "exec_signal_kill137", || {
            let (_, lines) = session.ctr(&["exec-signal", "ex-host"])?;
            if rc_out(&lines).0 != 137 {
                return Err(lines.join(" | "));
            }
            Ok(((), lines.join(" | ")))
        });
        step(&mut steps, "exec_runtime_error_is_structured", || {
            let (_, lines) = session.ctr(&[
                "try",
                "task",
                "exec",
                "--exec-id",
                "bad-exec",
                "ex-host",
                "/no-such-vesper-program",
            ])?;
            let (rc, out) = rc_out(&lines);
            if rc == 0
                || !out
                    .iter()
                    .any(|l| l.contains("ERROR {") && l.contains("guest session"))
            {
                return Err(lines.join(" | "));
            }
            Ok(((), lines.join(" | ")))
        });
        ctr_step(
            &mut session,
            &mut steps,
            "kill_exec_host",
            &["kill", "ex-host"],
            "KILLED",
        );
        ctr_step(
            &mut session,
            &mut steps,
            "delete_exec_host",
            &["delete", "ex-host"],
            "DELETED",
        );
    }
    for (name, signal, mode, expected, marker) in [
        ("ex-term", "TERM", "plain", 143, None),
        ("ex-trap", "TERM", "trap", 0, Some("TERM-caught")),
        ("ex-kill", "KILL", "plain", 137, None),
    ] {
        step(
            &mut steps,
            &format!("signal_{signal}_{mode}_exit{expected}"),
            || {
                let (driver_rc, lines) = session.ctr(&["signal-test", name, signal, mode])?;
                let (rc, out) = rc_out(&lines);
                if driver_rc != 0
                    || rc != expected
                    || marker.is_some_and(|m| !out.iter().any(|l| l == m))
                {
                    return Err(lines.join(" | "));
                }
                Ok(((), lines.join(" | ")))
            },
        );
    }
    for (name, args) in [
        ("init_reaps_orphan", vec!["run-orphan", "ex-orphan"]),
        (
            "tty_init_reaps_orphan",
            vec!["run-orphan", "ex-orphan-tty", "tty"],
        ),
        (
            "init_exit_terminates_exec",
            vec!["exit-with-exec", "ex-exit"],
        ),
    ] {
        step(&mut steps, name, || {
            let (driver_rc, lines) = session.ctr(&args)?;
            if driver_rc != 0 || rc_out(&lines).0 != 7 {
                return Err(lines.join(" | "));
            }
            Ok(((), lines.join(" | ")))
        });
    }
    for name in names {
        cleanup(&mut session, name);
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

fn cleanup(session: &mut Session, name: &str) {
    let _ = session.ctr(&["try", "task", "kill", "-s", "KILL", name]);
    let _ = session.ctr(&["try", "task", "delete", "-f", name]);
    let _ = session.ctr(&["try", "container", "delete", name]);
}

fn tty_check(session: &mut Session, args: &[&str]) -> Result<((), String), String> {
    let (_, lines) = session.ctr(args)?;
    let (rc, out) = rc_out(&lines);
    if rc != 7
        || !out.iter().any(|l| l.trim().starts_with("/dev/pts/"))
        || !out.iter().any(|l| l.trim() == "24 80")
        || !out.iter().any(|l| l.trim() == "37 101")
    {
        return Err(lines.join(" | "));
    }
    Ok(((), lines.join(" | ")))
}
