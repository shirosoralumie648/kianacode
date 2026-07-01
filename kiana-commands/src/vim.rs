use crate::local_state::{load_user_config, on_off, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct VimCommand;

#[async_trait]
impl Command for VimCommand {
    fn name(&self) -> &str {
        "vim"
    }

    fn description(&self) -> &str {
        "Toggle vim mode"
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
            config.settings.vim_mode = !config.settings.vim_mode;
        } else if matches!(arg.as_str(), "on" | "true" | "1") {
            config.settings.vim_mode = true;
        } else if matches!(arg.as_str(), "off" | "false" | "0") {
            config.settings.vim_mode = false;
        } else if arg == "status" {
            return Ok(CommandResult::text(format!(
                "Vim mode: {}",
                on_off(config.settings.vim_mode)
            )));
        } else if matches!(arg.as_str(), "help" | "--help" | "-h") {
            return Ok(CommandResult::text(usage()));
        } else {
            return Err(anyhow!(
                "unknown vim mode '{}'; expected on, off, toggle, or status\n\n{}",
                arg,
                usage()
            ));
        }

        let path = save_user_config(&config)?;
        Ok(CommandResult::text(format!(
            "Vim mode: {}\nfile: {}",
            on_off(config.settings.vim_mode),
            path.display()
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana vim [on|off|toggle|status]"
}

#[cfg(test)]
mod tests {
    use super::VimCommand;
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
            "kiana-vim-command-{}-{unique}.toml",
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
    async fn vim_help_reports_usage_without_mutating_config() {
        let _guard = env_lock().lock().unwrap();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        let mut config = kiana_bootstrap::config::Config::default();
        config.settings.vim_mode = true;
        save_user_config(&config).unwrap();

        let result = VimCommand.execute(context("--help")).await.unwrap();

        assert!(result.value.contains("Usage: kiana vim"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("vim_mode = true"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }
}
