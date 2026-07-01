use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub result: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<i32>,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self {
            result: true,
            message: None,
            error_code: None,
        }
    }

    pub fn err(message: String, error_code: i32) -> Self {
        Self {
            result: false,
            message: Some(message),
            error_code: Some(error_code),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDecision {
    pub granted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl PermissionDecision {
    pub fn allow() -> Self {
        Self {
            granted: true,
            reason: None,
        }
    }

    pub fn deny(reason: String) -> Self {
        Self {
            granted: false,
            reason: Some(reason),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Validation failed: {0}")]
    ValidationError(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("Tool error: {0}")]
    Other(String),
}

pub type ToolResult<T> = Result<T, ToolError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    pub tool_name: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

pub struct ToolContext {
    pub cwd: String,
    pub read_file_state: HashMap<String, FileState>,
    pub app_state: HashMap<String, Value>,
    pub abort_signal: tokio::sync::watch::Receiver<bool>,
}

pub const ACCESS_ROOTS_ENV: &str = "KIANA_ACCESS_ROOTS";
pub const ACCESS_ROOTS_APP_STATE_KEY: &str = "access_roots";
pub const EDITABLE_FILES_APP_STATE_KEY: &str = "editable_files";
pub const READ_ONLY_FILES_APP_STATE_KEY: &str = "read_only_files";
pub const READONLY_FILES_APP_STATE_KEY: &str = "readonly_files";

impl ToolContext {
    pub fn resolve_path(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            normalize_lexically(&path)
        } else {
            normalize_lexically(Path::new(&self.cwd).join(path))
        }
    }

    pub fn resolve_access_path(&self, path: &str) -> Result<PathBuf, String> {
        let resolved = self.resolve_path(path);
        self.ensure_path_access(&resolved)?;
        Ok(resolved)
    }

    pub fn ensure_path_access(&self, path: &Path) -> Result<(), String> {
        let Some(roots) = self.access_roots() else {
            return Ok(());
        };
        let candidate = canonicalize_existing_prefix(path)
            .map_err(|error| format!("failed to resolve path '{}': {}", path.display(), error))?;
        if roots.iter().any(|root| candidate.starts_with(root)) {
            return Ok(());
        }

        let allowed = roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "Path '{}' is outside the allowed access roots: {}",
            path.display(),
            allowed
        ))
    }

    pub fn ensure_file_editable(&self, path: &Path) -> Result<(), String> {
        let candidate = canonicalize_existing_prefix(path)
            .map_err(|error| format!("failed to resolve path '{}': {}", path.display(), error))?;
        let read_only_files = self.read_only_file_paths();
        if read_only_files.iter().any(|root| candidate == *root) {
            return Err(format!("Path '{}' is marked read-only", path.display()));
        }

        if let Some(editable_files) = self.editable_file_paths() {
            if editable_files.iter().any(|root| candidate == *root) {
                return Ok(());
            }
            return Err(format!(
                "Path '{}' is not in the editable files set: {}",
                path.display(),
                format_path_list(&editable_files)
            ));
        }

        Ok(())
    }

    pub fn access_roots(&self) -> Option<Vec<PathBuf>> {
        let mut raw_roots = Vec::new();
        raw_roots.push(PathBuf::from(&self.cwd));

        if let Some(value) = self.app_state.get(ACCESS_ROOTS_APP_STATE_KEY) {
            raw_roots.extend(paths_from_json_value(value));
        }

        let env_roots = std::env::var_os(ACCESS_ROOTS_ENV)
            .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .unwrap_or_default();
        let explicit_roots = raw_roots.len() > 1 || !env_roots.is_empty();
        raw_roots.extend(env_roots);

        if !explicit_roots {
            return None;
        }

        let mut roots = Vec::new();
        for root in raw_roots {
            let resolved = if root.is_absolute() {
                root
            } else {
                PathBuf::from(&self.cwd).join(root)
            };
            let Ok(canonical) = std::fs::canonicalize(&resolved) else {
                continue;
            };
            if canonical.is_dir() && !roots.iter().any(|existing| existing == &canonical) {
                roots.push(canonical);
            }
        }

        if roots.is_empty() {
            None
        } else {
            Some(roots)
        }
    }

    fn editable_file_paths(&self) -> Option<Vec<PathBuf>> {
        self.app_state
            .get(EDITABLE_FILES_APP_STATE_KEY)
            .map(|value| self.resolve_file_set(value))
    }

    fn read_only_file_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(value) = self.app_state.get(READ_ONLY_FILES_APP_STATE_KEY) {
            paths.extend(self.resolve_file_set(value));
        }
        if let Some(value) = self.app_state.get(READONLY_FILES_APP_STATE_KEY) {
            paths.extend(self.resolve_file_set(value));
        }
        paths
    }

    fn resolve_file_set(&self, value: &Value) -> Vec<PathBuf> {
        paths_from_json_value(value)
            .into_iter()
            .filter_map(|path| {
                let resolved = if path.is_absolute() {
                    path
                } else {
                    PathBuf::from(&self.cwd).join(path)
                };
                canonicalize_existing_prefix(&resolved).ok()
            })
            .collect()
    }
}

fn paths_from_json_value(value: &Value) -> Vec<PathBuf> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .collect(),
        Value::String(path) if !path.trim().is_empty() => vec![PathBuf::from(path.trim())],
        _ => Vec::new(),
    }
}

fn format_path_list(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "(empty)".to_string();
    }
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn canonicalize_existing_prefix(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return std::fs::canonicalize(path);
    }

    let mut missing = Vec::new();
    let mut ancestor = path;
    while let Some(parent) = ancestor.parent() {
        if parent.exists() {
            let mut canonical = std::fs::canonicalize(parent)?;
            for component in missing.iter().rev() {
                canonical.push(component);
            }
            return Ok(normalize_lexically(canonical));
        }
        if let Some(file_name) = ancestor.file_name() {
            missing.push(file_name.to_os_string());
        }
        ancestor = parent;
    }

    Ok(normalize_lexically(path))
}

fn normalize_lexically(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[derive(Debug, Clone)]
pub struct FileState {
    pub content: String,
    pub timestamp: i64,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn search_hint(&self) -> Option<&str> {
        None
    }

    fn workbench(&self) -> Option<&str> {
        None
    }

    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn input_schema(&self) -> Value;
    fn output_schema(&self) -> Value;

    async fn validate_input(&self, _input: &Value, _context: &ToolContext) -> ValidationResult {
        ValidationResult::ok()
    }

    async fn check_permissions(&self, input: &Value, context: &ToolContext) -> PermissionDecision {
        match crate::permissions::permission_check_for_tool(
            self.name(),
            self.is_read_only(),
            input,
            &context.app_state,
        ) {
            crate::permissions::ToolPermissionCheck::Allow => PermissionDecision::allow(),
            crate::permissions::ToolPermissionCheck::Deny(reason) => {
                PermissionDecision::deny(reason)
            }
            crate::permissions::ToolPermissionCheck::Ask(_reason) => {
                match crate::permissions::prompt_for_tool_permission(self.name(), input) {
                    Ok(true) => PermissionDecision::allow(),
                    Ok(false) => PermissionDecision::deny(format!(
                        "Tool {} was denied by the user in ask mode.",
                        self.name()
                    )),
                    Err(reason) => PermissionDecision::deny(reason),
                }
            }
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput>;

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let content = match &output.data {
            Value::String(text) => text.clone(),
            Value::Array(_) | Value::Object(_) => serde_json::to_string_pretty(&output.data)
                .unwrap_or_else(|_| output.data.to_string()),
            _ => output.data.to_string(),
        };
        serde_json::json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        })
    }
}
