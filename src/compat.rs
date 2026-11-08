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
    /// Who accepted the deviation as a pre-GA exception (a human; see AGENTS.md "Ask a human first").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<String>,
    /// Date of that approval, YYYY-MM-DD. Required when `approved_by` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_on: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub open: usize,
    pub closed: usize,
    /// Open exceptions carrying a human approval (`approved_by` and `approved_on`).
    pub approved: usize,
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
    let mut summary = Summary {
        open: 0,
        closed: 0,
        approved: 0,
    };

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

        match (&exception.approved_by, &exception.approved_on) {
            (None, None) => {}
            (Some(by), Some(on)) => {
                if by.trim().is_empty() {
                    errors.push(format!("{label}: approved_by must not be blank"));
                }
                match NaiveDate::parse_from_str(on.trim(), "%Y-%m-%d") {
                    Ok(approved) if approved > today => errors.push(format!(
                        "{label}: approved_on {} is in the future",
                        approved.format("%Y-%m-%d")
                    )),
                    Ok(_) => {
                        if status == "open" && !by.trim().is_empty() {
                            summary.approved += 1;
                        }
                    }
                    Err(_) => errors.push(format!("{label}: approved_on must be YYYY-MM-DD")),
                }
            }
            _ => errors.push(format!(
                "{label}: approved_by and approved_on must be set together"
            )),
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
                approved_by: None,
                approved_on: None,
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
                approved_by: None,
                approved_on: None,
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

    fn exception(approved_by: Option<&str>, approved_on: Option<&str>) -> Exception {
        Exception {
            id: "EX-3".to_string(),
            owner: "runtime".to_string(),
            issue: 21,
            expiry_date: "2030-01-01".to_string(),
            target_release: "parity-ga".to_string(),
            status: "open".to_string(),
            description: "".to_string(),
            approved_by: approved_by.map(str::to_string),
            approved_on: approved_on.map(str::to_string),
        }
    }

    #[test]
    fn counts_human_approved_open_exceptions() {
        let registry = Registry {
            version: 1,
            exceptions: vec![
                exception(Some("cityhunteur"), Some("2026-11-08")),
                exception(None, None),
            ],
        };
        let summary = validate(
            &registry,
            NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
            false,
        )
        .unwrap();
        assert_eq!(
            summary,
            Summary {
                open: 2,
                closed: 0,
                approved: 1
            }
        );
    }

    #[test]
    fn rejects_half_or_future_approvals() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
        let half = Registry {
            version: 1,
            exceptions: vec![exception(Some("cityhunteur"), None)],
        };
        assert!(
            validate(&half, today, false)
                .unwrap_err()
                .contains("must be set together")
        );
        let blank = Registry {
            version: 1,
            exceptions: vec![exception(Some("  "), Some("2026-11-08"))],
        };
        assert!(
            validate(&blank, today, false)
                .unwrap_err()
                .contains("approved_by must not be blank")
        );
        let future = Registry {
            version: 1,
            exceptions: vec![exception(Some("cityhunteur"), Some("2027-01-01"))],
        };
        assert!(
            validate(&future, today, false)
                .unwrap_err()
                .contains("is in the future")
        );
        let malformed = Registry {
            version: 1,
            exceptions: vec![exception(Some("cityhunteur"), Some("08/11/2026"))],
        };
        assert!(
            validate(&malformed, today, false)
                .unwrap_err()
                .contains("approved_on must be YYYY-MM-DD")
        );
    }
}
