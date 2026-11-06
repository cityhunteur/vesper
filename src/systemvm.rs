use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

mod image_lock;
mod packages;
pub use image_lock::*;
pub use packages::*;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const MANIFEST_SCHEMA_VERSION: &str = "2";
pub const IMAGE_LOCK_SCHEMA_VERSION: u32 = 4;
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
    pub network_mode: String,
    pub cpu_count: u16,
    pub memory_mib: u32,
    pub console_log_path: String,
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
    pub console_log_path: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootCheckConfig {
    pub iterations: u32,
    pub timeout_seconds: u64,
    pub command: String,
    pub args: Vec<String>,
    pub log_path: String,
    pub report_path: String,
    /// Fail an iteration whose boot time (launcher `ready` line, else wall clock) exceeds this.
    #[serde(default)]
    pub max_boot_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootCheckReport {
    pub iterations: u32,
    pub successful: u32,
    pub failed: u32,
    #[serde(default)]
    pub failures: Vec<String>,
    /// Wall-clock milliseconds per iteration, successful or not, in order.
    #[serde(default)]
    pub durations_ms: Vec<u64>,
    #[serde(default)]
    pub max_duration_ms: u64,
    #[serde(default)]
    pub mean_duration_ms: u64,
    /// Boot time per iteration: `elapsed_ms` of the launcher's `{"status":"ready",...}` line when the
    /// command printed one, otherwise the wall-clock duration.
    #[serde(default)]
    pub boot_ms: Vec<u64>,
    #[serde(default)]
    pub max_boot_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_boot_allowed_ms: Option<u64>,
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

    Ok(LaunchConfig {
        kernel_path: inputs.kernel_path.trim().to_string(),
        rootfs_path,
        initrd_path,
        boot_media_path,
        kernel_cmdline,
        ready_pattern,
        max_boot_ms: inputs.max_boot_ms,
        probe: inputs.probe.cloned(),
        network_mode: "nat".to_string(),
        cpu_count: 2,
        memory_mib: 2048,
        console_log_path: inputs.console_log_path.trim().to_string(),
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

pub fn run_boot_check(config: &BootCheckConfig) -> Result<BootCheckReport, String> {
    if config.iterations == 0 {
        return Err("iterations must be > 0".to_string());
    }
    if config.command.trim().is_empty() {
        return Err("boot command is required".to_string());
    }
    if config.log_path.trim().is_empty() {
        return Err("log path is required".to_string());
    }
    if config.report_path.trim().is_empty() {
        return Err("report path is required".to_string());
    }

    let log_path = Path::new(&config.log_path);
    let report_path = Path::new(&config.report_path);
    ensure_parent(log_path)?;
    ensure_parent(report_path)?;

    let mut report = BootCheckReport {
        iterations: config.iterations,
        successful: 0,
        failed: 0,
        failures: Vec::new(),
        durations_ms: Vec::new(),
        max_duration_ms: 0,
        mean_duration_ms: 0,
        boot_ms: Vec::new(),
        max_boot_ms: 0,
        max_boot_allowed_ms: config.max_boot_ms,
    };

    let mut log = String::new();
    let timeout = Duration::from_secs(config.timeout_seconds.max(1));

    for iteration in 1..=config.iterations {
        log.push_str(&format!(
            "iteration={iteration} command={}\n",
            config.command
        ));
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|err| format!("spawn boot command: {err}"))?;
        let stdout = child.stdout.take().ok_or("capture boot command stdout")?;
        let reader = thread::spawn(move || {
            let mut out = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::BufReader::new(stdout), &mut out);
            out
        });

        let started_at = Instant::now();
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|err| format!("wait boot command: {err}"))?
            {
                break Ok(status);
            }

            if started_at.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                break Err("timeout".to_string());
            }

            thread::sleep(Duration::from_millis(20));
        };

        let duration_ms = started_at.elapsed().as_millis() as u64;
        report.durations_ms.push(duration_ms);
        let output = reader.join().unwrap_or_default();
        for line in output.lines().filter(|line| !line.trim().is_empty()) {
            log.push_str(&format!("stdout={line}\n"));
        }
        let boot_ms = ready_elapsed_ms(&output).unwrap_or(duration_ms);
        report.boot_ms.push(boot_ms);
        let status = match (status, config.max_boot_ms) {
            (Ok(exit), Some(limit)) if exit.success() && boot_ms > limit => {
                Err(format!("boot took {boot_ms} ms, over the {limit} ms threshold"))
            }
            (other, _) => other,
        };
        match status {
            Ok(exit) if exit.success() => {
                report.successful += 1;
                log.push_str(&format!("result=ok duration_ms={duration_ms} boot_ms={boot_ms}\n"));
            }
            Ok(exit) => {
                report.failed += 1;
                let message = format!(
                    "iteration {iteration}: exit status {}",
                    exit.code().unwrap_or(-1)
                );
                report.failures.push(message.clone());
                log.push_str(&format!("result=failed error={message}\n"));
            }
            Err(error) => {
                report.failed += 1;
                let message = format!("iteration {iteration}: {error}");
                report.failures.push(message.clone());
                log.push_str(&format!("result=failed error={message}\n"));
            }
        }
    }

    report.max_duration_ms = report.durations_ms.iter().copied().max().unwrap_or(0);
    report.max_boot_ms = report.boot_ms.iter().copied().max().unwrap_or(0);
    report.mean_duration_ms = report.durations_ms.iter().sum::<u64>() / report.durations_ms.len().max(1) as u64;

    fs::write(log_path, log)
        .map_err(|err| format!("write boot log {}: {err}", log_path.display()))?;

    let mut report_json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("serialize boot report: {err}"))?;
    report_json.push('\n');
    fs::write(report_path, report_json)
        .map_err(|err| format!("write boot report {}: {err}", report_path.display()))?;

    if report.failed > 0 {
        return Err(format!(
            "boot check failed in {}/{} iterations",
            report.failed, report.iterations
        ));
    }

    Ok(report)
}

/// `elapsed_ms` from the first `{"status":"ready",...}` JSON line the launcher printed.
fn ready_elapsed_ms(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .find(|value| value.get("status").and_then(|s| s.as_str()) == Some("ready"))
        .and_then(|value| value.get("elapsed_ms").and_then(|ms| ms.as_u64()))
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

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create directory {}: {err}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn boot_check_detects_failure() {
        let dir = tempdir().unwrap();
        let config = BootCheckConfig {
            iterations: 1,
            timeout_seconds: 5,
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 1".to_string()],
            log_path: dir.path().join("boot.log").display().to_string(),
            report_path: dir.path().join("boot-report.json").display().to_string(),
            max_boot_ms: None,
        };

        let err = run_boot_check(&config).unwrap_err();
        assert!(err.contains("boot check failed"));
    }

    #[test]
    fn boot_check_reads_launcher_ready_time_and_enforces_threshold() {
        let dir = tempdir().unwrap();
        let script = r#"echo '{"status":"ready","elapsed_ms":1200,"ready_pattern":"login:"}'; echo '{"status":"stopped","elapsed_ms":1900}'"#;
        let mut config = BootCheckConfig {
            iterations: 1,
            timeout_seconds: 5,
            command: "sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            log_path: dir.path().join("boot.log").display().to_string(),
            report_path: dir.path().join("boot-report.json").display().to_string(),
            max_boot_ms: Some(5000),
        };
        let report = run_boot_check(&config).unwrap();
        assert_eq!(report.boot_ms, vec![1200]);
        assert_eq!(report.max_boot_ms, 1200);
        assert_eq!(report.max_boot_allowed_ms, Some(5000));
        let log = fs::read_to_string(&config.log_path).unwrap();
        assert!(log.contains("boot_ms=1200"));
        assert!(log.contains(r#"stdout={"status":"ready""#));

        config.max_boot_ms = Some(1000);
        let err = run_boot_check(&config).unwrap_err();
        assert!(err.contains("boot check failed"));
        let written: BootCheckReport =
            serde_json::from_str(&fs::read_to_string(&config.report_path).unwrap()).unwrap();
        assert_eq!(written.failed, 1);
        assert!(written.failures[0].contains("over the 1000 ms threshold"));
    }

    #[test]
    fn boot_check_records_durations() {
        let dir = tempdir().unwrap();
        let report_path = dir.path().join("boot-report.json");
        let config = BootCheckConfig {
            iterations: 2,
            timeout_seconds: 5,
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 0".to_string()],
            log_path: dir.path().join("boot.log").display().to_string(),
            report_path: report_path.display().to_string(),
            max_boot_ms: None,
        };

        let report = run_boot_check(&config).unwrap();
        assert_eq!(report.durations_ms.len(), 2);
        assert_eq!(report.boot_ms, report.durations_ms, "no ready line: boot_ms falls back to wall clock");
        assert!(report.max_duration_ms >= report.mean_duration_ms);
        let written: BootCheckReport =
            serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
        assert_eq!(written.durations_ms, report.durations_ms);
        let log = fs::read_to_string(dir.path().join("boot.log")).unwrap();
        assert_eq!(log.matches("duration_ms=").count(), 2);
    }
}
