//! ADR 0007 slice 6: the Backend A side of the behaviour-matrix replay. Each case runs in its own `vesperd
//! --backend systemvm` microVM, started with the case's files staged into a fresh jail; its steps go through the
//! `vesperd` socket, one connection per step, and the responses come back unjudged. This module knows nothing about
//! the matrix; `systemvmctl control replay-suite` expands the scripts (`src/api_replay.rs`) and judges.

use super::snapshot::cli;
use super::vesperd::{signal, stderr, wait_exit, Client, Vesperd, VesperdRun, TEARDOWN_TIMEOUT};
use super::launchapi::wait_absent;
use super::{connect, ControlEndpoint, Session};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

/// One step for the microVM: an API request, or a pause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayStep {
    Http { method: String, path: String, body: Option<String> },
    Wait { ms: u64 },
    /// Poll the microVM's console, at most `ms`, for the guest init's `VESPER-MICROVM-OK` marker.
    GuestReady { ms: u64 },
}

/// The status and body of one step (`0` and empty for a `Wait`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayResponse {
    pub status: u16,
    pub body: String,
}

/// Where the replay runs: the `vesperd` to start and a directory inside a share for the staged files.
pub struct ReplayRun {
    pub vesperd: VesperdRun,
    pub share_dir: PathBuf,
}

/// A running replay: the control session (to wait for jails to go) and the `--stage` flags every case gets.
pub struct ReplaySession {
    control: Session,
    dir: PathBuf,
    stages: Vec<String>,
    snapshot_stages: Vec<String>,
    /// `--tap` names every case's microVM gets (CE-0010).
    taps: Vec<String>,
}

/// The store names of the snapshot fixture for `prefix`.
pub fn fixture_names(prefix: &str) -> (String, String) {
    (format!("{prefix}.vmstate"), format!("{prefix}.mem"))
}

impl ReplayRun {
    /// Writes each `(jail path, bytes)` file once into a new `share_dir/replay-<time>/` and opens the control session.
    /// Files are zero-filled; the jail path's components become the file name, so `/tmp/a.img` is `tmp_a.img`. The
    /// directory is new each run because the System VM's view of a share file that was removed and written again
    /// under the same name can be stale (a staged copy read 0 bytes).
    /// Every case's microVM also gets a tap device for each of `taps`.
    pub fn prepare(&self, endpoint: &ControlEndpoint, files: &[(String, u64)], taps: &[String]) -> Result<ReplaySession, String> {
        if taps.len() > super::super::MAX_TAPS {
            return Err(format!("{} taps exceed {}", taps.len(), super::super::MAX_TAPS));
        }
        let control = connect(endpoint)?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let dir = self.share_dir.join(format!("replay-{stamp}"));
        std::fs::create_dir_all(&dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
        let mut stages = Vec::new();
        for (jail, bytes) in files {
            let name = jail.trim_start_matches('/').replace('/', "_");
            let host = dir.join(&name);
            std::fs::write(&host, vec![0u8; *bytes as usize]).map_err(|err| format!("write {}: {err}", host.display()))?;
            stages.push(format!("share:{}:{jail}", host.display()));
        }
        if stages.len() + 2 > super::super::stage::MAX_STAGES {
            return Err(format!("{} files and the snapshot exceed {} stages", stages.len(), super::super::stage::MAX_STAGES));
        }
        Ok(ReplaySession { control, dir, stages, snapshot_stages: Vec::new(), taps: taps.to_vec() })
    }

    /// Runs `steps` (each must return 2xx) in a microVM `id`, copies the jail files `made` (state, memory) into the
    /// store under `prefix`, and stages them at `staged` (state, memory) for every later case marked `snapshot`.
    pub fn make_fixture(
        &self,
        session: &mut ReplaySession,
        id: &str,
        prefix: &str,
        steps: &[ReplayStep],
        made: (&str, &str),
        staged: (&str, &str),
    ) -> Result<String, String> {
        let (state, mem) = fixture_names(prefix);
        for name in [&state, &mem] {
            let _ = session.control.vesperd(&["store-rm", name]);
        }
        let started = Instant::now();
        let mut failed = None;
        let _ = self.run_case(session, id, steps, false, &mut |v, responses| {
            for (step, r) in steps.iter().zip(responses) {
                if let ReplayStep::Http { method, path, .. } = step {
                    if !(200..300).contains(&r.status) {
                        failed = Some(format!("fixture {method} {path}: {} {}", r.status, r.body));
                        return Ok(());
                    }
                }
            }
            for (src, name) in [(made.0, &state), (made.1, &mem)] {
                let (code, out, err) = cli(v, &["stage", "--id", id, &format!("jail:{src}"), &format!("store:{name}")])?;
                if code != 0 {
                    return Err(format!("vesperd stage jail:{src} store:{name}: exit {code}: {out} {err}"));
                }
            }
            Ok(())
        })?;
        if let Some(err) = failed {
            return Err(err);
        }
        session.snapshot_stages = vec![format!("store:{state}:{}", staged.0), format!("store:{mem}:{}", staged.1)];
        Ok(format!("{} steps, store {state} and {mem}, {} ms", steps.len(), started.elapsed().as_millis()))
    }

    /// Runs one case: starts `vesperd --id ID` with the files (and the snapshot fixture if `snapshot`) staged and the
    /// session's taps, sends
    /// the steps, calls `after` with the responses while the microVM still runs, stops `vesperd` and waits for the jail to go.
    pub fn run_case(
        &self,
        session: &mut ReplaySession,
        id: &str,
        steps: &[ReplayStep],
        snapshot: bool,
        after: &mut dyn FnMut(&VesperdRun, &[ReplayResponse]) -> Result<(), String>,
    ) -> Result<Vec<ReplayResponse>, String> {
        if snapshot && session.snapshot_stages.is_empty() {
            return Err("the case needs the snapshot fixture, which was not made".to_string());
        }
        let mut flags = Vec::new();
        let extra = if snapshot { &session.snapshot_stages[..] } else { &[][..] };
        for stage in session.stages.iter().chain(extra) {
            flags.push("--stage".to_string());
            flags.push(stage.clone());
        }
        for tap in &session.taps {
            flags.push("--tap".to_string());
            flags.push(tap.clone());
        }
        let socket = format!("{id}.sock");
        let (mut v, _line, _took) = self.vesperd.start_with(id, &socket, &flags)?;
        let control = &mut session.control;
        let mut ready = |within: Duration| guest_ready(control, id, within);
        let result = send_steps(&v.socket, steps, &mut ready).and_then(|responses| after(&self.vesperd, &responses).map(|_| responses));
        let stopped = stop(&mut session.control, &mut v, id);
        let _ = std::fs::remove_file(self.vesperd.workdir.join(&socket));
        let responses = result?;
        stopped?;
        Ok(responses)
    }

    /// Removes the fixture from the store and the staged files from the share.
    pub fn finish(&self, session: &mut ReplaySession, prefix: &str) -> Result<(), String> {
        let (state, mem) = fixture_names(prefix);
        for name in [&state, &mem] {
            let _ = session.control.vesperd(&["store-rm", name]);
        }
        std::fs::remove_dir_all(&session.dir).map_err(|err| format!("remove {}: {err}", session.dir.display()))
    }
}

/// The guest init prints `VESPER-MICROVM-OK` after it loads the balloon and virtio-mem drivers, whose probe is what
/// activates those devices in Firecracker; a fixed sleep raced it on a loaded host.
fn guest_ready(control: &mut Session, id: &str, within: Duration) -> Result<(), String> {
    let deadline = Instant::now() + within;
    loop {
        let (_, lines) = control.vesperd(&["log", id, "200"])?;
        if lines.iter().any(|l| l.contains("VESPER-MICROVM-OK")) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("{id}: no VESPER-MICROVM-OK on the console within {} ms", within.as_millis()));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// One connection per step, so a step that makes Firecracker close its connection does not affect the next.
fn send_steps(
    socket: &Path,
    steps: &[ReplayStep],
    ready: &mut dyn FnMut(Duration) -> Result<(), String>,
) -> Result<Vec<ReplayResponse>, String> {
    let mut out = Vec::new();
    for step in steps {
        match step {
            ReplayStep::GuestReady { ms } => {
                ready(Duration::from_millis(*ms))?;
                out.push(ReplayResponse { status: 0, body: String::new() });
            }
            ReplayStep::Wait { ms } => {
                thread::sleep(Duration::from_millis(*ms));
                out.push(ReplayResponse { status: 0, body: String::new() });
            }
            ReplayStep::Http { method, path, body } => {
                let mut client = Client::connect(socket)?;
                let r = client.call(method, path, body.as_deref())?;
                out.push(ReplayResponse { status: r.status, body: r.body });
            }
        }
    }
    Ok(out)
}

/// SIGTERM, then the jail must be gone; Firecracker may already have exited (a failed snapshot load ends it).
fn stop(control: &mut Session, v: &mut Vesperd, id: &str) -> Result<(), String> {
    if v.child.try_wait().map_err(|e| e.to_string())?.is_none() {
        signal(v, "TERM")?;
    }
    let (code, err, _) = wait_exit(v, TEARDOWN_TIMEOUT)?;
    wait_absent(control, id, Duration::from_secs(10)).map_err(|e| format!("{id} not removed after exit {code:?}: {e}; {err}"))?;
    let _ = stderr(v);
    Ok(())
}
