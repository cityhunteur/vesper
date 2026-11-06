//! Host capability and entitlement policy (layer 1). Backend selection for GH #17: Backend A needs
//! nested virtualization (macOS 15 or later on an Apple M3 or later chip); other hosts route to the
//! fallback (Backend B, not implemented yet) with a diagnostic instead of a crash.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn validate_entitlements_file(
    path: Option<&Path>,
    required_keys: &[&str],
) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };

    let content = fs::read_to_string(path)
        .map_err(|err| format!("read entitlements file {}: {err}", path.display()))?;

    for key in required_keys {
        let needle = format!("<key>{key}</key>");
        if !content.contains(&needle) {
            return Err(format!(
                "missing required entitlement key {key:?} in {}",
                path.display()
            ));
        }
    }

    Ok(())
}

/// Host facts as printed by `vesper-vmlauncher --host-capabilities`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCapabilities {
    /// The nested-virtualization API exists (macOS 15 or later).
    pub api_available: bool,
    pub nested_virtualization_supported: bool,
    pub virtualization_supported: bool,
    pub macos: String,
    pub chip: String,
    pub model: String,
}

pub fn parse_host_capabilities(json: &str) -> Result<HostCapabilities, String> {
    serde_json::from_str(json.trim()).map_err(|err| format!("parse host capabilities: {err}"))
}

/// Runs the launcher's probe; `launcher` is the signed `vesper-vmlauncher` binary.
pub fn probe_host_capabilities(launcher: &Path) -> Result<HostCapabilities, String> {
    let output = Command::new(launcher)
        .arg("--host-capabilities")
        .output()
        .map_err(|err| format!("run {} --host-capabilities: {err} (run `make vmlauncher-build`)", launcher.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} --host-capabilities exited with {}: {}",
            launcher.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_host_capabilities(&String::from_utf8_lossy(&output.stdout))
}

/// Backend policy for a run: `auto` picks Backend A when nested virtualization is supported,
/// `require` refuses to run otherwise, `off` never enables it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NestedPolicy {
    Auto,
    Require,
    Off,
}

impl NestedPolicy {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(NestedPolicy::Auto),
            "require" => Ok(NestedPolicy::Require),
            "off" => Ok(NestedPolicy::Off),
            other => Err(format!("nested virtualization policy must be auto, require or off, got {other:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// Stock Firecracker inside the System VM under nested KVM (design-v0 Backend A).
    BackendA,
    /// Native macOS backend (design-v0 Backend B); designed, not implemented.
    BackendB,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendSelection {
    pub backend: Backend,
    /// Whether the System VM may be started with nested virtualization enabled.
    pub nested_virtualization: bool,
    pub implemented: bool,
    pub diagnostics: Vec<String>,
}

/// Decides the backend from host facts and policy. Never panics; unsupported hosts get Backend B with
/// diagnostics, `require` on an unsupported host is an error with the same diagnostics.
pub fn select_backend(caps: &HostCapabilities, policy: NestedPolicy) -> Result<BackendSelection, String> {
    let mut diagnostics = vec![format!(
        "host: {} ({}), macOS {}",
        caps.chip.trim(),
        caps.model.trim(),
        caps.macos.trim()
    )];
    if !caps.virtualization_supported {
        diagnostics.push("Virtualization.framework reports no support: no System VM can start on this host".to_string());
    }
    let supported = caps.api_available && caps.nested_virtualization_supported;
    if !caps.api_available {
        diagnostics.push("nested virtualization API unavailable: macOS 15 or later is required".to_string());
    } else if !caps.nested_virtualization_supported {
        diagnostics.push("nested virtualization unsupported by this chip: Backend A needs an Apple M3 or later".to_string());
    } else {
        diagnostics.push("nested virtualization supported: Backend A (stock Firecracker in the System VM) is available".to_string());
    }
    match policy {
        NestedPolicy::Off => {
            diagnostics.push("policy off: Backend A not selected; the System VM runs without nested virtualization".to_string());
            Ok(BackendSelection { backend: Backend::BackendB, nested_virtualization: false, implemented: false, diagnostics })
        }
        NestedPolicy::Require if !supported => {
            diagnostics.push("policy require: refusing to run without nested virtualization".to_string());
            Err(diagnostics.join("; "))
        }
        NestedPolicy::Auto if !supported => {
            diagnostics.push("fallback: Backend B (native macOS backend) is selected; it is designed but not implemented yet (design-v0 Phase 5)".to_string());
            Ok(BackendSelection { backend: Backend::BackendB, nested_virtualization: false, implemented: false, diagnostics })
        }
        _ => Ok(BackendSelection { backend: Backend::BackendA, nested_virtualization: true, implemented: true, diagnostics }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_entitlement_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.entitlements");
        let content = "<?xml version=\"1.0\"?><plist><dict><key>com.apple.security.virtualization</key><true/></dict></plist>";
        std::fs::write(&path, content).unwrap();

        validate_entitlements_file(Some(path.as_path()), &["com.apple.security.virtualization"])
            .unwrap();

        let err = validate_entitlements_file(Some(path.as_path()), &["com.apple.vm.networking"])
            .unwrap_err();
        assert!(err.contains("com.apple.vm.networking"));
    }

    fn caps(api: bool, nested: bool, chip: &str) -> HostCapabilities {
        HostCapabilities {
            api_available: api,
            nested_virtualization_supported: nested,
            virtualization_supported: true,
            macos: "15.1.0".to_string(),
            chip: chip.to_string(),
            model: "MacX,Y".to_string(),
        }
    }

    #[test]
    fn m3_host_selects_backend_a() {
        let selection = select_backend(&caps(true, true, "Apple M3 Pro"), NestedPolicy::Auto).unwrap();
        assert_eq!(selection.backend, Backend::BackendA);
        assert!(selection.nested_virtualization && selection.implemented);
        assert!(selection.diagnostics.iter().any(|d| d.contains("Backend A")));
        let required = select_backend(&caps(true, true, "Apple M3 Pro"), NestedPolicy::Require).unwrap();
        assert_eq!(required.backend, Backend::BackendA);
    }

    #[test]
    fn m1_host_routes_to_fallback_without_panicking() {
        let selection = select_backend(&caps(true, false, "Apple M1"), NestedPolicy::Auto).unwrap();
        assert_eq!(selection.backend, Backend::BackendB);
        assert!(!selection.nested_virtualization && !selection.implemented);
        assert!(selection.diagnostics.iter().any(|d| d.contains("M3 or later")));
        assert!(selection.diagnostics.iter().any(|d| d.contains("not implemented")));
        let err = select_backend(&caps(true, false, "Apple M1"), NestedPolicy::Require).unwrap_err();
        assert!(err.contains("refusing to run"));
        let old = select_backend(&caps(false, false, "Apple M2"), NestedPolicy::Auto).unwrap();
        assert!(old.diagnostics.iter().any(|d| d.contains("macOS 15")));
        let off = select_backend(&caps(true, true, "Apple M3"), NestedPolicy::Off).unwrap();
        assert!(!off.nested_virtualization);
    }

    #[test]
    fn parses_launcher_output_and_policy_names() {
        let json = r#"{"api_available":true,"nested_virtualization_supported":true,"virtualization_supported":true,"macos":"27.0.1","chip":"Apple M3 Pro","model":"Mac15,6"}"#;
        let caps = parse_host_capabilities(json).unwrap();
        assert_eq!(caps.model, "Mac15,6");
        assert!(parse_host_capabilities("{\"nope\":1}").is_err());
        assert_eq!(NestedPolicy::parse("Require").unwrap(), NestedPolicy::Require);
        assert!(NestedPolicy::parse("maybe").is_err());
        let err = probe_host_capabilities(Path::new("/nonexistent/vesper-vmlauncher")).unwrap_err();
        assert!(err.contains("make vmlauncher-build"));
    }
}
