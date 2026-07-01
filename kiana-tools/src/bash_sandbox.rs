use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

pub const SANDBOX_APP_STATE_KEY: &str = "sandbox";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashSandboxStatus {
    Disabled,
    Ready,
    Unavailable,
}

impl BashSandboxStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            BashSandboxStatus::Disabled => "disabled",
            BashSandboxStatus::Ready => "ready",
            BashSandboxStatus::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashSandboxDiagnostic {
    pub enabled: bool,
    pub fail_if_unavailable: bool,
    pub allow_unsandboxed_commands: bool,
    pub bwrap_path: Option<PathBuf>,
    pub status: BashSandboxStatus,
}

impl BashSandboxDiagnostic {
    pub fn runtime_label(&self) -> &'static str {
        if self.bwrap_path.is_some() {
            "bwrap"
        } else {
            "missing"
        }
    }

    pub fn bwrap_label(&self) -> String {
        self.bwrap_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "missing".to_string())
    }
}

pub fn bash_sandbox_diagnostic(app_state: &HashMap<String, Value>) -> BashSandboxDiagnostic {
    let enabled = bash_sandbox_enabled(app_state);
    let fail_if_unavailable = bash_sandbox_fail_if_unavailable(app_state);
    let allow_unsandboxed_commands = bash_sandbox_allow_unsandboxed_commands(app_state);
    let bwrap_path = bash_sandbox_bwrap_path(app_state);
    let status = if !enabled {
        BashSandboxStatus::Disabled
    } else if bwrap_path.is_some() {
        BashSandboxStatus::Ready
    } else {
        BashSandboxStatus::Unavailable
    };

    BashSandboxDiagnostic {
        enabled,
        fail_if_unavailable,
        allow_unsandboxed_commands,
        bwrap_path,
        status,
    }
}

pub fn bash_sandbox_enabled(app_state: &HashMap<String, Value>) -> bool {
    bool_from_app_state(app_state, "bash_sandbox_enabled")
        .or_else(|| sandbox_object_bool(app_state, "enabled", "enabled"))
        .or_else(|| env_bool("KIANA_BASH_SANDBOX"))
        .unwrap_or(false)
}

pub fn bash_sandbox_fail_if_unavailable(app_state: &HashMap<String, Value>) -> bool {
    bool_from_app_state(app_state, "bash_sandbox_fail_if_unavailable")
        .or_else(|| sandbox_object_bool(app_state, "fail_if_unavailable", "failIfUnavailable"))
        .or_else(|| env_bool("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE"))
        .unwrap_or(false)
}

pub fn bash_sandbox_allow_unsandboxed_commands(app_state: &HashMap<String, Value>) -> bool {
    bool_from_app_state(app_state, "bash_sandbox_allow_unsandboxed_commands")
        .or_else(|| {
            sandbox_object_bool(
                app_state,
                "allow_unsandboxed_commands",
                "allowUnsandboxedCommands",
            )
        })
        .or_else(|| env_bool("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED"))
        .unwrap_or(true)
}

pub fn bash_sandbox_bwrap_path(app_state: &HashMap<String, Value>) -> Option<PathBuf> {
    sandbox_object_string(app_state, "bwrap_path", "bwrapPath")
        .or_else(|| std::env::var("KIANA_BWRAP_PATH").ok())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_on_path("bwrap"))
}

fn bool_from_app_state(app_state: &HashMap<String, Value>, key: &str) -> Option<bool> {
    bool_from_value(app_state.get(key)?)
}

fn sandbox_object_bool(
    app_state: &HashMap<String, Value>,
    snake_key: &str,
    camel_key: &str,
) -> Option<bool> {
    let sandbox = app_state.get(SANDBOX_APP_STATE_KEY)?;
    match sandbox {
        Value::Bool(value) if snake_key == "enabled" => Some(*value),
        Value::Object(settings) => bool_from_value(
            settings
                .get(snake_key)
                .or_else(|| settings.get(camel_key))?,
        ),
        _ => None,
    }
}

fn sandbox_object_string(
    app_state: &HashMap<String, Value>,
    snake_key: &str,
    camel_key: &str,
) -> Option<String> {
    let sandbox = app_state.get(SANDBOX_APP_STATE_KEY)?;
    sandbox
        .as_object()
        .and_then(|settings| settings.get(snake_key).or_else(|| settings.get(camel_key)))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_from_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(value) => parse_bool(value),
        _ => None,
    }
}

fn env_bool(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_bool(&value))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn find_on_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|path| path.join(binary))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_env;
    use serde_json::json;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn set(values: &[(&'static str, Option<&str>)]) -> Self {
            let previous = values
                .iter()
                .map(|(key, _)| (*key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            for (key, value) in values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self { values: previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-bash-sandbox-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn diagnostic_uses_sandbox_object_and_env_fallbacks() {
        let _lock = lock_env();
        let bwrap = temp_path("bwrap");
        fs::write(&bwrap, "#!/bin/sh\n").unwrap();
        let _guard = EnvGuard::set(&[
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);
        let app_state = HashMap::from([(
            "sandbox".to_string(),
            json!({
                "enabled": true,
                "failIfUnavailable": true,
                "allowUnsandboxedCommands": false,
                "bwrapPath": bwrap
            }),
        )]);

        let diagnostic = bash_sandbox_diagnostic(&app_state);

        assert!(diagnostic.enabled);
        assert!(diagnostic.fail_if_unavailable);
        assert!(!diagnostic.allow_unsandboxed_commands);
        assert_eq!(diagnostic.status, BashSandboxStatus::Ready);
        assert_eq!(diagnostic.runtime_label(), "bwrap");

        let _ = fs::remove_file(bwrap);
    }

    #[test]
    fn diagnostic_accepts_boolean_sandbox_as_enabled_shorthand() {
        let _lock = lock_env();
        let path_dir = temp_path("empty-path");
        fs::create_dir_all(&path_dir).unwrap();
        let _guard = EnvGuard::set(&[
            ("PATH", Some(path_dir.to_str().unwrap())),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);
        let app_state = HashMap::from([("sandbox".to_string(), json!(true))]);

        let diagnostic = bash_sandbox_diagnostic(&app_state);

        assert!(diagnostic.enabled);
        assert!(!diagnostic.fail_if_unavailable);
        assert!(diagnostic.allow_unsandboxed_commands);
        assert_eq!(diagnostic.status, BashSandboxStatus::Unavailable);

        let _ = fs::remove_dir_all(path_dir);
    }
}
