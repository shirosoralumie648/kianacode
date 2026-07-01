use crate::local_state::{config_path, load_user_config, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct ModelCommand;

#[async_trait]
impl Command for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }

    fn description(&self) -> &str {
        "Configure model"
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
            "" => return Ok(CommandResult::text(model_status())),
            "status" if rest.is_empty() => return Ok(CommandResult::text(model_status())),
            "status" => return Err(anyhow!("unknown model command '{}'\n\n{}", arg, usage())),
            "list" if rest.is_empty() => return Ok(CommandResult::text(model_list_text())),
            "list" if rest == "--json" => return Ok(CommandResult::text(model_list_json()?)),
            "list" => return Err(anyhow!("unknown model command '{}'\n\n{}", arg, usage())),
            "help" | "--help" | "-h" if rest.is_empty() => return Ok(CommandResult::text(usage())),
            "help" | "--help" | "-h" => {
                return Err(anyhow!("unknown model command '{}'\n\n{}", arg, usage()))
            }
            "reset" | "default" if rest.is_empty() => {
                return save_model(kiana_bootstrap::config::Config::default().model)
            }
            "reset" | "default" => {
                return Err(anyhow!("unknown model command '{}'\n\n{}", arg, usage()))
            }
            other if other.starts_with('-') => {
                return Err(anyhow!("unknown model option '{}'\n\n{}", other, usage()))
            }
            _ if !rest.is_empty() => {
                return Err(anyhow!(
                    "model name cannot contain whitespace\n\n{}",
                    usage()
                ))
            }
            _ => {}
        }

        save_model(arg.to_string())
    }
}

fn model_status() -> String {
    let config = kiana_bootstrap::config::load_config();
    format!(
        "Model status\nmodel: {}\nconfig_file: {}\nusage: kiana model <model-name>\nlist: kiana model list --json",
        config.model,
        config_path().display()
    )
}

fn model_list_json() -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(
        &kiana_services::api::provider::built_in_model_profiles(),
    )?)
}

fn model_list_text() -> String {
    let mut lines = vec![
        "Available model profiles".to_string(),
        "provider\tmodel\ttools\tstreaming\tvision\tstructured_output\tcontext_window".to_string(),
    ];
    for profile in kiana_services::api::provider::built_in_model_profiles() {
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            profile.provider_id,
            profile.model_id,
            profile.supports_tools,
            profile.supports_streaming,
            profile.supports_vision,
            profile.supports_structured_output,
            profile.context_window
        ));
    }
    lines.join("\n")
}

fn save_model(model: String) -> anyhow::Result<CommandResult> {
    let mut config = load_user_config();
    config.model = model.clone();
    let path = save_user_config(&config)?;

    Ok(CommandResult::text(format!(
        "Model updated\nmodel: {}\nfile: {}",
        model,
        path.display()
    )))
}

fn usage() -> &'static str {
    "Usage: kiana model <model-name>\n       kiana model status\n       kiana model list [--json]\n       kiana model reset"
}

#[cfg(test)]
mod tests {
    use super::ModelCommand;
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
            "kiana-model-command-{}-{unique}.toml",
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

    fn write_config_with_model(path: &std::path::Path) {
        std::env::set_var("KIANA_CONFIG_FILE", path);
        let mut config = kiana_bootstrap::config::Config::default();
        config.model = "claude-existing-model".to_string();
        save_user_config(&config).unwrap();
    }

    fn file_contains(path: &std::path::Path, needle: &str) -> bool {
        fs::read_to_string(path).unwrap().contains(needle)
    }

    #[tokio::test]
    async fn model_help_does_not_persist_help_as_model() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_model(&path);

        let result = ModelCommand.execute(context("--help")).await.unwrap();

        assert!(result.value.contains("Usage: kiana model <model-name>"));
        assert!(file_contains(&path, "claude-existing-model"));
        assert!(!file_contains(&path, "model = \"--help\""));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn model_status_with_extra_words_is_not_saved_as_model() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_model(&path);

        let error = ModelCommand
            .execute(context("status please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown model command"));
        assert!(file_contains(&path, "claude-existing-model"));
        assert!(!file_contains(&path, "status please"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn model_list_json_outputs_provider_capabilities() {
        let result = ModelCommand.execute(context("list --json")).await.unwrap();
        let value: Value = serde_json::from_str(&result.value).unwrap();
        let profiles = value.as_array().unwrap();

        assert!(profiles.iter().any(|profile| {
            profile["provider_id"].as_str() == Some("anthropic")
                && profile["model_id"].as_str() == Some("claude-sonnet-4-6")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
        }));
        assert!(profiles.iter().any(|profile| {
            profile["provider_id"].as_str() == Some("fake")
                && profile["model_id"].as_str() == Some("fake-model")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["context_window"].as_u64().unwrap() > 0
        }));
    }
}
