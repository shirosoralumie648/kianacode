use crate::local_state::{config_path, load_user_config, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct LoginCommand;

#[async_trait]
impl Command for LoginCommand {
    fn name(&self) -> &str {
        "login"
    }

    fn description(&self) -> &str {
        "Login to account"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let arg = context.args.trim();
        let mut parts = arg.splitn(2, char::is_whitespace);
        let head = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();

        match head {
            "" => return Ok(CommandResult::text(login_status())),
            "status" if rest.is_empty() => return Ok(CommandResult::text(login_status())),
            "status" => return Err(anyhow!("unknown login command '{}'\n\n{}", arg, usage())),
            "help" | "--help" | "-h" if rest.is_empty() => return Ok(CommandResult::text(usage())),
            "help" | "--help" | "-h" => {
                return Err(anyhow!("unknown login command '{}'\n\n{}", arg, usage()))
            }
            "token" if rest.is_empty() => {
                return Err(anyhow!("api key cannot be empty\n\n{}", usage()))
            }
            "token" => return save_api_key(rest),
            other if other.starts_with('-') => {
                return Err(anyhow!("unknown login option '{}'\n\n{}", other, usage()))
            }
            _ => {}
        }

        save_api_key(arg)
    }
}

fn login_status() -> String {
    let env_key = std::env::var("ANTHROPIC_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let file_key = load_user_config()
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    format!(
        "Login status\napi_key: {}\nsource: {}\nconfig_file: {}\nusage: kiana login <api-key>",
        if env_key || file_key {
            "set"
        } else {
            "missing"
        },
        if env_key {
            "ANTHROPIC_API_KEY"
        } else if file_key {
            "config"
        } else {
            "none"
        },
        config_path().display()
    )
}

fn save_api_key(api_key: &str) -> anyhow::Result<CommandResult> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(anyhow!("api key cannot be empty\n\n{}", usage()));
    }

    let mut config = load_user_config();
    config.api_key = Some(api_key.to_string());
    let path = save_user_config(&config)?;
    Ok(CommandResult::text(format!(
        "Login updated\napi_key: {}\nfile: {}",
        mask_secret(api_key),
        path.display()
    )))
}

fn usage() -> &'static str {
    "Usage: kiana login <api-key>\n       kiana login token <api-key>\n       kiana login status"
}

fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    let start: String = chars.iter().take(4).collect();
    let end: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{start}...{end}")
}

#[cfg(test)]
mod tests {
    use super::LoginCommand;
    use crate::local_state::{env_lock, save_user_config};
    use crate::{Command, CommandContext};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{MutexGuard, PoisonError};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-login-command-{}-{unique}.toml",
            std::process::id()
        ))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::<String, Value>::new(),
        }
    }

    fn write_config_with_api_key(path: &std::path::Path) {
        std::env::set_var("KIANA_CONFIG_FILE", path);
        std::env::remove_var("ANTHROPIC_API_KEY");
        let mut config = kiana_bootstrap::config::Config::default();
        config.api_key = Some("sk-ant-login-test-key".to_string());
        save_user_config(&config).unwrap();
    }

    fn file_contains(path: &std::path::Path, needle: &str) -> bool {
        fs::read_to_string(path).unwrap().contains(needle)
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[tokio::test]
    async fn login_help_does_not_persist_help_as_api_key() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_api_key(&path);

        let result = LoginCommand.execute(context("--help")).await.unwrap();

        assert!(result
            .value
            .to_ascii_lowercase()
            .contains("usage: kiana login <api-key>"));
        assert!(!file_contains(&path, "--help"));
        assert!(file_contains(&path, "sk-ant-login-test-key"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn login_token_without_key_is_rejected_without_overwriting_config() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_api_key(&path);

        let error = LoginCommand
            .execute(context("token"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("api key cannot be empty"));
        assert!(file_contains(&path, "sk-ant-login-test-key"));
        assert!(!file_contains(&path, "api_key = \"token\""));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn login_status_with_extra_words_is_not_saved_as_api_key() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_api_key(&path);

        let error = LoginCommand
            .execute(context("status please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown login command"));
        assert!(file_contains(&path, "sk-ant-login-test-key"));
        assert!(!file_contains(&path, "status please"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }
}
