//! Behaviour-matrix replay (ADR 0007 slice 6): each case of `docs/generated/firecracker-api-behavior-matrix.json`
//! gets a script of API requests in `docs/specs/firecracker-api-behavior-replay.yaml`, and this module turns the
//! script into steps and judges the last response against the case.
//!
//! The module knows no backend. A backend runs one case as a fresh microVM: it creates the script's files under
//! its root, sends the steps in order and returns one response per step. The model backend (`tests/api_replay.rs`)
//! uses a temporary directory as the root; Backend A (`systemvmctl control replay-suite`) uses the jail, whose root
//! is `/`, so `{root}/tmp/x` is `/tmp/x` in the jail. The root is removed from response bodies before judging, so
//! one oracle serves both.

use crate::api_behavior_matrix::{
    ApiBehaviorMatrix, BehaviorArch, BehaviorBodyAssertion, BehaviorBodyAssertionKind, BehaviorCase,
    BehaviorJsonType, BehaviorOutcome,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const DEFAULT_REPLAY_SCRIPTS_PATH: &str = "docs/specs/firecracker-api-behavior-replay.yaml";
/// Macro expansion depth; a deeper chain is a cycle.
const MAX_MACRO_DEPTH: usize = 8;
/// The longest `WAIT` a script may ask for.
pub const MAX_WAIT_MS: u64 = 10_000;
/// Where the snapshot fixture leaves its files, and where cases marked `snapshot` find them (under the root).
pub const FIXTURE_STATE: &str = "/tmp/fixture.vmstate";
pub const FIXTURE_MEM: &str = "/tmp/fixture.mem";
pub const SNAPSHOT_STATE: &str = "/snap/vm.vmstate";
pub const SNAPSHOT_MEM: &str = "/snap/vm.mem";
/// The longest `{pad:N}`.
const MAX_PAD: usize = 200_000;
/// Taps a script set may list: `vesperd --tap` takes at most 8 (`vesper::control::MAX_TAPS`).
pub const MAX_TAPS: usize = 8;

/// A file every case's root holds before the first step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayFile {
    /// Absolute path under the root, for example `/tmp/rootfs.ext4`.
    pub path: String,
    pub bytes: u64,
}

/// The script of one matrix case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseScript {
    #[serde(default)]
    pub setup: Vec<String>,
    pub request: String,
    /// The case needs the snapshot fixture: `{root}/snap/vm.mem` and `{root}/snap/vm.vmstate`.
    #[serde(default)]
    pub snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayScripts {
    pub version: u32,
    pub firecracker_revision: String,
    #[serde(default)]
    pub files: Vec<ReplayFile>,
    /// Tap devices that exist on the host before the first step, as on the reference host the captures came from;
    /// every `host_dev_name` a script names is one of them (CE-0010).
    #[serde(default)]
    pub taps: Vec<String>,
    #[serde(default)]
    pub macros: BTreeMap<String, Vec<String>>,
    /// Keyed `METHOD PATH#case_id`, the path as the swagger template names it.
    pub cases: BTreeMap<String, CaseScript>,
    /// Steps that leave a paused microVM's snapshot at `FIXTURE_STATE` and `FIXTURE_MEM`; a backend runs them once
    /// and gives the files to every case marked `snapshot` at `SNAPSHOT_STATE` and `SNAPSHOT_MEM`.
    #[serde(default)]
    pub snapshot_fixture: Vec<String>,
}

/// One step a backend performs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Http { method: String, path: String, body: Option<String> },
    Wait { ms: u64 },
}

/// What a backend saw for one `Step::Http` (a `Wait` returns status 0 and no body).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The last response satisfies the case.
    Match,
    /// The last response differs from the case.
    Mismatch,
    /// A setup step did not return 2xx, so the case was not reached.
    SetupFailed,
    /// The backend could not run the case (for example the microVM did not start).
    BackendError,
    /// The case is for another architecture.
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseResult {
    pub key: String,
    pub verdict: Verdict,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub cases: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub setup_failed: usize,
    pub backend_error: usize,
    pub not_applicable: usize,
}

pub fn case_key(method: &str, path: &str, id: &str) -> String {
    format!("{method} {path}#{id}")
}

pub fn load_replay_scripts(path: impl AsRef<Path>) -> Result<ReplayScripts, String> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    serde_yaml::from_str(&text).map_err(|err| format!("parse {}: {err}", path.display()))
}

/// Every matrix case has exactly one script, every script names a case, and every script expands.
pub fn validate_replay_scripts(scripts: &ReplayScripts, matrix: &ApiBehaviorMatrix) -> Vec<String> {
    let mut errors = Vec::new();
    if scripts.version != 1 {
        errors.push(format!("version {} (want 1)", scripts.version));
    }
    if scripts.firecracker_revision != matrix.firecracker_revision {
        errors.push(format!(
            "firecracker_revision {} differs from the matrix's {}",
            scripts.firecracker_revision, matrix.firecracker_revision
        ));
    }
    let mut paths = BTreeSet::new();
    for file in &scripts.files {
        if !valid_root_path(&file.path) {
            errors.push(format!("file {:?}: want an absolute path of plain components", file.path));
        }
        if !paths.insert(file.path.clone()) {
            errors.push(format!("file {:?} listed twice", file.path));
        }
    }
    let mut taps = BTreeSet::new();
    if scripts.taps.len() > MAX_TAPS {
        errors.push(format!("{} taps exceed {MAX_TAPS}", scripts.taps.len()));
    }
    for tap in &scripts.taps {
        let ok = !tap.is_empty()
            && tap.len() <= 15
            && !tap.starts_with('-')
            && tap.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !ok || tap == "lo" {
            errors.push(format!("tap {tap:?}: want 1 to 15 letters, digits, _ and -, not starting with -, not lo"));
        }
        if !taps.insert(tap.clone()) {
            errors.push(format!("tap {tap:?} listed twice"));
        }
    }
    let keys: BTreeSet<String> = matrix
        .operations
        .iter()
        .flat_map(|op| op.cases.iter().map(move |case| case_key(&op.method, &op.path, &case.id)))
        .collect();
    for key in &keys {
        if !scripts.cases.contains_key(key) {
            errors.push(format!("{key}: no script"));
        }
    }
    for key in scripts.cases.keys() {
        if !keys.contains(key) {
            errors.push(format!("{key}: script for no matrix case"));
        }
        match expand(scripts, key, "") {
            Ok(steps) => {
                for name in host_dev_names(&steps) {
                    if !taps.contains(&name) {
                        errors.push(format!("{key}: host_dev_name {name:?} is not in taps"));
                    }
                }
            }
            Err(err) => errors.push(format!("{key}: {err}")),
        }
    }
    if scripts.cases.values().any(|c| c.snapshot) {
        match expand_fixture(scripts, "") {
            Ok(steps) if !steps.is_empty() => {
                for name in host_dev_names(&steps) {
                    if !taps.contains(&name) {
                        errors.push(format!("snapshot_fixture: host_dev_name {name:?} is not in taps"));
                    }
                }
            }
            Ok(_) => errors.push("cases need the snapshot fixture, but snapshot_fixture is empty".to_string()),
            Err(err) => errors.push(format!("snapshot_fixture: {err}")),
        }
    }
    errors
}

/// Every `host_dev_name` string in the steps' JSON bodies, at any depth (`PUT /snapshot/load` names them inside
/// `network_overrides`).
fn host_dev_names(steps: &[Step]) -> Vec<String> {
    fn walk(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, v) in map {
                    match (key.as_str(), v.as_str()) {
                        ("host_dev_name", Some(name)) => out.push(name.to_string()),
                        _ => walk(v, out),
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for step in steps {
        if let Step::Http { body: Some(body), .. } = step {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
                walk(&value, &mut out);
            }
        }
    }
    out
}

fn valid_root_path(path: &str) -> bool {
    path.strip_prefix('/').is_some_and(|rest| {
        !rest.is_empty() && rest.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
    })
}

/// The snapshot fixture's steps with `{root}` replaced.
pub fn expand_fixture(scripts: &ReplayScripts, root: &str) -> Result<Vec<Step>, String> {
    let mut lines = Vec::new();
    for line in &scripts.snapshot_fixture {
        expand_line(scripts, line, 0, &mut lines)?;
    }
    lines.iter().map(|line| parse_step(&expand_pads(&line.replace("{root}", root))?)).collect()
}

/// The case's steps with `{root}` replaced; the last step is the request under test.
pub fn expand(scripts: &ReplayScripts, key: &str, root: &str) -> Result<Vec<Step>, String> {
    let script = scripts.cases.get(key).ok_or_else(|| format!("no script for {key}"))?;
    let mut lines = Vec::new();
    for line in &script.setup {
        expand_line(scripts, line, 0, &mut lines)?;
    }
    if script.request.starts_with('@') || script.request.starts_with("WAIT") {
        return Err("the request must be one HTTP step".to_string());
    }
    lines.push(script.request.clone());
    let (method, path) = key.split_once('#').map(|(op, _)| op).and_then(|op| op.split_once(' ')).ok_or("bad key")?;
    let steps = lines.iter().map(|line| parse_step(&expand_pads(&line.replace("{root}", root))?)).collect::<Result<Vec<_>, _>>()?;
    match steps.last() {
        Some(Step::Http { method: m, path: p, .. }) if m == method && path_matches(path, p) => Ok(steps),
        _ => Err(format!("the request {:?} is not {method} {path}", script.request)),
    }
}

fn expand_line(scripts: &ReplayScripts, line: &str, depth: usize, out: &mut Vec<String>) -> Result<(), String> {
    let Some(name) = line.strip_prefix('@') else {
        out.push(line.to_string());
        return Ok(());
    };
    if depth >= MAX_MACRO_DEPTH {
        return Err(format!("macro @{name} nests deeper than {MAX_MACRO_DEPTH}"));
    }
    let body = scripts.macros.get(name).ok_or_else(|| format!("unknown macro @{name}"))?;
    for each in body {
        expand_line(scripts, each, depth + 1, out)?;
    }
    Ok(())
}

/// `{pad:N}` becomes N letters `a` and `{rep:N:TEXT}` becomes N copies of TEXT (which has no `}`), for bodies
/// near a size limit.
fn expand_pads(line: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = line;
    loop {
        let at = match (rest.find("{pad:"), rest.find("{rep:")) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => break,
        };
        out.push_str(&rest[..at]);
        let tail = &rest[at + 5..];
        let end = tail.find('}').ok_or("unterminated placeholder")?;
        let (count, text) = if rest[at..].starts_with("{pad:") {
            (&tail[..end], "a")
        } else {
            tail[..end].split_once(':').ok_or_else(|| format!("bad {{rep:{}}}", &tail[..end]))?
        };
        let n: usize = count.parse().map_err(|_| format!("bad count {count:?}"))?;
        if n.saturating_mul(text.len()) > MAX_PAD {
            return Err(format!("placeholder expands past {MAX_PAD} bytes"));
        }
        out.push_str(&text.repeat(n));
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `METHOD /path [JSON]` or `WAIT <ms>`.
pub fn parse_step(line: &str) -> Result<Step, String> {
    let line = line.trim();
    if let Some(ms) = line.strip_prefix("WAIT ") {
        let ms: u64 = ms.trim().parse().map_err(|_| format!("bad WAIT {line:?}"))?;
        if ms > MAX_WAIT_MS {
            return Err(format!("WAIT {ms} exceeds {MAX_WAIT_MS} ms"));
        }
        return Ok(Step::Wait { ms });
    }
    let (method, rest) = line.split_once(' ').ok_or_else(|| format!("bad step {line:?}"))?;
    if !matches!(method, "GET" | "PUT" | "PATCH") {
        return Err(format!("bad method in {line:?}"));
    }
    let (path, body) = match rest.split_once(' ') {
        Some((path, body)) => (path, Some(body.trim().to_string())),
        None => (rest, None),
    };
    if !path.starts_with('/') {
        return Err(format!("bad path in {line:?}"));
    }
    if let Some(body) = &body {
        serde_json::from_str::<serde_json::Value>(body).map_err(|err| format!("body of {line:?} is not JSON: {err}"))?;
    }
    Ok(Step::Http { method: method.to_string(), path: path.to_string(), body })
}

/// `/drives/{drive_id}` matches `/drives/rootfs`.
fn path_matches(template: &str, path: &str) -> bool {
    let t: Vec<&str> = template.split('/').collect();
    let p: Vec<&str> = path.split('/').collect();
    t.len() == p.len() && t.iter().zip(&p).all(|(t, p)| (t.starts_with('{') && t.ends_with('}') && !p.is_empty()) || t == p)
}

/// Whether the case applies on `arch` (`std::env::consts::ARCH`).
pub fn applies(case: &BehaviorCase, arch: &str) -> bool {
    match case.arch {
        BehaviorArch::Any => true,
        BehaviorArch::X86_64 => arch == "x86_64",
        BehaviorArch::Aarch64 => arch == "aarch64",
    }
}

/// Judges one case from the responses to its steps. `root` is removed from bodies; `check_code` also compares the
/// model's `code` field, which Firecracker does not send.
pub fn judge(case: &BehaviorCase, steps: &[Step], responses: &[Response], root: &str, check_code: bool) -> (Verdict, String) {
    let http: Vec<(usize, &Step)> = steps.iter().enumerate().filter(|(_, s)| matches!(s, Step::Http { .. })).collect();
    if responses.len() != steps.len() || http.is_empty() {
        return (Verdict::BackendError, format!("{} responses for {} steps", responses.len(), steps.len()));
    }
    let last = steps.len() - 1;
    for (i, step) in &http {
        if *i == last {
            break;
        }
        let r = &responses[*i];
        if !(200..300).contains(&r.status) {
            let Step::Http { method, path, .. } = step else { unreachable!() };
            return (Verdict::SetupFailed, format!("setup {method} {path}: {} {}", r.status, strip_root(&r.body, root)));
        }
    }
    let r = &responses[last];
    let body = strip_root(&r.body, root);
    match judge_response(case, r.status, &body, check_code) {
        Ok(detail) => (Verdict::Match, detail),
        Err(detail) => (Verdict::Mismatch, detail),
    }
}

fn strip_root(body: &str, root: &str) -> String {
    if root.is_empty() { body.to_string() } else { body.replace(root, "") }
}

fn judge_response(case: &BehaviorCase, status: u16, body: &str, check_code: bool) -> Result<String, String> {
    if status != case.status_code {
        return Err(format!("status {status} (want {}): {}", case.status_code, clip(body)));
    }
    match case.outcome {
        BehaviorOutcome::NoContent => {
            if body.trim().is_empty() { Ok(format!("{status}")) } else { Err(format!("{status} with body {}", clip(body))) }
        }
        BehaviorOutcome::ErrorEnvelope => {
            let json: serde_json::Value = serde_json::from_str(body).map_err(|_| format!("{status} body is not JSON: {}", clip(body)))?;
            let got = json.get("fault_message").and_then(|v| v.as_str()).ok_or_else(|| format!("no fault_message: {}", clip(body)))?;
            let want = case.message.as_deref().unwrap_or_default();
            if got != want {
                return Err(format!("fault_message {got:?} (want {want:?})"));
            }
            if check_code {
                let code = json.get("code").and_then(|v| v.as_str());
                if code != case.code.as_deref() {
                    return Err(format!("code {code:?} (want {:?})", case.code));
                }
            }
            Ok(format!("{status} {got:?}"))
        }
        BehaviorOutcome::SuccessBody => {
            let json: serde_json::Value = serde_json::from_str(body).map_err(|_| format!("{status} body is not JSON: {}", clip(body)))?;
            for assertion in &case.body_assertions {
                check_assertion(&json, assertion).map_err(|e| format!("{e}; body {}", clip(body)))?;
            }
            Ok(format!("{status}, {} assertions", case.body_assertions.len()))
        }
    }
}

fn check_assertion(json: &serde_json::Value, a: &BehaviorBodyAssertion) -> Result<(), String> {
    let found = json.pointer(&a.pointer);
    match a.kind {
        BehaviorBodyAssertionKind::Absent => match found {
            None => Ok(()),
            Some(v) => Err(format!("{} is {v} (want absent)", a.pointer)),
        },
        BehaviorBodyAssertionKind::Exact => match (found, &a.value) {
            (Some(v), Some(want)) if v == want => Ok(()),
            (v, want) => Err(format!("{} is {} (want {})", a.pointer, show(v), show(want.as_ref()))),
        },
        BehaviorBodyAssertionKind::Type => {
            let ok = match (found, a.value_type) {
                (Some(v), Some(t)) => match t {
                    BehaviorJsonType::Null => v.is_null(),
                    BehaviorJsonType::Boolean => v.is_boolean(),
                    BehaviorJsonType::Number => v.is_number(),
                    BehaviorJsonType::String => v.is_string(),
                    BehaviorJsonType::Array => v.is_array(),
                    BehaviorJsonType::Object => v.is_object(),
                },
                _ => false,
            };
            if ok { Ok(()) } else { Err(format!("{} is {} (want type {:?})", a.pointer, show(found), a.value_type)) }
        }
    }
}

fn show(v: Option<&serde_json::Value>) -> String {
    v.map_or_else(|| "absent".to_string(), |v| v.to_string())
}

fn clip(s: &str) -> String {
    let s = s.trim();
    if s.len() <= 400 {
        return s.to_string();
    }
    let mut end = 400;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Replays every case of `matrix` that `filter` accepts. `run(key, steps, snapshot)` runs one case on a fresh
/// microVM and returns one response per step, or an error if the backend could not run it.
pub fn replay(
    scripts: &ReplayScripts,
    matrix: &ApiBehaviorMatrix,
    root: &str,
    arch: &str,
    check_code: bool,
    filter: &dyn Fn(&str) -> bool,
    run: &mut dyn FnMut(&str, &[Step], bool) -> Result<Vec<Response>, String>,
) -> Vec<CaseResult> {
    let mut results = Vec::new();
    for op in &matrix.operations {
        for case in &op.cases {
            let key = case_key(&op.method, &op.path, &case.id);
            if !filter(&key) {
                continue;
            }
            if !applies(case, arch) {
                results.push(CaseResult { key, verdict: Verdict::NotApplicable, detail: format!("{:?} case on {arch}", case.arch), status: None, body: None });
                continue;
            }
            let steps = match expand(scripts, &key, root) {
                Ok(steps) => steps,
                Err(err) => {
                    results.push(CaseResult { key, verdict: Verdict::BackendError, detail: err, status: None, body: None });
                    continue;
                }
            };
            let snapshot = scripts.cases.get(&key).is_some_and(|s| s.snapshot);
            let (verdict, detail, last) = match run(&key, &steps, snapshot) {
                Ok(responses) => {
                    let (verdict, detail) = judge(case, &steps, &responses, root, check_code);
                    (verdict, detail, responses.last().cloned())
                }
                Err(err) => (Verdict::BackendError, err, None),
            };
            results.push(CaseResult {
                key,
                verdict,
                detail,
                status: last.as_ref().map(|r| r.status),
                body: last.map(|r| clip(&strip_root(&r.body, root))).filter(|b| !b.is_empty()),
            });
        }
    }
    results
}

pub fn summarize(results: &[CaseResult]) -> ReplaySummary {
    let count = |v: Verdict| results.iter().filter(|r| r.verdict == v).count();
    ReplaySummary {
        cases: results.len(),
        matched: count(Verdict::Match),
        mismatched: count(Verdict::Mismatch),
        setup_failed: count(Verdict::SetupFailed),
        backend_error: count(Verdict::BackendError),
        not_applicable: count(Verdict::NotApplicable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(outcome: BehaviorOutcome, status: u16) -> BehaviorCase {
        BehaviorCase {
            id: "c".into(),
            arch: BehaviorArch::Any,
            when: String::new(),
            status_code: status,
            outcome,
            code: Some("InvalidState".into()),
            message: Some("nope".into()),
            body_assertions: vec![],
            evidence_files: vec![],
        }
    }

    fn scripts(setup: &[&str], request: &str) -> ReplayScripts {
        let mut cases = BTreeMap::new();
        cases.insert("PUT /drives/{drive_id}#c".to_string(), CaseScript { setup: setup.iter().map(|s| s.to_string()).collect(), request: request.into(), snapshot: false });
        let mut macros = BTreeMap::new();
        macros.insert("boot".to_string(), vec!["PUT /boot-source {\"kernel_image_path\":\"{root}/Image\"}".to_string(), "@start".to_string()]);
        macros.insert("start".to_string(), vec!["PUT /actions {\"action_type\":\"InstanceStart\"}".to_string()]);
        macros.insert("loop".to_string(), vec!["@loop".to_string()]);
        ReplayScripts { version: 1, firecracker_revision: "r".into(), files: vec![], taps: vec![], macros, cases, snapshot_fixture: vec![] }
    }

    #[test]
    fn expands_macros_and_root() {
        let s = scripts(&["@boot", "WAIT 5"], "PUT /drives/rootfs {\"path_on_host\":\"{root}/tmp/r\"}");
        let steps = expand(&s, "PUT /drives/{drive_id}#c", "/t").unwrap();
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0], Step::Http { method: "PUT".into(), path: "/boot-source".into(), body: Some("{\"kernel_image_path\":\"/t/Image\"}".into()) });
        assert_eq!(steps[2], Step::Wait { ms: 5 });
        assert!(matches!(&steps[3], Step::Http { body: Some(b), .. } if b.contains("/t/tmp/r")));
    }

    #[test]
    fn refuses_bad_scripts() {
        assert!(expand(&scripts(&["@loop"], "PUT /drives/x {}"), "PUT /drives/{drive_id}#c", "").unwrap_err().contains("deeper"));
        assert!(expand(&scripts(&["@nope"], "PUT /drives/x {}"), "PUT /drives/{drive_id}#c", "").unwrap_err().contains("unknown macro"));
        assert!(expand(&scripts(&[], "PUT /pmem/x {}"), "PUT /drives/{drive_id}#c", "").unwrap_err().contains("is not PUT"));
        assert!(expand(&scripts(&[], "PUT /drives/x {nope"), "PUT /drives/{drive_id}#c", "").unwrap_err().contains("not JSON"));
        assert!(parse_step("WAIT 999999").is_err());
        assert!(parse_step("DELETE /x").is_err());
        assert_eq!(expand_pads("{\"a\":\"{pad:3}\"}").unwrap(), "{\"a\":\"aaa\"}");
        assert!(expand_pads("{pad:x}").is_err());
        assert_eq!(expand_pads("[{rep:3:1e2,}0]").unwrap(), "[1e2,1e2,1e2,0]");
        assert!(expand_pads("{rep:3}").is_err());
    }

    #[test]
    fn judges_outcomes() {
        let steps = vec![parse_step("PUT /actions {}").unwrap(), parse_step("PUT /drives/x {}").unwrap()];
        let ok = |status, body: &str| vec![Response { status: 204, body: String::new() }, Response { status, body: body.into() }];
        let err = case(BehaviorOutcome::ErrorEnvelope, 400);
        assert_eq!(judge(&err, &steps, &ok(400, r#"{"fault_message":"nope"}"#), "", false).0, Verdict::Match);
        assert_eq!(judge(&err, &steps, &ok(400, r#"{"fault_message":"nope"}"#), "", true).0, Verdict::Mismatch);
        assert_eq!(judge(&err, &steps, &ok(500, r#"{"fault_message":"nope"}"#), "", false).0, Verdict::Mismatch);
        let none = case(BehaviorOutcome::NoContent, 204);
        assert_eq!(judge(&none, &steps, &ok(204, ""), "", false).0, Verdict::Match);
        let failed = vec![Response { status: 400, body: "{}".into() }, Response { status: 204, body: String::new() }];
        assert_eq!(judge(&none, &steps, &failed, "", false).0, Verdict::SetupFailed);
        assert_eq!(judge(&none, &steps, &failed[..1], "", false).0, Verdict::BackendError);
    }

    #[test]
    fn judges_assertions_after_stripping_root() {
        let mut c = case(BehaviorOutcome::SuccessBody, 200);
        c.body_assertions = vec![
            BehaviorBodyAssertion { pointer: "/p".into(), kind: BehaviorBodyAssertionKind::Exact, value: Some(serde_json::json!("/tmp/x")), value_type: None },
            BehaviorBodyAssertion { pointer: "/n".into(), kind: BehaviorBodyAssertionKind::Type, value: None, value_type: Some(BehaviorJsonType::Null) },
            BehaviorBodyAssertion { pointer: "/gone".into(), kind: BehaviorBodyAssertionKind::Absent, value: None, value_type: None },
        ];
        let steps = vec![parse_step("GET /vm/config").unwrap()];
        let r = |b: &str| vec![Response { status: 200, body: b.into() }];
        assert_eq!(judge(&c, &steps, &r(r#"{"p":"/root/tmp/x","n":null}"#), "/root", false).0, Verdict::Match);
        assert_eq!(judge(&c, &steps, &r(r#"{"p":"/tmp/x","n":null,"gone":1}"#), "", false).0, Verdict::Mismatch);
        assert_eq!(judge(&c, &steps, &r(r#"{"p":"/tmp/x","n":0}"#), "", false).0, Verdict::Mismatch);
    }
}
