//! The bounds suite (TD-055, TD-056): the System VM's shared resources stay bounded whatever a microVM does.
//!
//! A guest floods its console (`vesper_console_flood=`), and the console log stays within its two 1 MiB files; the
//! reaper removes the log of a microVM gone longer than `LOG_KEEP_S`. With the memory budget's reserve raised so
//! that exactly one default-size microVM fits beside whatever already runs, a second `vesperd` microVM is refused,
//! a shim microVM counts against the same budget, and the budget is whole again afterwards.

use super::launchapi::first;
use super::vesperd::{signal, wait_exit, Client, Vesperd, BOOT_ARGS, TEARDOWN_TIMEOUT};
use super::super::Session;
use super::{connect, report, step, ControlEndpoint, SuiteReport, SuiteStep, VesperdRun};
use std::thread;
use std::time::{Duration, Instant};

/// Steps a full run records.
const STEPS: usize = 7;
/// Bytes the guest writes to its console: more than both log files hold together.
/// 2.5 MB rotates the log twice. The console is slow under nested virtualisation (one MMIO exit per byte), so the
/// step allows 600 s.
const FLOOD: u64 = 2_500_000;
/// How long the guest may take to write `FLOOD` bytes to its console.
const FLOOD_WAIT: Duration = Duration::from_secs(600);
/// `vesperd`'s default ceiling (640 MiB). The shim's microVMs use 384 MiB, so one fits in the room left for this.
const CEILING: u64 = 671_088_640;
/// Room above one ceiling, so a little drift in what is committed does not admit or refuse by accident.
const SLACK: u64 = 64 << 20;

/// `KEY=N` fields of a helper line.
fn field(line: &str, key: &str) -> Option<u64> {
    line.split_whitespace().find_map(|w| w.strip_prefix(key)?.strip_prefix('=')).and_then(|v| v.parse().ok())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Budget {
    total: u64,
    reserve: u64,
    committed: u64,
    entries: u64,
}

fn parse_budget(line: &str) -> Option<Budget> {
    line.starts_with("MEMBUDGET ").then_some(())?;
    Some(Budget {
        total: field(line, "total")?,
        reserve: field(line, "reserve")?,
        committed: field(line, "committed")?,
        entries: field(line, "entries")?,
    })
}

fn budget(control: &mut Session) -> Result<Budget, String> {
    let (_, lines) = control.vesperd(&["mem-budget"])?;
    parse_budget(&first(&lines)).ok_or_else(|| format!("mem-budget: {}", lines.join(" | ")))
}

fn stop(control: &mut Session, v: &mut Vesperd, id: &str) -> Result<(), String> {
    signal(v, "TERM")?;
    let _ = wait_exit(v, TEARDOWN_TIMEOUT)?;
    super::launchapi::wait_absent(control, id, Duration::from_secs(10)).map(|_| ())
}

/// Boots `v`'s microVM with extra kernel arguments.
fn boot(v: &Vesperd, extra: &str) -> Result<(), String> {
    let mut api = Client::connect(&v.socket)?;
    api.expect("PUT", "/machine-config", Some(r#"{"vcpu_count":1,"mem_size_mib":256}"#), 204)?;
    let boot = format!(r#"{{"kernel_image_path":"/Image","initrd_path":"/initrd.gz","boot_args":"{BOOT_ARGS} {extra}"}}"#);
    api.expect("PUT", "/boot-source", Some(&boot), 204)?;
    api.expect("PUT", "/actions", Some(r#"{"action_type":"InstanceStart"}"#), 204)?;
    Ok(())
}

/// A `vesperd` start that must be refused for memory: the error names the budget.
fn refused(run: &VesperdRun, id: &str) -> Result<String, String> {
    match run.start_with(id, &format!("{id}.sock"), &[]) {
        Ok(_) => Err(format!("{id} was admitted over the budget")),
        Err(err) if err.contains("ERR memory") || err.contains("memory: a ") => Ok(err),
        Err(err) => Err(format!("{id} failed, but not for memory: {err}")),
    }
}

pub fn run_bounds_suite(endpoint: &ControlEndpoint, run: &VesperdRun, id: &str) -> SuiteReport {
    let mut steps: Vec<SuiteStep> = Vec::new();
    let [log_id, a, b, shim] = [format!("{id}-log"), format!("{id}-a"), format!("{id}-b"), format!("{id}-shim")];
    let control = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        Ok((session, "connected".to_string()))
    });
    let Some(mut control) = control else {
        return report(endpoint, steps, false);
    };
    let _ = std::fs::create_dir_all(&run.workdir);
    for each in [&log_id, &a, &b] {
        let _ = control.vesperd(&["test-age", each, "3600"]);
    }
    let _ = control.vesperd(&["reap"]);
    let _ = control.microvm(&["cleanup", &shim, "--logs"]);
    let _ = control.vesperd(&["test-mem-reserve", "clear"]);

    step(&mut steps, "console_log_bounded", || {
        let (mut v, _, _) = run.start_with(&log_id, &format!("{log_id}.sock"), &[])?;
        let checked = (|| -> Result<String, String> {
            boot(&v, &format!("vesper_console_flood={FLOOD}"))?;
            let started = Instant::now();
            let deadline = started + FLOOD_WAIT;
            loop {
                let (_, tail) = control.vesperd(&["log", &log_id, "5"])?;
                if tail.iter().any(|l| l.contains("VESPER-FLOOD-DONE")) {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(format!("no VESPER-FLOOD-DONE within {} s: {}", FLOOD_WAIT.as_secs(), tail.join(" | ")));
                }
                thread::sleep(Duration::from_millis(500));
            }
            let (_, size) = control.vesperd(&["logsize", &log_id])?;
            let line = first(&size);
            let (cur, prev, max) = (field(&line, "current"), field(&line, "previous"), field(&line, "max"));
            let (Some(cur), Some(prev), Some(max)) = (cur, prev, max) else { return Err(format!("logsize: {line}")) };
            // One line of up to 4096 bytes may land past the limit before the file is rotated.
            if cur > max + 4097 || prev > max + 4097 || prev == 0 {
                return Err(format!("{FLOOD} bytes written: {line}"));
            }
            Ok(format!("{FLOOD} bytes written in {} s; {line}: rotated and bounded", started.elapsed().as_secs()))
        })();
        let stopped = stop(&mut control, &mut v, &log_id);
        let detail = checked?;
        stopped?;
        Ok(((), detail))
    });

    step(&mut steps, "log_pruned_after_keep_time", || {
        let (_, reap) = control.vesperd(&["reap"])?;
        if reap.iter().any(|l| l.starts_with(&format!("PRUNED {log_id} "))) {
            return Err(format!("pruned at once: {}", reap.join(" | ")));
        }
        let (rc, aged) = control.vesperd(&["test-age-log", &log_id, "3600"])?;
        if rc != 0 {
            return Err(format!("test-age-log: {}", first(&aged)));
        }
        let (_, reap) = control.vesperd(&["reap"])?;
        let pruned = reap.iter().find(|l| l.starts_with(&format!("PRUNED {log_id} "))).cloned();
        let (rc, after) = control.vesperd(&["logsize", &log_id])?;
        match pruned {
            Some(p) if rc != 0 => Ok(((), format!("kept while recent; after {}: {p}; {}", first(&aged), first(&after)))),
            _ => Err(format!("reap: {}; logsize: {}", reap.join(" | "), first(&after))),
        }
    });

    let mut base = Budget::default();
    step(&mut steps, "budget_refuses_a_second_vesperd", || {
        base = budget(&mut control)?;
        // Leave room for exactly one more default-size microVM.
        let reserve = base.total.saturating_sub(base.committed + CEILING + SLACK);
        let (_, r) = control.vesperd(&["test-mem-reserve", &reserve.to_string()])?;
        let (mut va, _, _) = run.start_with(&a, &format!("{a}.sock"), &[])?;
        let second = refused(run, &b);
        let during = budget(&mut control);
        let stopped = stop(&mut control, &mut va, &a);
        let err = second?;
        let during = during?;
        stopped?;
        if during.committed != base.committed + CEILING {
            return Err(format!("committed {} with {a} running, expected {}", during.committed, base.committed + CEILING));
        }
        Ok(((), format!("{}; {a} admitted (committed {}), {b} refused: {}", first(&r), during.committed, err.chars().take(200).collect::<String>())))
    });

    step(&mut steps, "budget_counts_shim_microvms", || {
        let (rc, launched) = control.microvm(&["launch", &shim])?;
        if rc != 0 {
            let _ = control.microvm(&["cleanup", &shim, "--logs"]);
            return Err(format!("shim microVM: {}", launched.join(" | ")));
        }
        let second = refused(run, &b);
        let (_, clean) = control.microvm(&["cleanup", &shim, "--logs"])?;
        let err = second?;
        let (mut vb, _, _) = run.start_with(&b, &format!("{b}.sock"), &[])?;
        stop(&mut control, &mut vb, &b)?;
        Ok(((), format!("{}; {b} refused while it ran ({}); {}; then {b} admitted", first(&launched), err.chars().take(120).collect::<String>(), first(&clean))))
    });

    step(&mut steps, "budget_whole_again", || {
        let (_, r) = control.vesperd(&["test-mem-reserve", "clear"])?;
        let after = budget(&mut control)?;
        if after.committed != base.committed || after.entries != base.entries {
            return Err(format!("before {base:?}, after {after:?}"));
        }
        Ok(((), format!("{}; committed {} in {} entries, as before", first(&r), after.committed, after.entries)))
    });

    step(&mut steps, "nothing_left", || {
        let (_, list) = control.vesperd(&["list"])?;
        let left: Vec<&String> = list.iter().filter(|l| [&log_id, &a, &b].iter().any(|i| l.contains(i.as_str()))).collect();
        if left.is_empty() { Ok(((), "no record, jail or log of the suite's ids".to_string())) } else { Err(format!("left: {left:?}")) }
    });
    let passed = steps.iter().all(|s| s.ok) && steps.len() == STEPS;
    report(endpoint, steps, passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_and_size_lines_parse() {
        let b = parse_budget("MEMBUDGET total=2061000704 reserve=268435456 budget=1792565248 committed=671088640 entries=1").unwrap();
        assert_eq!((b.total, b.committed, b.entries), (2061000704, 671088640, 1));
        assert!(parse_budget("ERR x").is_none());
        let line = "LOGSIZE vm current=10 previous=1048600 max=1048576";
        assert_eq!((field(line, "current"), field(line, "previous"), field(line, "max")), (Some(10), Some(1048600), Some(1048576)));
        assert_eq!(field(line, "cur"), None);
    }
}
