//! Validation of the API behaviour matrix and of its capture registry (moved from `api_behavior_matrix.rs`, TD-023).

use super::*;

pub fn validate_api_behavior_matrix(
    matrix: &ApiBehaviorMatrix,
    routes: &[RouteSpec],
    firecracker_revision: &str,
    firecracker_version: &str,
) -> Result<(), String> {
    if matrix.version != 1 {
        return Err(format!(
            "API behavior matrix version must be 1, found {}",
            matrix.version
        ));
    }
    if matrix.firecracker_revision.trim() != firecracker_revision.trim() {
        return Err(format!(
            "API behavior matrix firecracker revision mismatch: expected {}, found {}",
            firecracker_revision.trim(),
            matrix.firecracker_revision
        ));
    }
    if matrix.firecracker_version.trim() != firecracker_version.trim() {
        return Err(format!(
            "API behavior matrix firecracker version mismatch: expected {}, found {}",
            firecracker_version.trim(),
            matrix.firecracker_version
        ));
    }
    if matrix.notes.is_empty() {
        return Err("API behavior matrix notes must not be empty".to_string());
    }
    if matrix.source.captures.trim().is_empty() {
        return Err("API behavior matrix capture registry source must not be empty".to_string());
    }
    if matrix.operations.is_empty() {
        return Err("API behavior matrix operations must not be empty".to_string());
    }

    let expected = routes
        .iter()
        .map(|route| operation_key(&route.method, &route.path))
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();

    for operation in &matrix.operations {
        let key = operation_key(&operation.method, &operation.path);
        let Some(semantics) = documented_route_semantics(&key) else {
            return Err(format!(
                "API behavior matrix operation {key} is missing documented semantics"
            ));
        };
        if !actual.insert(key.clone()) {
            return Err(format!("API behavior matrix operation {key} is duplicated"));
        }
        if operation.semantics != semantics {
            return Err(format!(
                "API behavior matrix semantics mismatch for {key}: expected {:?}, found {:?}",
                semantics, operation.semantics
            ));
        }
        if operation.behavior_path != behavior_path_for(&key) {
            return Err(format!(
                "API behavior matrix behavior path mismatch for {key}: expected {:?}, found {:?}",
                behavior_path_for(&key),
                operation.behavior_path
            ));
        }
        validate_behavior_entry(operation, &key)?;
    }

    if expected != actual {
        let missing = expected
            .difference(&actual)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let extra = actual
            .difference(&expected)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "API behavior matrix route coverage mismatch (missing: [{}], extra: [{}])",
            missing, extra
        ));
    }

    Ok(())
}

pub fn validate_api_behavior_capture_registry(
    registry: &ApiBehaviorCaptureRegistry,
) -> Result<(), String> {
    if registry.version == 0 {
        return Err("API behavior capture registry version must be set".to_string());
    }

    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();

    for capture in &registry.captures {
        let key = operation_key(&capture.method, &capture.path);
        if capture.method.trim().is_empty() {
            errors.push(format!(
                "API behavior capture registry operation {key} method is required"
            ));
        }
        if capture.path.trim().is_empty() {
            errors.push(format!(
                "API behavior capture registry operation {key} path is required"
            ));
        }
        if !seen.insert(key.clone()) {
            errors.push(format!(
                "API behavior capture registry operation {key} is duplicated"
            ));
        }
        if capture.note.trim().is_empty() {
            errors.push(format!(
                "API behavior capture registry operation {key} note is required"
            ));
        }
        if capture.cases.is_empty() {
            errors.push(format!(
                "API behavior capture registry operation {key} must include at least one case"
            ));
        }

        let mut case_ids = BTreeSet::new();
        for case in &capture.cases {
            let case_label = format!(
                "API behavior capture registry operation {key} case {}",
                case.id
            );
            if case.id.trim().is_empty() {
                errors.push(format!("{case_label}: id is required"));
            }
            if !case_ids.insert(case.id.clone()) {
                errors.push(format!("{case_label}: duplicate case id"));
            }
            if let Err(err) = validate_behavior_case(case, &case_label) {
                errors.push(err);
            }
        }
    }

    if !errors.is_empty() {
        errors.sort();
        return Err(errors.join("; "));
    }

    Ok(())
}

fn validate_behavior_entry(entry: &ApiBehaviorMatrixEntry, operation: &str) -> Result<(), String> {
    match entry.capture_state {
        BehaviorCaptureState::PendingBaselineCapture => {
            if !entry.cases.is_empty() {
                return Err(format!(
                    "API behavior matrix operation {operation} must not include cases while capture is pending"
                ));
            }
            if !entry.capture_note.trim().is_empty() {
                return Err(format!(
                    "API behavior matrix operation {operation} must not include a capture note while capture is pending"
                ));
            }
        }
        BehaviorCaptureState::CapturedInRepo => {
            if entry.cases.is_empty() {
                return Err(format!(
                    "API behavior matrix operation {operation} must include at least one case when capture is captured_in_repo"
                ));
            }
            if entry.capture_note.trim().is_empty() {
                return Err(format!(
                    "API behavior matrix operation {operation} must include a capture note when capture is captured_in_repo"
                ));
            }

            let mut case_ids = BTreeSet::new();
            for case in &entry.cases {
                let case_label =
                    format!("API behavior matrix operation {operation} case {}", case.id);
                if !case_ids.insert(case.id.clone()) {
                    return Err(format!("{case_label}: duplicate case id"));
                }
                validate_behavior_case(case, &case_label)?;
            }
        }
    }

    Ok(())
}

pub(super) fn validate_behavior_case(case: &BehaviorCase, label: &str) -> Result<(), String> {
    if case.id.trim().is_empty() {
        return Err(format!("{label}: id is required"));
    }
    if case.when.trim().is_empty() {
        return Err(format!("{label}: when is required"));
    }
    if !(100..=599).contains(&case.status_code) {
        return Err(format!("{label}: status_code must be between 100 and 599"));
    }
    if case.evidence_files.is_empty() {
        return Err(format!("{label}: at least one evidence file is required"));
    }

    match case.outcome {
        BehaviorOutcome::NoContent => {
            if case.status_code >= 400 {
                return Err(format!(
                    "{label}: no_content outcome cannot use an error status code"
                ));
            }
            if case.code.is_some() || case.message.is_some() || !case.body_assertions.is_empty() {
                return Err(format!(
                    "{label}: no_content outcome must not include error code, message, or body assertions"
                ));
            }
        }
        BehaviorOutcome::SuccessBody => {
            if case.status_code >= 400 {
                return Err(format!(
                    "{label}: success_body outcome cannot use an error status code"
                ));
            }
            if case.code.is_some() || case.message.is_some() {
                return Err(format!(
                    "{label}: success_body outcome must not include error code or message"
                ));
            }
            if case.body_assertions.is_empty() {
                return Err(format!(
                    "{label}: success_body outcome requires at least one body assertion"
                ));
            }
            let mut seen = BTreeSet::new();
            for assertion in &case.body_assertions {
                if !seen.insert(assertion.pointer.clone()) {
                    return Err(format!(
                        "{label}: duplicate body assertion pointer {}",
                        assertion.pointer
                    ));
                }
                validate_body_assertion(assertion, label)?;
            }
        }
        BehaviorOutcome::ErrorEnvelope => {
            if case.status_code < 400 {
                return Err(format!(
                    "{label}: error_envelope outcome requires an error status code"
                ));
            }
            if case.code.as_deref().map(str::trim).unwrap_or("").is_empty() {
                return Err(format!("{label}: error_envelope outcome requires a code"));
            }
            if case
                .message
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                return Err(format!(
                    "{label}: error_envelope outcome requires a message"
                ));
            }
            if !case.body_assertions.is_empty() {
                return Err(format!(
                    "{label}: error_envelope outcome must not include body assertions"
                ));
            }
        }
    }

    let mut seen = BTreeSet::new();
    for evidence in &case.evidence_files {
        if evidence.trim().is_empty() {
            return Err(format!("{label}: evidence file path must not be empty"));
        }
        if !seen.insert(evidence.clone()) {
            return Err(format!("{label}: duplicate evidence file {evidence}"));
        }
        if !Path::new(evidence).exists() {
            return Err(format!("{label}: missing evidence file {evidence}"));
        }
    }

    Ok(())
}

fn validate_body_assertion(assertion: &BehaviorBodyAssertion, label: &str) -> Result<(), String> {
    let pointer = assertion.pointer.trim();
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(format!(
            "{label}: body assertion pointer must start with '/' or be empty for the root document"
        ));
    }

    match assertion.kind {
        BehaviorBodyAssertionKind::Exact => {
            if assertion.value.is_none() {
                return Err(format!("{label}: exact body assertion requires a value"));
            }
            if assertion.value_type.is_some() {
                return Err(format!(
                    "{label}: exact body assertion must not include value_type"
                ));
            }
        }
        BehaviorBodyAssertionKind::Type => {
            if assertion.value.is_some() {
                return Err(format!(
                    "{label}: type body assertion must not include an exact value"
                ));
            }
            if assertion.value_type.is_none() {
                return Err(format!(
                    "{label}: type body assertion requires a value_type"
                ));
            }
        }
        BehaviorBodyAssertionKind::Absent => {
            if assertion.value.is_some() {
                return Err(format!(
                    "{label}: absent body assertion must not include an exact value"
                ));
            }
            if assertion.value_type.is_some() {
                return Err(format!(
                    "{label}: absent body assertion must not include value_type"
                ));
            }
        }
    }

    Ok(())
}
