use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub const MANIFEST_SCHEMA_VERSION: &str = "2";
pub const IMAGE_LOCK_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_KERNEL_CMDLINE: &str = "console=hvc0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageBuildConfig {
    pub kernel_path: String,
    #[serde(default)]
    pub rootfs_path: Option<String>,
    #[serde(default)]
    pub initrd_path: Option<String>,
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
    pub image_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchConfig {
    pub kernel_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rootfs_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initrd_path: Option<String>,
    pub kernel_cmdline: String,
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

/// Pinned upstream System VM image, parsed from `docs/specs/systemvm-image.lock.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageLock {
    pub schema_version: u32,
    pub distribution: String,
    pub branch: String,
    pub release: String,
    pub arch: String,
    pub flavor: String,
    pub kernel_version: String,
    pub source: ImageSource,
    pub artifacts: ImageArtifacts,
    pub expected_image_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageSource {
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub published: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageArtifacts {
    pub kernel: ImageArtifact,
    pub initrd: ImageArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageArtifact {
    /// Path of the member inside the source archive, for example `boot/vmlinuz-virt`.
    pub member: String,
    pub sha256: String,
    pub format: String,
}

impl ImageArtifact {
    /// File name the fetch script extracts the member to inside the image directory.
    pub fn file_name(&self) -> Result<&str, String> {
        Path::new(&self.member)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("archive member {:?} has no file name", self.member))
    }
}

pub fn build_manifest(config: &ImageBuildConfig) -> Result<ImageManifest, String> {
    if config.kernel_path.trim().is_empty() {
        return Err("kernel path is required".to_string());
    }
    let rootfs_path = non_blank(config.rootfs_path.as_deref());
    let initrd_path = non_blank(config.initrd_path.as_deref());
    if rootfs_path.is_none() && initrd_path.is_none() {
        return Err("a rootfs path or an initrd path is required".to_string());
    }

    let kernel_sha = digest_file(&config.kernel_path)?;
    let rootfs_sha = rootfs_path
        .as_deref()
        .map(digest_file)
        .transpose()?;
    let initrd_sha = initrd_path
        .as_deref()
        .map(digest_file)
        .transpose()?;

    Ok(ImageManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        kernel_path: config.kernel_path.clone(),
        image_id: image_id(&kernel_sha, rootfs_sha.as_deref(), initrd_sha.as_deref()),
        kernel_sha256: kernel_sha,
        rootfs_path,
        rootfs_sha256: rootfs_sha,
        initrd_path,
        initrd_sha256: initrd_sha,
    })
}

/// `sha256("<kernel>:<rootfs or empty>:<initrd or empty>")` over the hex digests.
pub fn image_id(kernel_sha256: &str, rootfs_sha256: Option<&str>, initrd_sha256: Option<&str>) -> String {
    let mut identity = Sha256::new();
    identity.update(kernel_sha256.as_bytes());
    identity.update(b":");
    identity.update(rootfs_sha256.unwrap_or("").as_bytes());
    identity.update(b":");
    identity.update(initrd_sha256.unwrap_or("").as_bytes());
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

pub fn load_image_lock(path: impl AsRef<Path>) -> Result<ImageLock, String> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|err| format!("read image lock {}: {err}", path.display()))?;
    let lock: ImageLock = serde_yaml::from_str(&content)
        .map_err(|err| format!("parse image lock {}: {err}", path.display()))?;
    validate_image_lock(&lock)?;
    Ok(lock)
}

pub fn validate_image_lock(lock: &ImageLock) -> Result<(), String> {
    if lock.schema_version != IMAGE_LOCK_SCHEMA_VERSION {
        return Err(format!(
            "image lock schema_version {} is not supported (expected {IMAGE_LOCK_SCHEMA_VERSION})",
            lock.schema_version
        ));
    }
    for (name, value) in [
        ("distribution", &lock.distribution),
        ("branch", &lock.branch),
        ("release", &lock.release),
        ("arch", &lock.arch),
        ("flavor", &lock.flavor),
        ("kernel_version", &lock.kernel_version),
        ("source.published", &lock.source.published),
        ("artifacts.kernel.format", &lock.artifacts.kernel.format),
        ("artifacts.initrd.format", &lock.artifacts.initrd.format),
    ] {
        if value.trim().is_empty() {
            return Err(format!("image lock field {name} must not be blank"));
        }
    }
    if !lock.source.url.starts_with("https://") {
        return Err("image lock source.url must use https".to_string());
    }
    if lock.source.size_bytes == 0 {
        return Err("image lock source.size_bytes must be > 0".to_string());
    }
    for (name, value) in [
        ("source.sha256", &lock.source.sha256),
        ("artifacts.kernel.sha256", &lock.artifacts.kernel.sha256),
        ("artifacts.initrd.sha256", &lock.artifacts.initrd.sha256),
        ("expected_image_id", &lock.expected_image_id),
    ] {
        if !is_sha256_hex(value) {
            return Err(format!("image lock field {name} must be 64 lowercase hex characters"));
        }
    }
    lock.artifacts.kernel.file_name()?;
    lock.artifacts.initrd.file_name()?;
    if lock.artifacts.kernel.file_name()? == lock.artifacts.initrd.file_name()? {
        return Err("image lock kernel and initrd members must have distinct file names".to_string());
    }
    Ok(())
}

/// Shell-sourceable `KEY=VALUE` lines for `scripts/systemvm/fetch-image.sh`.
pub fn image_lock_env(lock: &ImageLock) -> String {
    let pairs = [
        ("VESPER_IMAGE_DISTRIBUTION", lock.distribution.as_str()),
        ("VESPER_IMAGE_RELEASE", lock.release.as_str()),
        ("VESPER_IMAGE_ARCH", lock.arch.as_str()),
        ("VESPER_IMAGE_URL", lock.source.url.as_str()),
        ("VESPER_IMAGE_SHA256", lock.source.sha256.as_str()),
        ("VESPER_IMAGE_KERNEL_MEMBER", lock.artifacts.kernel.member.as_str()),
        ("VESPER_IMAGE_KERNEL_SHA256", lock.artifacts.kernel.sha256.as_str()),
        ("VESPER_IMAGE_INITRD_MEMBER", lock.artifacts.initrd.member.as_str()),
        ("VESPER_IMAGE_INITRD_SHA256", lock.artifacts.initrd.sha256.as_str()),
        ("VESPER_IMAGE_EXPECTED_ID", lock.expected_image_id.as_str()),
    ];
    let mut out = String::new();
    for (key, value) in pairs {
        out.push_str(key);
        out.push_str("='");
        out.push_str(&value.replace('\'', "'\\''"));
        out.push_str("'\n");
    }
    out
}

/// Checks that `dir` holds the kernel and initrd named by the lock with the pinned digests, and
/// returns the manifest for them. The manifest `image_id` must equal `lock.expected_image_id`.
pub fn verify_image(lock: &ImageLock, dir: impl AsRef<Path>) -> Result<ImageManifest, String> {
    let dir = dir.as_ref();
    let kernel = artifact_path(dir, &lock.artifacts.kernel)?;
    let initrd = artifact_path(dir, &lock.artifacts.initrd)?;
    check_digest("kernel", &kernel, &lock.artifacts.kernel.sha256)?;
    check_digest("initrd", &initrd, &lock.artifacts.initrd.sha256)?;

    let manifest = build_manifest(&ImageBuildConfig {
        kernel_path: kernel.display().to_string(),
        rootfs_path: None,
        initrd_path: Some(initrd.display().to_string()),
    })?;
    if manifest.image_id != lock.expected_image_id {
        return Err(format!(
            "image_id {} does not match expected_image_id {} in the lock",
            manifest.image_id, lock.expected_image_id
        ));
    }
    Ok(manifest)
}

pub fn default_launch_config(
    kernel_path: &str,
    rootfs_path: Option<&str>,
    initrd_path: Option<&str>,
    console_log_path: &str,
) -> Result<LaunchConfig, String> {
    if kernel_path.trim().is_empty() {
        return Err("kernel path is required".to_string());
    }
    let rootfs_path = non_blank(rootfs_path);
    let initrd_path = non_blank(initrd_path);
    if rootfs_path.is_none() && initrd_path.is_none() {
        return Err("a rootfs path or an initrd path is required".to_string());
    }
    if console_log_path.trim().is_empty() {
        return Err("console log path is required".to_string());
    }

    let mut kernel_cmdline = DEFAULT_KERNEL_CMDLINE.to_string();
    if rootfs_path.is_some() {
        kernel_cmdline.push_str(" root=/dev/vda rw");
    }

    Ok(LaunchConfig {
        kernel_path: kernel_path.trim().to_string(),
        rootfs_path,
        initrd_path,
        kernel_cmdline,
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

fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn artifact_path(dir: &Path, artifact: &ImageArtifact) -> Result<PathBuf, String> {
    let path = dir.join(artifact.file_name()?);
    if !path.is_file() {
        return Err(format!(
            "image artifact {} is missing; run `make systemvm-fetch`",
            path.display()
        ));
    }
    Ok(path)
}

fn check_digest(name: &str, path: &Path, expected: &str) -> Result<(), String> {
    let actual = digest_file(&path.display().to_string())?;
    if actual != expected {
        return Err(format!(
            "{name} {} sha256 {actual} does not match pinned {expected}",
            path.display()
        ));
    }
    Ok(())
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

    const REPO_LOCK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/specs/systemvm-image.lock.yaml");

    fn sample_lock() -> ImageLock {
        load_image_lock(REPO_LOCK).unwrap()
    }

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
        })
        .unwrap_err();
        assert!(err.contains("rootfs path or an initrd path"));

        let as_rootfs = build_manifest(&ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: Some(blob.display().to_string()),
            initrd_path: None,
        })
        .unwrap();
        let as_initrd = build_manifest(&ImageBuildConfig {
            kernel_path: kernel.display().to_string(),
            rootfs_path: None,
            initrd_path: Some(blob.display().to_string()),
        })
        .unwrap();
        assert_ne!(as_rootfs.image_id, as_initrd.image_id);
        assert_eq!(as_initrd.initrd_sha256, as_rootfs.rootfs_sha256);

        let json = serde_json::to_string(&as_initrd).unwrap();
        assert!(!json.contains("rootfs_path"));
        assert!(json.contains("initrd_sha256"));
    }

    #[test]
    fn validates_repo_image_lock() {
        let lock = sample_lock();
        assert_eq!(lock.schema_version, IMAGE_LOCK_SCHEMA_VERSION);
        assert_eq!(lock.arch, "aarch64");
        assert_eq!(lock.artifacts.kernel.file_name().unwrap(), "vmlinuz-virt");
        assert_eq!(lock.artifacts.initrd.file_name().unwrap(), "initramfs-virt");
        assert_eq!(
            lock.expected_image_id,
            image_id(
                &lock.artifacts.kernel.sha256,
                None,
                Some(&lock.artifacts.initrd.sha256)
            ),
            "expected_image_id must be derived from the pinned artifact digests"
        );
    }

    #[test]
    fn image_lock_rejects_bad_digests_and_unknown_fields() {
        let mut lock = sample_lock();
        lock.source.sha256 = "ABC".to_string();
        let err = validate_image_lock(&lock).unwrap_err();
        assert!(err.contains("source.sha256"));

        let mut lock = sample_lock();
        lock.source.url = "http://example.invalid/x.tar.gz".to_string();
        assert!(validate_image_lock(&lock).unwrap_err().contains("https"));

        let with_extra = fs::read_to_string(REPO_LOCK).unwrap() + "\nextra_field: 1\n";
        let err = serde_yaml::from_str::<ImageLock>(&with_extra).unwrap_err().to_string();
        assert!(err.contains("extra_field"));
    }

    #[test]
    fn image_lock_env_is_shell_sourceable() {
        let env = image_lock_env(&sample_lock());
        for key in [
            "VESPER_IMAGE_URL=",
            "VESPER_IMAGE_SHA256=",
            "VESPER_IMAGE_KERNEL_MEMBER='boot/vmlinuz-virt'",
            "VESPER_IMAGE_INITRD_MEMBER='boot/initramfs-virt'",
            "VESPER_IMAGE_EXPECTED_ID=",
        ] {
            assert!(env.contains(key), "missing {key} in\n{env}");
        }
        assert!(env.lines().all(|line| line.contains("='") && line.ends_with('\'')));
    }

    #[test]
    fn verify_image_checks_presence_digests_and_image_id() {
        let dir = tempdir().unwrap();
        let mut lock = sample_lock();

        let err = verify_image(&lock, dir.path()).unwrap_err();
        assert!(err.contains("missing"));

        let kernel = dir.path().join("vmlinuz-virt");
        let initrd = dir.path().join("initramfs-virt");
        fs::write(&kernel, "kernel").unwrap();
        fs::write(&initrd, "initrd").unwrap();
        let err = verify_image(&lock, dir.path()).unwrap_err();
        assert!(err.contains("kernel") && err.contains("does not match pinned"));

        lock.artifacts.kernel.sha256 = digest_file(&kernel.display().to_string()).unwrap();
        lock.artifacts.initrd.sha256 = digest_file(&initrd.display().to_string()).unwrap();
        let err = verify_image(&lock, dir.path()).unwrap_err();
        assert!(err.contains("expected_image_id"));

        lock.expected_image_id = image_id(
            &lock.artifacts.kernel.sha256,
            None,
            Some(&lock.artifacts.initrd.sha256),
        );
        let manifest = verify_image(&lock, dir.path()).unwrap();
        assert_eq!(manifest.image_id, lock.expected_image_id);
        assert_eq!(manifest.initrd_path.as_deref(), Some(initrd.display().to_string().as_str()));
    }

    #[test]
    fn launch_config_defaults_cover_initrd_and_rootfs_boots() {
        let initrd = default_launch_config("vmlinuz", None, Some("initramfs"), "console.log").unwrap();
        assert_eq!(initrd.kernel_cmdline, DEFAULT_KERNEL_CMDLINE);
        assert_eq!(initrd.network_mode, "nat");
        assert!(initrd.rootfs_path.is_none());

        let rootfs = default_launch_config("vmlinuz", Some("rootfs.ext4"), None, "console.log").unwrap();
        assert_eq!(rootfs.kernel_cmdline, "console=hvc0 root=/dev/vda rw");

        let err = default_launch_config("vmlinuz", None, None, "console.log").unwrap_err();
        assert!(err.contains("rootfs path or an initrd path"));
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
