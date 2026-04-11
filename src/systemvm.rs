use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageBuildConfig {
    pub kernel_path: String,
    pub rootfs_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageManifest {
    pub schema_version: String,
    pub kernel_path: String,
    pub kernel_sha256: String,
    pub rootfs_path: String,
    pub rootfs_sha256: String,
    pub image_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchConfig {
    pub kernel_path: String,
    pub rootfs_path: String,
    pub network_mode: String,
    pub cpu_count: u16,
    pub memory_mib: u32,
    pub console_log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootCheckConfig {
    pub iterations: u32,
    pub timeout_seconds: u64,
    pub command: String,
    pub args: Vec<String>,
    pub log_path: String,
    pub report_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootCheckReport {
    pub iterations: u32,
    pub successful: u32,
    pub failed: u32,
    #[serde(default)]
    pub failures: Vec<String>,
}

pub fn build_manifest(config: &ImageBuildConfig) -> Result<ImageManifest, String> {
    if config.kernel_path.trim().is_empty() {
        return Err("kernel path is required".to_string());
    }
    if config.rootfs_path.trim().is_empty() {
        return Err("rootfs path is required".to_string());
    }

    let kernel_sha = digest_file(&config.kernel_path)?;
    let rootfs_sha = digest_file(&config.rootfs_path)?;

    let mut identity = Sha256::new();
    identity.update(kernel_sha.as_bytes());
    identity.update(b":");
    identity.update(rootfs_sha.as_bytes());

    Ok(ImageManifest {
        schema_version: "1".to_string(),
        kernel_path: config.kernel_path.clone(),
        kernel_sha256: kernel_sha,
        rootfs_path: config.rootfs_path.clone(),
        rootfs_sha256: rootfs_sha,
        image_id: hex::encode(identity.finalize()),
    })
}

pub fn write_manifest(path: impl AsRef<Path>, manifest: &ImageManifest) -> Result<(), String> {
    let path = path.as_ref();
    ensure_parent(path)?;
    let mut content = serde_json::to_string_pretty(manifest)
        .map_err(|err| format!("serialize manifest: {err}"))?;
    content.push('\n');
    fs::write(path, content).map_err(|err| format!("write manifest {}: {err}", path.display()))
}

pub fn default_launch_config(
    kernel_path: &str,
    rootfs_path: &str,
    console_log_path: &str,
) -> Result<LaunchConfig, String> {
    if kernel_path.trim().is_empty() {
        return Err("kernel path is required".to_string());
    }
    if rootfs_path.trim().is_empty() {
        return Err("rootfs path is required".to_string());
    }
    if console_log_path.trim().is_empty() {
        return Err("console log path is required".to_string());
    }

    Ok(LaunchConfig {
        kernel_path: kernel_path.trim().to_string(),
        rootfs_path: rootfs_path.trim().to_string(),
        network_mode: "nat".to_string(),
        cpu_count: 2,
        memory_mib: 2048,
        console_log_path: console_log_path.trim().to_string(),
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
            .spawn()
            .map_err(|err| format!("spawn boot command: {err}"))?;

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

        match status {
            Ok(exit) if exit.success() => {
                report.successful += 1;
                log.push_str("result=ok\n");
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

fn digest_file(path: &str) -> Result<String, String> {
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

        let a = build_manifest(&ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: rootfs.display().to_string(),
        })
        .unwrap();

        let b = build_manifest(&ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: rootfs.display().to_string(),
        })
        .unwrap();

        assert_eq!(a.image_id, b.image_id);
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
        };

        let err = run_boot_check(&config).unwrap_err();
        assert!(err.contains("boot check failed"));
    }
}
