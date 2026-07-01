use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use std::collections::HashMap;

pub struct ClearCommand;

#[async_trait]
impl Command for ClearCommand {
    fn name(&self) -> &str {
        "clear"
    }

    fn description(&self) -> &str {
        "Clear the conversation"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        match context.args.trim() {
            "" => Ok(clear_result()),
            "status" => Ok(CommandResult::text(status())),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown clear command '{}'\n\n{}", other, usage())),
        }
    }
}

fn clear_result() -> CommandResult {
    let mut result = CommandResult::text(
        "Conversation clear requested\nscope: active REPL/TUI session\nusage: kiana clear",
    );
    result.metadata = Some(HashMap::from([(
        "action".to_string(),
        "clear_conversation".to_string(),
    )]));
    result
}

fn status() -> &'static str {
    "Clear\nstate: available for active REPL/TUI sessions\nusage: kiana clear"
}

fn usage() -> &'static str {
    "Usage: kiana clear\n       kiana clear status"
}

#[cfg(test)]
mod tests {
    use super::ClearCommand;
    use crate::{Command, CommandContext};
    use serde_json::Value;
    use std::collections::HashMap;

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::<String, Value>::new(),
        }
    }

    #[tokio::test]
    async fn clear_without_args_returns_actionable_result() {
        let result = ClearCommand.execute(context("")).await.unwrap();

        assert_eq!(result.output_type, "text");
        assert!(result.value.contains("Conversation clear requested"));
        assert!(result.value.contains("usage: kiana clear"));
    }

    #[tokio::test]
    async fn clear_status_reports_usage_without_empty_output() {
        let result = ClearCommand.execute(context("status")).await.unwrap();

        assert!(result.value.contains("Clear"));
        assert!(result.value.contains("usage: kiana clear"));
        assert!(!result.value.trim().is_empty());
    }

    #[tokio::test]
    async fn clear_rejects_unknown_args() {
        let error = ClearCommand
            .execute(context("status please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown clear command"));
        assert!(error.contains("Usage: kiana clear"));
    }
}
