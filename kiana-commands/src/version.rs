use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct VersionCommand;

#[async_trait]
impl Command for VersionCommand {
    fn name(&self) -> &str {
        "version"
    }

    fn description(&self) -> &str {
        "Print the version this session is running"
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

        Ok(CommandResult::text(env!("CARGO_PKG_VERSION")))
    }
}

fn usage() -> &'static str {
    "Usage: kiana version"
}

#[cfg(test)]
mod tests {
    use super::VersionCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn version_rejects_unknown_args_instead_of_printing_version() {
        let result = VersionCommand
            .execute(CommandContext {
                args: "anything".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }
}
