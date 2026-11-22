//! TD-009: CI and local builds use one Rust release. `rust-toolchain.toml` names it, and every
//! `dtolnay/rust-toolchain` step in `.github/workflows/` must name the same one.

use std::fs;

fn pinned() -> String {
    let text = fs::read_to_string("rust-toolchain.toml").expect("read rust-toolchain.toml");
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("channel"))
        .expect("rust-toolchain.toml has a channel");
    line.split('"').nth(1).expect("channel is quoted").to_string()
}

#[test]
fn every_workflow_uses_the_pinned_toolchain() {
    let want = pinned();
    assert!(want.split('.').count() == 3, "pin an exact release, not {want}");
    let mut steps = 0;
    for entry in fs::read_dir(".github/workflows").expect("read workflows") {
        let path = entry.expect("dir entry").path();
        let text = fs::read_to_string(&path).expect("read workflow");
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("dtolnay/rust-toolchain@") {
                continue;
            }
            steps += 1;
            assert!(!line.contains("@stable"), "{}: line {} floats on stable", path.display(), i + 1);
            let named = lines[i + 1..]
                .iter()
                .take(4)
                .find_map(|l| l.trim().strip_prefix("toolchain:"))
                .map(|v| v.split('#').next().unwrap().trim().trim_matches('"').to_string());
            assert_eq!(named.as_deref(), Some(want.as_str()), "{}: line {}", path.display(), i + 1);
        }
    }
    assert!(steps >= 6, "found only {steps} toolchain steps");
}
