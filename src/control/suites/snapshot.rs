//! ADR 0007 slice 5 checklist: staging, the store and the snapshot cycle through `vesperd --backend systemvm`.
//!
//! microVM A starts with a drive staged from a share, takes a second drive staged while it runs, boots a guest that
//! keeps a random token in memory and prints it with a tick every second, is paused and snapshotted, and its files
//! are copied into the store before it stops. A new `vesperd` (B) stages the snapshot by link, loads it and resumes:
//! the guest prints the same token with later ticks. A create onto the linked files fails (CE-0008). Around the
//! cycle the suite shows each staging rule: symlinks and paths outside the roots are refused, a jail-side read runs
//! as uid 20000, and a copy that would breach the free-space reserve is refused. Last, it times `PUT
//! /snapshot/create` of the largest guest the default ceiling allows, with that guest's memory dirtied.
//! Needs a launch config with a share (`make systemvm-snapshot-suite` names `run/vesperd-share`).

use super::launchapi::{first, wait_absent};
use super::vesperd::{signal, state_of, wait_exit, Client, Vesperd, BOOT_ARGS, TEARDOWN_TIMEOUT};
use super::{connect, report, step, ControlEndpoint, Session, SuiteReport, SuiteStep, VesperdRun, VESPERD_REQUEST_CEILING};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// The default `memory.max` of a `vesperd` microVM (`DEFAULT_CEILING` in `vesper-vesperd`).
pub const DEFAULT_CEILING_MIB: u64 = 640;
/// The largest guest the measurement step boots under the default ceiling, and how much of it the guest dirties.
pub const MEASURE_GUEST_MIB: u64 = 640;
pub const MEASURE_FILL_MIB: u64 = 512;
const STEPS: usize = 24;
const DRIVE_BYTES: usize = 1024 * 1024;

/// Where the suite works: the `vesperd` runner and an absolute directory inside a configured share.
pub struct SnapshotRun {
    pub vesperd: VesperdRun,
    pub share_dir: PathBuf,
}

/// `vesperd stage|store …` with the runner's bridge flags; returns the exit code, stdout and stderr.
pub(super) fn cli(run: &VesperdRun, args: &[&str]) -> Result<(i32, String, String), String> {
    let out = Command::new(&run.binary)
        .args(args)
        .args(&run.extra_args)
        .output()
        .map_err(|err| format!("run {}: {err}", run.binary.display()))?;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    ))
}

/// `vesperd stage` that must succeed; returns its `STAGED` line.
fn stage_ok(run: &VesperdRun, id: &str, src: &str, dest: &str) -> Result<String, String> {
    let (code, out, err) = cli(run, &["stage", "--id", id, src, dest])?;
    if code != 0 || !out.starts_with("STAGED ") {
        return Err(format!("vesperd stage {src} {dest}: exit {code}: {out} {err}"));
    }
    Ok(out)
}

/// `vesperd stage` that must be refused with exit `want_code` and a reason containing `want`.
fn stage_refused(run: &VesperdRun, id: &str, src: &str, dest: &str, want_code: i32, want: &str) -> Result<String, String> {
    let (code, out, err) = cli(run, &["stage", "--id", id, src, dest])?;
    if code != want_code || !err.contains(want) {
        return Err(format!("vesperd stage {src} {dest}: exit {code} (want {want_code}, {want:?}): {out} {err}"));
    }
    Ok(format!("{src} -> {dest}: exit {code}: {err}"))
}

/// `FILE PATH uid=U mode=M links=L bytes=N` for `path` in `id`'s jail.
fn jail_file(control: &mut Session, id: &str, path: &str) -> Result<String, String> {
    let (_, lines) = control.vesperd(&["files", id])?;
    lines
        .into_iter()
        .find(|l| l.split(' ').nth(1) == Some(path))
        .ok_or_else(|| format!("{path} not in {id}'s jail"))
}

/// The store's `STORE` lines and `FREE` line.
fn store(run: &VesperdRun) -> Result<(Vec<String>, u64), String> {
    let (code, out, err) = cli(run, &["store", "list"])?;
    if code != 0 {
        return Err(format!("vesperd store list: exit {code}: {err}"));
    }
    let lines: Vec<String> = out.lines().map(str::to_string).collect();
    let avail = lines
        .iter()
        .find_map(|l| l.strip_prefix("FREE avail="))
        .and_then(|rest| rest.split(' ').next())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("no FREE line: {out}"))?;
    Ok((lines.into_iter().filter(|l| l.starts_with("STORE ")).collect(), avail))
}

fn store_line<'a>(lines: &'a [String], name: &str) -> Option<&'a String> {
    lines.iter().find(|l| l.split(' ').nth(1) == Some(name))
}

/// `(token, tick)` of every `VESPER-MARK` line in `id`'s serial console.
fn marks(control: &mut Session, id: &str) -> Result<Vec<(String, u64)>, String> {
    let (_, lines) = control.vesperd(&["log", id, "400"])?;
    Ok(lines
        .iter()
        .filter_map(|l| {
            let rest = l.split("VESPER-MARK token=").nth(1)?;
            let (token, tick) = rest.split_once(" tick=")?;
            Some((token.to_string(), tick.trim().parse().ok()?))
        })
        .collect())
}

fn wait_log(control: &mut Session, id: &str, marker: &str, within: Duration) -> Result<String, String> {
    let deadline = Instant::now() + within;
    loop {
        let (_, lines) = control.vesperd(&["log", id, "200"])?;
        if let Some(line) = lines.iter().find(|l| l.contains(marker)) {
            return Ok(line.clone());
        }
        if Instant::now() >= deadline {
            return Err(format!("no {marker} within {} s: {}", within.as_secs(), lines.iter().rev().take(4).cloned().collect::<Vec<_>>().join(" | ")));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn boot(api: &mut Client, mem_mib: u64, extra_args: &str, drives: &[(&str, &str)]) -> Result<Duration, String> {
    api.expect("PUT", "/machine-config", Some(&format!(r#"{{"vcpu_count":1,"mem_size_mib":{mem_mib}}}"#)), 204)?;
    let boot = format!(r#"{{"kernel_image_path":"/Image","initrd_path":"/initrd.gz","boot_args":"{BOOT_ARGS} {extra_args}"}}"#);
    api.expect("PUT", "/boot-source", Some(&boot), 204)?;
    for (drive, path) in drives {
        let body = format!(r#"{{"drive_id":"{drive}","path_on_host":"{path}","is_root_device":false,"is_read_only":false}}"#);
        api.expect("PUT", &format!("/drives/{drive}"), Some(&body), 204)?;
    }
    let t = Instant::now();
    api.expect("PUT", "/actions", Some(r#"{"action_type":"InstanceStart"}"#), 204)?;
    Ok(t.elapsed())
}

fn stop(control: &mut Session, v: &mut Vesperd, id: &str) -> Result<String, String> {
    signal(v, "TERM")?;
    let (code, err, took) = wait_exit(v, TEARDOWN_TIMEOUT)?;
    if code != Some(0) {
        return Err(format!("exit {code:?}: {err}"));
    }
    let gone = wait_absent(control, id, Duration::from_secs(5))?;
    Ok(format!("SIGTERM: exit 0 after {} ms; ABSENT {id} {} ms later", took.as_millis(), gone.as_millis()))
}

fn memory(control: &mut Session, id: &str) -> String {
    control.vesperd(&["memory", id]).map(|(_, l)| first(&l)).unwrap_or_else(|e| e)
}

/// Runs the slice 5 checklist. Ids and store names are derived from `id`; `run.share_dir` must be inside a share.
pub fn run_snapshot_suite(endpoint: &ControlEndpoint, run: &SnapshotRun, id: &str) -> SuiteReport {
    let mut steps: Vec<SuiteStep> = Vec::new();
    let v = &run.vesperd;
    let ids = [format!("{id}-a"), format!("{id}-b"), format!("{id}-c"), format!("{id}-m")];
    let [a, b, c, m] = &ids;
    let names = [format!("{id}.mem"), format!("{id}.vmstate"), format!("{id}-d1.img"), format!("{id}-d2.img"), format!("{id}-probe")];
    let [mem, vmstate, d1, d2, probe] = &names;
    let share = run.share_dir.display().to_string();
    let control = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut control) = control else {
        return report(endpoint, steps, false);
    };
    // Leftovers of an earlier run: their vesperd processes are gone, so the reaper removes their microVMs.
    for each in &ids {
        let _ = control.vesperd(&["test-age", each, "3600"]);
    }
    let _ = control.vesperd(&["reap"]);
    let _ = control.vesperd(&["test-reserve", "clear"]);
    for name in &names {
        let _ = control.vesperd(&["store-rm", name]);
    }

    let ready = step(&mut steps, "data_disk_and_user", || {
        let (_, data) = control.data(&["status"])?;
        if !super::super::data_disk_in_use(&first(&data)) {
            return Err(format!("data disk not in use: {}", first(&data)));
        }
        let (rc, user) = control.vesperd(&["setup-user"])?;
        if rc != 0 || first(&user) != "USER vesperd-jail 20000" {
            return Err(format!("setup-user: {}", user.join(" | ")));
        }
        Ok(((), format!("{}; {}", first(&data), first(&user))))
    });
    let shared = step(&mut steps, "share_files", || {
        let (_, mounts) = control.share(&["list"])?;
        if !mounts.iter().any(|l| run.share_dir.starts_with(l.split(' ').nth(2).unwrap_or("\0"))) {
            return Err(format!("{share} is in no mounted share: {}", mounts.join(" | ")));
        }
        std::fs::create_dir_all(&run.share_dir).map_err(|e| e.to_string())?;
        let random: Vec<u8> = (0..DRIVE_BYTES).map(|i| (i * 7 + i / 4096) as u8).collect();
        for name in ["d1.img", "d2.img"] {
            std::fs::write(run.share_dir.join(name), &random).map_err(|e| e.to_string())?;
        }
        let link = run.share_dir.join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("/etc/shadow", &link).map_err(|e| e.to_string())?;
        Ok(((), format!("{share}: d1.img and d2.img ({DRIVE_BYTES} bytes), link -> /etc/shadow; mounts: {}", mounts.join(" | "))))
    });
    if ready.is_none() || shared.is_none() {
        return report(endpoint, steps, false);
    }

    let started = step(&mut steps, "start_with_share_stage", || {
        let flags = vec!["--stage".to_string(), format!("share:{share}/d1.img:/d1.img")];
        let (va, line, took) = v.start_with(a, "a.sock", &flags)?;
        let file = jail_file(&mut control, a, "/d1.img")?;
        if !file.contains("uid=20000") || !file.contains("links=1") || !file.contains(&format!("bytes={DRIVE_BYTES}")) {
            return Err(format!("staged at start: {file}"));
        }
        Ok((va, format!("{line} after {} ms; --stage copied: {file}", took.as_millis())))
    });
    let Some(mut va) = started else {
        return report(endpoint, steps, false);
    };
    let mut last_a = (String::new(), 0u64);
    let mut cycle_ok = step(&mut steps, "stage_into_running_jail", || {
        let staged = stage_ok(v, a, &format!("share:{share}/d2.img"), "/d2.img")?;
        let file = jail_file(&mut control, a, "/d2.img")?;
        if !file.contains("uid=20000") {
            return Err(format!("{staged}; {file}"));
        }
        Ok(((), format!("{staged}; {file}")))
    })
    .is_some();
    cycle_ok &= step(&mut steps, "boot_with_both_drives", || {
        let took = boot(&mut Client::connect(&va.socket)?, 256, "vesper_mark=1", &[("d1", "/d1.img"), ("d2", "/d2.img")])?;
        let line = wait_log(&mut control, a, "VESPER-MARK", Duration::from_secs(20))?;
        Ok(((), format!("PUT /drives d1 (staged at start) and d2 (staged while Firecracker ran) -> 204, so Firecracker opens files staged into its running jail; InstanceStart {} ms; {line}", took.as_millis())))
    })
    .is_some();

    // The staging rules, against the running microVM A.
    step(&mut steps, "refuses_symlinked_src", || {
        let share_link = stage_refused(v, a, &format!("share:{share}/link"), "/link", 1, "symlink")?;
        let (_, planted) = control.vesperd(&["test-plant", a])?;
        let jail_link = stage_refused(v, a, "jail:/planted-link", &format!("store:{probe}"), 1, "symlink at /planted-link")?;
        Ok(((), format!("{share_link}; {}; {jail_link}", first(&planted))))
    });
    step(&mut steps, "refuses_symlinked_dest", || {
        let dir = stage_refused(v, a, &format!("share:{share}/d1.img"), "/planted-dir/evil", 1, "symlink at /planted-dir")?;
        let (stored, _) = store(v)?;
        if store_line(&stored, "evil").is_some() {
            return Err("the store got a file through the symlink".to_string());
        }
        Ok(((), format!("{dir}; nothing written into the store")))
    });
    step(&mut steps, "refuses_src_outside_roots", || {
        let outside = stage_refused(v, a, "share:/etc/passwd", "/passwd", 1, "not inside a configured share")?;
        let dotdot = stage_refused(v, a, &format!("share:{share}/../../../etc/passwd"), "/passwd", 2, "no . or .. component")?;
        let other = stage_refused(v, a, "share:/var/lib/vesper/state/vesperd/store/x", "/x", 1, "not inside a configured share")?;
        let relative = stage_refused(v, a, "jail:planted-root", "store:x", 2, "bad SRC")?;
        Ok(((), format!("{outside}; {dotdot}; {other}; {relative}")))
    });
    step(&mut steps, "jail_side_read_runs_as_uid_20000", || {
        // /planted-root is a regular file, owned by root, mode 0600: root could read it, uid 20000 cannot.
        let file = jail_file(&mut control, a, "/planted-root")?;
        let refused = stage_refused(v, a, "jail:/planted-root", &format!("store:{probe}"), 1, "permission denied for uid 20000")?;
        let (stored, _) = store(v)?;
        if store_line(&stored, probe).is_some() {
            return Err(format!("store:{probe} was written"));
        }
        Ok(((), format!("{file}; {refused}; store:{probe} not written")))
    });
    step(&mut steps, "reserve_refuses_breach", || {
        let (_, avail) = store(v)?;
        let reserve = avail - (DRIVE_BYTES as u64) / 2;
        let (_, set) = control.vesperd(&["test-reserve", &reserve.to_string()])?;
        let refused = stage_refused(v, a, &format!("share:{share}/d1.img"), &format!("store:{probe}"), 1, "free-space reserve");
        let into_jail = stage_refused(v, a, &format!("share:{share}/d1.img"), "/d3.img", 1, "free-space reserve");
        let (_, clear) = control.vesperd(&["test-reserve", "clear"])?;
        let (refused, into_jail) = (refused?, into_jail?);
        let (stored, _) = store(v)?;
        if store_line(&stored, probe).is_some() || jail_file(&mut control, a, "/d3.img").is_ok() {
            return Err("a refused copy left a file".to_string());
        }
        Ok(((), format!("{}; {refused}; {into_jail}; {}", first(&set), first(&clear))))
    });

    // TD-053: the store holds at most its quota; a copy that would pass it is refused and leaves no file.
    step(&mut steps, "store_quota_refuses_breach", || {
        let (code, out, err) = cli(v, &["store", "list"])?;
        let used = out
            .lines()
            .find(|l| l.starts_with("FREE "))
            .and_then(|l| l.split(' ').find_map(|w| w.strip_prefix("store=")))
            .and_then(|n| n.parse::<u64>().ok())
            .ok_or_else(|| format!("store list (exit {code}) has no store= on its FREE line: {out} {err}"))?;
        let quota = used + (DRIVE_BYTES as u64) / 2;
        let (_, set) = control.vesperd(&["test-store-quota", &quota.to_string()])?;
        let refused = stage_refused(v, a, &format!("share:{share}/d1.img"), &format!("store:{probe}"), 1, "store quota");
        let (_, clear) = control.vesperd(&["test-store-quota", "clear"])?;
        let refused = refused?;
        let (stored, _) = store(v)?;
        if store_line(&stored, probe).is_some() {
            return Err("a refused copy left a store file".to_string());
        }
        Ok(((), format!("{}; {refused}; {}", first(&set), first(&clear))))
    });

    cycle_ok &= step(&mut steps, "pause_and_create", || {
        let mut api = Client::connect(&va.socket)?;
        thread::sleep(Duration::from_secs(2));
        api.expect("PATCH", "/vm", Some(r#"{"state":"Paused"}"#), 204)?;
        last_a = marks(&mut control, a)?.pop().ok_or("no VESPER-MARK line before the pause")?;
        let t = Instant::now();
        api.expect("PUT", "/snapshot/create", Some(r#"{"snapshot_type":"Full","snapshot_path":"/snap.vmstate","mem_file_path":"/snap.mem"}"#), 204)?;
        Ok(((), format!("paused at tick {} (token {}); PUT /snapshot/create (Full, 256 MiB) -> 204 in {} ms, jail on the data disk; {}", last_a.1, last_a.0, t.elapsed().as_millis(), memory(&mut control, a))))
    })
    .is_some();
    cycle_ok &= step(&mut steps, "copy_into_store", || {
        let mut done = Vec::new();
        for (src, name) in [("/snap.mem", mem), ("/snap.vmstate", vmstate), ("/d1.img", d1), ("/d2.img", d2)] {
            let t = Instant::now();
            let line = stage_ok(v, a, &format!("jail:{src}"), &format!("store:{name}"))?;
            done.push(format!("{line} in {} ms", t.elapsed().as_millis()));
        }
        let (stored, _) = store(v)?;
        let line = store_line(&stored, mem).ok_or("no memory file in the store")?;
        if !line.ends_with("links=1") || !line.contains("bytes=268435456") {
            return Err(format!("store: {line}"));
        }
        Ok(((), format!("{}; {line}", done.join("; "))))
    })
    .is_some();
    cycle_ok &= step(&mut steps, "stop_original", || Ok(((), stop(&mut control, &mut va, a)?))).is_some();

    if cycle_ok {
        let flags: Vec<String> = [(mem, "/snap.mem"), (vmstate, "/snap.vmstate"), (d1, "/d1.img"), (d2, "/d2.img")]
            .iter()
            .flat_map(|(name, dest)| ["--stage".to_string(), format!("store:{name}:{dest}")])
            .collect();
        let restored = step(&mut steps, "new_vesperd_stages_by_link", || {
            let (vb, line, took) = v.start_with(b, "b.sock", &flags)?;
            let memf = jail_file(&mut control, b, "/snap.mem")?;
            let statef = jail_file(&mut control, b, "/snap.vmstate")?;
            let drive = jail_file(&mut control, b, "/d1.img")?;
            for linked in [&memf, &statef] {
                if !linked.contains("uid=0 mode=444 links=2") {
                    return Err(format!("not a root-owned read-only link: {linked}"));
                }
            }
            if !drive.contains("uid=20000") || !drive.contains("links=1") {
                return Err(format!("drive not copied: {drive}"));
            }
            Ok((vb, format!("{line} after {} ms; {memf}; {statef}; {drive}", took.as_millis())))
        });
        if let Some(mut vb) = restored {
            let loaded = step(&mut steps, "load_and_resume", || {
                let mut api = Client::connect(&vb.socket)?;
                let t = Instant::now();
                api.expect("PUT", "/snapshot/load", Some(r#"{"snapshot_path":"/snap.vmstate","mem_backend":{"backend_type":"File","backend_path":"/snap.mem"},"resume_vm":false}"#), 204)?;
                let load = t.elapsed();
                api.expect("PATCH", "/vm", Some(r#"{"state":"Resumed"}"#), 204)?;
                let body = api.expect("GET", "/", None, 200)?;
                if state_of(&body) != "Running" {
                    return Err(format!("GET /: {body}"));
                }
                Ok(((), format!("PUT /snapshot/load -> 204 in {} ms; PATCH /vm Resumed -> 204; GET / -> {body}", load.as_millis())))
            });
            if loaded.is_some() {
                step(&mut steps, "guest_state_survives", || {
                    thread::sleep(Duration::from_secs(3));
                    let after = marks(&mut control, b)?;
                    let firstb = after.first().cloned().ok_or("no VESPER-MARK line after resume")?;
                    if after.iter().any(|(t, _)| *t != last_a.0) || firstb.1 <= last_a.1 {
                        return Err(format!("before: {last_a:?}; after: {after:?}"));
                    }
                    Ok(((), format!("the guest kept token {} in memory and counts on: tick {} before the snapshot, ticks {}..{} after the restore in {b}", last_a.0, last_a.1, firstb.1, after.last().map(|m| m.1).unwrap_or(0))))
                });
                step(&mut steps, "create_onto_linked_file_fails", || {
                    let mut api = Client::connect(&vb.socket)?;
                    api.expect("PATCH", "/vm", Some(r#"{"state":"Paused"}"#), 204)?;
                    let over = api.call("PUT", "/snapshot/create", Some(r#"{"snapshot_type":"Full","snapshot_path":"/snap.vmstate","mem_file_path":"/snap.mem"}"#))?;
                    if over.status != 400 || !over.body.contains("Permission denied") {
                        return Err(format!("create onto the links: {} {}", over.status, over.body));
                    }
                    let (stored, _) = store(v)?;
                    let line = store_line(&stored, mem).cloned().unwrap_or_default();
                    if !line.contains("bytes=268435456") {
                        return Err(format!("store file changed: {line}"));
                    }
                    api.expect("PUT", "/snapshot/create", Some(r#"{"snapshot_type":"Full","snapshot_path":"/snap2.vmstate","mem_file_path":"/snap2.mem"}"#), 204)?;
                    Ok(((), format!("PUT /snapshot/create onto /snap.mem and /snap.vmstate -> 400 {} (upstream overwrites; CE-0008); the store file is unchanged ({line}); a create onto new paths -> 204", over.body)))
                });
            }
            step(&mut steps, "stop_restored", || {
                let done = stop(&mut control, &mut vb, b)?;
                let (stored, _) = store(v)?;
                let line = store_line(&stored, mem).cloned().unwrap_or_default();
                if !line.ends_with("links=1") {
                    return Err(format!("{done}; the jail's link outlived it: {line}"));
                }
                Ok(((), format!("{done}; {line}")))
            });
        }
    }

    step(&mut steps, "stage_refused_at_start", || {
        let flags = vec!["--stage".to_string(), "share:/etc/passwd:/passwd".to_string()];
        let mut vc = v.spawn_with(c, "c.sock", &flags)?;
        let (code, err, _) = wait_exit(&mut vc, Duration::from_secs(30))?;
        if code == Some(0) || !err.contains("not inside a configured share") {
            return Err(format!("exit {code:?}: {err}"));
        }
        let gone = wait_absent(&mut control, c, TEARDOWN_TIMEOUT)?;
        Ok(((), format!("exit {code:?}: {err}; ABSENT {c} after {} ms", gone.as_millis())))
    });

    step(&mut steps, "measure_create_at_default_ceiling", || {
        let (mut vm, _, _) = v.start_with(m, "m.sock", &[])?;
        let mut api = Client::connect(&vm.socket)?;
        boot(&mut api, MEASURE_GUEST_MIB, &format!("vesper_fill_mib={MEASURE_FILL_MIB}"), &[])?;
        let filled = wait_log(&mut control, m, "VESPER-FILL-OK", Duration::from_secs(60))?;
        let before = memory(&mut control, m);
        api.expect("PATCH", "/vm", Some(r#"{"state":"Paused"}"#), 204)?;
        let t = Instant::now();
        api.expect("PUT", "/snapshot/create", Some(r#"{"snapshot_type":"Full","snapshot_path":"/m.vmstate","mem_file_path":"/m.mem"}"#), 204)?;
        let took = t.elapsed();
        let after = memory(&mut control, m);
        let state = state_of(&api.expect("GET", "/", None, 200)?);
        let done = stop(&mut control, &mut vm, m)?;
        if took * 10 > VESPERD_REQUEST_CEILING {
            return Err(format!("PUT /snapshot/create took {} ms, over a tenth of the {} s ceiling", took.as_millis(), VESPERD_REQUEST_CEILING.as_secs()));
        }
        Ok(((), format!("{MEASURE_GUEST_MIB} MiB guest, {filled}, memory.max {DEFAULT_CEILING_MIB} MiB: PUT /snapshot/create -> 204 in {} ms (ceiling {} s); before: {before}; after: {after}; GET / {state}; {done}", took.as_millis(), VESPERD_REQUEST_CEILING.as_secs())))
    });

    step(&mut steps, "store_rm", || {
        let mut removed = Vec::new();
        for name in [mem, vmstate, d1, d2] {
            let (code, out, err) = cli(v, &["store", "rm", name])?;
            if code != 0 {
                return Err(format!("store rm {name}: exit {code}: {err}"));
            }
            removed.push(out);
        }
        let (code, _, err) = cli(v, &["store", "rm", mem])?;
        if code != 1 || !err.contains("no store file") {
            return Err(format!("a second rm: exit {code}: {err}"));
        }
        let (code, _, err) = cli(v, &["store", "rm", "../x"])?;
        if code != 2 {
            return Err(format!("store rm ../x: exit {code}: {err}"));
        }
        Ok(((), format!("{}; a second rm and a bad name are refused", removed.join("; "))))
    });

    step(&mut steps, "nothing_left", || {
        let sockets: Vec<String> = std::fs::read_dir(&v.workdir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| std::os::unix::fs::FileTypeExt::is_socket(&t)).unwrap_or(false))
            .map(|e| e.path().display().to_string())
            .collect();
        if !sockets.is_empty() {
            return Err(format!("sockets left: {sockets:?}"));
        }
        let (_, lines) = control.vesperd(&["list"])?;
        let ours: Vec<&String> = lines.iter().filter(|l| ids.iter().any(|i| l.split_whitespace().nth(1) == Some(i.as_str()))).collect();
        let (stored, _) = store(v)?;
        let kept: Vec<&String> = stored.iter().filter(|l| names.iter().any(|n| l.split(' ').nth(1) == Some(n.as_str()))).collect();
        if !ours.is_empty() || !kept.is_empty() {
            return Err(format!("leftovers: {ours:?} {kept:?}"));
        }
        for name in ["d1.img", "d2.img", "link"] {
            let _ = std::fs::remove_file(run.share_dir.join(name));
        }
        Ok(((), format!("no lease or jail for {}; no suite file in the store; no socket in {}", ids.join(", "), v.workdir.display())))
    });

    let passed = steps.iter().all(|s| s.ok) && steps.len() == STEPS;
    report(endpoint, steps, passed)
}

/// Makes `dir` owner-only, as the vesperd sockets need.
pub fn prepare_workdir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .and_then(|_| std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)))
        .map_err(|e| e.to_string())
}
