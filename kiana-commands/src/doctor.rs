use crate::local_state::{bool_label, config_path, sdk_sessions_dir};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use async_trait::async_trait;
use kiana_tools::bash_sandbox::{bash_sandbox_diagnostic, BashSandboxStatus};
use kiana_tools::permissions::{effective_tool_permissions, EffectiveToolPermissions};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command as ProcessCommand;

pub struct DoctorCommand;

#[async_trait]
impl Command for DoctorCommand {
    fn name(&self) -> &str {
        "doctor"
    }

    fn description(&self) -> &str {
        "Run diagnostics"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        match context.args.trim() {
            "" => {}
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow::anyhow!(usage())),
        }

        let config = kiana_bootstrap::config::load_config();
        let config_path = config_path();
        let sdk_dir = sdk_sessions_dir();
        let cwd = std::env::current_dir()?;
        let cargo_status = command_first_line("cargo", &["--version"]);
        let git_status = command_first_line("git", &["rev-parse", "--show-toplevel"]);
        let modifier_status = kiana_modifiers::get_modifier_status();
        let modifier_keys = if modifier_status.modifiers.is_empty() {
            "none".to_string()
        } else {
            modifier_status.modifiers.join(",")
        };
        let code_session_token_status = code_session_live_smoke_token_status();
        let sandbox_state =
            sandbox_diagnostic_app_state(&context.app_state, config.sandbox.as_ref());
        let bash_sandbox = bash_sandbox_diagnostic(&sandbox_state);
        let tool_permissions = effective_tool_permissions(&context.app_state);
        let commercial_security_issues =
            commercial_security_issues(&tool_permissions, &bash_sandbox);

        let mut lines = vec![
            "Doctor".to_string(),
            format!("cwd: {}", cwd.display()),
            format!(
                "cargo: {}",
                cargo_status.unwrap_or_else(|| "missing".to_string())
            ),
            format!(
                "git_root: {}",
                git_status.unwrap_or_else(|| "not a git repository".to_string())
            ),
            format!(
                "config_file: {} ({})",
                config_path.display(),
                if config_path.is_file() {
                    "found"
                } else {
                    "missing"
                }
            ),
            format!(
                "sdk_sessions_dir: {} ({})",
                sdk_dir.display(),
                if sdk_dir.is_dir() { "found" } else { "missing" }
            ),
            format!(
                "api_key_set: {}",
                bool_label(
                    config
                        .api_key
                        .as_deref()
                        .is_some_and(|v| !v.trim().is_empty())
                )
            ),
            format!("model: {}", config.model),
            format!(
                "remote_settings: status={} file={}",
                remote_settings_status(),
                remote_settings_file_label()
            ),
            format!(
                "tui_permission_request: active={} queued={}",
                bool_label(app_state_bool(
                    &context.app_state,
                    "tui_permission_request_active"
                )),
                app_state_usize(&context.app_state, "tui_permission_request_queue_len")
            ),
            "mcp_transport: stdio,http,sse,ws wired; surfaces: tools,resources,resource_templates,prompts".to_string(),
            format!(
                "modifiers: platform={} backend={} available={} current={}",
                modifier_status.platform,
                modifier_status.backend,
                bool_label(modifier_status.available),
                modifier_keys
            ),
            format!(
                "remote_bridge: start command wired with SDK runner; token_configured: {}",
                bool_label(bridge_access_token_configured())
            ),
            format!(
                "remote_code_session: live_smoke_token={}",
                code_session_token_status.label()
            ),
            format!(
                "bash_sandbox: enabled={} status={} runtime={} fail_if_unavailable={} allow_unsandboxed_commands={} bwrap={}",
                bool_label(bash_sandbox.enabled),
                bash_sandbox.status.as_str(),
                bash_sandbox.runtime_label(),
                bool_label(bash_sandbox.fail_if_unavailable),
                bool_label(bash_sandbox.allow_unsandboxed_commands),
                bash_sandbox.bwrap_label()
            ),
            format!(
                "commercial_security: {}",
                if commercial_security_issues.is_empty() {
                    "ready".to_string()
                } else {
                    format!("not_ready({} issue(s))", commercial_security_issues.len())
                }
            ),
        ];

        if config
            .api_key
            .as_deref()
            .is_none_or(|v| v.trim().is_empty())
        {
            lines.push("warning: set ANTHROPIC_API_KEY or ~/.kiana/config.toml before sending model prompts".to_string());
        }
        if !bridge_access_token_configured() {
            lines.push("warning: remote bridge start requires KIANA_BRIDGE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN".to_string());
        }
        if let Some(warning) = code_session_token_status.warning() {
            lines.push(warning);
        }
        if !modifier_status.available {
            let reason = modifier_status
                .message
                .unwrap_or_else(|| "modifier polling is unavailable".to_string());
            lines.push(format!(
                "warning: modifier key polling unavailable: {reason}"
            ));
        }
        if bash_sandbox.status == BashSandboxStatus::Unavailable && bash_sandbox.fail_if_unavailable
        {
            lines.push(
                "warning: bash sandbox enabled with failIfUnavailable=true, but bubblewrap (bwrap) is not available"
                    .to_string(),
            );
        } else if bash_sandbox.status == BashSandboxStatus::Unavailable
            && !bash_sandbox.allow_unsandboxed_commands
        {
            lines.push(
                "warning: bash sandbox enabled but unavailable and allowUnsandboxedCommands=false; Bash commands will fail"
                    .to_string(),
            );
        } else if bash_sandbox.status == BashSandboxStatus::Unavailable {
            lines.push(
                "warning: bash sandbox enabled but bubblewrap (bwrap) is not available; Bash may run unsandboxed if allowed"
                    .to_string(),
            );
        }
        for issue in commercial_security_issues {
            lines.push(format!("commercial_security_issue: {issue}"));
        }

        Ok(CommandResult::text(lines.join("\n")))
    }
}

fn usage() -> &'static str {
    "Usage: kiana doctor"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeSessionLiveSmokeTokenStatus {
    Configured(&'static str),
    Missing,
    AnthropicApiKeyMisuse,
}

impl CodeSessionLiveSmokeTokenStatus {
    fn label(self) -> String {
        match self {
            CodeSessionLiveSmokeTokenStatus::Configured(source) => {
                format!("yes ({source})")
            }
            CodeSessionLiveSmokeTokenStatus::Missing => "no".to_string(),
            CodeSessionLiveSmokeTokenStatus::AnthropicApiKeyMisuse => {
                "invalid (ANTHROPIC_AUTH_TOKEN is sk-* API key)".to_string()
            }
        }
    }

    fn warning(self) -> Option<String> {
        match self {
            CodeSessionLiveSmokeTokenStatus::AnthropicApiKeyMisuse => Some(
                "warning: live smoke requires KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN; ANTHROPIC_AUTH_TOKEN=sk-* is an API key, not a remote access token"
                    .to_string(),
            ),
            CodeSessionLiveSmokeTokenStatus::Missing => Some(
                "warning: live smoke requires KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN"
                    .to_string(),
            ),
            CodeSessionLiveSmokeTokenStatus::Configured(_) => None,
        }
    }
}

fn app_state_bool(app_state: &HashMap<String, Value>, key: &str) -> bool {
    app_state.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn app_state_usize(app_state: &HashMap<String, Value>, key: &str) -> usize {
    app_state
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn sandbox_diagnostic_app_state(
    app_state: &HashMap<String, Value>,
    config_sandbox: Option<&Value>,
) -> HashMap<String, Value> {
    let mut state = app_state.clone();
    if !state.contains_key("sandbox") {
        if let Some(sandbox) = config_sandbox {
            state.insert("sandbox".to_string(), sandbox.clone());
        }
    }
    state
}

fn commercial_security_issues(
    permissions: &EffectiveToolPermissions,
    bash_sandbox: &kiana_tools::bash_sandbox::BashSandboxDiagnostic,
) -> Vec<String> {
    let mut issues = Vec::new();
    if permissions.profile != "commercial" {
        issues.push("set `kiana permissions profile commercial`".to_string());
    }
    if permissions.mode != "ask" {
        issues.push("commercial profile must resolve to ask permission mode".to_string());
    }
    if !bash_sandbox.enabled {
        issues.push("enable bash sandbox in config sandbox.enabled=true".to_string());
    }
    if bash_sandbox.status != BashSandboxStatus::Ready {
        issues.push("install/configure bubblewrap (bwrap) for bash sandbox".to_string());
    }
    if !bash_sandbox.fail_if_unavailable {
        issues.push("set sandbox.failIfUnavailable=true".to_string());
    }
    if bash_sandbox.allow_unsandboxed_commands {
        issues.push("set sandbox.allowUnsandboxedCommands=false".to_string());
    }
    issues
}

fn bridge_access_token_configured() -> bool {
    std::env::var("KIANA_BRIDGE_ACCESS_TOKEN")
        .or_else(|_| std::env::var("CLAUDE_ACCESS_TOKEN"))
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn code_session_live_smoke_token_status() -> CodeSessionLiveSmokeTokenStatus {
    for source in [
        "KIANA_REMOTE_ACCESS_TOKEN",
        "CLAUDE_ACCESS_TOKEN",
        "ANTHROPIC_AUTH_TOKEN",
    ] {
        let Ok(value) = std::env::var(source) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if source == "ANTHROPIC_AUTH_TOKEN" && value.starts_with("sk-") {
            return CodeSessionLiveSmokeTokenStatus::AnthropicApiKeyMisuse;
        }
        return CodeSessionLiveSmokeTokenStatus::Configured(source);
    }
    CodeSessionLiveSmokeTokenStatus::Missing
}

fn command_first_line(program: &str, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}

fn remote_settings_status() -> String {
    std::env::var("KIANA_REMOTE_SETTINGS_STATUS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "not_loaded".to_string())
}

fn remote_settings_file_label() -> String {
    let Some(path) = std::env::var_os("KIANA_REMOTE_SETTINGS_FILE") else {
        return "none".to_string();
    };
    let path = std::path::PathBuf::from(path);
    format!(
        "{} ({})",
        path.display(),
        if path.is_file() { "found" } else { "missing" }
    )
}

#[cfg(test)]
mod tests {
    use super::DoctorCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
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
            "kiana-doctor-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn doctor_reports_wired_transports_without_stale_warning() {
        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("mcp_transport: stdio,http,sse,ws wired; surfaces: tools,resources,resource_templates,prompts"));
        assert!(result.value.contains("remote_bridge: start command wired"));
        assert!(result.value.contains("modifiers: platform="));
        assert!(result.value.contains("remote_settings:"));
        assert!(!result.value.contains("not wired yet"));
    }

    #[tokio::test]
    async fn doctor_reports_tui_permission_request_state() {
        let mut app_state = HashMap::new();
        app_state.insert("tui_permission_request_active".to_string(), json!(true));
        app_state.insert("tui_permission_request_queue_len".to_string(), json!(2));

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state,
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("tui_permission_request: active=yes queued=2"));
    }

    #[tokio::test]
    async fn doctor_reports_code_session_live_smoke_token_misuse() {
        let _lock = env_lock().lock().unwrap();
        let _guard = EnvGuard::set(&[
            ("KIANA_REMOTE_ACCESS_TOKEN", None),
            ("CLAUDE_ACCESS_TOKEN", None),
            ("ANTHROPIC_AUTH_TOKEN", Some("sk-test-api-key")),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains(
            "remote_code_session: live_smoke_token=invalid (ANTHROPIC_AUTH_TOKEN is sk-* API key)"
        ));
        assert!(result.value.contains(
            "warning: live smoke requires KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN; ANTHROPIC_AUTH_TOKEN=sk-* is an API key, not a remote access token"
        ));
    }

    #[tokio::test]
    async fn doctor_reports_ready_bash_sandbox_when_bwrap_is_available() {
        let _lock = env_lock().lock().unwrap();
        let bwrap = temp_path("fake-bwrap");
        fs::write(&bwrap, "#!/bin/sh\n").unwrap();
        let settings = json!({
            "sandbox": {
                "enabled": true,
                "failIfUnavailable": true,
                "allowUnsandboxedCommands": false,
                "bwrapPath": bwrap,
            }
        })
        .to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_SETTINGS_JSON", Some(&settings)),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains(
            "bash_sandbox: enabled=yes status=ready runtime=bwrap fail_if_unavailable=yes allow_unsandboxed_commands=no"
        ));
        assert!(result.value.contains(&format!("bwrap={}", bwrap.display())));
        assert!(!result
            .value
            .contains("warning: bash sandbox enabled with failIfUnavailable=true"));

        let _ = fs::remove_file(bwrap);
    }

    #[tokio::test]
    async fn doctor_warns_when_required_bash_sandbox_lacks_bwrap() {
        let _lock = env_lock().lock().unwrap();
        let path_dir = temp_path("empty-path");
        fs::create_dir_all(&path_dir).unwrap();
        let _guard = EnvGuard::set(&[
            (
                "KIANA_SETTINGS_JSON",
                Some(
                    r#"{"sandbox":{"enabled":true,"failIfUnavailable":true,"allowUnsandboxedCommands":false}}"#,
                ),
            ),
            ("PATH", Some(path_dir.to_str().unwrap())),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains(
            "bash_sandbox: enabled=yes status=unavailable runtime=missing fail_if_unavailable=yes allow_unsandboxed_commands=no bwrap=missing"
        ));
        assert!(result.value.contains(
            "warning: bash sandbox enabled with failIfUnavailable=true, but bubblewrap (bwrap) is not available"
        ));

        let _ = fs::remove_dir_all(path_dir);
    }

    #[tokio::test]
    async fn doctor_reports_commercial_security_ready() {
        let _lock = env_lock().lock().unwrap();
        let bwrap = temp_path("commercial-bwrap");
        fs::write(&bwrap, "#!/bin/sh\n").unwrap();
        let settings = json!({
            "sandbox": {
                "enabled": true,
                "failIfUnavailable": true,
                "allowUnsandboxedCommands": false,
                "bwrapPath": bwrap,
            }
        })
        .to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_SETTINGS_JSON", Some(&settings)),
            ("KIANA_PERMISSION_PROFILE", Some("commercial")),
            ("KIANA_PERMISSION_MODE", None),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("commercial_security: ready"));
        assert!(!result.value.contains("commercial_security_issue:"));

        let _ = fs::remove_file(bwrap);
    }

    #[tokio::test]
    async fn doctor_reports_commercial_security_gaps() {
        let _lock = env_lock().lock().unwrap();
        let path_dir = temp_path("commercial-empty-path");
        fs::create_dir_all(&path_dir).unwrap();
        let _guard = EnvGuard::set(&[
            (
                "KIANA_SETTINGS_JSON",
                Some(r#"{"sandbox":{"enabled":false}}"#),
            ),
            ("KIANA_PERMISSION_PROFILE", Some("workspace")),
            ("KIANA_PERMISSION_MODE", None),
            ("PATH", Some(path_dir.to_str().unwrap())),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("commercial_security: not_ready"));
        assert!(result
            .value
            .contains("commercial_security_issue: set `kiana permissions profile commercial`"));
        assert!(result
            .value
            .contains("commercial_security_issue: enable bash sandbox"));

        let _ = fs::remove_dir_all(path_dir);
    }
}
