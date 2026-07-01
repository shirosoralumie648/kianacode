use crate::local_state::{load_user_config, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct AdvisorCommand;

#[async_trait]
impl Command for AdvisorCommand {
    fn name(&self) -> &str {
        "advisor"
    }

    fn description(&self) -> &str {
        "Configure the advisor model"
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
            "" => return Ok(CommandResult::text(advisor_status())),
            "status" if rest.is_empty() => return Ok(CommandResult::text(advisor_status())),
            "status" => return Err(anyhow!("unknown advisor command '{}'\n\n{}", arg, usage())),
            "help" | "--help" | "-h" if rest.is_empty() => return Ok(CommandResult::text(usage())),
            "help" | "--help" | "-h" => {
                return Err(anyhow!("unknown advisor command '{}'\n\n{}", arg, usage()))
            }
            "unset" | "off" | "none" | "disable" | "disabled" if rest.is_empty() => {
                return disable_advisor()
            }
            "unset" | "off" | "none" | "disable" | "disabled" => {
                return Err(anyhow!("unknown advisor command '{}'\n\n{}", arg, usage()))
            }
            other if other.starts_with('-') => {
                return Err(anyhow!("unknown advisor option '{}'\n\n{}", other, usage()))
            }
            _ if !rest.is_empty() => {
                return Err(anyhow!(
                    "advisor model cannot contain whitespace\n\n{}",
                    usage()
                ))
            }
            _ => {}
        }

        save_advisor_model(arg)
    }
}

fn advisor_status() -> String {
    let config = load_user_config();
    let advisor = config.settings.advisor_model.as_deref().unwrap_or("off");
    format!("Advisor status\nadvisor_model: {advisor}\nusage: kiana advisor <model|off>")
}

fn disable_advisor() -> anyhow::Result<CommandResult> {
    let mut config = load_user_config();
    config.settings.advisor_model = None;
    let path = save_user_config(&config)?;
    Ok(CommandResult::text(format!(
        "Advisor disabled\nfile: {}",
        path.display()
    )))
}

fn save_advisor_model(model: &str) -> anyhow::Result<CommandResult> {
    let mut config = load_user_config();
    config.settings.advisor_model = Some(model.to_string());
    let path = save_user_config(&config)?;
    Ok(CommandResult::text(format!(
        "Advisor updated\nadvisor_model: {}\nfile: {}",
        model,
        path.display()
    )))
}

fn usage() -> &'static str {
    "Usage: kiana advisor <model|off>\n       kiana advisor status"
}

#[cfg(test)]
mod tests {
    use super::AdvisorCommand;
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
            "kiana-advisor-command-{}-{unique}.toml",
            std::process::id()
        ))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::<String, Value>::new(),
        }
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_config_with_advisor(path: &std::path::Path) {
        std::env::set_var("KIANA_CONFIG_FILE", path);
        let mut config = kiana_bootstrap::config::Config::default();
        config.settings.advisor_model = Some("claude-existing-advisor".to_string());
        save_user_config(&config).unwrap();
    }

    fn file_contains(path: &std::path::Path, needle: &str) -> bool {
        fs::read_to_string(path).unwrap().contains(needle)
    }

    #[tokio::test]
    async fn advisor_help_does_not_persist_help_as_model() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_advisor(&path);

        let result = AdvisorCommand.execute(context("--help")).await.unwrap();

        assert!(result.value.contains("Usage: kiana advisor <model|off>"));
        assert!(file_contains(&path, "claude-existing-advisor"));
        assert!(!file_contains(&path, "advisor_model = \"--help\""));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn advisor_status_with_extra_words_is_not_saved_as_model() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_advisor(&path);

        let error = AdvisorCommand
            .execute(context("status please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown advisor command"));
        assert!(file_contains(&path, "claude-existing-advisor"));
        assert!(!file_contains(&path, "status please"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }
}
