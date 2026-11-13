use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) use crate::platform::NestedPolicy;

mod boot_check;
mod bridge;
mod data_disk;
mod image_lock;
mod packages;
pub use boot_check::*;
pub use bridge::*;
pub use data_disk::*;
pub use image_lock::*;
pub use packages::*;

pub const MANIFEST_SCHEMA_VERSION: &str = "2";
pub const IMAGE_LOCK_SCHEMA_VERSION: u32 = 6;
pub const DEFAULT_KERNEL_CMDLINE: &str = "console=hvc0";
pub const DEFAULT_READY_PATTERN: &str = "login:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageBuildConfig {
    pub kernel_path: String,
    #[serde(default)]
    pub rootfs_path: Option<String>,
    #[serde(default)]
    pub initrd_path: Option<String>,
    /// Read-only block device holding the distribution's boot media (for example an ISO).
    #[serde(default)]
    pub boot_media_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageManifest {
    pub schema_version: String,
    pub kernel_path: String,
    pub kernel_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rootfs_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rootfs_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initrd_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initrd_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_media_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_media_sha256: Option<String>,
    /// Derived initramfs (pinned initramfs + overlay archive) when the lock has an `overlay` section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_initrd_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_initrd_sha256: Option<String>,
    pub image_id: String,
}

/// A host directory shared into the System VM over virtio-fs (GH #29). The guest mounts `tag` at the
/// same absolute path as `host_path`, so Docker bind mounts name host paths unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedDirectory {
    pub tag: String,
    pub host_path: String,
    #[serde(default)]
    pub read_only: bool,
    /// The launcher watches the host directory and asks the guest to replay edits as inotify events
    /// (GH #150). Never set for a read-only share: the guest cannot touch files there.
    #[serde(default)]
    pub watch: bool,
}

/// Builds the share list from `PATH[:ro][:nowatch]` specs; tags are `share0`, `share1`, … (virtio-fs tags
/// are short). A read-write share is watched for host edits unless the spec ends in `:nowatch`.
pub fn shared_directories_from_specs(specs: &[String]) -> Result<Vec<SharedDirectory>, String> {
    let mut out = Vec::new();
    for (i, spec) in specs.iter().enumerate() {
        let (mut path, mut ro, mut nowatch) = (spec.as_str(), false, false);
        loop {
            if let Some(p) = path.strip_suffix(":ro").filter(|_| !ro) {
                (path, ro) = (p, true);
            } else if let Some(p) = path.strip_suffix(":nowatch").filter(|_| !nowatch) {
                (path, nowatch) = (p, true);
            } else {
                break;
            }
        }
        if path.is_empty() || !path.starts_with('/') {
            return Err(format!("share {spec:?}: the host path must be absolute"));
        }
        if !Path::new(path).is_dir() {
            return Err(format!("share {spec:?}: {path} is not a directory"));
        }
        out.push(SharedDirectory { tag: format!("share{i}"), host_path: path.trim_end_matches('/').to_string(), read_only: ro, watch: !ro && !nowatch });
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchConfig {
    pub kernel_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rootfs_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initrd_path: Option<String>,
    /// Attached read-only as a virtio block device; the rootfs (if any) is attached writable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_media_path: Option<String>,
    pub kernel_cmdline: String,
    /// Console text that marks a healthy boot for the launcher and boot smoke.
    pub ready_pattern: String,
    /// Boot-time threshold checked by boot smoke against the launcher's reported ready time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_boot_ms: Option<u64>,
    /// Console-driven guest probe the launcher runs after the ready pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<ProbeSettings>,
    /// Host Unix socket the launcher serves as the vsock bridge (`CONNECT <port>` preface).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vsock_bridge_path: Option<String>,
    /// Guest vsock port of the Vesper agent, for `systemvmctl control`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_port: Option<u32>,
    /// `auto`, `require` or `off` (GH #17); the launcher enables nested virtualization accordingly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested_virtualization: Option<String>,
    pub network_mode: String,
    pub cpu_count: u16,
    pub memory_mib: u32,
    pub console_log_path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_directories: Vec<SharedDirectory>,
    /// Persistent data disk on macOS, attached writable after the boot media and rootfs (GH #164).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_disk_path: Option<String>,
    /// System VM vsock ports the bridge may connect to; every other port is refused (TD-027).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bridge_allowed_ports: Vec<u32>,
}

/// Console-driven guest probe: log in on the serial console, run commands, expect a pattern, shut down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeSettings {
    /// Typed at the `ready_pattern` prompt (password-less login expected).
    pub login_user: String,
    /// Console text that marks an interactive shell, for example `~# `.
    pub shell_prompt: String,
    /// Typed one per line at the shell prompt, in order.
    pub commands: Vec<String>,
    /// Console text that proves the probe succeeded; must be produced by the commands, not echoed input.
    pub success_pattern: String,
    /// Typed last; the guest must stop on its own afterwards.
    pub shutdown_command: String,
}

impl ProbeSettings {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("login_user", &self.login_user),
            ("shell_prompt", &self.shell_prompt),
            ("success_pattern", &self.success_pattern),
            ("shutdown_command", &self.shutdown_command),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{name} must not be blank"));
            }
        }
        if self.commands.is_empty() || self.commands.iter().any(|c| c.trim().is_empty()) {
            return Err("commands must be a non-empty list of non-blank lines".to_string());
        }
        let lines = [self.login_user.as_str(), self.shutdown_command.as_str()]
            .into_iter()
            .chain(self.commands.iter().map(String::as_str));
        for line in lines {
            if line.contains('\n') {
                return Err("console lines must not contain newlines".to_string());
            }
            if line.contains(&self.success_pattern) {
                return Err(format!(
                    "success_pattern {:?} appears in a typed line; the console echoes input, so the match would be a false positive",
                    self.success_pattern
                ));
            }
        }
        Ok(())
    }
}

/// Inputs for `default_launch_config`; every optional field may be omitted.
#[derive(Debug, Clone, Default)]
pub struct LaunchInputs<'a> {
    pub kernel_path: &'a str,
    pub rootfs_path: Option<&'a str>,
    pub initrd_path: Option<&'a str>,
    pub boot_media_path: Option<&'a str>,
    /// Full kernel command line; `None` derives one from `DEFAULT_KERNEL_CMDLINE` and the disks.
    pub kernel_cmdline: Option<&'a str>,
    pub ready_pattern: Option<&'a str>,
    pub max_boot_ms: Option<u64>,
    pub probe: Option<&'a ProbeSettings>,
    pub vsock_bridge_path: Option<&'a str>,
    pub agent_port: Option<u32>,
    pub nested_virtualization: Option<&'a str>,
    pub console_log_path: &'a str,
}


pub fn build_manifest(config: &ImageBuildConfig) -> Result<ImageManifest, String> {
    if config.kernel_path.trim().is_empty() {
        return Err("kernel path is required".to_string());
    }
    let rootfs_path = non_blank(config.rootfs_path.as_deref());
    let initrd_path = non_blank(config.initrd_path.as_deref());
    let boot_media_path = non_blank(config.boot_media_path.as_deref());
    if rootfs_path.is_none() && initrd_path.is_none() {
        return Err("a rootfs path or an initrd path is required".to_string());
    }

    let kernel_sha = digest_file(&config.kernel_path)?;
    let rootfs_sha = rootfs_path.as_deref().map(digest_file).transpose()?;
    let initrd_sha = initrd_path.as_deref().map(digest_file).transpose()?;
    let boot_media_sha = boot_media_path.as_deref().map(digest_file).transpose()?;

    Ok(ImageManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        kernel_path: config.kernel_path.clone(),
        image_id: image_id(
            &kernel_sha,
            rootfs_sha.as_deref(),
            initrd_sha.as_deref(),
            boot_media_sha.as_deref(),
        ),
        kernel_sha256: kernel_sha,
        rootfs_path,
        rootfs_sha256: rootfs_sha,
        initrd_path,
        initrd_sha256: initrd_sha,
        boot_media_path,
        boot_media_sha256: boot_media_sha,
        overlay_initrd_path: None,
        overlay_initrd_sha256: None,
    })
}

/// `sha256("<kernel>:<rootfs or empty>:<initrd or empty>:<boot media or empty>")` over the hex digests.
pub fn image_id(
    kernel_sha256: &str,
    rootfs_sha256: Option<&str>,
    initrd_sha256: Option<&str>,
    boot_media_sha256: Option<&str>,
) -> String {
    let mut identity = Sha256::new();
    identity.update(kernel_sha256.as_bytes());
    identity.update(b":");
    identity.update(rootfs_sha256.unwrap_or("").as_bytes());
    identity.update(b":");
    identity.update(initrd_sha256.unwrap_or("").as_bytes());
    identity.update(b":");
    identity.update(boot_media_sha256.unwrap_or("").as_bytes());
    hex::encode(identity.finalize())
}

pub fn write_manifest(path: impl AsRef<Path>, manifest: &ImageManifest) -> Result<(), String> {
    let path = path.as_ref();
    ensure_parent(path)?;
    let mut content = serde_json::to_string_pretty(manifest)
        .map_err(|err| format!("serialize manifest: {err}"))?;
    content.push('\n');
    fs::write(path, content).map_err(|err| format!("write manifest {}: {err}", path.display()))
}

pub fn default_launch_config(inputs: &LaunchInputs<'_>) -> Result<LaunchConfig, String> {
    if inputs.kernel_path.trim().is_empty() {
        return Err("kernel path is required".to_string());
    }
    let rootfs_path = non_blank(inputs.rootfs_path);
    let initrd_path = non_blank(inputs.initrd_path);
    let boot_media_path = non_blank(inputs.boot_media_path);
    if rootfs_path.is_none() && initrd_path.is_none() {
        return Err("a rootfs path or an initrd path is required".to_string());
    }
    if inputs.console_log_path.trim().is_empty() {
        return Err("console log path is required".to_string());
    }

    let kernel_cmdline = match non_blank(inputs.kernel_cmdline) {
        Some(cmdline) => cmdline,
        None => {
            let mut cmdline = DEFAULT_KERNEL_CMDLINE.to_string();
            if rootfs_path.is_some() {
                // The rootfs is the first block device unless boot media precedes it.
                let root = if boot_media_path.is_some() { "/dev/vdb" } else { "/dev/vda" };
                cmdline.push_str(&format!(" root={root} rw"));
            }
            cmdline
        }
    };
    let ready_pattern =
        non_blank(inputs.ready_pattern).unwrap_or_else(|| DEFAULT_READY_PATTERN.to_string());
    if inputs.max_boot_ms == Some(0) {
        return Err("max_boot_ms must be > 0 when given".to_string());
    }
    if let Some(probe) = inputs.probe {
        probe.validate().map_err(|err| format!("probe: {err}"))?;
    }
    let nested_virtualization = match non_blank(inputs.nested_virtualization) {
        Some(value) => {
            NestedPolicy::parse(&value)?;
            Some(value.to_ascii_lowercase())
        }
        None => None,
    };

    Ok(LaunchConfig {
        kernel_path: inputs.kernel_path.trim().to_string(),
        rootfs_path,
        initrd_path,
        boot_media_path,
        kernel_cmdline,
        ready_pattern,
        max_boot_ms: inputs.max_boot_ms,
        probe: inputs.probe.cloned(),
        vsock_bridge_path: non_blank(inputs.vsock_bridge_path),
        agent_port: inputs.agent_port,
        nested_virtualization,
        network_mode: "nat".to_string(),
        cpu_count: 2,
        memory_mib: 2048,
        console_log_path: inputs.console_log_path.trim().to_string(),
        shared_directories: Vec::new(),
        data_disk_path: None,
        bridge_allowed_ports: Vec::new(),
    })
}

pub fn write_launch_config(path: impl AsRef<Path>, config: &LaunchConfig) -> Result<(), String> {
    let path = path.as_ref();
    ensure_parent(path)?;
    let mut content = serde_json::to_string_pretty(config)
        .map_err(|err| format!("serialize launch config: {err}"))?;
    content.push('\n');
    fs::write(path, content).map_err(|err| format!("write launch config {}: {err}", path.display()))
}

pub(crate) fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn artifact_path(dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err(format!(
            "image artifact {} is missing; run `make systemvm-fetch`",
            path.display()
        ));
    }
    Ok(path)
}

pub(crate) fn check_digest(name: &str, path: &Path, expected: &str) -> Result<(), String> {
    let actual = digest_file(&path.display().to_string())?;
    if actual != expected {
        return Err(format!(
            "{name} {} sha256 {actual} does not match pinned {expected}",
            path.display()
        ));
    }
    Ok(())
}

pub(crate) fn digest_file(path: &str) -> Result<String, String> {
    let content = fs::read(path).map_err(|err| format!("read file {path}: {err}"))?;
    let mut hasher = Sha256::new();
    hasher.update(content);
    Ok(hex::encode(hasher.finalize()))
}

pub(crate) fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create directory {}: {err}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_specs_set_read_only_and_watch() {
        let dir = std::env::temp_dir().to_string_lossy().trim_end_matches('/').to_string();
        let specs: Vec<String> =
            ["", ":ro", ":nowatch", ":ro:nowatch", ":nowatch:ro"].iter().map(|suffix| format!("{dir}{suffix}")).collect();
        let shares = shared_directories_from_specs(&specs).expect("specs parse");
        let flags: Vec<(bool, bool)> = shares.iter().map(|s| (s.read_only, s.watch)).collect();
        // A read-write share is watched by default; `:nowatch` opts out; a read-only share is never watched.
        assert_eq!(flags, vec![(false, true), (true, false), (false, false), (true, false), (true, false)]);
        assert!(shares.iter().all(|s| s.host_path == dir));
        assert_eq!(shares[4].tag, "share4");
        assert!(shared_directories_from_specs(&["relative:nowatch".to_string()]).is_err());
        // Configs written before the field existed still load, unwatched.
        let old: SharedDirectory = serde_json::from_str(r#"{"tag":"share0","host_path":"/x"}"#).expect("old config");
        assert!(!old.watch && !old.read_only);
    }
    use tempfile::tempdir;

    #[test]
    fn build_manifest_is_deterministic() {
        let dir = tempdir().unwrap();
        let kernel = dir.path().join("vmlinux");
        let rootfs = dir.path().join("rootfs.ext4");

        fs::write(&kernel, "kernel").unwrap();
        fs::write(&rootfs, "rootfs").unwrap();

        let config = ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: Some(rootfs.display().to_string()),
            initrd_path: None,
            boot_media_path: None,
        };
        let a = build_manifest(&config).unwrap();
        let b = build_manifest(&config).unwrap();

        assert_eq!(a.image_id, b.image_id);
        assert_eq!(a.schema_version, MANIFEST_SCHEMA_VERSION);
        assert!(a.initrd_path.is_none());
    }

    #[test]
    fn build_manifest_requires_rootfs_or_initrd_and_distinguishes_them() {
        let dir = tempdir().unwrap();
        let kernel = dir.path().join("vmlinux");
        let blob = dir.path().join("blob");
        fs::write(&kernel, "kernel").unwrap();
        fs::write(&blob, "blob").unwrap();

        let err = build_manifest(&ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: Some("  ".to_string()),
            initrd_path: None,
            boot_media_path: None,
        })
        .unwrap_err();
        assert!(err.contains("rootfs path or an initrd path"));

        let as_rootfs = build_manifest(&ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: Some(blob.display().to_string()),
            initrd_path: None,
            boot_media_path: None,
        })
        .unwrap();
        let as_initrd = build_manifest(&ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: None,
            initrd_path: Some(blob.display().to_string()),
            boot_media_path: None,
        })
        .unwrap();
        let with_media = build_manifest(&ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: None,
            initrd_path: Some(blob.display().to_string()),
            boot_media_path: Some(blob.display().to_string()),
        })
        .unwrap();
        assert_ne!(as_rootfs.image_id, as_initrd.image_id);
        assert_ne!(as_initrd.image_id, with_media.image_id);
        assert_eq!(as_initrd.initrd_sha256, as_rootfs.rootfs_sha256);
        assert_eq!(with_media.boot_media_sha256, with_media.initrd_sha256);

        let json = serde_json::to_string(&as_initrd).unwrap();
        assert!(!json.contains("rootfs_path"));
        assert!(!json.contains("boot_media"));
        assert!(json.contains("initrd_sha256"));
    }

    #[test]
    fn launch_config_defaults_cover_initrd_and_rootfs_boots() {
        let initrd = default_launch_config(&LaunchInputs {
            kernel_path: "vmlinuz",
            initrd_path: Some("initramfs"),
            console_log_path: "console.log",
            ..Default::default()
        })
        .unwrap();
        assert_eq!(initrd.kernel_cmdline, DEFAULT_KERNEL_CMDLINE);
        assert_eq!(initrd.ready_pattern, DEFAULT_READY_PATTERN);
        assert_eq!(initrd.network_mode, "nat");
        assert!(initrd.rootfs_path.is_none());
        assert!(initrd.boot_media_path.is_none());

        let rootfs = default_launch_config(&LaunchInputs {
            kernel_path: "vmlinuz",
            rootfs_path: Some("rootfs.ext4"),
            console_log_path: "console.log",
            ..Default::default()
        })
        .unwrap();
        assert_eq!(rootfs.kernel_cmdline, "console=hvc0 root=/dev/vda rw");

        let with_media = default_launch_config(&LaunchInputs {
            kernel_path: "vmlinuz",
            rootfs_path: Some("rootfs.ext4"),
            boot_media_path: Some("alpine.iso"),
            console_log_path: "console.log",
            ..Default::default()
        })
        .unwrap();
        assert_eq!(with_media.kernel_cmdline, "console=hvc0 root=/dev/vdb rw");

        let explicit = default_launch_config(&LaunchInputs {
            kernel_path: "vmlinuz",
            initrd_path: Some("initramfs"),
            kernel_cmdline: Some("console=hvc0 quiet"),
            ready_pattern: Some("Welcome"),
            console_log_path: "console.log",
            ..Default::default()
        })
        .unwrap();
        assert_eq!(explicit.kernel_cmdline, "console=hvc0 quiet");
        assert_eq!(explicit.ready_pattern, "Welcome");
        assert!(explicit.vsock_bridge_path.is_none() && explicit.agent_port.is_none());
        assert!(explicit.nested_virtualization.is_none());
        let nested = default_launch_config(&LaunchInputs {
            kernel_path: "vmlinuz",
            initrd_path: Some("initramfs"),
            nested_virtualization: Some("Require"),
            console_log_path: "console.log",
            ..Default::default()
        })
        .unwrap();
        assert_eq!(nested.nested_virtualization.as_deref(), Some("require"));
        let err = default_launch_config(&LaunchInputs {
            kernel_path: "vmlinuz",
            initrd_path: Some("initramfs"),
            nested_virtualization: Some("maybe"),
            console_log_path: "console.log",
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("auto, require or off"));

        let err = default_launch_config(&LaunchInputs {
            kernel_path: "vmlinuz",
            console_log_path: "console.log",
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("rootfs path or an initrd path"));

        let bad_probe = ProbeSettings {
            login_user: "root".to_string(),
            shell_prompt: "~# ".to_string(),
            commands: vec![],
            success_pattern: "OK".to_string(),
            shutdown_command: "poweroff".to_string(),
        };
        let err = default_launch_config(&LaunchInputs {
            kernel_path: "vmlinuz",
            initrd_path: Some("initramfs"),
            probe: Some(&bad_probe),
            console_log_path: "console.log",
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("probe"));
        let json = serde_json::to_string(&initrd).unwrap();
        assert!(!json.contains("probe") && !json.contains("max_boot_ms"));
    }

}
