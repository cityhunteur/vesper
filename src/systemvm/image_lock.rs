//! Pinned System VM image lock (`docs/specs/systemvm-image.lock.yaml`): typed parsing, validation,
//! shell export, verification of fetched artifacts, and the launch config derived from it.

use super::{
    artifact_path, build_manifest, check_digest, default_launch_config, is_sha256_hex,
    ImageBuildConfig, ImageManifest, LaunchConfig, LaunchInputs, IMAGE_LOCK_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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
    /// The source file itself when it is attached to the VM read-only (for example an ISO).
    #[serde(default)]
    pub boot_media: Option<BootMedia>,
    pub boot: BootSettings,
    pub expected_image_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootMedia {
    /// File name under the image directory; the fetch script copies the source there.
    pub file: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootSettings {
    pub kernel_cmdline: String,
    pub ready_pattern: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnpackMethod {
    /// Use the archive member as is.
    None,
    /// The member is a Linux EFI zboot PE (`MZ..zimg`) wrapping a gzip payload; unpack to the raw Image.
    ZbootGzip,
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
    /// Digest of the member as extracted, before unpacking.
    pub member_sha256: String,
    pub unpack: UnpackMethod,
    /// File name under the image directory after unpacking.
    pub file: String,
    /// Digest of `file`.
    pub sha256: String,
    pub format: String,
}

impl ImageArtifact {
    /// File name of the artifact inside the image directory.
    pub fn file_name(&self) -> Result<&str, String> {
        let name = self.file.trim();
        if name.is_empty() || name.contains('/') {
            return Err(format!("artifact file {:?} must be a bare file name", self.file));
        }
        Ok(name)
    }
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
        ("boot.kernel_cmdline", &lock.boot.kernel_cmdline),
        ("boot.ready_pattern", &lock.boot.ready_pattern),
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
    let mut digests = vec![
        ("source.sha256", &lock.source.sha256),
        ("artifacts.kernel.member_sha256", &lock.artifacts.kernel.member_sha256),
        ("artifacts.kernel.sha256", &lock.artifacts.kernel.sha256),
        ("artifacts.initrd.member_sha256", &lock.artifacts.initrd.member_sha256),
        ("artifacts.initrd.sha256", &lock.artifacts.initrd.sha256),
        ("expected_image_id", &lock.expected_image_id),
    ];
    if let Some(media) = &lock.boot_media {
        digests.push(("boot_media.sha256", &media.sha256));
    }
    for (name, value) in digests {
        if !is_sha256_hex(value) {
            return Err(format!("image lock field {name} must be 64 lowercase hex characters"));
        }
    }
    for artifact in [&lock.artifacts.kernel, &lock.artifacts.initrd] {
        if artifact.unpack == UnpackMethod::None && artifact.member_sha256 != artifact.sha256 {
            return Err(format!(
                "artifact {} has unpack none but member_sha256 differs from sha256",
                artifact.member
            ));
        }
    }
    let mut names = vec![lock.artifacts.kernel.file_name()?, lock.artifacts.initrd.file_name()?];
    if let Some(media) = &lock.boot_media {
        let name = media.file.trim();
        if name.is_empty() || name.contains('/') {
            return Err("boot_media.file must be a bare file name".to_string());
        }
        if media.sha256 != lock.source.sha256 {
            return Err("boot_media.sha256 must equal source.sha256 (the source file is attached as is)".to_string());
        }
        names.push(name);
    }
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() != names.len() {
        return Err("image lock artifact and boot media file names must be distinct".to_string());
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
        ("VESPER_IMAGE_KERNEL_MEMBER_SHA256", lock.artifacts.kernel.member_sha256.as_str()),
        ("VESPER_IMAGE_KERNEL_UNPACK", unpack_name(lock.artifacts.kernel.unpack)),
        ("VESPER_IMAGE_KERNEL_FILE", lock.artifacts.kernel.file.as_str()),
        ("VESPER_IMAGE_KERNEL_SHA256", lock.artifacts.kernel.sha256.as_str()),
        ("VESPER_IMAGE_INITRD_MEMBER", lock.artifacts.initrd.member.as_str()),
        ("VESPER_IMAGE_INITRD_MEMBER_SHA256", lock.artifacts.initrd.member_sha256.as_str()),
        ("VESPER_IMAGE_INITRD_UNPACK", unpack_name(lock.artifacts.initrd.unpack)),
        ("VESPER_IMAGE_INITRD_FILE", lock.artifacts.initrd.file.as_str()),
        ("VESPER_IMAGE_INITRD_SHA256", lock.artifacts.initrd.sha256.as_str()),
        (
            "VESPER_IMAGE_BOOT_MEDIA_FILE",
            lock.boot_media.as_ref().map(|media| media.file.as_str()).unwrap_or(""),
        ),
        ("VESPER_IMAGE_KERNEL_CMDLINE", lock.boot.kernel_cmdline.as_str()),
        ("VESPER_IMAGE_READY_PATTERN", lock.boot.ready_pattern.as_str()),
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

fn unpack_name(method: UnpackMethod) -> &'static str {
    match method {
        UnpackMethod::None => "none",
        UnpackMethod::ZbootGzip => "zboot-gzip",
    }
}

/// Checks that `dir` holds the kernel, initrd and boot media named by the lock with the pinned
/// digests, and returns the manifest for them. The manifest `image_id` must equal
/// `lock.expected_image_id`.
pub fn verify_image(lock: &ImageLock, dir: impl AsRef<Path>) -> Result<ImageManifest, String> {
    let dir = dir.as_ref();
    let kernel = artifact_path(dir, lock.artifacts.kernel.file_name()?)?;
    let initrd = artifact_path(dir, lock.artifacts.initrd.file_name()?)?;
    check_digest("kernel", &kernel, &lock.artifacts.kernel.sha256)?;
    check_digest("initrd", &initrd, &lock.artifacts.initrd.sha256)?;
    let boot_media = match &lock.boot_media {
        Some(media) => {
            let path = artifact_path(dir, media.file.trim())?;
            check_digest("boot media", &path, &media.sha256)?;
            Some(path.display().to_string())
        }
        None => None,
    };

    let manifest = build_manifest(&ImageBuildConfig {
        kernel_path: kernel.display().to_string(),
        rootfs_path: None,
        initrd_path: Some(initrd.display().to_string()),
        boot_media_path: boot_media,
    })?;
    if manifest.image_id != lock.expected_image_id {
        return Err(format!(
            "image_id {} does not match expected_image_id {} in the lock",
            manifest.image_id, lock.expected_image_id
        ));
    }
    Ok(manifest)
}

/// Launch config for the pinned image in `dir`: kernel, initrd and boot media from the lock, the
/// lock's kernel command line and ready pattern, NAT networking.
pub fn launch_config_from_lock(
    lock: &ImageLock,
    dir: impl AsRef<Path>,
    console_log_path: &str,
) -> Result<LaunchConfig, String> {
    let dir = dir.as_ref();
    let kernel = dir.join(lock.artifacts.kernel.file_name()?).display().to_string();
    let initrd = dir.join(lock.artifacts.initrd.file_name()?).display().to_string();
    let boot_media = lock
        .boot_media
        .as_ref()
        .map(|media| dir.join(media.file.trim()).display().to_string());
    default_launch_config(&LaunchInputs {
        kernel_path: &kernel,
        rootfs_path: None,
        initrd_path: Some(&initrd),
        boot_media_path: boot_media.as_deref(),
        kernel_cmdline: Some(&lock.boot.kernel_cmdline),
        ready_pattern: Some(&lock.boot.ready_pattern),
        console_log_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{digest_file, image_id, DEFAULT_READY_PATTERN};
    use tempfile::tempdir;

    const REPO_LOCK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/specs/systemvm-image.lock.yaml");

    fn sample_lock() -> ImageLock {
        load_image_lock(REPO_LOCK).unwrap()
    }

    #[test]
    fn validates_repo_image_lock() {
        let lock = sample_lock();
        assert_eq!(lock.schema_version, IMAGE_LOCK_SCHEMA_VERSION);
        assert_eq!(lock.arch, "aarch64");
        assert_eq!(lock.artifacts.kernel.unpack, UnpackMethod::ZbootGzip);
        assert_eq!(lock.artifacts.kernel.file_name().unwrap(), "Image-virt");
        assert_eq!(lock.artifacts.initrd.unpack, UnpackMethod::None);
        assert_eq!(lock.artifacts.initrd.file_name().unwrap(), "initramfs-virt");
        let media = lock.boot_media.as_ref().expect("the ISO is the boot media");
        assert!(media.file.ends_with(".iso"));
        assert!(lock.boot.kernel_cmdline.contains("console=hvc0"));
        assert_eq!(lock.boot.ready_pattern, DEFAULT_READY_PATTERN);
        assert_eq!(
            lock.expected_image_id,
            image_id(
                &lock.artifacts.kernel.sha256,
                None,
                Some(&lock.artifacts.initrd.sha256),
                Some(&media.sha256),
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

        let mut lock = sample_lock();
        lock.artifacts.initrd.member_sha256 = lock.artifacts.kernel.sha256.clone();
        assert!(validate_image_lock(&lock).unwrap_err().contains("unpack none"));

        let mut lock = sample_lock();
        lock.boot_media.as_mut().unwrap().sha256 = lock.artifacts.kernel.sha256.clone();
        assert!(validate_image_lock(&lock).unwrap_err().contains("source.sha256"));

        let mut lock = sample_lock();
        lock.artifacts.kernel.file = "initramfs-virt".to_string();
        assert!(validate_image_lock(&lock).unwrap_err().contains("distinct"));

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
            "VESPER_IMAGE_KERNEL_UNPACK='zboot-gzip'",
            "VESPER_IMAGE_KERNEL_FILE='Image-virt'",
            "VESPER_IMAGE_INITRD_MEMBER='boot/initramfs-virt'",
            "VESPER_IMAGE_INITRD_UNPACK='none'",
            "VESPER_IMAGE_BOOT_MEDIA_FILE='alpine-virt-",
            "VESPER_IMAGE_KERNEL_CMDLINE='console=hvc0",
            "VESPER_IMAGE_READY_PATTERN='login:'",
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

        let kernel = dir.path().join("Image-virt");
        let initrd = dir.path().join("initramfs-virt");
        let media = dir.path().join(lock.boot_media.as_ref().unwrap().file.clone());
        fs::write(&kernel, "kernel").unwrap();
        fs::write(&initrd, "initrd").unwrap();
        fs::write(&media, "iso").unwrap();
        let err = verify_image(&lock, dir.path()).unwrap_err();
        assert!(err.contains("kernel") && err.contains("does not match pinned"));

        lock.artifacts.kernel.sha256 = digest_file(&kernel.display().to_string()).unwrap();
        lock.artifacts.initrd.sha256 = digest_file(&initrd.display().to_string()).unwrap();
        lock.artifacts.initrd.member_sha256 = lock.artifacts.initrd.sha256.clone();
        let err = verify_image(&lock, dir.path()).unwrap_err();
        assert!(err.contains("boot media") && err.contains("does not match pinned"));

        let media_sha = digest_file(&media.display().to_string()).unwrap();
        lock.boot_media.as_mut().unwrap().sha256 = media_sha.clone();
        lock.source.sha256 = media_sha.clone();
        let err = verify_image(&lock, dir.path()).unwrap_err();
        assert!(err.contains("expected_image_id"));

        lock.expected_image_id = image_id(
            &lock.artifacts.kernel.sha256,
            None,
            Some(&lock.artifacts.initrd.sha256),
            Some(&media_sha),
        );
        let manifest = verify_image(&lock, dir.path()).unwrap();
        assert_eq!(manifest.image_id, lock.expected_image_id);
        assert_eq!(manifest.initrd_path.as_deref(), Some(initrd.display().to_string().as_str()));
        assert_eq!(manifest.boot_media_sha256.as_deref(), Some(media_sha.as_str()));

        let launch = launch_config_from_lock(&lock, dir.path(), "console.log").unwrap();
        assert_eq!(launch.kernel_path, kernel.display().to_string());
        assert_eq!(launch.boot_media_path.as_deref(), Some(media.display().to_string().as_str()));
        assert_eq!(launch.kernel_cmdline, lock.boot.kernel_cmdline);
        assert_eq!(launch.ready_pattern, lock.boot.ready_pattern);
    }
}
