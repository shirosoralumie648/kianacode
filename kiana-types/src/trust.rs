use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTrust {
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTrustRecord {
    pub trusted: bool,
}

impl ProjectTrust {
    pub fn allows_project_resources(self) -> bool {
        matches!(self, ProjectTrust::Trusted)
    }

    pub fn as_bool(self) -> bool {
        matches!(self, ProjectTrust::Trusted)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProjectTrust::Trusted => "trusted",
            ProjectTrust::Untrusted => "untrusted",
        }
    }
}

pub fn project_trust_from_app_state(app_state: &HashMap<String, Value>) -> ProjectTrust {
    if let Some(trusted) = app_state_bool(app_state, "project_trusted") {
        return if trusted {
            ProjectTrust::Trusted
        } else {
            ProjectTrust::Untrusted
        };
    }
    if let Some(trusted) = app_state_bool(app_state, "projectTrusted") {
        return if trusted {
            ProjectTrust::Trusted
        } else {
            ProjectTrust::Untrusted
        };
    }
    if let Some(trusted) = nested_bool(app_state, "trust", "project") {
        return if trusted {
            ProjectTrust::Trusted
        } else {
            ProjectTrust::Untrusted
        };
    }
    if let Some(trusted) = nested_bool(app_state, "project", "trusted") {
        return if trusted {
            ProjectTrust::Trusted
        } else {
            ProjectTrust::Untrusted
        };
    }
    if let Some(cwd) = app_state_path(app_state, "cwd")
        .or_else(|| app_state_path(app_state, "project_root"))
        .or_else(|| app_state_path(app_state, "projectRoot"))
    {
        if let Ok(Some(project_trust)) = read_project_trust(&cwd) {
            return project_trust;
        }
    }

    ProjectTrust::Trusted
}

pub fn has_explicit_project_trust(app_state: &HashMap<String, Value>) -> bool {
    has_app_state_project_trust(app_state)
        || app_state_path(app_state, "cwd")
            .or_else(|| app_state_path(app_state, "project_root"))
            .or_else(|| app_state_path(app_state, "projectRoot"))
            .and_then(|cwd| read_project_trust(&cwd).ok().flatten())
            .is_some()
}

pub fn project_trust_file_path(cwd: impl AsRef<Path>) -> PathBuf {
    cwd.as_ref().join(".kiana").join("trust.json")
}

pub fn find_project_trust_file(cwd: impl AsRef<Path>) -> Option<PathBuf> {
    for ancestor in cwd.as_ref().ancestors() {
        let candidate = project_trust_file_path(ancestor);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn read_project_trust(cwd: impl AsRef<Path>) -> Result<Option<ProjectTrust>, String> {
    let Some(path) = find_project_trust_file(cwd) else {
        return Ok(None);
    };
    let contents = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    let record = serde_json::from_str::<ProjectTrustRecord>(&contents)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))?;
    Ok(Some(if record.trusted {
        ProjectTrust::Trusted
    } else {
        ProjectTrust::Untrusted
    }))
}

pub fn write_project_trust(
    cwd: impl AsRef<Path>,
    project_trust: ProjectTrust,
) -> Result<PathBuf, String> {
    let path =
        find_project_trust_file(cwd.as_ref()).unwrap_or_else(|| project_trust_file_path(cwd));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {}", parent.display(), error))?;
    }
    let record = ProjectTrustRecord {
        trusted: project_trust.as_bool(),
    };
    let contents = serde_json::to_string_pretty(&record)
        .map_err(|error| format!("failed to serialize project trust: {error}"))?;
    std::fs::write(&path, format!("{contents}\n"))
        .map_err(|error| format!("failed to write {}: {}", path.display(), error))?;
    Ok(path)
}

pub fn remove_project_trust(cwd: impl AsRef<Path>) -> Result<Option<PathBuf>, String> {
    let Some(path) = find_project_trust_file(cwd) else {
        return Ok(None);
    };
    std::fs::remove_file(&path)
        .map_err(|error| format!("failed to remove {}: {}", path.display(), error))?;
    Ok(Some(path))
}

fn app_state_bool(app_state: &HashMap<String, Value>, key: &str) -> Option<bool> {
    match app_state.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::String(value)) => parse_boolish(value),
        _ => None,
    }
}

fn has_app_state_project_trust(app_state: &HashMap<String, Value>) -> bool {
    app_state.contains_key("project_trusted")
        || app_state.contains_key("projectTrusted")
        || app_state
            .get("trust")
            .and_then(Value::as_object)
            .is_some_and(|trust| trust.contains_key("project"))
        || app_state
            .get("project")
            .and_then(Value::as_object)
            .is_some_and(|project| project.contains_key("trusted"))
}

fn app_state_path(app_state: &HashMap<String, Value>, key: &str) -> Option<PathBuf> {
    app_state
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn nested_bool(app_state: &HashMap<String, Value>, outer: &str, inner: &str) -> Option<bool> {
    let value = app_state.get(outer)?;
    let nested = value.get(inner)?;
    match nested {
        Value::Bool(value) => Some(*value),
        Value::String(value) => parse_boolish(value),
        _ => None,
    }
}

fn parse_boolish(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "trusted" | "yes" | "on" | "1" => Some(true),
        "false" | "untrusted" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;

    #[test]
    fn app_state_uses_persisted_project_trust_from_cwd_when_no_explicit_state_exists() {
        let root = std::env::temp_dir().join(format!(
            "kiana-project-trust-types-{}",
            uuid::Uuid::new_v4()
        ));
        let project = root.join("project");
        fs::create_dir_all(project.join(".kiana")).unwrap();
        fs::write(
            project.join(".kiana").join("trust.json"),
            serde_json::to_string_pretty(&json!({ "trusted": false })).unwrap(),
        )
        .unwrap();
        let app_state = HashMap::from([("cwd".to_string(), json!(project))]);

        assert_eq!(
            project_trust_from_app_state(&app_state),
            ProjectTrust::Untrusted
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_project_trust_requires_app_state_or_file_source() {
        let root = std::env::temp_dir().join(format!(
            "kiana-project-trust-explicit-types-{}",
            uuid::Uuid::new_v4()
        ));
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();

        let default_state = HashMap::from([("cwd".to_string(), json!(project.clone()))]);
        assert_eq!(
            project_trust_from_app_state(&default_state),
            ProjectTrust::Trusted
        );
        assert!(!has_explicit_project_trust(&default_state));

        let explicit_state = HashMap::from([
            ("cwd".to_string(), json!(project.clone())),
            ("project_trusted".to_string(), json!(true)),
        ]);
        assert!(has_explicit_project_trust(&explicit_state));

        fs::create_dir_all(project.join(".kiana")).unwrap();
        fs::write(
            project.join(".kiana").join("trust.json"),
            serde_json::to_string_pretty(&json!({ "trusted": true })).unwrap(),
        )
        .unwrap();
        assert!(has_explicit_project_trust(&default_state));

        let _ = fs::remove_dir_all(root);
    }
}
