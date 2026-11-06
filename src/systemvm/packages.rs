//! Pinned Alpine packages installed into the System VM at boot
//! (`docs/specs/systemvm-packages.lock.yaml`): typed parsing, validation and verification.

use super::{check_digest, is_sha256_hex};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const PACKAGES_LOCK_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackagesLock {
    pub schema_version: u32,
    pub branch: String,
    pub arch: String,
    /// The top-level packages that were requested; the closure is `packages`.
    pub requested: Vec<String>,
    pub packages: Vec<Package>,
    /// Binary archives (not apks) whose members are placed into the overlay at `guest_path`.
    #[serde(default)]
    pub archives: Vec<Archive>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub members: Vec<ArchiveMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveMember {
    /// Path inside the archive.
    pub member: String,
    /// Absolute path inside the guest.
    pub guest_path: String,
    pub sha256: String,
    /// Octal file mode as a string, for example "0755".
    pub mode: String,
}

impl ArchiveMember {
    /// File name under `cache/archives/<archive>/` after extraction.
    pub fn file_name(&self) -> Result<&str, String> {
        Path::new(&self.member)
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| format!("archive member {:?} has no file name", self.member))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
}

pub fn load_packages_lock(path: impl AsRef<Path>) -> Result<PackagesLock, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|err| format!("read packages lock {}: {err}", path.display()))?;
    let lock: PackagesLock = serde_yaml::from_str(&content)
        .map_err(|err| format!("parse packages lock {}: {err}", path.display()))?;
    validate_packages_lock(&lock)?;
    Ok(lock)
}

pub fn validate_packages_lock(lock: &PackagesLock) -> Result<(), String> {
    if lock.schema_version != PACKAGES_LOCK_SCHEMA_VERSION {
        return Err(format!(
            "packages lock schema_version {} is not supported (expected {PACKAGES_LOCK_SCHEMA_VERSION})",
            lock.schema_version
        ));
    }
    if lock.branch.trim().is_empty() || lock.arch.trim().is_empty() {
        return Err("packages lock branch and arch must not be blank".to_string());
    }
    if lock.requested.is_empty() || lock.packages.is_empty() {
        return Err("packages lock must request and pin at least one package".to_string());
    }
    let names: Vec<&str> = lock.packages.iter().map(|p| p.name.as_str()).collect();
    for requested in &lock.requested {
        if !names.contains(&requested.as_str()) {
            return Err(format!("requested package {requested} is not in the pinned closure"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for (index, package) in lock.packages.iter().enumerate() {
        if index > 0 && lock.packages[index - 1].name >= package.name {
            return Err("packages must be sorted by name with no duplicates".to_string());
        }
        if !seen.insert(package.file.as_str()) {
            return Err(format!("package file {} is listed twice", package.file));
        }
        if package.file != format!("{}-{}.apk", package.name, package.version) {
            return Err(format!("package {} file name must be <name>-<version>.apk", package.name));
        }
        if !package.url.starts_with("https://") || !package.url.ends_with(&package.file) {
            return Err(format!("package {} url must be https and end with its file name", package.name));
        }
        if !is_sha256_hex(&package.sha256) {
            return Err(format!("package {} sha256 must be 64 lowercase hex characters", package.name));
        }
        if package.size_bytes == 0 || package.repo.trim().is_empty() {
            return Err(format!("package {} needs a non-zero size and a repo", package.name));
        }
    }
    let mut names = std::collections::HashSet::new();
    for archive in &lock.archives {
        if archive.name.trim().is_empty() || !names.insert(archive.name.as_str()) {
            return Err("archive names must be non-blank and unique".to_string());
        }
        if !archive.url.starts_with("https://") || !is_sha256_hex(&archive.sha256) || archive.size_bytes == 0 {
            return Err(format!("archive {} needs an https url, a sha256 and a size", archive.name));
        }
        if archive.members.is_empty() {
            return Err(format!("archive {} has no members", archive.name));
        }
        for member in &archive.members {
            member.file_name()?;
            if !member.guest_path.starts_with('/') || member.guest_path.ends_with('/') {
                return Err(format!("archive member {} guest_path must be an absolute file path", member.member));
            }
            if !is_sha256_hex(&member.sha256) {
                return Err(format!("archive member {} sha256 must be 64 lowercase hex characters", member.member));
            }
            if u32::from_str_radix(&member.mode, 8).is_err() {
                return Err(format!("archive member {} mode must be octal", member.member));
            }
        }
    }
    Ok(())
}

/// `name<TAB>url<TAB>sha256<TAB>member<TAB>file<TAB>member_sha256<TAB>guest_path<TAB>mode` per member,
/// for `scripts/systemvm/fetch-packages.sh` and `build-overlay.sh`.
pub fn archive_list(lock: &PackagesLock) -> Result<String, String> {
    let mut out = String::new();
    for archive in &lock.archives {
        for member in &archive.members {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                archive.name, archive.url, archive.sha256, member.member, member.file_name()?, member.sha256, member.guest_path, member.mode
            ));
        }
    }
    Ok(out)
}

/// Checks that every extracted archive member exists under `dir/<archive>/` with its pinned digest.
pub fn verify_archives(lock: &PackagesLock, dir: impl AsRef<Path>) -> Result<usize, String> {
    let dir = dir.as_ref();
    let mut count = 0;
    for archive in &lock.archives {
        for member in &archive.members {
            let path = dir.join(&archive.name).join(member.file_name()?);
            if !path.is_file() {
                return Err(format!("archive member {} is missing; run `make systemvm-overlay`", path.display()));
            }
            check_digest(&format!("archive {} member {}", archive.name, member.member), &path, &member.sha256)?;
            count += 1;
        }
    }
    Ok(count)
}

/// `file<TAB>url<TAB>sha256` per package, for `scripts/systemvm/fetch-packages.sh`.
pub fn packages_list(lock: &PackagesLock) -> String {
    lock.packages
        .iter()
        .map(|p| format!("{}\t{}\t{}\n", p.file, p.url, p.sha256))
        .collect()
}

/// Checks that every pinned package file exists in `dir` with its pinned digest.
pub fn verify_packages(lock: &PackagesLock, dir: impl AsRef<Path>) -> Result<usize, String> {
    let dir = dir.as_ref();
    for package in &lock.packages {
        let path = dir.join(&package.file);
        if !path.is_file() {
            return Err(format!(
                "package {} is missing; run `make systemvm-overlay`",
                path.display()
            ));
        }
        check_digest(&format!("package {}", package.name), &path, &package.sha256)?;
    }
    Ok(lock.packages.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const REPO_LOCK: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/docs/specs/systemvm-packages.lock.yaml");

    #[test]
    fn validates_repo_packages_lock() {
        let lock = load_packages_lock(REPO_LOCK).unwrap();
        assert_eq!(lock.arch, "aarch64");
        for name in ["socat", "containerd", "docker-engine", "docker-cli", "runc", "e2fsprogs"] {
            assert!(lock.packages.iter().any(|p| p.name == name), "missing {name}");
        }
        let fc = lock.archives.iter().find(|a| a.name == "firecracker").expect("firecracker archive");
        assert_eq!(fc.version, "v1.16.0");
        assert!(fc.members.iter().any(|m| m.guest_path == "/var/lib/vesper/firecracker/jailer"));
        let list = archive_list(&lock).unwrap();
        assert_eq!(list.lines().count(), 2);
        assert!(list.contains("\tfirecracker-v1.16.0-aarch64\t"));
        let list = packages_list(&lock);
        assert_eq!(list.lines().count(), lock.packages.len());
        assert!(list.contains("\thttps://dl-cdn.alpinelinux.org/"));
    }

    #[test]
    fn packages_lock_rejects_unsorted_or_mismatched_entries() {
        let mut lock = load_packages_lock(REPO_LOCK).unwrap();
        lock.packages.swap(0, 1);
        assert!(validate_packages_lock(&lock).unwrap_err().contains("sorted"));

        let mut lock = load_packages_lock(REPO_LOCK).unwrap();
        lock.packages[0].file = "wrong.apk".to_string();
        assert!(validate_packages_lock(&lock).unwrap_err().contains("<name>-<version>.apk"));

        let mut lock = load_packages_lock(REPO_LOCK).unwrap();
        lock.requested.push("not-pinned".to_string());
        assert!(validate_packages_lock(&lock).unwrap_err().contains("not in the pinned closure"));

        let mut lock = load_packages_lock(REPO_LOCK).unwrap();
        lock.archives[0].members[0].guest_path = "relative/path".to_string();
        assert!(validate_packages_lock(&lock).unwrap_err().contains("absolute file path"));
        let mut lock = load_packages_lock(REPO_LOCK).unwrap();
        lock.archives[0].members[0].mode = "rwx".to_string();
        assert!(validate_packages_lock(&lock).unwrap_err().contains("octal"));
    }

    #[test]
    fn verify_packages_checks_presence_and_digest() {
        let dir = tempdir().unwrap();
        let mut lock = load_packages_lock(REPO_LOCK).unwrap();
        lock.packages.truncate(1);
        lock.requested = vec![lock.packages[0].name.clone()];
        assert!(verify_packages(&lock, dir.path()).unwrap_err().contains("missing"));
        let path = dir.path().join(&lock.packages[0].file);
        fs::write(&path, b"not the package").unwrap();
        assert!(verify_packages(&lock, dir.path()).unwrap_err().contains("does not match pinned"));
        lock.packages[0].sha256 = super::super::digest_file(&path.display().to_string()).unwrap();
        assert_eq!(verify_packages(&lock, dir.path()).unwrap(), 1);

        let adir = tempdir().unwrap();
        assert!(verify_archives(&lock, adir.path()).unwrap_err().contains("missing"));
        let member_dir = adir.path().join("firecracker");
        fs::create_dir_all(&member_dir).unwrap();
        for m in &lock.archives[0].members {
            fs::write(member_dir.join(m.file_name().unwrap()), m.member.as_bytes()).unwrap();
        }
        assert!(verify_archives(&lock, adir.path()).unwrap_err().contains("does not match pinned"));
        for m in lock.archives[0].members.iter_mut() {
            m.sha256 = super::super::digest_file(&member_dir.join(m.file_name().unwrap()).display().to_string()).unwrap();
        }
        assert_eq!(verify_archives(&lock, adir.path()).unwrap(), 2);
    }
}
