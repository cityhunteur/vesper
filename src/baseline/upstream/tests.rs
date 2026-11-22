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
    message: ["MemoryUsageExceededError: ('Memory usage (6."]
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
    res.insert(
        "integration_tests/functional/a.py::t".into(),
        r(
            "error",
            "MemoryUsageExceededError: ('Memory usage (6.5 MiB)",
        ),
    );
    res.insert(
        "integration_tests/functional/b.py::t".into(),
        r(
            "error",
            "MemoryUsageExceededError: ('Memory usage (7.5 MiB)",
        ),
    );
    let mut alone = r("passed", "");
    alone.under_load = Some(Attempt {
        outcome: "failed".into(),
        message: "AssertionError".into(),
    });
    res.insert("integration_tests/functional/c.py::t".into(), alone);
    let out = compare(&res, None, &c, None, false);
    assert_eq!(out.per_class["overshoot"], 1);
    assert_eq!(out.unmatched.len(), 1);
    assert_eq!(
        out.unmatched[0].nodeid,
        "integration_tests/functional/b.py::t"
    );
    assert_eq!((out.passed, out.passed_alone.len()), (1, 1));
    assert_eq!(out.passed_alone[0].outcome, "failed");
    // A pass alone never clears a listed symptom, and the cap on passes alone is enforced.
    let mut strict = c.clone();
    strict.never_pass_alone = vec!["MemoryUsageExceededError".into()];
    strict.max_passed_alone = Some(0);
    strict.validate().unwrap();
    let mut big = r("passed", "");
    big.under_load = Some(Attempt {
        outcome: "error".into(),
        message: "MemoryUsageExceededError: ('Memory usage (9.1 MiB)".into(),
    });
    res.insert("integration_tests/functional/d.py::t".into(), big);
    let out = compare(&res, None, &strict, None, false);
    assert_eq!(
        (out.passed, out.passed_alone.len(), out.unmatched.len()),
        (1, 1, 2)
    );
    assert!(
        out.failures()
            .iter()
            .any(|f| f.contains("more than the cap of 0")),
        "{:?}",
        out.failures()
    );
    // A test unmatched under load that fails again alone with a classified symptom counts under that class and
    // within the rerun cap; one whose result alone matches nothing, or whose result under load is a symptom a
    // rerun never clears, stays unmatched (D-039).
    let mut res = Results::new();
    let mut settled = r(
        "failed",
        "ssh: connect to host 192.168.0.2 port 22: Connection timed out",
    );
    settled.alone = Some(Attempt {
        outcome: "error".into(),
        message: "MemoryUsageExceededError: ('Memory usage (6.1 MiB)".into(),
    });
    res.insert("integration_tests/functional/e.py::t".into(), settled);
    let mut unknown = r("failed", "ssh timeout");
    unknown.alone = Some(Attempt {
        outcome: "failed".into(),
        message: "AssertionError".into(),
    });
    res.insert("integration_tests/functional/f.py::t".into(), unknown);
    let mut stuck = r(
        "error",
        "MemoryUsageExceededError: ('Memory usage (9.1 MiB)",
    );
    stuck.alone = Some(Attempt {
        outcome: "error".into(),
        message: "MemoryUsageExceededError: ('Memory usage (6.1 MiB)".into(),
    });
    res.insert("integration_tests/functional/g.py::t".into(), stuck);
    let mut lenient = strict.clone();
    lenient.max_passed_alone = Some(1);
    let out = compare(&res, None, &lenient, None, false);
    assert_eq!(out.per_class["overshoot"], 1);
    assert_eq!(
        out.explained_alone,
        vec![ExplainedAlone {
            nodeid: "integration_tests/functional/e.py::t".into(),
            class: "overshoot".into(),
            outcome: "failed".into(),
            message: "ssh: connect to host 192.168.0.2 port 22: Connection timed out".into(),
        }]
    );
    let names: Vec<_> = out.unmatched.iter().map(|u| u.nodeid.as_str()).collect();
    assert_eq!(
        names,
        [
            "integration_tests/functional/f.py::t",
            "integration_tests/functional/g.py::t"
        ]
    );
    lenient.max_passed_alone = Some(0);
    let out = compare(&res, None, &lenient, None, false);
    assert!(
        out.failures()
            .iter()
            .any(|f| f.contains("1 tests were settled only by the isolated rerun")),
        "{:?}",
        out.failures()
    );
    // any_test with a message glob, a short message, a skip outcome, or a second any_test class is refused.
    for (key, value) in [
        ("message_glob", r#"["assert * > *"]"#),
        ("message", r#"["e"]"#),
        ("outcomes", "[skipped]"),
    ] {
        let mut bad = v.clone();
        bad["classes"][0][key] = serde_yaml::from_str(value).unwrap();
        let c: ClassFile = serde_yaml::from_value(bad).unwrap();
        assert!(c.validate().unwrap_err().contains("any_test"), "{key}");
    }
    let mut two = v.clone();
    let mut second = two["classes"][0].clone();
    second["id"] = "other".into();
    two["classes"].as_sequence_mut().unwrap().push(second);
    let c: ClassFile = serde_yaml::from_value(two).unwrap();
    assert!(c.validate().unwrap_err().contains("at most one"));
    v["never_pass_alone"] = serde_yaml::from_str(r#"["x"]"#).unwrap();
    let c: ClassFile = serde_yaml::from_value(v).unwrap();
    assert!(c.validate().unwrap_err().contains("never_pass_alone"));
}

fn r(outcome: &str, message: &str) -> TestResult {
    TestResult {
        outcome: outcome.into(),
        message: message.into(),
        under_load: None,
        alone: None,
    }
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
    assert!(ssb.matches(
        n,
        &r(
            "failed",
            "/sys/devices/system/cpu/vulnerabilities/spec_store_bypass:Vulnerable"
        )
    ));
    let meltdown = host.replace(
        r#""CVE-2017-5754","VULNERABLE":false"#,
        r#""CVE-2017-5754","VULNERABLE":true"#,
    );
    assert!(
        !ssb.matches(n, &r("failed", &meltdown)),
        "an extra vulnerability must not be explained"
    );
    let two_files = "/sys/devices/system/cpu/vulnerabilities/spec_store_bypass:Vulnerable\n/sys/devices/system/cpu/vulnerabilities/meltdown:Vulnerable";
    assert!(!ssb.matches(n, &r("failed", two_files)));
    assert!(
        !ssb.matches(n, &r("failed", &format!("{host} {TRUNCATED}"))),
        "a truncated report proves nothing"
    );
    assert!(!ssb.matches(
        n,
        &r("failed", "spec_store_bypass mentioned, nothing reported")
    ));
    let no_cve = host.replace(r#"}]"#, r#"},{"NAME":"NEW","VULNERABLE":true}]"#);
    assert!(
        !ssb.matches(n, &r("failed", &no_cve)),
        "an entry without a CVE must not borrow the previous one's"
    );
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
    res.insert(
        "integration_tests/functional/test_uffd.py::test_a".into(),
        r("failed", "UFFD object: System error"),
    );
    res.insert(
        "integration_tests/functional/test_api.py::test_b".into(),
        r("passed", ""),
    );
    res.insert(
        "integration_tests/functional/test_cpu_template_helper.py::t".into(),
        r("skipped", "x86"),
    );
    let c = full(&res);
    assert!(c.ok(), "{:?}", c.failures());
    assert_eq!(
        (c.passed, c.per_class["no-uffd"], c.per_class["x86-only"]),
        (1, 1, 1)
    );

    // An unmatched failure, a hang, and a class whose outcomes or message exclude the result all fail.
    res.insert(
        "integration_tests/functional/test_vsock.py::t".into(),
        r("hung", ""),
    );
    res.insert(
        "integration_tests/functional/test_uffd.py::test_c".into(),
        r("error", "UFFD object"),
    );
    res.insert(
        "integration_tests/functional/test_uffd.py::test_d".into(),
        r("failed", "other"),
    );
    let c = full(&res);
    let names: Vec<&str> = c.unmatched.iter().map(|u| u.nodeid.as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(!c.ok());
}

#[test]
fn an_empty_run_a_class_over_its_max_and_a_stale_class_fail() {
    let c = full(&Results::new());
    assert!(
        c.failures().iter().any(|f| f.contains("no results")),
        "{:?}",
        c.failures()
    );

    let mut res = Results::new();
    for t in ["a", "b", "c"] {
        res.insert(
            format!("integration_tests/functional/test_uffd.py::{t}"),
            r("failed", "UFFD object"),
        );
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
    res.insert(
        "integration_tests/functional/test_uffd.py::a".into(),
        r("failed", "UFFD object"),
    );
    let meta = |code: Option<i64>, errs: &[&str]| {
        Some(RunMeta {
            exitcode: code,
            collection_errors: errs.iter().map(|e| e.to_string()).collect(),
        })
    };
    assert!(compare(&res, None, &classes(), meta(Some(1), &[]), false).ok());
    // A stopped run has no pytest verdict, and that fails it.
    let stopped = compare(&res, None, &classes(), meta(None, &[]), false);
    assert!(
        stopped
            .failures()
            .iter()
            .any(|f| f.contains("exit code is unknown")),
        "{:?}",
        stopped.failures()
    );
    assert!(!compare(&res, None, &classes(), meta(Some(3), &[]), false).ok());
    assert!(
        !compare(
            &res,
            None,
            &classes(),
            meta(Some(1), &["integration_tests/functional/test_x.py"]),
            false
        )
        .ok()
    );
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
    assert!(
        !c.classes[0].matches(n, &r("failed", "AssertionError: assert 1 == 0")),
        "the shape, not any assertion"
    );
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
    res.insert(
        "integration_tests/functional/test_uffd.py::a".into(),
        r("failed", "UFFD object"),
    );
    let reference = res.clone();
    let c = compare(
        &res,
        Some(&reference),
        &classes(),
        Some(RunMeta {
            exitcode: Some(1),
            ..RunMeta::default()
        }),
        false,
    );
    assert_eq!(c.per_class["no-uffd"], 0);
    assert!(c.stale_required.is_empty() && c.ok(), "{:?}", c.failures());
}

#[test]
fn an_unmatched_message_keeps_its_head_and_its_tail() {
    let long = format!(
        "Command: ssh {} stderr: Connection timed out",
        "x".repeat(1000)
    );
    let m = head_and_tail(&long, 200, 200);
    assert!(
        m.starts_with("Command: ssh")
            && m.ends_with("stderr: Connection timed out")
            && m.contains(" … ")
    );
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
    assert!(
        c.failures()
            .iter()
            .any(|f| f.contains("missing from the run"))
    );
    assert!(
        !compare(&res, Some(&linux), &classes(), None, true)
            .failures()
            .iter()
            .any(|f| f.contains("missing"))
    );
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
    for (nodeid, message) in [
        (vec!["*"], vec!["x"]),
        (vec!["t::*"], vec![]),
        (vec!["t::*"], vec![" "]),
    ] {
        let mut c = classes();
        c.classes[0].nodeid = nodeid.into_iter().map(String::from).collect();
        c.classes[0].message = message.into_iter().map(String::from).collect();
        assert!(c.validate().is_err());
    }
    assert!(serde_yaml::from_str::<ClassKind>("harness").is_err());

    let pins = UpstreamSuite {
        container: format!(
            "public.ecr.aws/firecracker/fcuvm:v90@sha256:{}",
            "a".repeat(64)
        ),
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
        let bad = UpstreamSuite {
            paths: vec![p.into()],
            ..pins.clone()
        };
        assert!(bad.validate("aarch64").is_err(), "{p}");
    }
    let bad = UpstreamSuite {
        container: "public.ecr.aws/firecracker/fcuvm:v90".into(),
        ..pins.clone()
    };
    assert!(bad.validate("aarch64").unwrap_err().contains("digest"));
    let bad = UpstreamSuite { workers: 0, ..pins };
    assert!(bad.validate("aarch64").is_err());
}
