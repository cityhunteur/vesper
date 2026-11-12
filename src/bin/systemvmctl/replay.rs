//! `systemvmctl control replay-suite`: the behaviour-matrix replay on Backend A (ADR 0007 slice 6). It joins the
//! matrix-agnostic runner in `vesper::control` to the scripts and the judge in `vesper::api_replay`.

use serde::Serialize;
use std::process::Command;
use vesper::api_replay::{self, CaseResult, ReplaySummary, Response, Step, Verdict};
use vesper::control::{ControlEndpoint, ReplayRun, ReplayStep};

/// What the committed summary records about the run (the baseline requirements in CLAUDE.md).
#[derive(Serialize)]
struct Metadata {
    firecracker_revision: String,
    backend: &'static str,
    host_os: String,
    host_cpu: String,
    timestamp: String,
    git_sha: String,
    scripts: String,
    matrix: String,
}

#[derive(Serialize)]
struct Report<'a> {
    metadata: Metadata,
    summary: ReplaySummary,
    fixture: String,
    cases: &'a [CaseResult],
}

fn write_text(path: &str, text: &str) -> Result<(), String> {
    if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("write {path}: {e}"))
}

fn output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn to_runner(steps: &[Step]) -> Vec<ReplayStep> {
    steps
        .iter()
        .map(|s| match s {
            Step::Http { method, path, body } => ReplayStep::Http { method: method.clone(), path: path.clone(), body: body.clone() },
            Step::Wait { ms } => ReplayStep::Wait { ms: *ms },
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    endpoint: &ControlEndpoint,
    run: &ReplayRun,
    scripts_path: &str,
    matrix_path: &str,
    filter: Option<&str>,
    id: &str,
    report_path: &str,
    publish: Option<&str>,
) -> Result<(), String> {
    let scripts = api_replay::load_replay_scripts(scripts_path)?;
    let matrix = vesper::api_behavior_matrix::load_api_behavior_matrix(matrix_path)?;
    let errors = api_replay::validate_replay_scripts(&scripts, &matrix);
    if !errors.is_empty() {
        return Err(format!("replay scripts:\n  {}", errors.join("\n  ")));
    }
    let files: Vec<(String, u64)> = scripts.files.iter().map(|f| (f.path.clone(), f.bytes)).collect();
    let mut session = run.prepare(endpoint, &files)?;
    let fixture_steps = to_runner(&api_replay::expand_fixture(&scripts, "")?);
    let fixture = run.make_fixture(
            &mut session,
            &format!("{id}-fx"),
            id,
            &fixture_steps,
            (api_replay::FIXTURE_STATE, api_replay::FIXTURE_MEM),
            (api_replay::SNAPSHOT_STATE, api_replay::SNAPSHOT_MEM),
        ).map_err(|e| format!("snapshot fixture: {e}"))?;
    println!("fixture: {fixture}");

    let mut n = 0usize;
    let mut runner = |key: &str, steps: &[Step], snapshot: bool| -> Result<Vec<Response>, String> {
        n += 1;
        let case_id = format!("{id}-{n}");
        let started = std::time::Instant::now();
        let out = run.run_case(&mut session, &case_id, &to_runner(steps), snapshot, &mut |_, _| Ok(()));
        println!("  {:>3} {key} ({} ms){}", n, started.elapsed().as_millis(), out.as_ref().err().map(|e| format!(" ERROR {e}")).unwrap_or_default());
        out.map(|rs| rs.into_iter().map(|r| Response { status: r.status, body: r.body }).collect())
    };
    let accept = |key: &str| filter.is_none_or(|f| key.contains(f));
    let results = api_replay::replay(&scripts, &matrix, "", std::env::consts::ARCH, false, &accept, &mut runner);
    let cleanup = run.finish(&mut session, id);
    let summary = api_replay::summarize(&results);

    let metadata = Metadata {
        firecracker_revision: matrix.firecracker_revision.clone(),
        backend: "systemvm",
        host_os: format!("macOS {}", output("sw_vers", &["-productVersion"])),
        host_cpu: output("sysctl", &["-n", "machdep.cpu.brand_string"]),
        timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        git_sha: output("git", &["rev-parse", "HEAD"]),
        scripts: scripts_path.to_string(),
        matrix: matrix_path.to_string(),
    };
    let report = Report { metadata, summary: summary.clone(), fixture, cases: &results };
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n";
    write_text(report_path, &text)?;
    if let Some(path) = publish {
        if filter.is_some() {
            return Err("--publish needs every case; drop --filter".to_string());
        }
        write_text(path, &text)?;
    }
    for r in results.iter().filter(|r| !matches!(r.verdict, Verdict::Match | Verdict::NotApplicable)) {
        println!("{:?}: {} — {}", r.verdict, r.key, r.detail);
    }
    println!(
        "replay: {} cases, {} match, {} mismatch, {} setup failed, {} backend error, {} not applicable (report: {report_path})",
        summary.cases, summary.matched, summary.mismatched, summary.setup_failed, summary.backend_error, summary.not_applicable
    );
    cleanup?;
    if summary.backend_error > 0 {
        return Err(format!("{} cases could not run on Backend A", summary.backend_error));
    }
    Ok(())
}
