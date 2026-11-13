//! The upstream Firecracker suite: its pins in the baseline lock and the comparator that explains every result
//! (GH #172, D-023).
//!
//! A run on Backend A produces `{nodeid: {outcome, message}}`. A test needs explaining when it did not pass here but
//! passed on the reference. The reference is the Linux pass set (CE-0003) when one is given; without one, every test
//! is taken to pass there, so every failure, error, hang and skip needs explaining. Each such test must match an
//! exception class (node-id patterns, optional message substrings, the outcomes it may explain), or the comparison
//! fails. Patterns use `*` as a wildcard; there is no regular-expression engine, so no new dependency.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `baselines.firecracker.upstream_suite` in the lock: what a run fetches and how it runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamSuite {
    /// The upstream build container, as `tools/devtool` pins it.
    pub container: String,
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
            ("artifacts_bucket", &self.artifacts_bucket),
            ("artifacts_prefix", &self.artifacts_prefix),
            ("release_asset", &self.release_asset),
        ] {
            if value.trim().is_empty() || value.contains(char::is_whitespace) {
                return Err(format!("upstream_suite.{name} must be a non-blank word"));
            }
        }
        if !self.artifacts_prefix.ends_with(&format!("/{arch}")) {
            return Err(format!("upstream_suite.artifacts_prefix {:?} is not for {arch}", self.artifacts_prefix));
        }
        if !self.release_asset.contains(arch) {
            return Err(format!("upstream_suite.release_asset {:?} is not for {arch}", self.release_asset));
        }
        if self.paths.is_empty() || self.paths.iter().any(|p| p.is_empty() || p.starts_with('/') || p.contains("..")) {
            return Err("upstream_suite.paths must be non-empty relative paths".to_string());
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
    Harness,
    Unexplained,
    Skip,
}

/// One exception class: why a set of tests cannot pass on Backend A.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Class {
    pub id: String,
    pub kind: ClassKind,
    /// Node-id patterns (`*` matches any run of characters); a test matches when any pattern does.
    pub nodeid: Vec<String>,
    /// Message substrings; when given, the message must contain one of them.
    #[serde(default)]
    pub message: Vec<String>,
    /// Outcomes this class may explain.
    pub outcomes: Vec<String>,
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
            if c.nodeid.is_empty() || c.nodeid.iter().any(|p| p.trim().is_empty()) {
                return Err(format!("class {}: nodeid needs at least one non-blank pattern", c.id));
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
            && self.nodeid.iter().any(|p| glob(p, nodeid))
            && (self.message.is_empty() || self.message.iter().any(|m| r.message.contains(m.as_str())))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Unmatched {
    pub nodeid: String,
    pub outcome: String,
    pub message: String,
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
    /// Classes that explained no test in this run (reported, not fatal: a partial run selects few tests).
    pub stale_classes: Vec<String>,
    /// Tests that did not pass on the reference either, so need no class.
    pub not_passing_on_reference: usize,
    /// Tests in the reference that this run did not select or report.
    pub missing_from_run: usize,
}

impl Comparison {
    pub fn ok(&self) -> bool {
        self.unmatched.is_empty()
    }
}

pub fn compare(results: &Results, reference: Option<&Results>, classes: &ClassFile) -> Comparison {
    let mut out = Comparison {
        reference: if reference.is_some() { "linux" } else { "all-pass" }.to_string(),
        total: results.len(),
        ..Comparison::default()
    };
    for c in &classes.classes {
        out.per_class.insert(c.id.clone(), 0);
    }
    for (nodeid, r) in results {
        if is_pass(&r.outcome) {
            out.passed += 1;
            continue;
        }
        if let Some(reference) = reference {
            match reference.get(nodeid) {
                Some(l) if is_pass(&l.outcome) => {}
                // Not passing on Linux, or not run there: the reference cannot ask more of Backend A.
                _ => {
                    out.not_passing_on_reference += 1;
                    continue;
                }
            }
        }
        *out.to_explain.entry(r.outcome.clone()).or_default() += 1;
        match classes.classes.iter().find(|c| c.matches(nodeid, r)) {
            Some(c) => *out.per_class.entry(c.id.clone()).or_default() += 1,
            None => out.unmatched.push(Unmatched {
                nodeid: nodeid.clone(),
                outcome: r.outcome.clone(),
                message: r.message.chars().take(300).collect(),
            }),
        }
    }
    if let Some(reference) = reference {
        out.missing_from_run = reference.keys().filter(|k| !results.contains_key(*k)).count();
    }
    out.stale_classes = out.per_class.iter().filter(|(_, n)| **n == 0).map(|(k, _)| k.clone()).collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(outcome: &str, message: &str) -> TestResult {
        TestResult { outcome: outcome.into(), message: message.into() }
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
    tracked_by: TD-054
    reason: no CONFIG_USERFAULTFD
  - id: x86-only
    kind: skip
    nodeid: ["*test_cpu_template*"]
    outcomes: [skipped]
    tracked_by: CE-0001
    reason: x86-only
"#;
        let c: ClassFile = serde_yaml::from_str(yaml).unwrap();
        c.validate().unwrap();
        c
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
        res.insert("integration_tests/functional/test_cpu_template.py::t".into(), r("skipped", "x86"));
        let c = compare(&res, None, &classes());
        assert!(c.ok(), "{:?}", c.unmatched);
        assert_eq!((c.passed, c.per_class["no-uffd"], c.per_class["x86-only"]), (1, 1, 1));

        // An unmatched failure, a hang, and a class whose outcomes or message exclude the result all fail.
        res.insert("integration_tests/functional/test_vsock.py::t".into(), r("hung", ""));
        res.insert("integration_tests/functional/test_uffd.py::test_c".into(), r("error", "UFFD object"));
        res.insert("integration_tests/functional/test_uffd.py::test_d".into(), r("failed", "other"));
        let c = compare(&res, None, &classes());
        let names: Vec<&str> = c.unmatched.iter().map(|u| u.nodeid.as_str()).collect();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(!c.ok());
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
        let c = compare(&res, Some(&linux), &classes());
        let names: Vec<&str> = c.unmatched.iter().map(|u| u.nodeid.as_str()).collect();
        // `new_here` is not in the reference, so it needs no class; `skipped_here` passed there, so it does.
        assert_eq!(names, vec!["t::skipped_here"]);
        assert_eq!((c.not_passing_on_reference, c.missing_from_run), (2, 1));
        assert_eq!(c.reference, "linux");
        assert_eq!(c.stale_classes, vec!["no-uffd".to_string(), "x86-only".to_string()]);
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

        let pins = UpstreamSuite {
            container: "public.ecr.aws/firecracker/fcuvm:v90".into(),
            artifacts_bucket: "spec.ccfc.min".into(),
            artifacts_prefix: "firecracker-ci/20260603-e656faf58d6c-0/aarch64".into(),
            release_asset: "firecracker-v1.16.0-aarch64.tgz".into(),
            paths: vec!["integration_tests/functional".into()],
            workers: 3,
            test_timeout_s: 600,
        };
        pins.validate("aarch64").unwrap();
        assert!(pins.validate("x86_64").is_err());
        let bad = UpstreamSuite { paths: vec!["../x".into()], ..pins.clone() };
        assert!(bad.validate("aarch64").is_err());
        let bad = UpstreamSuite { workers: 0, ..pins };
        assert!(bad.validate("aarch64").is_err());
    }
}
