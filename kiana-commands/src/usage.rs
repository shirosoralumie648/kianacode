use crate::local_state::{app_state_array_len, sdk_session_stats};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct UsageCommand;

#[async_trait]
impl Command for UsageCommand {
    fn name(&self) -> &str {
        "usage"
    }

    fn description(&self) -> &str {
        "Show usage statistics"
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

        let sessions = sdk_session_stats();
        Ok(CommandResult::text(format!(
            "Usage\nsdk_sessions: {}\nsdk_messages: {}\nsession_tasks: {}\nteams: {}\nlocal_mcp_invocations: {}",
            sessions.session_count,
            sessions.message_count,
            app_state_array_len(&context.app_state, "tasks"),
            app_state_array_len(&context.app_state, "teams"),
            app_state_array_len(&context.app_state, "mcp_invocations")
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana usage"
}

#[cfg(test)]
mod tests {
    use super::UsageCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn usage_rejects_unknown_args_instead_of_returning_usage() {
        let result = UsageCommand
            .execute(CommandContext {
                args: "details".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }
}
