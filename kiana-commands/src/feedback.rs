use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::json;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct FeedbackCommand;

#[async_trait]
impl Command for FeedbackCommand {
    fn name(&self) -> &str {
        "feedback"
    }

    fn description(&self) -> &str {
        "Send feedback"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let message = context.args.trim();
        let path = kiana_home_dir().join("feedback.jsonl");
        if matches!(message, "help" | "--help" | "-h") {
            return Ok(CommandResult::text(usage()));
        }
        if message.is_empty() || matches!(message, "status" | "path") {
            return Ok(CommandResult::text(format!(
                "Feedback\nfile: {}\nusage: kiana feedback <message>",
                path.display()
            )));
        }

        if let Some((command, _rest)) = message.split_once(char::is_whitespace) {
            if matches!(command, "status" | "path" | "help" | "--help" | "-h") {
                return Err(anyhow!(
                    "unknown feedback command '{}'; usage: kiana feedback <message>",
                    message
                ));
            }
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let cwd = std::env::current_dir()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| anyhow!("system clock error: {}", error))?
            .as_secs();
        let entry = json!({
            "created_at": now,
            "cwd": cwd,
            "message": message
        });
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?
            .write_all(line.as_bytes())?;
        Ok(CommandResult::text(format!(
            "Feedback recorded\nfile: {}",
            path.display()
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana feedback <message>"
}

#[cfg(test)]
mod tests {
    use super::FeedbackCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{MutexGuard, PoisonError};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-feedback-command-{}-{unique}",
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

    #[tokio::test]
    async fn feedback_help_reports_usage_without_recording_feedback() {
        let _guard = lock_env();
        let root = temp_home();
        std::env::set_var("KIANA_HOME", &root);

        let result = FeedbackCommand.execute(context("--help")).await.unwrap();

        assert!(result.value.contains("Usage: kiana feedback <message>"));
        assert!(!root.join("feedback.jsonl").exists());

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn feedback_status_with_extra_words_is_rejected_without_recording_feedback() {
        let _guard = lock_env();
        let root = temp_home();
        std::env::set_var("KIANA_HOME", &root);

        let error = FeedbackCommand
            .execute(context("status please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown feedback command"));
        assert!(!root.join("feedback.jsonl").exists());

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }
}
