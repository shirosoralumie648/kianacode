use crate::local_state::{load_user_config, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct ThemeCommand;

#[async_trait]
impl Command for ThemeCommand {
    fn name(&self) -> &str {
        "theme"
    }

    fn description(&self) -> &str {
        "Configure theme"
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
        let config = load_user_config();
        if head.is_empty() || (head == "status" && rest.is_empty()) {
            let theme = config.settings.theme.as_deref().unwrap_or("system");
            return Ok(CommandResult::text(format!(
                "Theme status\ntheme: {}\nusage: kiana theme <system|dark|light>",
                theme
            )));
        }
        match head {
            "status" => return Err(anyhow!("unknown theme command '{}'\n\n{}", arg, usage())),
            "help" | "--help" | "-h" if rest.is_empty() => return Ok(CommandResult::text(usage())),
            "help" | "--help" | "-h" => {
                return Err(anyhow!("unknown theme command '{}'\n\n{}", arg, usage()))
            }
            other if other.starts_with('-') => {
                return Err(anyhow!("unknown theme option '{}'\n\n{}", other, usage()))
            }
            _ if !rest.is_empty() => return Err(anyhow!("theme accepts one value\n\n{}", usage())),
            _ => {}
        }

        let normalized = match head.to_ascii_lowercase().as_str() {
            "system" | "auto" | "reset" | "default" => None,
            "dark" => Some("dark".to_string()),
            "light" => Some("light".to_string()),
            other => {
                return Err(anyhow!(
                    "unknown theme '{}'; expected system, dark, or light",
                    other
                ))
            }
        };

        let mut config = config;
        config.settings.theme = normalized.clone();
        let path = save_user_config(&config)?;
        Ok(CommandResult::text(format!(
            "Theme updated\ntheme: {}\nfile: {}",
            normalized.as_deref().unwrap_or("system"),
            path.display()
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana theme <system|dark|light>\n       kiana theme status\n       kiana theme reset"
}

#[cfg(test)]
mod tests {
    use super::ThemeCommand;
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
            "kiana-theme-command-{}-{unique}.toml",
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

    fn write_config_with_theme(path: &std::path::Path) {
        std::env::set_var("KIANA_CONFIG_FILE", path);
        let mut config = kiana_bootstrap::config::Config::default();
        config.settings.theme = Some("dark".to_string());
        save_user_config(&config).unwrap();
    }

    fn file_contains(path: &std::path::Path, needle: &str) -> bool {
        fs::read_to_string(path).unwrap().contains(needle)
    }

    #[tokio::test]
    async fn theme_help_does_not_persist_help_as_theme() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_theme(&path);

        let result = ThemeCommand.execute(context("--help")).await.unwrap();

        assert!(result
            .value
            .contains("Usage: kiana theme <system|dark|light>"));
        assert!(file_contains(&path, "theme = \"dark\""));
        assert!(!file_contains(&path, "--help"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }
}
