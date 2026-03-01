use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    #[serde(default)]
    pub exceptions: Vec<Exception>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exception {
    pub id: String,
    pub owner: String,
    pub issue: i64,
    pub expiry_date: String,
    pub target_release: String,
    pub status: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub open: usize,
    pub closed: usize,
}

pub fn load(path: impl AsRef<Path>) -> Result<Registry, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|err| format!("read registry {}: {err}", path.display()))?;
    let registry: Registry = serde_yaml::from_str(&content)
        .map_err(|err| format!("parse registry {}: {err}", path.display()))?;

    if registry.version == 0 {
        return Err("registry version must be set".to_string());
    }

    Ok(registry)
}

pub fn validate(
    registry: &Registry,
    today: NaiveDate,
    require_zero_open: bool,
) -> Result<Summary, String> {
    let mut errors = Vec::new();
    let mut summary = Summary { open: 0, closed: 0 };

    for (index, exception) in registry.exceptions.iter().enumerate() {
        let label = if exception.id.trim().is_empty() {
            format!("entry-{}", index + 1)
        } else {
            exception.id.clone()
        };

        if exception.id.trim().is_empty() {
            errors.push(format!("{label}: id is required"));
        }
        if exception.owner.trim().is_empty() {
            errors.push(format!("{label}: owner is required"));
        }
        if exception.issue <= 0 {
            errors.push(format!("{label}: issue must be a positive number"));
        }
        if exception.target_release.trim().is_empty() {
            errors.push(format!("{label}: target_release is required"));
        }

        let status = exception.status.trim().to_ascii_lowercase();
        match status.as_str() {
            "open" => summary.open += 1,
            "closed" => summary.closed += 1,
            _ => {
                errors.push(format!("{label}: status must be open or closed"));
                continue;
            }
        }

        let expiry = match NaiveDate::parse_from_str(exception.expiry_date.trim(), "%Y-%m-%d") {
            Ok(value) => value,
            Err(_) => {
                errors.push(format!("{label}: expiry_date must be YYYY-MM-DD"));
                continue;
            }
        };

        if status == "open" && expiry < today {
            errors.push(format!(
                "{label}: open exception expired on {}",
                expiry.format("%Y-%m-%d")
            ));
        }
    }

    if require_zero_open && summary.open > 0 {
        errors.push(format!(
            "parity-GA gate failed: {} open compatibility exception(s)",
            summary.open
        ));
    }

    if !errors.is_empty() {
        errors.sort();
        return Err(errors.join("; "));
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_fields() {
        let registry = Registry {
            version: 1,
            exceptions: vec![Exception {
                id: "EX-1".to_string(),
                owner: "".to_string(),
                issue: 0,
                expiry_date: "2030-01-01".to_string(),
                target_release: "".to_string(),
                status: "open".to_string(),
                description: "".to_string(),
            }],
        };

        let err = validate(
            &registry,
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            false,
        )
        .unwrap_err();

        assert!(err.contains("owner is required"));
        assert!(err.contains("issue must be a positive number"));
    }

    #[test]
    fn supports_zero_open_gate() {
        let registry = Registry {
            version: 1,
            exceptions: vec![Exception {
                id: "EX-2".to_string(),
                owner: "runtime".to_string(),
                issue: 9,
                expiry_date: "2030-01-01".to_string(),
                target_release: "v0.3.0".to_string(),
                status: "open".to_string(),
                description: "".to_string(),
            }],
        };

        let err = validate(
            &registry,
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            true,
        )
        .unwrap_err();
        assert!(err.contains("parity-GA gate failed"));
    }
}
