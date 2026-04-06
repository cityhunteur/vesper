use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Lock {
    pub version: u32,
    pub updated_at: String,
    pub baselines: BTreeMap<String, BaselineRef>,
    pub reporting: Reporting,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BaselineRef {
    pub repo: String,
    pub revision: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Reporting {
    pub required_metadata: Vec<String>,
}

pub fn load(path: impl AsRef<Path>) -> Result<Lock, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|err| format!("read baseline lock {}: {err}", path.display()))?;
    let lock: Lock = serde_yaml::from_str(&content)
        .map_err(|err| format!("parse baseline lock {}: {err}", path.display()))?;

    if lock.version == 0 {
        return Err("baseline lock version must be set".to_string());
    }
    if lock.baselines.is_empty() {
        return Err("baseline lock baselines must not be empty".to_string());
    }
    if lock.reporting.required_metadata.is_empty() {
        return Err("baseline lock reporting.required_metadata must not be empty".to_string());
    }

    Ok(lock)
}

pub fn build_metadata(
    lock: &Lock,
    backend: &str,
    host_os: &str,
    host_cpu: &str,
) -> Result<BTreeMap<String, String>, String> {
    if backend.trim().is_empty() {
        return Err("candidate backend is required".to_string());
    }
    if host_os.trim().is_empty() {
        return Err("candidate host OS is required".to_string());
    }
    if host_cpu.trim().is_empty() {
        return Err("candidate host CPU is required".to_string());
    }

    let mut metadata = BTreeMap::new();
    metadata.insert("candidate.backend".to_string(), backend.trim().to_string());
    metadata.insert("candidate.host_os".to_string(), host_os.trim().to_string());
    metadata.insert(
        "candidate.host_cpu".to_string(),
        host_cpu.trim().to_string(),
    );

    for (name, reference) in &lock.baselines {
        metadata.insert(
            format!("baseline.{name}.revision"),
            reference.revision.trim().to_string(),
        );
    }

    validate_metadata(lock, &metadata)?;
    Ok(metadata)
}

pub fn validate_metadata(lock: &Lock, metadata: &BTreeMap<String, String>) -> Result<(), String> {
    let mut missing = Vec::new();
    for key in &lock.reporting.required_metadata {
        match metadata.get(key) {
            Some(value) if !value.trim().is_empty() => {}
            _ => missing.push(key.clone()),
        }
    }

    if !missing.is_empty() {
        missing.sort();
        return Err(format!(
            "metadata missing required keys: {}",
            missing.join(", ")
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lock() -> Lock {
        Lock {
            version: 1,
            updated_at: "2026-03-01".to_string(),
            baselines: BTreeMap::from([
                (
                    "firecracker".to_string(),
                    BaselineRef {
                        repo: "firecracker-microvm/firecracker".to_string(),
                        revision: "abc".to_string(),
                    },
                ),
                (
                    "containerd".to_string(),
                    BaselineRef {
                        repo: "containerd/containerd".to_string(),
                        revision: "def".to_string(),
                    },
                ),
            ]),
            reporting: Reporting {
                required_metadata: vec![
                    "baseline.firecracker.revision".to_string(),
                    "baseline.containerd.revision".to_string(),
                    "candidate.backend".to_string(),
                    "candidate.host_os".to_string(),
                    "candidate.host_cpu".to_string(),
                ],
            },
        }
    }

    #[test]
    fn builds_metadata() {
        let lock = sample_lock();
        let metadata = build_metadata(&lock, "backend-a", "darwin", "arm64").unwrap();
        assert_eq!(metadata.get("candidate.backend").unwrap(), "backend-a");
        assert_eq!(
            metadata.get("baseline.firecracker.revision").unwrap(),
            "abc"
        );
    }

    #[test]
    fn detects_missing_keys() {
        let lock = sample_lock();
        let metadata = BTreeMap::from([("candidate.backend".to_string(), "backend-a".to_string())]);
        let err = validate_metadata(&lock, &metadata).unwrap_err();
        assert!(err.contains("baseline.firecracker.revision"));
    }
}
