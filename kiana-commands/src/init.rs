use crate::types::{Command, CommandContext, CommandResult, CommandType};
use async_trait::async_trait;

pub struct InitCommand;

#[async_trait]
impl Command for InitCommand {
    fn name(&self) -> &str {
        "init"
    }

    fn description(&self) -> &str {
        "Initialize a new CLAUDE.md file with codebase documentation"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Prompt
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        if matches!(context.args.trim(), "help" | "--help" | "-h") {
            return Ok(CommandResult::text(usage()));
        }

        let prompt = r#"Please analyze this codebase and create a CLAUDE.md file, which will be given to future instances of Claude Code to operate in this repository.

What to add:
1. Commands that will be commonly used, such as how to build, lint, and run tests. Include the necessary commands to develop in this codebase, such as how to run a single test.
2. High-level code architecture and structure so that future instances can be productive more quickly. Focus on the "big picture" architecture that requires reading multiple files to understand.

Usage notes:
- If there's already a CLAUDE.md, suggest improvements to it.
- When you make the initial CLAUDE.md, do not repeat yourself and do not include obvious instructions like "Provide helpful error messages to users", "Write unit tests for all new utilities", "Never include sensitive information (API keys, tokens) in code or commits".
- Avoid listing every component or file structure that can be easily discovered.
- Don't include generic development practices.
- If there are Cursor rules (in .cursor/rules/ or .cursorrules) or Copilot rules (in .github/copilot-instructions.md), make sure to include the important parts.
- If there is a README.md, make sure to include the important parts.
- Do not make up information such as "Common Development Tasks", "Tips for Development", "Support and Documentation" unless this is expressly included in other files that you read.
- Be sure to prefix the file with the following text:

```
# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
```"#;

        Ok(CommandResult::text(append_user_arguments(
            prompt,
            &context.args,
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana init [instructions]\n       Run /init from the REPL to let the agent create or update CLAUDE.md."
}

fn append_user_arguments(prompt: &str, args: &str) -> String {
    let args = args.trim();
    if args.is_empty() {
        return prompt.to_string();
    }

    format!("{prompt}\n\n## Additional user instructions\n\n{args}")
}

#[cfg(test)]
mod tests {
    use super::InitCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn init_preserves_user_arguments_in_generated_prompt() {
        let result = InitCommand
            .execute(CommandContext {
                args: "focus on the release workflow".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Additional user instructions"));
        assert!(result.value.contains("focus on the release workflow"));
    }

    #[tokio::test]
    async fn init_help_reports_usage_instead_of_generated_prompt() {
        let result = InitCommand
            .execute(CommandContext {
                args: "--help".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Usage: kiana init"));
        assert!(!result.value.contains("Please analyze this codebase"));
    }
}
