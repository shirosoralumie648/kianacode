use crate::local_state::{app_state_array_len, app_state_keys, sdk_session_stats};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct StatsCommand;

#[async_trait]
impl Command for StatsCommand {
    fn name(&self) -> &str {
        "stats"
    }

    fn description(&self) -> &str {
        "Show statistics"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        match context.args.trim() {
            "" => {}
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }

        let keys = app_state_keys(&context.app_state);
        let sessions = sdk_session_stats();
        Ok(CommandResult::text(format!(
            "Stats\nstate_keys: {}\nsession_tasks: {}\nteams: {}\ntodos: {}\nmcp_invocations: {}\nsdk_sessions: {}\nsdk_messages: {}\nlatest_session_updated: {}",
            keys.len(),
            app_state_array_len(&context.app_state, "tasks"),
            app_state_array_len(&context.app_state, "teams"),
            app_state_array_len(&context.app_state, "todos"),
            app_state_array_len(&context.app_state, "mcp_invocations"),
            sessions.session_count,
            sessions.message_count,
            sessions
                .latest_updated_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana stats"
}

#[cfg(test)]
mod tests {
    use super::StatsCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn stats_rejects_unknown_args_instead_of_returning_stats() {
        let result = StatsCommand
            .execute(CommandContext {
                args: "anything".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }
}
