use std::fs;

#[test]
fn golden_contract_matches_pinned_swagger() {
    let routes = vesper::spec::load_routes("third_party/firecracker/firecracker.yaml").unwrap();

    let content = fs::read_to_string("testdata/contracts/firecracker-routes.golden.json").unwrap();
    let golden: Vec<vesper::spec::Route> = serde_json::from_str(&content).unwrap();

    assert_eq!(golden.len(), routes.len());
    for (expected, actual) in golden.iter().zip(routes.iter()) {
        assert_eq!(expected.method, actual.method);
        assert_eq!(expected.path, actual.path);
    }
}
