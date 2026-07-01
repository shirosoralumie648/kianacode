use crate::local_state::{config_path, load_user_config};
use crate::login::LoginCommand;
use crate::logout::LogoutCommand;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::json;

pub struct AuthCommand;

#[async_trait]
impl Command for AuthCommand {
    fn name(&self) -> &str {
        "auth"
    }

    fn description(&self) -> &str {
        "Manage authentication"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("status") {
            "" | "status" => auth_status(rest),
            "login" => {
                LoginCommand
                    .execute(CommandContext {
                        args: rest.to_string(),
                        app_state: context.app_state,
                    })
                    .await
            }
            "logout" => {
                LogoutCommand
                    .execute(CommandContext {
                        args: rest.to_string(),
                        app_state: context.app_state,
                    })
                    .await
            }
            "help" | "--help" | "-h" if rest.is_empty() => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown auth command '{}'\n\n{}", other, usage())),
        }
    }
}

fn auth_status(rest: &str) -> anyhow::Result<CommandResult> {
    match rest.trim() {
        "" | "--text" => Ok(CommandResult::text(auth_status_text())),
        "--json" => Ok(CommandResult::text(auth_status_json()?)),
        "help" | "--help" | "-h" => Ok(CommandResult::text(status_usage())),
        other => Err(anyhow!(
            "unknown auth status option '{}'\n\n{}",
            other,
            status_usage()
        )),
    }
}

fn auth_status_text() -> String {
    let state = auth_state();
    format!(
        "Auth status\napi_key: {}\nsource: {}\nconfig_file: {}\nusage: kiana auth status [--json|--text]",
        state.api_key,
        state.source,
        state.config_file
    )
}

fn auth_status_json() -> anyhow::Result<String> {
    let state = auth_state();
    Ok(serde_json::to_string_pretty(&json!({
        "api_key": state.api_key,
        "source": state.source,
        "config_file": state.config_file,
    }))?)
}

fn auth_state() -> AuthState {
    let env_key = std::env::var("ANTHROPIC_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let file_key = load_user_config()
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    AuthState {
        api_key: if env_key || file_key {
            "set".to_string()
        } else {
            "missing".to_string()
        },
        source: if env_key {
            "ANTHROPIC_API_KEY".to_string()
        } else if file_key {
            "config".to_string()
        } else {
            "none".to_string()
        },
        config_file: config_path().display().to_string(),
    }
}

struct AuthState {
    api_key: String,
    source: String,
    config_file: String,
}

fn usage() -> &'static str {
    "Usage: kiana auth [status|login|logout]\n       kiana auth status [--json|--text]\n       kiana auth login <api-key>\n       kiana auth logout"
}

fn status_usage() -> &'static str {
    "Usage: kiana auth status [--json|--text]"
}

fn split_word(input: &str) -> (Option<&str>, &str) {
    let input = input.trim();
    if input.is_empty() {
        return (None, "");
    }
    match input.find(char::is_whitespace) {
        Some(index) => (Some(&input[..index]), input[index..].trim()),
        None => (Some(input), ""),
    }
}
