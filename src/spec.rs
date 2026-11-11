use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Route {
    pub method: String,
    pub path: String,
}

pub fn load_routes(path: impl AsRef<Path>) -> Result<Vec<Route>, String> {
    let yaml = load_swagger_yaml(path.as_ref())?;

    let paths = yaml
        .get("paths")
        .and_then(Value::as_mapping)
        .ok_or_else(|| "swagger spec has no paths section".to_string())?;

    let mut routes = Vec::new();
    for (path_key, operations_value) in paths {
        let path_str = match path_key.as_str() {
            Some(value) => value,
            None => continue,
        };
        let operations = match operations_value.as_mapping() {
            Some(value) => value,
            None => continue,
        };

        for (method_key, _op_value) in operations {
            let method = match method_key.as_str() {
                Some(value) => value.to_ascii_uppercase(),
                None => continue,
            };
            routes.push(Route {
                method,
                path: path_str.to_string(),
            });
        }
    }

    routes.sort_by(|a, b| {
        if a.path == b.path {
            a.method.cmp(&b.method)
        } else {
            a.path.cmp(&b.path)
        }
    });
    routes.dedup();

    if routes.is_empty() {
        return Err("swagger spec did not produce any routes".to_string());
    }

    Ok(routes)
}

pub fn load_info_version(path: impl AsRef<Path>) -> Result<String, String> {
    let yaml = load_swagger_yaml(path.as_ref())?;

    yaml.get("info")
        .and_then(Value::as_mapping)
        .and_then(|info| info.get("version"))
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "swagger spec missing info.version".to_string())
}

fn load_swagger_yaml(path: &Path) -> Result<Value, String> {
    let content = fs::read_to_string(path)
        .map_err(|err| format!("read swagger spec {}: {err}", path.display()))?;
    serde_yaml::from_str(&content).map_err(|err| format!("parse swagger yaml: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_routes_from_pinned_swagger() {
        let routes = load_routes("third_party/firecracker/firecracker.yaml").unwrap();
        assert!(!routes.is_empty());
        assert!(routes.iter().any(|route| route.path == "/actions"));
    }

    #[test]
    fn loads_swagger_info_version() {
        let version = load_info_version("third_party/firecracker/firecracker.yaml").unwrap();
        assert_eq!(version, "1.16.0");
    }
}
