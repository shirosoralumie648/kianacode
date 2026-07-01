use crate::local_state::{
    app_state_array_len, app_state_keys, bool_label, config_path, sdk_sessions_dir,
};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use kiana_tools::permissions::effective_tool_permissions;
use serde_json::Value;
use std::collections::HashMap;

pub struct StatusCommand;

#[async_trait]
impl Command for StatusCommand {
    fn name(&self) -> &str {
        "status"
    }

    fn description(&self) -> &str {
        "Show status"
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
            _ => return Err(anyhow!(usage())),
        }

        let config = kiana_bootstrap::config::load_config();
        let cwd = std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com".to_string());
        let keys = app_state_keys(&context.app_state);
        let permissions = effective_tool_permissions(&context.app_state);
        let lsp_status = kiana_tools::lsp_tool::lsp_runtime_status().await;

        let mut lines = vec![
            format!("Kiana Code {}", env!("CARGO_PKG_VERSION")),
            format!("cwd: {}", cwd),
            format!("model: {}", config.model),
            format!(
                "api_key: {}",
                if config
                    .api_key
                    .as_deref()
                    .is_some_and(|v| !v.trim().is_empty())
                {
                    "set"
                } else {
                    "missing"
                }
            ),
            format!("base_url: {}", base_url),
            format!(
                "config_file: {} ({})",
                config_path().display(),
                if config_path().is_file() {
                    "found"
                } else {
                    "missing"
                }
            ),
            format!("sdk_sessions_dir: {}", sdk_sessions_dir().display()),
            format!(
                "session_tasks: {}",
                app_state_array_len(&context.app_state, "tasks")
            ),
            format!(
                "teams: {}",
                app_state_array_len(&context.app_state, "teams")
            ),
            format!(
                "mcp_invocations: {}",
                app_state_array_len(&context.app_state, "mcp_invocations")
            ),
            format!("permissions_mode: {}", permissions.mode),
            format!(
                "permission_rules: allow={} deny={}",
                permissions.allowed_tools.len(),
                permissions.disallowed_tools.len()
            ),
            format!(
                "tui_permission_request: active={} queued={}",
                bool_label(app_state_bool(
                    &context.app_state,
                    "tui_permission_request_active"
                )),
                app_state_usize(&context.app_state, "tui_permission_request_queue_len")
            ),
            format!(
                "remote_settings: status={} file={}",
                remote_settings_status(),
                remote_settings_file_label()
            ),
            format!(
                "lsp_clients: {} open_files: {}",
                lsp_status
                    .get("active_clients")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                lsp_status
                    .get("open_files")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
            ),
            format!(
                "lsp_diagnostics: pending_notifications={} pending_files={} pending_diagnostics={} delivered_files={}",
                lsp_status
                    .get("pending_notifications")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                lsp_status
                    .get("pending_files")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                lsp_status
                    .get("pending_diagnostics")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                lsp_status
                    .get("delivered_files")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
            ),
            "mcp_transport: stdio,http,sse,ws wired; surfaces: tools,resources,resource_templates,prompts".to_string(),
            format!(
                "remote_bridge: start command wired; token_configured: {}",
                bool_label(bridge_access_token_configured())
            ),
            format!("has_repl_state: {}", bool_label(!keys.is_empty())),
        ];

        if !keys.is_empty() {
            lines.push(format!("state_keys: {}", keys.join(", ")));
        }

        Ok(CommandResult::text(lines.join("\n")))
    }
}

fn usage() -> &'static str {
    "Usage: kiana status"
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

fn bridge_access_token_configured() -> bool {
    std::env::var("KIANA_BRIDGE_ACCESS_TOKEN")
        .or_else(|_| std::env::var("CLAUDE_ACCESS_TOKEN"))
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || kiana_services::oauth::load_oauth_tokens()
            .ok()
            .flatten()
            .is_some_and(|tokens| !tokens.access_token.trim().is_empty())
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
    use super::StatusCommand;
    use crate::{Command, CommandContext};
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn status_reports_repl_state_counts() {
        let mut app_state = HashMap::new();
        app_state.insert("tasks".to_string(), json!([{ "id": "t1" }]));
        app_state.insert("teams".to_string(), json!([]));
        app_state.insert("tui_permission_request_active".to_string(), json!(true));
        app_state.insert("tui_permission_request_queue_len".to_string(), json!(2));

        let result = StatusCommand
            .execute(CommandContext {
                args: String::new(),
                app_state,
            })
            .await
            .unwrap();

        assert!(result.value.contains("session_tasks: 1"));
        assert!(result
            .value
            .contains("tui_permission_request: active=yes queued=2"));
        assert!(result.value.contains("state_keys: tasks, teams"));
        assert!(result
            .value
            .contains("mcp_transport: stdio,http,sse,ws wired; surfaces: tools,resources,resource_templates,prompts"));
        assert!(result.value.contains("permissions_mode:"));
        assert!(result.value.contains("remote_settings:"));
        assert!(result.value.contains("lsp_clients:"));
        assert!(result.value.contains("lsp_diagnostics:"));
    }

    #[tokio::test]
    async fn status_rejects_unknown_args_instead_of_returning_status() {
        let result = StatusCommand
            .execute(CommandContext {
                args: "details".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }
}
