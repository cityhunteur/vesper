//! TD-008: `vesperd` is the only endpoint that serves the Firecracker API, and every socket the library or a binary
//! listens on outside tests is on this list. A new listener fails this test until it is added here with its reason,
//! which is the review that AGENTS.md asks for before a host-visible socket is widened.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// (file, pattern) -> how many times it may appear outside test code, and why.
const ALLOWED: &[(&str, &str, usize, &str)] = &[
    ("src/bin/vesperd.rs", ".router()", 1, "the model backend's API router"),
    ("src/bin/vesperd.rs", "axum::serve", 1, "serves that router on the owner-only socket"),
    ("src/bin/vesperd.rs", "bind_owner_only(", 2, "the API socket, made owner-only before it listens (TD-050)"),
    ("src/bin/vesperd/systemvm.rs", "bind_owner_only(", 1, "the API socket of the System VM backend (ADR 0007)"),
    ("src/proxy.rs", "UnixListener::bind", 1, "vesper-proxy's listen socket (the Docker socket on the Mac)"),
    ("src/ports.rs", "TcpListener::bind", 1, "published container ports, loopback only (ADR 0005)"),
];

const PATTERNS: &[&str] = &[
    ".router()",
    "axum::serve",
    "bind_owner_only(",
    "UnixListener::bind",
    "TcpListener::bind",
    "UdpSocket::bind",
];

/// The text of a file without its test code: a `#[cfg(test)] mod tests { … }` block at the end, or a whole test file.
fn non_test(rel: &str, text: &str) -> String {
    if rel.contains("/tests/") || rel.ends_with("/tests.rs") {
        return String::new();
    }
    match text.find("#[cfg(test)]\nmod tests {") {
        Some(i) => text[..i].to_string(),
        None => text.to_string(),
    }
}

fn walk(dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn only_the_listed_sockets_listen_and_only_vesperd_serves_the_api() {
    let mut files = Vec::new();
    walk(Path::new("src"), &mut files);
    let mut found: BTreeMap<(String, &str), usize> = BTreeMap::new();
    for rel in &files {
        let text = non_test(rel, &fs::read_to_string(rel).expect("read source"));
        for pattern in PATTERNS {
            let n = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .map(|l| l.matches(pattern).count())
                .sum::<usize>();
            if n > 0 {
                found.insert((rel.clone(), pattern), n);
            }
        }
    }
    let allowed: BTreeMap<(String, &str), usize> =
        ALLOWED.iter().map(|(f, p, n, _)| ((f.to_string(), *p), *n)).collect();
    assert_eq!(found, allowed, "a listener or API server outside the list (tests/listeners.rs, TD-008)");
}
