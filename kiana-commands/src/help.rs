use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct HelpCommand;

#[async_trait]
impl Command for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }

    fn description(&self) -> &str {
        "Display available commands"
    }

    fn command_type(&self) -> CommandType {
        CommandType::LocalJsx
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        match context.args.trim() {
            "" => {}
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }

        Ok(CommandResult::text(help_text()))
    }
}

fn usage() -> &'static str {
    "Usage: kiana help"
}

fn help_text() -> String {
    [
        "Kiana local commands",
        "",
        "Core: /help, /status, /doctor, /version, /clear, /exit",
        "Sessions: /session, /compact, /export",
        "Configuration: /config, /model, /theme, /vim, /brief, /output-style, /permissions",
        "Extensions: /plugin, /reload-plugins, /skills, /mcp, /hooks",
        "Workflow: /tasks, /memory, /context, /diff, /cost, /stats, /usage, /release",
        "Prompt commands: /init and /commit expand into model prompts.",
        "",
        "For full CLI, remote, bridge, TUI, MCP server, and native-host usage, run: kiana --help",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::HelpCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn help_rejects_unknown_args_instead_of_returning_general_help() {
        let result = HelpCommand
            .execute(CommandContext {
                args: "anything".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn slash_help_lists_real_operational_command_groups() {
        let result = HelpCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        for expected in [
            "/session",
            "/compact",
            "/export",
            "/permissions",
            "/plugin",
            "/skills",
            "/mcp",
            "/hooks",
            "/release",
            "kiana --help",
        ] {
            assert!(
                result.value.contains(expected),
                "slash help did not mention {expected}; output was:\n{}",
                result.value
            );
        }
    }
}
