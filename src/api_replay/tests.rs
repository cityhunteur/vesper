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
    cases.insert(
        "PUT /drives/{drive_id}#c".to_string(),
        CaseScript {
            setup: setup.iter().map(|s| s.to_string()).collect(),
            request: request.into(),
            snapshot: false,
        },
    );
    let mut macros = BTreeMap::new();
    macros.insert(
        "boot".to_string(),
        vec![
            "PUT /boot-source {\"kernel_image_path\":\"{root}/Image\"}".to_string(),
            "@start".to_string(),
        ],
    );
    macros.insert(
        "start".to_string(),
        vec!["PUT /actions {\"action_type\":\"InstanceStart\"}".to_string()],
    );
    macros.insert("loop".to_string(), vec!["@loop".to_string()]);
    ReplayScripts {
        version: 1,
        firecracker_revision: "r".into(),
        files: vec![],
        taps: vec![],
        macros,
        cases,
        snapshot_fixture: vec![],
    }
}

#[test]
fn expands_macros_and_root() {
    let s = scripts(
        &["@boot", "WAIT 5"],
        "PUT /drives/rootfs {\"path_on_host\":\"{root}/tmp/r\"}",
    );
    let steps = expand(&s, "PUT /drives/{drive_id}#c", "/t").unwrap();
    assert_eq!(steps.len(), 4);
    assert_eq!(
        steps[0],
        Step::Http {
            method: "PUT".into(),
            path: "/boot-source".into(),
            body: Some("{\"kernel_image_path\":\"/t/Image\"}".into())
        }
    );
    assert_eq!(steps[2], Step::Wait { ms: 5 });
    assert_eq!(
        parse_step("GUEST_READY 10000"),
        Ok(Step::GuestReady { ms: 10000 })
    );
    assert!(parse_step("GUEST_READY 10001").is_err() && parse_step("GUEST_READY x").is_err());
    assert!(matches!(&steps[3], Step::Http { body: Some(b), .. } if b.contains("/t/tmp/r")));
}

#[test]
fn refuses_bad_scripts() {
    assert!(
        expand(
            &scripts(&["@loop"], "PUT /drives/x {}"),
            "PUT /drives/{drive_id}#c",
            ""
        )
        .unwrap_err()
        .contains("deeper")
    );
    assert!(
        expand(
            &scripts(&["@nope"], "PUT /drives/x {}"),
            "PUT /drives/{drive_id}#c",
            ""
        )
        .unwrap_err()
        .contains("unknown macro")
    );
    assert!(
        expand(
            &scripts(&[], "PUT /pmem/x {}"),
            "PUT /drives/{drive_id}#c",
            ""
        )
        .unwrap_err()
        .contains("is not PUT")
    );
    assert!(
        expand(
            &scripts(&[], "PUT /drives/x {nope"),
            "PUT /drives/{drive_id}#c",
            ""
        )
        .unwrap_err()
        .contains("not JSON")
    );
    assert!(parse_step("WAIT 999999").is_err());
    assert!(parse_step("DELETE /x").is_err());
    assert_eq!(
        expand_pads("{\"a\":\"{pad:3}\"}").unwrap(),
        "{\"a\":\"aaa\"}"
    );
    assert!(expand_pads("{pad:x}").is_err());
    assert_eq!(expand_pads("[{rep:3:1e2,}0]").unwrap(), "[1e2,1e2,1e2,0]");
    assert!(expand_pads("{rep:3}").is_err());
}

#[test]
fn judges_outcomes() {
    let steps = vec![
        parse_step("PUT /actions {}").unwrap(),
        parse_step("PUT /drives/x {}").unwrap(),
    ];
    let ok = |status, body: &str| {
        vec![
            Response {
                status: 204,
                body: String::new(),
            },
            Response {
                status,
                body: body.into(),
            },
        ]
    };
    let err = case(BehaviorOutcome::ErrorEnvelope, 400);
    assert_eq!(
        judge(
            &err,
            &steps,
            &ok(400, r#"{"fault_message":"nope"}"#),
            "",
            false
        )
        .0,
        Verdict::Match
    );
    assert_eq!(
        judge(
            &err,
            &steps,
            &ok(400, r#"{"fault_message":"nope"}"#),
            "",
            true
        )
        .0,
        Verdict::Mismatch
    );
    assert_eq!(
        judge(
            &err,
            &steps,
            &ok(500, r#"{"fault_message":"nope"}"#),
            "",
            false
        )
        .0,
        Verdict::Mismatch
    );
    let none = case(BehaviorOutcome::NoContent, 204);
    assert_eq!(
        judge(&none, &steps, &ok(204, ""), "", false).0,
        Verdict::Match
    );
    let failed = vec![
        Response {
            status: 400,
            body: "{}".into(),
        },
        Response {
            status: 204,
            body: String::new(),
        },
    ];
    assert_eq!(
        judge(&none, &steps, &failed, "", false).0,
        Verdict::SetupFailed
    );
    assert_eq!(
        judge(&none, &steps, &failed[..1], "", false).0,
        Verdict::BackendError
    );
}

#[test]
fn judges_assertions_after_stripping_root() {
    let mut c = case(BehaviorOutcome::SuccessBody, 200);
    c.body_assertions = vec![
        BehaviorBodyAssertion {
            pointer: "/p".into(),
            kind: BehaviorBodyAssertionKind::Exact,
            value: Some(serde_json::json!("/tmp/x")),
            value_type: None,
        },
        BehaviorBodyAssertion {
            pointer: "/n".into(),
            kind: BehaviorBodyAssertionKind::Type,
            value: None,
            value_type: Some(BehaviorJsonType::Null),
        },
        BehaviorBodyAssertion {
            pointer: "/gone".into(),
            kind: BehaviorBodyAssertionKind::Absent,
            value: None,
            value_type: None,
        },
    ];
    let steps = vec![parse_step("GET /vm/config").unwrap()];
    let r = |b: &str| {
        vec![Response {
            status: 200,
            body: b.into(),
        }]
    };
    assert_eq!(
        judge(
            &c,
            &steps,
            &r(r#"{"p":"/root/tmp/x","n":null}"#),
            "/root",
            false
        )
        .0,
        Verdict::Match
    );
    assert_eq!(
        judge(
            &c,
            &steps,
            &r(r#"{"p":"/tmp/x","n":null,"gone":1}"#),
            "",
            false
        )
        .0,
        Verdict::Mismatch
    );
    assert_eq!(
        judge(&c, &steps, &r(r#"{"p":"/tmp/x","n":0}"#), "", false).0,
        Verdict::Mismatch
    );
}
