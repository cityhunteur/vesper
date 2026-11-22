//! GH #19: the rootfs pipeline (reproducible build, provenance, invalid image, boot in a microVM).
use super::*;

pub(super) fn rootfs_step(
    session: &mut Session,
    steps: &mut Vec<SuiteStep>,
    name: &str,
    args: &[&str],
    expect: &str,
) -> Option<Vec<String>> {
    step(steps, name, || {
        let (rc, lines) = session.rootfs(args)?;
        let first = lines.first().cloned().unwrap_or_default();
        if rc != 0 || !first.starts_with(expect) {
            return Err(format!(
                "`vesper-rootfs {}` rc={rc}: {}",
                args.join(" "),
                lines.join(" | ")
            ));
        }
        Ok((lines.clone(), first))
    })
}

/// Runs the GH #19 checklist: import a trivial image, build the rootfs twice (cached and uncached) with an
/// identical digest, check the embedded provenance, reject an unknown image, and boot the rootfs in a jailed
/// microVM.
pub fn run_rootfs_suite(endpoint: &ControlEndpoint, id: &str) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!(
            "protocol {} host {}",
            session.handshake.protocol_version, session.handshake.hostname
        );
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return SuiteReport {
            bridge_path: endpoint.bridge_path.display().to_string(),
            port: endpoint.port,
            steps,
            passed: false,
        };
    };
    let imported = rootfs_step(
        &mut session,
        &mut steps,
        "import_trivial_image",
        &["import-trivial"],
        "IMPORTED",
    );
    let Some(imported) = imported else {
        return SuiteReport {
            bridge_path: endpoint.bridge_path.display().to_string(),
            port: endpoint.port,
            steps,
            passed: false,
        };
    };
    let image_id = imported[0]
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .to_string();
    let built = rootfs_step(
        &mut session,
        &mut steps,
        "build",
        &["build", &image_id, "--no-cache"],
        "BUILT",
    );
    let (path, sha) = match &built {
        Some(lines) => (
            lines[0].split_whitespace().nth(1).unwrap_or("").to_string(),
            field(&lines[0], "sha256=").unwrap_or("").to_string(),
        ),
        None => (String::new(), String::new()),
    };
    if built.is_some() {
        step(&mut steps, "reproducible_rebuild", || {
            let (rc, lines) = session.rootfs(&["build", &image_id, "--no-cache"])?;
            let again = lines.first().cloned().unwrap_or_default();
            if rc != 0 || field(&again, "sha256=") != Some(sha.as_str()) {
                return Err(format!(
                    "second build differs: {again} (first sha256={sha})"
                ));
            }
            Ok(((), format!("sha256={sha} twice from image {image_id}")))
        });
        step(&mut steps, "cache_hit", || {
            let (rc, lines) = session.rootfs(&["build", &image_id])?;
            let line = lines.first().cloned().unwrap_or_default();
            if rc != 0
                || field(&line, "cached=") != Some("true")
                || field(&line, "sha256=") != Some(sha.as_str())
            {
                return Err(format!("expected a cache hit with the same digest: {line}"));
            }
            Ok(((), line))
        });
        step(&mut steps, "provenance_embedded", || {
            let (rc, lines) = session.rootfs(&["provenance", &path])?;
            let text = lines.join("\n");
            if rc != 0 {
                return Err(format!("provenance failed: {text}"));
            }
            let json: serde_json::Value = serde_json::from_str(text.trim())
                .map_err(|e| format!("provenance JSON: {e}: {text}"))?;
            if json["source"]["image_id"].as_str() != Some(image_id.as_str()) {
                return Err(format!(
                    "source.image_id mismatch: {}",
                    json["source"]["image_id"]
                ));
            }
            if json["source"]["layers"]
                .as_array()
                .map(|l| l.is_empty())
                .unwrap_or(true)
            {
                return Err("source.layers missing".to_string());
            }
            if json["baseline"]["firecracker_revision"]
                .as_str()
                .map(|s| s.len())
                .unwrap_or(0)
                < 7
            {
                return Err(format!("baseline revision missing: {}", json["baseline"]));
            }
            if json["output_sha256"].as_str() != Some(sha.as_str()) {
                return Err(format!("output_sha256 {} != {sha}", json["output_sha256"]));
            }
            Ok((
                (),
                format!(
                    "image {}, {} layer(s), firecracker {}, created {}",
                    image_id,
                    json["source"]["layers"]
                        .as_array()
                        .map(|l| l.len())
                        .unwrap_or(0),
                    json["baseline"]["firecracker_revision"],
                    json["source"]["created"]
                ),
            ))
        });
    }
    step(&mut steps, "invalid_image_rejected", || {
        let (rc, lines) = session.rootfs(&["build", "vesper/does-not-exist:nope"])?;
        let line = lines.first().cloned().unwrap_or_default();
        if rc == 0 || !line.starts_with("ERR image not found") {
            return Err(format!("expected a clear error, got rc={rc}: {line}"));
        }
        Ok(((), line))
    });
    if built.is_some() {
        let _ = microvm_step(
            &mut session,
            &mut steps,
            "cleanup_before",
            &["cleanup", id],
            "CLEAN",
        );
        if microvm_step(
            &mut session,
            &mut steps,
            "launch_with_rootfs",
            &["launch", id, "--rootfs", &path],
            "LAUNCHED",
        )
        .is_some()
        {
            step(&mut steps, "rootfs_boots", || {
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    let (_, lines) = session.microvm(&["log", id, "200"])?;
                    let ok = lines.iter().find(|l| l.starts_with("VESPER-ROOTFS-OK"));
                    let exec_ok = lines.iter().any(|l| l.starts_with("VESPER-ROOTFS-EXEC-OK"));
                    if let (Some(ok), true) = (ok, exec_ok) {
                        if ok.contains(&image_id) {
                            return Ok(((), format!("{ok}; command executed from the rootfs")));
                        }
                        return Err(format!(
                            "provenance from the booted rootfs does not match: {ok}"
                        ));
                    }
                    if lines.iter().any(|l| {
                        l.contains("VESPER-ROOTFS-MOUNT-FAILED")
                            || l.contains("VESPER-ROOTFS-EXEC-FAILED")
                    }) {
                        return Err(lines
                            .iter()
                            .rev()
                            .take(12)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" | "));
                    }
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "no rootfs marker within 30 s; tail: {}",
                            lines
                                .iter()
                                .rev()
                                .take(8)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(" | ")
                        ));
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            });
            microvm_step(
                &mut session,
                &mut steps,
                "cleanup_after_rootfs_boot",
                &["cleanup", id],
                "CLEAN",
            );
        }
    }
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 10;
    SuiteReport {
        bridge_path: endpoint.bridge_path.display().to_string(),
        port: endpoint.port,
        steps,
        passed,
    }
}
