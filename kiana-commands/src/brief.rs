use crate::local_state::{load_user_config, on_off, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct BriefCommand;

#[async_trait]
impl Command for BriefCommand {
    fn name(&self) -> &str {
        "brief"
    }

    fn description(&self) -> &str {
        "Toggle brief-only mode"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let arg = context.args.trim().to_ascii_lowercase();
        let mut config = load_user_config();
        if arg.is_empty() || arg == "toggle" {
            config.settings.brief = !config.settings.brief;
        } else if matches!(arg.as_str(), "on" | "true" | "1") {
            config.settings.brief = true;
        } else if matches!(arg.as_str(), "off" | "false" | "0") {
            config.settings.brief = false;
        } else if arg == "status" {
            return Ok(CommandResult::text(format!(
                "Brief mode: {}",
                on_off(config.settings.brief)
            )));
        } else if matches!(arg.as_str(), "help" | "--help" | "-h") {
            return Ok(CommandResult::text(usage()));
        } else {
            return Err(anyhow!(
                "unknown brief mode '{}'; expected on, off, toggle, or status\n\n{}",
                arg,
                usage()
            ));
        }

        let path = save_user_config(&config)?;
        Ok(CommandResult::system(format!(
            "Brief mode: {}\nfile: {}",
            on_off(config.settings.brief),
            path.display()
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana brief [on|off|toggle|status]"
}

#[cfg(test)]
mod tests {
    use super::BriefCommand;
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
            "kiana-brief-command-{}-{unique}.toml",
            std::process::id()
        ))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::<String, Value>::new(),
        }
    }

    #[tokio::test]
    async fn brief_help_reports_usage_without_mutating_config() {
        let _guard = env_lock().lock().unwrap();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        let mut config = kiana_bootstrap::config::Config::default();
        config.settings.brief = true;
        save_user_config(&config).unwrap();

        let result = BriefCommand.execute(context("--help")).await.unwrap();

        assert!(result.value.contains("Usage: kiana brief"));
        assert!(fs::read_to_string(&path).unwrap().contains("brief = true"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }
}
