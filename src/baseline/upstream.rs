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
    pub classes: Vec<Class>,
}

impl ClassFile {
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
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
        ..Comparison::default()
    };
    for c in &classes.classes {
        out.per_class.insert(c.id.clone(), 0);
    }
    for (nodeid, r) in results {
        if is_pass(&r.outcome) {
            out.passed += 1;
            if let Some(l) = &r.under_load {
                out.passed_alone.push(Unmatched {
                    nodeid: nodeid.clone(),
                    outcome: l.outcome.clone(),
                    message: head_and_tail(&l.message, 200, 200),
                    alone: None,
                });
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
        match classes.classes.iter().find(|c| c.matches(nodeid, r)) {
            Some(c) => *out.per_class.entry(c.id.clone()).or_default() += 1,
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
mod tests {
    use super::*;

    #[test]
    fn an_any_test_class_matches_by_symptom_and_a_pass_alone_is_listed() {
        let mut v: serde_yaml::Value = serde_yaml::from_str(
            r#"
version: 1
classes:
  - id: overshoot
    kind: unexplained
    any_test: true
    message: ["Memory usage (6."]
    outcomes: [error]
    max: 1
    tracked_by: CE-0001
    reason: r
"#,
        )
        .unwrap();
        let c: ClassFile = serde_yaml::from_value(v.clone()).unwrap();
        c.validate().unwrap();
        let mut res = Results::new();
        res.insert("integration_tests/functional/a.py::t".into(), r("error", "Memory usage (6.5 MiB)"));
        res.insert("integration_tests/functional/b.py::t".into(), r("error", "Memory usage (7.5 MiB)"));
        let mut alone = r("passed", "");
        alone.under_load = Some(Attempt { outcome: "failed".into(), message: "AssertionError".into() });
        res.insert("integration_tests/functional/c.py::t".into(), alone);
        let out = compare(&res, None, &c, None, false);
        assert_eq!(out.per_class["overshoot"], 1);
        assert_eq!(out.unmatched.len(), 1);
        assert_eq!(out.unmatched[0].nodeid, "integration_tests/functional/b.py::t");
        assert_eq!((out.passed, out.passed_alone.len()), (1, 1));
        assert_eq!(out.passed_alone[0].outcome, "failed");
        // any_test with a nodeid list or a message glob is refused.
        v["classes"][0]["message_glob"] = serde_yaml::from_str(r#"["assert * > *"]"#).unwrap();
        let c: ClassFile = serde_yaml::from_value(v).unwrap();
        assert!(c.validate().unwrap_err().contains("any_test"));
    }

    fn r(outcome: &str, message: &str) -> TestResult {
        TestResult { outcome: outcome.into(), message: message.into(), under_load: None, alone: None }
    }

    #[test]
    fn a_vulnerability_class_rejects_one_it_does_not_allow_and_a_truncated_report() {
        let yaml = r#"
version: 1
classes:
  - id: ssb
    kind: environment
    nodeid: ["integration_tests/security/test_vulnerabilities.py::*"]
    message: ["spec_store_bypass", "CVE-2018-3639"]
    outcomes: [failed]
    max: 10
    vulnerable_allowed: [spec_store_bypass, CVE-2018-3639, CVE-2018-3640]
    tracked_by: CE-0001
    reason: Apple CPU
"#;
        let c: ClassFile = serde_yaml::from_str(yaml).unwrap();
        c.validate().unwrap();
        let ssb = &c.classes[0];
        let n = "integration_tests/security/test_vulnerabilities.py::t";
        let host = r#"stdout: [{"NAME":"MELTDOWN","CVE":"CVE-2017-5754","VULNERABLE":false},{"NAME":"VARIANT 4","CVE":"CVE-2018-3639","VULNERABLE":true}]"#;
        assert!(ssb.matches(n, &r("failed", host)));
        let guest = r#"assert {'{"NAME": "VARIANT 3A", "CVE": "CVE-2018-3640", "VULNERABLE": true}', '{"CVE": "CVE-2018-3639", ' + '"VULNERABLE": true}'} == set()"#;
        assert!(ssb.matches(n, &r("failed", guest)));
        assert!(ssb.matches(n, &r("failed", "/sys/devices/system/cpu/vulnerabilities/spec_store_bypass:Vulnerable")));
        let meltdown = host.replace(r#""CVE-2017-5754","VULNERABLE":false"#, r#""CVE-2017-5754","VULNERABLE":true"#);
        assert!(!ssb.matches(n, &r("failed", &meltdown)), "an extra vulnerability must not be explained");
        let two_files = "/sys/devices/system/cpu/vulnerabilities/spec_store_bypass:Vulnerable\n/sys/devices/system/cpu/vulnerabilities/meltdown:Vulnerable";
        assert!(!ssb.matches(n, &r("failed", two_files)));
        assert!(!ssb.matches(n, &r("failed", &format!("{host} {TRUNCATED}"))), "a truncated report proves nothing");
        assert!(!ssb.matches(n, &r("failed", "spec_store_bypass mentioned, nothing reported")));
        let no_cve = host.replace(r#"}]"#, r#"},{"NAME":"NEW","VULNERABLE":true}]"#);
        assert!(!ssb.matches(n, &r("failed", &no_cve)), "an entry without a CVE must not borrow the previous one's");
    }

    fn classes() -> ClassFile {
        let yaml = r#"
version: 1
classes:
  - id: no-uffd
    kind: environment
    nodeid: ["integration_tests/functional/test_uffd.py::*"]
    message: ["UFFD object"]
    outcomes: [failed]
    max: 2
    tracked_by: TD-054
    reason: no CONFIG_USERFAULTFD
  - id: x86-only
    kind: skip
    nodeid: ["integration_tests/functional/test_cpu_template_helper.py::*"]
    message: ["x86"]
    outcomes: [skipped]
    max: 5
    flaky: true
    tracked_by: CE-0001
    reason: x86-only
"#;
        let c: ClassFile = serde_yaml::from_str(yaml).unwrap();
        c.validate().unwrap();
        c
    }

    fn full(res: &Results) -> Comparison {
        compare(res, None, &classes(), None, false)
    }

    #[test]
    fn glob_matches_whole_strings() {
        assert!(glob("a::*", "a::b[x]"));
        assert!(glob("*mid*", "xmidy"));
        assert!(glob("exact", "exact") && !glob("exact", "exactly"));
        assert!(!glob("a*b*c", "acb"));
        assert!(glob("a*b*c", "a-b-c"));
        assert!(!glob("ab*ba", "aba"));
    }

    #[test]
    fn every_non_pass_needs_a_class_without_a_reference() {
        let mut res = Results::new();
        res.insert("integration_tests/functional/test_uffd.py::test_a".into(), r("failed", "UFFD object: System error"));
        res.insert("integration_tests/functional/test_api.py::test_b".into(), r("passed", ""));
        res.insert("integration_tests/functional/test_cpu_template_helper.py::t".into(), r("skipped", "x86"));
        let c = full(&res);
        assert!(c.ok(), "{:?}", c.failures());
        assert_eq!((c.passed, c.per_class["no-uffd"], c.per_class["x86-only"]), (1, 1, 1));

        // An unmatched failure, a hang, and a class whose outcomes or message exclude the result all fail.
        res.insert("integration_tests/functional/test_vsock.py::t".into(), r("hung", ""));
        res.insert("integration_tests/functional/test_uffd.py::test_c".into(), r("error", "UFFD object"));
        res.insert("integration_tests/functional/test_uffd.py::test_d".into(), r("failed", "other"));
        let c = full(&res);
        let names: Vec<&str> = c.unmatched.iter().map(|u| u.nodeid.as_str()).collect();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(!c.ok());
    }

    #[test]
    fn an_empty_run_a_class_over_its_max_and_a_stale_class_fail() {
        let c = full(&Results::new());
        assert!(c.failures().iter().any(|f| f.contains("no results")), "{:?}", c.failures());

        let mut res = Results::new();
        for t in ["a", "b", "c"] {
            res.insert(format!("integration_tests/functional/test_uffd.py::{t}"), r("failed", "UFFD object"));
        }
        let c = full(&res);
        assert_eq!(c.over_max.len(), 1);
        assert!(!c.ok());

        // no-uffd is not flaky: explaining nothing on a full run fails, but not on a partial one. x86-only is flaky.
        let mut res = Results::new();
        res.insert("t::ok".into(), r("passed", ""));
        let c = full(&res);
        assert_eq!(c.stale_required, vec!["no-uffd".to_string()]);
        assert!(!c.ok());
        assert!(compare(&res, None, &classes(), None, true).ok());
    }

    #[test]
    fn pytest_exit_status_and_collection_errors_fail_the_run() {
        let mut res = Results::new();
        res.insert("integration_tests/functional/test_uffd.py::a".into(), r("failed", "UFFD object"));
        let meta = |code: Option<i64>, errs: &[&str]| {
            Some(RunMeta { exitcode: code, collection_errors: errs.iter().map(|e| e.to_string()).collect() })
        };
        assert!(compare(&res, None, &classes(), meta(Some(1), &[]), false).ok());
        // A stopped run has no pytest verdict, and that fails it.
        let stopped = compare(&res, None, &classes(), meta(None, &[]), false);
        assert!(stopped.failures().iter().any(|f| f.contains("exit code is unknown")), "{:?}", stopped.failures());
        assert!(!compare(&res, None, &classes(), meta(Some(3), &[]), false).ok());
        assert!(!compare(&res, None, &classes(), meta(Some(1), &["integration_tests/functional/test_x.py"]), false).ok());
    }

    #[test]
    fn a_message_glob_matches_the_whole_message_and_validation_refuses_catch_alls() {
        let yaml = r#"
version: 1
classes:
  - id: tolerance
    kind: unexplained
    nodeid: ["integration_tests/functional/test_balloon.py::test_hinting_reporting_snapshot*"]
    message_glob: ["assert * > *"]
    outcomes: [failed]
    max: 4
    tracked_by: CE-0001
    reason: timing
"#;
        let c: ClassFile = serde_yaml::from_str(yaml).unwrap();
        c.validate().unwrap();
        let n = "integration_tests/functional/test_balloon.py::test_hinting_reporting_snapshot[x]";
        assert!(c.classes[0].matches(n, &r("failed", "assert 187840 > 192048")));
        assert!(!c.classes[0].matches(n, &r("failed", "AssertionError: assert 1 == 0")), "the shape, not any assertion");
        for (field, bad) in [
            ("nodeid", r#"["*::*"]"#),
            ("nodeid", r#"["integration_tests/*.py::t"]"#),
            ("nodeid", r#"["tests/test_x.py::t"]"#),
            ("message_glob", r#"["* *"]"#),
        ] {
            let mut v: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
            v["classes"][0][field] = serde_yaml::from_str(bad).unwrap();
            let c: ClassFile = serde_yaml::from_value(v).unwrap();
            assert!(c.validate().is_err(), "{field} = {bad} must be refused");
        }
        let mut v: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        v["classes"][0].as_mapping_mut().unwrap().remove("max");
        let c: ClassFile = serde_yaml::from_value(v).unwrap();
        assert!(c.validate().unwrap_err().contains("max is required"));
    }

    #[test]
    fn a_class_whose_tests_the_reference_exempts_is_not_stale() {
        let mut res = Results::new();
        res.insert("integration_tests/functional/test_uffd.py::a".into(), r("failed", "UFFD object"));
        let reference = res.clone();
        let c = compare(&res, Some(&reference), &classes(), Some(RunMeta { exitcode: Some(1), ..RunMeta::default() }), false);
        assert_eq!(c.per_class["no-uffd"], 0);
        assert!(c.stale_required.is_empty() && c.ok(), "{:?}", c.failures());
    }

    #[test]
    fn an_unmatched_message_keeps_its_head_and_its_tail() {
        let long = format!("Command: ssh {} stderr: Connection timed out", "x".repeat(1000));
        let m = head_and_tail(&long, 200, 200);
        assert!(m.starts_with("Command: ssh") && m.ends_with("stderr: Connection timed out") && m.contains(" … "));
        assert_eq!(head_and_tail("short", 200, 200), "short");
    }

    #[test]
    fn a_reference_limits_what_needs_explaining() {
        let mut res = Results::new();
        let mut linux = Results::new();
        res.insert("t::fails_both".into(), r("failed", "x"));
        linux.insert("t::fails_both".into(), r("failed", "x"));
        res.insert("t::skipped_here".into(), r("skipped", "no"));
        linux.insert("t::skipped_here".into(), r("passed", ""));
        res.insert("t::new_here".into(), r("failed", "x"));
        linux.insert("t::not_run_here".into(), r("passed", ""));
        let c = compare(&res, Some(&linux), &classes(), None, false);
        let names: Vec<&str> = c.unmatched.iter().map(|u| u.nodeid.as_str()).collect();
        // `skipped_here` passed there, so it needs a class; `new_here` was never run there, so it needs one too.
        assert_eq!(names, vec!["t::new_here", "t::skipped_here"]);
        assert_eq!((c.not_passing_on_reference, c.missing_from_run), (1, 1));
        assert_eq!(c.reference, "linux");
        assert!(c.failures().iter().any(|f| f.contains("missing from the run")));
        assert!(!compare(&res, Some(&linux), &classes(), None, true).failures().iter().any(|f| f.contains("missing")));
    }

    #[test]
    fn class_files_and_pins_are_validated() {
        let mut c = classes();
        c.classes[0].tracked_by = "CE-1".into();
        assert!(c.validate().unwrap_err().contains("tracked_by"));
        let mut c = classes();
        c.classes[1].outcomes = vec!["passed".into()];
        assert!(c.validate().is_err());
        let mut c = classes();
        c.classes.push(c.classes[0].clone());
        assert!(c.validate().unwrap_err().contains("twice"));
        // A catch-all class: a bare `*`, no message, or a blank message.
        for (nodeid, message) in [(vec!["*"], vec!["x"]), (vec!["t::*"], vec![]), (vec!["t::*"], vec![" "])] {
            let mut c = classes();
            c.classes[0].nodeid = nodeid.into_iter().map(String::from).collect();
            c.classes[0].message = message.into_iter().map(String::from).collect();
            assert!(c.validate().is_err());
        }
        assert!(serde_yaml::from_str::<ClassKind>("harness").is_err());

        let pins = UpstreamSuite {
            container: format!("public.ecr.aws/firecracker/fcuvm:v90@sha256:{}", "a".repeat(64)),
            helper_image: format!("alpine:3.22@sha256:{}", "b".repeat(64)),
            artifacts_bucket: "spec.ccfc.min".into(),
            artifacts_prefix: "firecracker-ci/20260603-e656faf58d6c-0/aarch64".into(),
            release_asset: "firecracker-v1.16.0-aarch64.tgz".into(),
            paths: vec!["integration_tests/functional".into()],
            workers: 3,
            test_timeout_s: 600,
        };
        pins.validate("aarch64").unwrap();
        assert!(pins.validate("x86_64").is_err());
        for p in ["../x", "-k", "a b", "/abs"] {
            let bad = UpstreamSuite { paths: vec![p.into()], ..pins.clone() };
            assert!(bad.validate("aarch64").is_err(), "{p}");
        }
        let bad = UpstreamSuite { container: "public.ecr.aws/firecracker/fcuvm:v90".into(), ..pins.clone() };
        assert!(bad.validate("aarch64").unwrap_err().contains("digest"));
        let bad = UpstreamSuite { workers: 0, ..pins };
        assert!(bad.validate("aarch64").is_err());
    }
}
