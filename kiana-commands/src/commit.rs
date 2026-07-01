use crate::types::{Command, CommandContext, CommandResult, CommandType};
use async_trait::async_trait;

pub struct CommitCommand;

#[async_trait]
impl Command for CommitCommand {
    fn name(&self) -> &str {
        "commit"
    }

    fn description(&self) -> &str {
        "Create a git commit"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Prompt
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        if matches!(context.args.trim(), "help" | "--help" | "-h") {
            return Ok(CommandResult::text(usage()));
        }

        let prompt = r#"## Context

- Current git status: !`git status`
- Current git diff (staged and unstaged changes): !`git diff HEAD`
- Current branch: !`git branch --show-current`
- Recent commits: !`git log --oneline -10`

## Git Safety Protocol

- NEVER update the git config
- NEVER skip hooks (--no-verify, --no-gpg-sign, etc) unless the user explicitly requests it
- CRITICAL: ALWAYS create NEW commits. NEVER use git commit --amend, unless the user explicitly requests it
- Do not commit files that likely contain secrets (.env, credentials.json, etc). Warn the user if they specifically request to commit those files
- If there are no changes to commit (i.e., no untracked files and no modifications), do not create an empty commit
- Never use git commands with the -i flag (like git rebase -i or git add -i) since they require interactive input which is not supported

## Your task

Based on the above changes, create a single git commit:

1. Analyze all staged changes and draft a commit message:
   - Look at the recent commits above to follow this repository's commit message style
   - Summarize the nature of the changes (new feature, enhancement, bug fix, refactoring, test, docs, etc.)
   - Ensure the message accurately reflects the changes and their purpose (i.e. "add" means a wholly new feature, "update" means an enhancement to an existing feature, "fix" means a bug fix, etc.)
   - Draft a concise (1-2 sentences) commit message that focuses on the "why" rather than the "what"

2. Stage relevant files and create the commit using HEREDOC syntax:
```
git commit -m "$(cat <<'EOF'
Commit message here.
EOF
)"
```

You have the capability to call multiple tools in a single response. Stage and create the commit using a single message. Do not use any other tools or do anything else. Do not send any other text or messages besides these tool calls."#;

        Ok(CommandResult::text(append_user_arguments(
            prompt,
            &context.args,
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana commit [instructions]\n       Run /commit from the REPL to let the agent create a git commit."
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
    use super::CommitCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn commit_preserves_user_arguments_in_generated_prompt() {
        let result = CommitCommand
            .execute(CommandContext {
                args: "only commit staged docs".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Additional user instructions"));
        assert!(result.value.contains("only commit staged docs"));
    }

    #[tokio::test]
    async fn commit_help_reports_usage_instead_of_generated_prompt() {
        let result = CommitCommand
            .execute(CommandContext {
                args: "--help".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Usage: kiana commit"));
        assert!(!result.value.contains("Current git status"));
    }
}
