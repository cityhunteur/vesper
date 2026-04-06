use std::fs;
use std::path::Path;

pub fn validate_entitlements_file(
    path: Option<&Path>,
    required_keys: &[&str],
) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };

    let content = fs::read_to_string(path)
        .map_err(|err| format!("read entitlements file {}: {err}", path.display()))?;

    for key in required_keys {
        let needle = format!("<key>{key}</key>");
        if !content.contains(&needle) {
            return Err(format!(
                "missing required entitlement key {key:?} in {}",
                path.display()
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_entitlement_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.entitlements");
        let content = "<?xml version=\"1.0\"?><plist><dict><key>com.apple.security.virtualization</key><true/></dict></plist>";
        std::fs::write(&path, content).unwrap();

        validate_entitlements_file(Some(path.as_path()), &["com.apple.security.virtualization"])
            .unwrap();

        let err = validate_entitlements_file(Some(path.as_path()), &["com.apple.vm.networking"])
            .unwrap_err();
        assert!(err.contains("com.apple.vm.networking"));
    }
}
