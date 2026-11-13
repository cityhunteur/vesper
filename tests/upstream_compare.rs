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
fn every_class_is_tracked_by_a_row_that_exists() {
    let classes = fs::read_to_string(root().join("docs/specs/upstream-suite-classes.yaml")).unwrap();
    let registry = fs::read_to_string(root().join("docs/specs/compat-exceptions.yaml")).unwrap();
    let debt = fs::read_to_string(root().join("docs/exec-plans/tech-debt-tracker.md")).unwrap();
    for line in classes.lines().filter_map(|l| l.trim().strip_prefix("tracked_by:")) {
        let id = line.trim();
        let found = if id.starts_with("CE-") { registry.contains(&format!("id: {id}")) } else { debt.contains(id) };
        assert!(found, "tracked_by {id} names no compat exception or tech-debt row");
    }
}
