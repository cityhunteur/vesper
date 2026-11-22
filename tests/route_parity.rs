//! TD-003: a reachable route is not parity. Every route of the pinned swagger has behaviour cases captured in the
//! repository (the matrix that `tests/api_replay.rs` replays against the model and `make systemvm-replay-suite` on
//! Backend A), and no API source answers `501 Not Implemented` (invariant 5).

use std::collections::BTreeSet;
use std::fs;

#[test]
fn every_pinned_route_has_captured_behaviour_cases() {
    let golden: Vec<serde_json::Value> = serde_json::from_str(
        &fs::read_to_string("testdata/contracts/firecracker-routes.golden.json").unwrap(),
    )
    .unwrap();
    let matrix: serde_json::Value = serde_json::from_str(
        &fs::read_to_string("docs/generated/firecracker-api-behavior-matrix.json").unwrap(),
    )
    .unwrap();
    let mut covered = BTreeSet::new();
    for op in matrix["operations"].as_array().unwrap() {
        let cases = op["cases"].as_array().map_or(0, Vec::len);
        let key = (
            op["method"].as_str().unwrap().to_string(),
            op["path"].as_str().unwrap().to_string(),
        );
        assert!(cases > 0, "{key:?} has no behaviour case");
        assert_eq!(
            op["capture_state"], "captured_in_repo",
            "{key:?} is not captured"
        );
        covered.insert(key);
    }
    let pinned: BTreeSet<(String, String)> = golden
        .iter()
        .map(|r| {
            (
                r["method"].as_str().unwrap().to_string(),
                r["path"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let missing: Vec<_> = pinned.difference(&covered).collect();
    assert!(
        missing.is_empty(),
        "pinned routes with no behaviour cases: {missing:?}"
    );
}

#[test]
fn no_api_source_answers_not_implemented() {
    let mut files = vec!["src/api.rs".to_string()];
    for entry in fs::read_dir("src/api").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") {
            files.push(path.to_string_lossy().into_owned());
        }
    }
    for f in files {
        let text = fs::read_to_string(&f).unwrap();
        for needle in ["NOT_IMPLEMENTED", "StatusCode::from_u16(501)", "501"] {
            assert!(
                !text.contains(needle),
                "{f} contains {needle}: a 501 is not parity (TD-003)"
            );
        }
    }
}
