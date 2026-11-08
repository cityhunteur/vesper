//! GH #24 checklist: block-first rootfs cache keyed by the image layer chain, exercised through `ctr`.

use super::{connect, ctr_step, rc_out, report, step, ControlEndpoint, SuiteReport};

fn meta_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let rest = line[start..].trim_start();
    if let Some(s) = rest.strip_prefix('"') {
        s.split('"').next()
    } else {
        rest.split(|c| c == ',' || c == '}').next().map(str::trim)
    }
}

/// Runs the GH #24 checklist: a two-layer image boots from the block-first root drive, the second task
/// from the same layer chain is a cache hit with the same digest, the cache holds one drive per chain, the
/// metadata record names the strategy and layers, and a shell-less failure surfaces an actionable error.
pub fn run_bundle_suite(endpoint: &ControlEndpoint) -> SuiteReport {
    let mut steps = Vec::new();
    let session = step(&mut steps, "handshake", || {
        let session = connect(endpoint)?;
        let detail = format!("protocol {} host {}", session.handshake.protocol_version, session.handshake.hostname);
        Ok((session, detail))
    });
    let Some(mut session) = session else {
        return report(endpoint, steps, false);
    };
    for name in ["l1", "la", "lb", "e2e-base"] {
        let _ = session.ctr(&["try", "task", "kill", "-s", "KILL", name]);
        let _ = session.ctr(&["try", "task", "delete", "-f", name]);
        let _ = session.ctr(&["try", "container", "delete", name]);
    }
    if ctr_step(&mut session, &mut steps, "import_base_image", &["import"], "IMPORTED").is_none() {
        return report(endpoint, steps, false);
    }
    let Some(imported) = ctr_step(&mut session, &mut steps, "import_layered_image", &["import-layered"], "IMPORTED") else {
        return report(endpoint, steps, false);
    };
    let image = imported[0].split_whitespace().nth(1).unwrap_or("").to_string();
    step(&mut steps, "layered_rootfs_boots", || {
        let (_, lines) = session.ctr(&["run-image", &image, "l1", "cat", "/etc/layer2-marker;", "exit", "0"])?;
        let (rc, out) = rc_out(&lines);
        if rc != 0 || !out.iter().any(|l| l == "layer-two") {
            return Err(format!("expected rc 0 with layer-two, got rc={rc}: {}", lines.join(" | ")));
        }
        Ok(((), "second layer's file read from the block-first root drive".to_string()))
    });
    let mut meta_a = String::new();
    if ctr_step(&mut session, &mut steps, "start_la", &["start-image", &image, "la", "sleep", "300"], "STARTED").is_some() {
        step(&mut steps, "meta_la_recorded", || {
            let (rc, lines) = session.ctr(&["meta", "la"])?;
            let line = lines.first().cloned().unwrap_or_default();
            if rc != 0 {
                return Err(line);
            }
            let layers = line.split("\"lower\":[").nth(1).and_then(|s| s.split(']').next()).map(|s| s.matches("/fs").count()).unwrap_or(0);
            if meta_field(&line, "strategy") != Some("block-first") || meta_field(&line, "kind") != Some("overlay") || layers < 2 {
                return Err(format!("expected block-first overlay with 2 lower layers: {line}"));
            }
            meta_a = line.clone();
            Ok(((), format!("strategy block-first, overlay, {} lower layers, cached={}, build_ms={}", layers, meta_field(&line, "cached").unwrap_or("?"), meta_field(&line, "build_ms").unwrap_or("?"))))
        });
    }
    if ctr_step(&mut session, &mut steps, "start_lb_same_chain", &["start-image", &image, "lb", "sleep", "300"], "STARTED").is_some() {
        step(&mut steps, "meta_lb_cache_hit", || {
            let (rc, lines) = session.ctr(&["meta", "lb"])?;
            let line = lines.first().cloned().unwrap_or_default();
            if rc != 0 {
                return Err(line);
            }
            if meta_field(&line, "cached") != Some("true") {
                return Err(format!("second task from the same chain must hit the cache: {line}"));
            }
            if meta_field(&line, "key") != meta_field(&meta_a, "key") || meta_field(&line, "sha256") != meta_field(&meta_a, "sha256") {
                return Err(format!("key or digest differ between la and lb: {} vs {}", meta_a, line));
            }
            Ok(((), format!("cached=true, same key and sha256 as la, build_ms={}", meta_field(&line, "build_ms").unwrap_or("?"))))
        });
    }
    step(&mut steps, "cache_one_drive_per_chain", || {
        let (_, lines) = session.ctr(&["cache"])?;
        let n: usize = lines.first().and_then(|l| l.strip_prefix("CACHE ")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
        if n == 0 || lines.len() != n + 1 {
            return Err(format!("unexpected cache listing: {}", lines.join(" | ")));
        }
        Ok(((), format!("{n} cached drive(s): {}", lines[1..].join("; "))))
    });
    step(&mut steps, "layer_dedup_shared_base", || {
        let (_, lines) = session.ctr(&["start", "e2e-base", "sleep", "300"])?;
        if !lines.first().map(|l| l.starts_with("STARTED")).unwrap_or(false) {
            return Err(lines.join(" | "));
        }
        let (_, m) = session.ctr(&["meta", "e2e-base"])?;
        let base = m.first().cloned().unwrap_or_default();
        let _ = session.ctr(&["kill", "e2e-base"]);
        let _ = session.ctr(&["delete", "e2e-base"]);
        let base_lower = base.split("\"lower\":[").nth(1).and_then(|s| s.split(']').next()).unwrap_or("").replace('"', "");
        let base_lower = base_lower.split(',').last().unwrap_or("").trim().to_string();
        if base_lower.is_empty() || !meta_a.contains(&base_lower) {
            return Err(format!("the layered image's chain does not reuse the base layer snapshot {base_lower:?}: {meta_a}"));
        }
        Ok(((), format!("base layer snapshot {base_lower} shared by both images (content store dedup)")))
    });
    for name in ["la", "lb"] {
        ctr_step(&mut session, &mut steps, &format!("kill_{name}"), &["kill", name], "KILLED");
        ctr_step(&mut session, &mut steps, &format!("delete_{name}"), &["delete", name], "DELETED");
    }
    ctr_step(&mut session, &mut steps, "no_leftovers", &["leftovers"], "CLEAN");
    let passed = steps.iter().all(|s| s.ok) && steps.len() >= 13;
    report(endpoint, steps, passed)
}

