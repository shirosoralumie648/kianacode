use crate::local_state::{config_path, load_user_config, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct LogoutCommand;

#[async_trait]
impl Command for LogoutCommand {
    fn name(&self) -> &str {
        "logout"
    }

    fn description(&self) -> &str {
        "Logout from account"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let arg = context.args.trim();
        match arg {
            "" => logout(),
            "status" => Ok(CommandResult::text(logout_status())),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown logout command '{}'\n\n{}", other, usage())),
        }
    }
}

fn logout() -> anyhow::Result<CommandResult> {
    let mut config = load_user_config();
    let had_file_key = config
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    config.api_key = None;
    let path = save_user_config(&config)?;
    let env_key = std::env::var("ANTHROPIC_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    Ok(CommandResult::text(format!(
        "Logout complete\nremoved_config_key: {}\nfile: {}\nenv_key_still_set: {}",
        if had_file_key { "yes" } else { "no" },
        path.display(),
        if env_key { "yes" } else { "no" }
    )))
}

fn logout_status() -> String {
    let config = load_user_config();
    let file_key = config
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let env_key = std::env::var("ANTHROPIC_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    format!(
        "Logout status\nfile_api_key: {}\nenv_api_key: {}\nconfig_file: {}\nusage: kiana logout",
        if file_key { "set" } else { "missing" },
        if env_key { "set" } else { "missing" },
        config_path().display()
    )
}

fn usage() -> &'static str {
    "Usage: kiana logout\n       kiana logout status"
}

#[cfg(test)]
mod tests {
    use super::LogoutCommand;
    use crate::local_state::{env_lock, save_user_config};
    use crate::{Command, CommandContext};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-logout-command-{}-{unique}.toml",
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
        config.api_key = Some("sk-ant-logout-test-key".to_string());
        save_user_config(&config).unwrap();
    }

    #[tokio::test]
    async fn logout_status_reports_without_removing_config_key() {
        let _guard = env_lock().lock().unwrap();
        let path = temp_config_path();
        write_config_with_api_key(&path);

        let result = LogoutCommand.execute(context("status")).await.unwrap();

        assert!(result.value.contains("Logout status"));
        assert!(result.value.contains("file_api_key: set"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("sk-ant-logout-test-key"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn logout_rejects_unknown_args_without_removing_config_key() {
        let _guard = env_lock().lock().unwrap();
        let path = temp_config_path();
        write_config_with_api_key(&path);

        let error = LogoutCommand
            .execute(context("status please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown logout command"));
        assert!(error.contains("Usage: kiana logout"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("sk-ant-logout-test-key"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }
}
