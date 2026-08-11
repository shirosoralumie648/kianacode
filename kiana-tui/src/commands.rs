use serde_json::{json, Value};

/// Represents a slash command in the TUI
#[derive(Clone, Debug)]
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub acp_command: &'static str,
}

impl SlashCommand {
    /// Check if a query matches this command (for filtering)
    pub fn matches(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.name.to_lowercase().contains(&query_lower)
            || self.description.to_lowercase().contains(&query_lower)
    }

    /// Build ACP command arguments for this slash command
    pub fn build_arguments(&self, user_input: &str) -> Value {
        match self.name {
            "review" => json!({
                "action": "review_code",
                "content": user_input
            }),
            "fix" => json!({
                "action": "fix_issue",
                "issue": user_input
            }),
            "test" => json!({
                "action": "generate_tests",
                "target": user_input
            }),
            "explain" => json!({
                "action": "explain_code",
                "query": user_input
            }),
            "commit" => json!({
                "action": "generate_commit_message",
                "changes": user_input
            }),
            "config" => json!({
                "action": "open_config"
            }),
            "help" => json!({
                "action": "show_help"
            }),
            "clear" => json!({
                "action": "clear_messages"
            }),
            "refactor" => json!({
                "action": "refactor_code",
                "target": user_input
            }),
            "docs" => json!({
                "action": "generate_docs",
                "target": user_input
            }),
            _ => json!({
                "query": user_input
            }),
        }
    }
}

/// Register all available slash commands
pub fn register_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand {
            name: "help",
            description: "Show available commands",
            acp_command: "system.help.v1",
        },
        SlashCommand {
            name: "review",
            description: "Code review",
            acp_command: "eda.review.v1",
        },
        SlashCommand {
            name: "fix",
            description: "Fix issue",
            acp_command: "eda.fix.v1",
        },
        SlashCommand {
            name: "test",
            description: "Generate tests",
            acp_command: "eda.test.v1",
        },
        SlashCommand {
            name: "explain",
            description: "Explain code",
            acp_command: "eda.explain.v1",
        },
        SlashCommand {
            name: "commit",
            description: "Generate commit message",
            acp_command: "eda.commit.v1",
        },
        SlashCommand {
            name: "config",
            description: "Open config",
            acp_command: "system.config.v1",
        },
        SlashCommand {
            name: "clear",
            description: "Clear conversation",
            acp_command: "system.clear.v1",
        },
        SlashCommand {
            name: "refactor",
            description: "Refactor code",
            acp_command: "eda.refactor.v1",
        },
        SlashCommand {
            name: "docs",
            description: "Generate documentation",
            acp_command: "eda.docs.v1",
        },
    ]
}

/// Find a command by name from the registered commands
pub fn find_command(name: &str) -> Option<SlashCommand> {
    register_commands()
        .into_iter()
        .find(|cmd| cmd.name == name)
}
