//! The upstream Firecracker suite: its pins in the baseline lock and the comparator that explains every result
//! (GH #172, D-023).
//!
//! A run on Backend A produces `{nodeid: {outcome, message}}`. A test needs explaining when it did not pass here but
//! passed on the reference. The reference is the Linux pass set (CE-0003) when one is given; without one, every test
//! is taken to pass there, so every failure, error, hang and skip needs explaining. Each such test must match an
//! exception class (node-id patterns, message substrings, the outcomes it may explain and an optional ceiling on how
//! many it explains), or the comparison fails. Patterns use `*` as a wildcard; there is no regular-expression engine,
//! so no new dependency. A comparison also fails on an empty run, on a pytest exit status that is not 0 or 1 or on a
//! collection error, and, on a full run, on a reference test missing from the run or a non-flaky class that explained
//! nothing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `baselines.firecracker.upstream_suite` in the lock: what a run fetches and how it runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamSuite {
    /// The upstream build container, as `tools/devtool` pins it, by digest (`name:tag@sha256:...`).
    pub container: String,
    /// The small image the runner stages files and loads modules with, by digest.
    pub helper_image: String,
    /// The public bucket and prefix of the CI artefacts (guest kernels and root filesystems).
    pub artifacts_bucket: String,
    pub artifacts_prefix: String,
    /// The release tarball whose binaries the tests run.
    pub release_asset: String,
    /// Test directories, relative to `tests/` in the upstream tree.
    pub paths: Vec<String>,
    /// pytest-xdist workers.
    pub workers: u32,
    /// Per-test timeout.
    pub test_timeout_s: u64,
}

impl UpstreamSuite {
    pub fn validate(&self, arch: &str) -> Result<(), String> {
        for (name, value) in [
            ("container", &self.container),
            ("helper_image", &self.helper_image),
            ("artifacts_bucket", &self.artifacts_bucket),
            ("artifacts_prefix", &self.artifacts_prefix),
            ("release_asset", &self.release_asset),
        ] {
            if value.trim().is_empty() || value.contains(char::is_whitespace) {
                return Err(format!("upstream_suite.{name} must be a non-blank word"));
            }
        }
        for (name, image) in [("container", &self.container), ("helper_image", &self.helper_image)] {
            let digest = image.split_once("@sha256:").map(|(_, d)| d).unwrap_or_default();
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("upstream_suite.{name} {image:?} must be pinned by digest (@sha256:<64 hex>)"));
            }
        }
        if !self.artifacts_prefix.ends_with(&format!("/{arch}")) {
            return Err(format!("upstream_suite.artifacts_prefix {:?} is not for {arch}", self.artifacts_prefix));
        }
        if !self.release_asset.contains(arch) {
            return Err(format!("upstream_suite.release_asset {:?} is not for {arch}", self.release_asset));
        }
        let word = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b));
        if self.paths.is_empty()
            || self.paths.iter().any(|p| !word(p) || p.starts_with('/') || p.starts_with('-') || p.contains(".."))
        {
            return Err("upstream_suite.paths must be relative paths of [A-Za-z0-9_./-], not starting with - or /".to_string());
        }
        if !(1..=8).contains(&self.workers) {
            return Err(format!("upstream_suite.workers {} is outside 1 to 8", self.workers));
        }
        if !(60..=3600).contains(&self.test_timeout_s) {
            return Err(format!("upstream_suite.test_timeout_s {} is outside 60 to 3600", self.test_timeout_s));
        }
        Ok(())
    }
}

/// One test's result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TestResult {
    /// `passed`, `failed`, `error`, `skipped`, `xfailed`, `xpassed` or `hung`.
    pub outcome: String,
    #[serde(default)]
    pub message: String,
    /// Set by the runner's isolated rerun: what this test did in the full run, under load, when `outcome` is the
    /// pass it then gave on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub under_load: Option<Attempt>,
    /// Set by the runner's isolated rerun when the test did not pass on its own either: that attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alone: Option<Attempt>,
}

/// One attempt at a test, beside the result that counts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Attempt {
    pub outcome: String,
    #[serde(default)]
    pub message: String,
}

pub type Results = BTreeMap<String, TestResult>;

/// Outcomes that count as a pass.
pub fn is_pass(outcome: &str) -> bool {
    matches!(outcome, "passed" | "xfailed" | "xpassed")
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClassKind {
    Environment,
    TestAssumption,
    Unexplained,
    Skip,
}

/// One exception class: why a set of tests cannot pass on Backend A.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Class {
    pub id: String,
    pub kind: ClassKind,
    /// Node-id patterns (`*` matches any run of characters); a test matches when any pattern does. Empty exactly
    /// when `any_test` is set.
    #[serde(default)]
    pub nodeid: Vec<String>,
    /// The symptom is not a property of particular tests (it moves between runs), so the class matches any test and
    /// only `message`, `outcomes` and `max` bound it. Needs `message` substrings and no `message_glob`.
    #[serde(default)]
    pub any_test: bool,
    /// Message substrings: the message must contain one of them, or match one of `message_glob`. One of the two is
    /// required, so that a class names a symptom, not a file.
    #[serde(default)]
    pub message: Vec<String>,
    /// Whole-message patterns (`*` matches any run of characters), for a symptom whose only stable part is its shape,
    /// such as `assert * > *`; a substring there would match most assertion failures in the test.
    #[serde(default)]
    pub message_glob: Vec<String>,
    /// Outcomes this class may explain.
    pub outcomes: Vec<String>,
    /// The most tests this class may explain in one run; more is a failure, not an explanation.
    #[serde(default)]
    pub max: Option<usize>,
    /// The class's symptom does not appear in every run, so explaining nothing on a full run is not a failure.
    #[serde(default)]
    pub flaky: bool,
    /// For a CPU vulnerability report: the vulnerabilities (CVE ids or `/sys/devices/system/cpu/vulnerabilities`
    /// names) the class accepts. When set, the message must be complete, must report at least one vulnerability,
    /// and every vulnerability it reports must be in this list, so a new one fails the run.
    #[serde(default)]
    pub vulnerable_allowed: Vec<String>,
    /// The compat exception or tech-debt row that tracks it (`CE-nnnn` or `TD-nnn`).
    pub tracked_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassFile {
    pub version: u32,
    /// Message substrings of results that a pass on the isolated rerun never clears: a test that showed one under
    /// load stays unmatched, however it did alone (D-031: an overshoot outside its class is not a timing effect).
    #[serde(default)]
    pub never_pass_alone: Vec<String>,
    /// The most tests one run may pass only on the isolated rerun; more is a failure. Unset, any number may.
    #[serde(default)]
    pub max_passed_alone: Option<usize>,
    pub classes: Vec<Class>,
}

impl ClassFile {
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        if self.never_pass_alone.iter().any(|m| m.trim().len() < MIN_ANY_MESSAGE) {
            return Err(format!("never_pass_alone entries need at least {MIN_ANY_MESSAGE} characters"));
        }
        if self.classes.iter().filter(|c| c.any_test).count() > 1 {
            return Err("at most one class may be any_test".to_string());
        }
        for c in &self.classes {
            if !seen.insert(c.id.as_str()) {
                return Err(format!("class {} is listed twice", c.id));
            }
            // Each pattern names a test file under integration_tests/ and a test in it, so `*::*` cannot match everything.
            let names_a_test = |p: &str| {
                p.strip_prefix("integration_tests/")
                    .and_then(|rest| rest.split_once("::"))
                    .is_some_and(|(file, _)| file.ends_with(".py") && !file.contains('*'))
            };
            if c.any_test {
                if !c.nodeid.is_empty() || c.message.is_empty() || !c.message_glob.is_empty() {
                    return Err(format!("class {}: any_test takes message substrings, and no nodeid or message_glob", c.id));
                }
                // With no test names, the symptom and the outcomes are all that bound the class.
                if c.message.iter().any(|m| m.trim().len() < MIN_ANY_MESSAGE)
                    || c.outcomes.iter().any(|o| !matches!(o.as_str(), "failed" | "error"))
                {
                    return Err(format!(
                        "class {}: any_test needs messages of at least {MIN_ANY_MESSAGE} characters and only failed or error outcomes",
                        c.id
                    ));
                }
            } else if c.nodeid.is_empty() || !c.nodeid.iter().all(|p| names_a_test(p)) {
                return Err(format!(
                    "class {}: every nodeid pattern must be integration_tests/<file>.py::<test>, with no * in the file",
                    c.id
                ));
            }
            let blank = |v: &Vec<String>| v.iter().any(|m| m.trim().is_empty() || m.chars().all(|ch| ch == '*' || ch == ' '));
            if (c.message.is_empty() && c.message_glob.is_empty()) || blank(&c.message) || blank(&c.message_glob) {
                return Err(format!("class {}: message or message_glob needs at least one non-blank symptom", c.id));
            }
            if c.max.is_none() {
                return Err(format!("class {}: max is required, so a symptom that spreads fails the run", c.id));
            }
            if c.vulnerable_allowed.iter().any(|v| v.trim().is_empty()) {
                return Err(format!("class {}: vulnerable_allowed entries must not be blank", c.id));
            }
            if c.max == Some(0) {
                return Err(format!("class {}: max must be at least 1", c.id));
            }
            if c.outcomes.is_empty() || c.outcomes.iter().any(|o| is_pass(o) || !KNOWN.contains(&o.as_str())) {
                return Err(format!("class {}: outcomes must be from failed, error, skipped, hung", c.id));
            }
            let t = c.tracked_by.as_str();
            let ok = |p: &str, n: usize| t.strip_prefix(p).is_some_and(|d| d.len() == n && d.bytes().all(|b| b.is_ascii_digit()));
            if !(ok("CE-", 4) || ok("TD-", 3)) {
                return Err(format!("class {}: tracked_by {t:?} is not CE-nnnn or TD-nnn", c.id));
            }
            if c.reason.trim().is_empty() {
                return Err(format!("class {}: reason must not be blank", c.id));
            }
        }
        Ok(())
    }
}

const KNOWN: [&str; 4] = ["failed", "error", "skipped", "hung"];
/// The shortest message an `any_test` class or `never_pass_alone` may match on.
const MIN_ANY_MESSAGE: usize = 16;

/// `*`-wildcard match over the whole string.
pub fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || text.len() < first.len() + last.len() || !text.ends_with(last) {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

impl Class {
    pub fn matches(&self, nodeid: &str, r: &TestResult) -> bool {
        self.outcomes.iter().any(|o| *o == r.outcome)
            && (self.any_test || self.nodeid.iter().any(|p| glob(p, nodeid)))
            && (self.message.iter().any(|m| r.message.contains(m.as_str()))
                || self.message_glob.iter().any(|g| glob(g, &r.message)))
            && (self.vulnerable_allowed.is_empty() || self.only_allowed_vulnerabilities(&r.message))
    }

    fn only_allowed_vulnerabilities(&self, message: &str) -> bool {
        if message.ends_with(TRUNCATED) {
            return false;
        }
        let reported = reported_vulnerabilities(message);
        !reported.is_empty() && reported.iter().all(|v| self.vulnerable_allowed.iter().any(|a| a == v))
    }
}

/// The suffix `collect.py` adds to a message it had to shorten.
pub const TRUNCATED: &str = "[truncated]";

/// The vulnerabilities a CPU vulnerability report names: each CVE whose JSON entry (spectre-meltdown-checker
/// `--batch json`) says `"VULNERABLE": true`, and each file under `/sys/devices/system/cpu/vulnerabilities/` it lists
/// (the upstream tests list only the files that say `Vulnerable`).
pub fn reported_vulnerabilities(message: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let squeezed: String = message.chars().filter(|c| !c.is_whitespace()).collect();
    let mut at = 0;
    while let Some(i) = squeezed[at..].find("\"VULNERABLE\":true") {
        let end = at + i;
        // The CVE must be in the same JSON entry; an entry without one is reported as such, and no class allows it.
        let id = squeezed[..end]
            .rfind("\"CVE\":\"")
            .filter(|c| !squeezed[*c..end].contains('}'))
            .map(|c| squeezed[c + 7..].chars().take_while(|ch| *ch != '"').collect::<String>())
            .filter(|id| !id.is_empty());
        out.insert(id.unwrap_or_else(|| "<a vulnerable entry with no CVE>".into()));
        at = end + 1;
    }
    const SYS: &str = "/sys/devices/system/cpu/vulnerabilities/";
    let mut at = 0;
    while let Some(i) = message[at..].find(SYS) {
        let start = at + i + SYS.len();
        let name: String = message[start..].chars().take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_').collect();
        if !name.is_empty() {
            out.insert(name);
        }
        at = start;
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Unmatched {
    pub nodeid: String,
    pub outcome: String,
    pub message: String,
    /// The isolated rerun's result, when the runner made one and it did not pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alone: Option<Attempt>,
}

/// A test settled by its isolated rerun: `outcome` and `message` are its result under load, `class` explains the
/// result alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExplainedAlone {
    pub nodeid: String,
    pub class: String,
    pub outcome: String,
    pub message: String,
}

/// What pytest itself reported about the run, beside the per-test results.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct RunMeta {
    /// pytest's exit status; `None` when the run was stopped before pytest wrote its report.
    #[serde(default)]
    pub exitcode: Option<i64>,
    /// Modules that failed to collect (an import error drops their tests from the run).
    #[serde(default)]
    pub collection_errors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OverMax {
    pub class: String,
    pub matched: usize,
    pub max: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Comparison {
    /// `linux` when a reference was given, else `all-pass`.
    pub reference: String,
    pub total: usize,
    pub passed: usize,
    /// Tests that need explaining, by outcome.
    pub to_explain: BTreeMap<String, usize>,
    /// Explained tests per class id.
    pub per_class: BTreeMap<String, usize>,
    pub unmatched: Vec<Unmatched>,
    /// Tests that matched no class under load and passed when the runner reran them alone: counted as passed, and
    /// listed with what they did under load.
    pub passed_alone: Vec<Unmatched>,
    /// Tests that matched no class under load, failed again when rerun alone, and whose isolated result a class
    /// explains (D-039): counted under that class, and listed with what they did under load.
    pub explained_alone: Vec<ExplainedAlone>,
    /// The class file's cap on `passed_alone` and `explained_alone` together: the tests a rerun alone may settle.
    pub max_passed_alone: Option<usize>,
    /// Classes that explained more tests than their `max`.
    pub over_max: Vec<OverMax>,
    /// Classes that explained no test in this run.
    pub stale_classes: Vec<String>,
    /// Stale classes not marked `flaky`: fatal on a full run.
    pub stale_required: Vec<String>,
    /// A selection of tests, so stale classes and tests missing from the run are not failures.
    pub partial: bool,
    /// pytest's own verdict on the run, when given.
    pub meta: Option<RunMeta>,
    /// Tests that did not pass on the reference either, so need no class.
    pub not_passing_on_reference: usize,
    /// Tests in the reference that this run did not select or report.
    pub missing_from_run: usize,
}

impl Comparison {
    /// Every reason the comparison fails; empty when it passes.
    pub fn failures(&self) -> Vec<String> {
        let mut f = Vec::new();
        if self.total == 0 {
            f.push("the run has no results".to_string());
        }
        if !self.unmatched.is_empty() {
            f.push(format!("{} result(s) match no exception class", self.unmatched.len()));
        }
        let settled = self.passed_alone.len() + self.explained_alone.len();
        if let Some(max) = self.max_passed_alone.filter(|m| settled > *m) {
            f.push(format!("{settled} tests were settled only by the isolated rerun, more than the cap of {max}"));
        }
        for o in &self.over_max {
            f.push(format!("class {} explained {} tests, more than its max of {}", o.class, o.matched, o.max));
        }
        if let Some(m) = &self.meta {
            if m.exitcode.is_none() {
                f.push("pytest's exit code is unknown: the run was stopped before pytest finished".to_string());
            }
            if let Some(code) = m.exitcode.filter(|c| !matches!(c, 0 | 1)) {
                f.push(format!("pytest exited {code} (interrupted, internal error, usage error or nothing collected)"));
            }
            if !m.collection_errors.is_empty() {
                f.push(format!("{} module(s) failed to collect: {}", m.collection_errors.len(), m.collection_errors.join(", ")));
            }
        }
        if !self.partial {
            if self.missing_from_run > 0 {
                f.push(format!("{} reference test(s) are missing from the run", self.missing_from_run));
            }
            if !self.stale_required.is_empty() {
                f.push(format!("classes that explained nothing on a full run: {}", self.stale_required.join(", ")));
            }
        }
        f
    }

    pub fn ok(&self) -> bool {
        self.failures().is_empty()
    }
}

pub fn compare(
    results: &Results,
    reference: Option<&Results>,
    classes: &ClassFile,
    meta: Option<RunMeta>,
    partial: bool,
) -> Comparison {
    let mut out = Comparison {
        reference: if reference.is_some() { "linux" } else { "all-pass" }.to_string(),
        total: results.len(),
        partial,
        meta,
        max_passed_alone: classes.max_passed_alone,
        ..Comparison::default()
    };
    for c in &classes.classes {
        out.per_class.insert(c.id.clone(), 0);
    }
    for (nodeid, r) in results {
        if is_pass(&r.outcome) {
            match &r.under_load {
                // A symptom that a pass alone never clears: the result under load stands, unmatched.
                Some(l) if classes.never_pass_alone.iter().any(|m| l.message.contains(m.as_str())) => {
                    *out.to_explain.entry(l.outcome.clone()).or_default() += 1;
                    out.unmatched.push(Unmatched {
                        nodeid: nodeid.clone(),
                        outcome: l.outcome.clone(),
                        message: head_and_tail(&l.message, 200, 200),
                        alone: Some(Attempt { outcome: r.outcome.clone(), message: String::new() }),
                    });
                }
                Some(l) => {
                    out.passed += 1;
                    out.passed_alone.push(Unmatched {
                        nodeid: nodeid.clone(),
                        outcome: l.outcome.clone(),
                        message: head_and_tail(&l.message, 200, 200),
                        alone: None,
                    });
                }
                None => out.passed += 1,
            }
            continue;
        }
        if let Some(reference) = reference {
            // Not passing on Linux: the reference cannot ask more of Backend A. A test the reference never ran still
            // needs explaining, so a renamed or new test cannot fail unnoticed.
            if reference.get(nodeid).is_some_and(|l| !is_pass(&l.outcome)) {
                out.not_passing_on_reference += 1;
                continue;
            }
        }
        *out.to_explain.entry(r.outcome.clone()).or_default() += 1;
        if let Some(c) = classes.classes.iter().find(|c| c.matches(nodeid, r)) {
            *out.per_class.entry(c.id.clone()).or_default() += 1;
            continue;
        }
        // Unmatched under load but failed again alone: the isolated result stands in for the one under load when a
        // class explains it and the result under load is not a symptom a rerun never clears (D-039).
        let alone_class = r.alone.as_ref().filter(|a| !is_pass(&a.outcome)).and_then(|a| {
            if classes.never_pass_alone.iter().any(|m| r.message.contains(m.as_str())) {
                return None;
            }
            let again = TestResult { outcome: a.outcome.clone(), message: a.message.clone(), under_load: None, alone: None };
            classes.classes.iter().find(|c| c.matches(nodeid, &again))
        });
        match alone_class {
            Some(c) => {
                *out.per_class.entry(c.id.clone()).or_default() += 1;
                out.explained_alone.push(ExplainedAlone {
                    nodeid: nodeid.clone(),
                    class: c.id.clone(),
                    outcome: r.outcome.clone(),
                    message: head_and_tail(&r.message, 200, 200),
                });
            }
            None => out.unmatched.push(Unmatched {
                nodeid: nodeid.clone(),
                outcome: r.outcome.clone(),
                message: head_and_tail(&r.message, 200, 200),
                alone: r.alone.as_ref().map(|a| Attempt { outcome: a.outcome.clone(), message: head_and_tail(&a.message, 200, 200) }),
            }),
        }
    }
    if let Some(reference) = reference {
        out.missing_from_run = reference.keys().filter(|k| !results.contains_key(*k)).count();
    }
    out.stale_classes = out.per_class.iter().filter(|(_, n)| **n == 0).map(|(k, _)| k.clone()).collect();
    for c in &classes.classes {
        let n = out.per_class.get(&c.id).copied().unwrap_or(0);
        if let Some(max) = c.max.filter(|m| n > *m) {
            out.over_max.push(OverMax { class: c.id.clone(), matched: n, max });
        }
        // With a reference, a class may explain nothing because the reference exempts its tests; that is not stale.
        if n == 0 && !c.flaky && reference.is_none() {
            out.stale_required.push(c.id.clone());
        }
    }
    out
}

/// The first `head` and last `tail` characters of `s`, joined by ` … ` when the middle is dropped: the symptom of a
/// long message is often at its end (an SSH command's stderr).
fn head_and_tail(s: &str, head: usize, tail: usize) -> String {
    let n = s.chars().count();
    if n <= head + tail {
        return s.to_string();
    }
    let h: String = s.chars().take(head).collect();
    let t: String = s.chars().skip(n - tail).collect();
    format!("{h} … {t}")
}

#[cfg(test)]
mod tests;
