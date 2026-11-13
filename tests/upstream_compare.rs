//! `baselinectl --mode upstream-pins` and `--mode upstream-compare` end to end, and the class file's references
//! (GH #172).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn baselinectl(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_baselinectl")).args(args).current_dir(root()).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), text)
}

fn scratch(name: &str, body: &str) -> String {
    let dir = std::env::temp_dir().join(format!("vesper-upstream-compare-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn pins_are_quoted_and_pinned_by_digest() {
    let (rc, out) = baselinectl(&["--mode", "upstream-pins"]);
    assert_eq!(rc, 0, "{out}");
    for key in ["CONTAINER", "HELPER_IMAGE"] {
        let line = out.lines().find(|l| l.starts_with(&format!("{key}="))).unwrap_or_else(|| panic!("{key}: {out}"));
        assert!(line.contains("@sha256:") && line.ends_with('\''), "{line}");
    }
}

/// The isolated rerun (D-031): a pass alone is listed and counts; a memory overshoot under load is never cleared by
/// it; and a test that failed alone too shows that attempt.
#[test]
fn a_pass_alone_counts_unless_its_symptom_is_never_cleared() {
    let t = "integration_tests/functional/test_api.py::t";
    let alone = scratch("alone.json", &serde_json::json!({t: {"outcome": "passed", "message": "",
        "under_load": {"outcome": "failed", "message": "AssertionError: timing"}}}).to_string());
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &alone, "--partial"]);
    assert_eq!(rc, 0, "{out}");
    assert!(out.contains(&format!("PASSED-ALONE {t} under load: failed AssertionError: timing")), "{out}");

    let big = scratch("big.json", &serde_json::json!({t: {"outcome": "passed", "message": "",
        "under_load": {"outcome": "error", "message": "MemoryUsageExceededError: ('Memory usage (9.10 MiB) exceeded"}}}).to_string());
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &big, "--partial"]);
    assert_ne!(rc, 0, "{out}");
    assert!(out.contains(&format!("UNMATCHED {t} error [alone: passed]")), "{out}");

    let twice = scratch("twice.json", &serde_json::json!({t: {"outcome": "failed", "message": "AssertionError: x",
        "alone": {"outcome": "failed", "message": "AssertionError: x"}}}).to_string());
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &twice, "--partial"]);
    assert_ne!(rc, 0, "{out}");
    assert!(out.contains(&format!("UNMATCHED {t} failed [alone: failed]")), "{out}");
}

#[test]
fn compare_fails_an_empty_run_an_unexplained_failure_and_a_pytest_error() {
    let empty = scratch("empty.json", "{}");
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &empty, "--partial"]);
    assert_ne!(rc, 0, "{out}");
    assert!(out.contains("no results"), "{out}");

    // A Meltdown report in the file the SSB class covers is not the SSB symptom, so nothing explains it.
    let meltdown = scratch(
        "meltdown.json",
        r#"{"integration_tests/security/test_vulnerabilities.py::test_vulnerabilities_on_host":
            {"outcome":"failed","message":"AssertionError: /sys/devices/system/cpu/vulnerabilities/meltdown:Vulnerable"}}"#,
    );
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &meltdown, "--partial"]);
    assert_ne!(rc, 0, "{out}");
    assert!(out.contains("UNMATCHED integration_tests/security/test_vulnerabilities.py"), "{out}");

    // A checker report that names SSB and also Meltdown is not explained: every reported vulnerability must be allowed.
    let host = r#"[{"NAME":"MELTDOWN","CVE":"CVE-2017-5754","VULNERABLE":true},{"NAME":"VARIANT 4","CVE":"CVE-2018-3639","VULNERABLE":true}]"#;
    let both = scratch(
        "ssb-and-meltdown.json",
        &serde_json::json!({"integration_tests/security/test_vulnerabilities.py::test_spectre_meltdown_checker_on_host":
            {"outcome": "failed", "message": format!("ChildProcessError: stdout: {host}")}})
        .to_string(),
    );
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &both, "--partial"]);
    assert_ne!(rc, 0, "{out}");
    assert!(out.contains("UNMATCHED integration_tests/security/test_vulnerabilities.py"), "{out}");

    // The known SSB result is explained, unless pytest itself reports an internal error.
    let ssb = scratch(
        "ssb.json",
        r#"{"integration_tests/security/test_vulnerabilities.py::test_vulnerabilities_on_host":
            {"outcome":"failed","message":"AssertionError: /sys/devices/system/cpu/vulnerabilities/spec_store_bypass:Vulnerable"}}"#,
    );
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &ssb, "--partial"]);
    assert_eq!(rc, 0, "{out}");
    let meta = scratch("meta.json", r#"{"exitcode":3,"collection_errors":[]}"#);
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &ssb, "--partial", "--meta", &meta]);
    assert_ne!(rc, 0, "{out}");
    assert!(out.contains("pytest exited 3"), "{out}");

    // Without --partial, the classes that explained nothing fail the run.
    let (rc, out) = baselinectl(&["--mode", "upstream-compare", "--results", &ssb]);
    assert_ne!(rc, 0, "{out}");
    assert!(out.contains("classes that explained nothing on a full run"), "{out}");
}

#[test]
fn every_class_is_tracked_by_an_open_row() {
    let classes: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root().join("docs/specs/upstream-suite-classes.yaml")).unwrap()).unwrap();
    let registry: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root().join("docs/specs/compat-exceptions.yaml")).unwrap()).unwrap();
    let debt = fs::read_to_string(root().join("docs/exec-plans/tech-debt-tracker.md")).unwrap();
    let rows = registry["exceptions"].as_sequence().expect("compat-exceptions.yaml has exceptions");
    for class in classes["classes"].as_sequence().unwrap() {
        let id = class["tracked_by"].as_str().unwrap();
        if id.starts_with("CE-") {
            let row = rows.iter().find(|r| r["id"].as_str() == Some(id));
            let status = row.and_then(|r| r["status"].as_str());
            assert_eq!(status, Some("open"), "class {:?}: {id} must be an open compat exception", class["id"]);
        } else {
            // The tracker's last column is the status: open, open (narrowed) or resolved.
            let status = debt
                .lines()
                .find(|l| l.starts_with(&format!("| {id} |")))
                .and_then(|r| r.trim_end().trim_end_matches('|').rsplit('|').next())
                .map(str::trim);
            assert!(status.is_some_and(|s| s.starts_with("open")), "class {:?}: {id} must be an open tech-debt row, is {status:?}", class["id"]);
        }
    }
}

#[test]
fn the_artefact_manifest_lists_each_key_once_with_a_size_and_a_sha256() {
    let text = fs::read_to_string(root().join("docs/specs/upstream-suite-artifacts.sha256")).unwrap();
    let mut keys = std::collections::BTreeSet::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split(' ').collect();
        assert_eq!(f.len(), 3, "KEY SIZE SHA256: {line}");
        assert!(!f[0].is_empty() && !f[0].starts_with('/') && !f[0].contains(".."), "key: {line}");
        assert!(f[1].parse::<u64>().is_ok_and(|n| n > 0), "size: {line}");
        assert!(f[2].len() == 64 && f[2].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "sha256: {line}");
        assert!(keys.insert(f[0]), "listed twice: {}", f[0]);
    }
    assert!(keys.len() >= 10, "{} keys", keys.len());
}
