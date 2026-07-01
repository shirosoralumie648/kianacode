use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct ExitCommand;

#[async_trait]
impl Command for ExitCommand {
    fn name(&self) -> &str {
        "exit"
    }

    fn description(&self) -> &str {
        "Exit the session"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        match context.args.trim() {
            "" => {}
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }

        Ok(CommandResult::exit("Goodbye!"))
    }
}

fn usage() -> &'static str {
    "Usage: kiana exit"
}

#[cfg(test)]
mod tests {
    use super::ExitCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn exit_rejects_unknown_args_without_exiting() {
        let result = ExitCommand
            .execute(CommandContext {
                args: "anything".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn exit_returns_control_result_without_terminating_process() {
        let result = ExitCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert_eq!(result.output_type, "exit");
        assert_eq!(result.value, "Goodbye!");
    }
}
