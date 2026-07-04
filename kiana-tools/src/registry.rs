use crate::tool::Tool;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn list_tools(&self) -> Vec<&Arc<dyn Tool>> {
        self.tools.values().collect()
    }

    pub fn get_schemas(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|tool| {
                let mut schema = serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.input_schema()
                });
                if let Some(workbench) = tool.workbench() {
                    schema["workbench"] = serde_json::json!(workbench);
                }
                schema
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    registry.register(Arc::new(crate::file_read::FileReadTool::new()));
    registry.register(Arc::new(crate::file_write::FileWriteTool::new()));
    registry.register(Arc::new(crate::file_edit::FileEditTool::new()));
    registry.register(Arc::new(crate::file_delete::FileDeleteTool::new()));
    registry.register(Arc::new(crate::grep::GrepTool::new()));
    registry.register(Arc::new(crate::glob::GlobTool::new()));
    registry.register(Arc::new(crate::agent::AgentTool::new()));
    registry.register(Arc::new(crate::repl_tool::ReplTool::new()));
    registry.register(Arc::new(crate::bash_tool::BashTool::new()));
    registry.register(Arc::new(crate::powershell_tool::PowerShellTool::new()));
    registry.register(Arc::new(crate::todo_write::TodoWriteTool::new()));
    registry.register(Arc::new(crate::web_fetch::WebFetchTool::new()));
    registry.register(Arc::new(crate::web_search::WebSearchTool::new()));
    registry.register(Arc::new(crate::remote_trigger::RemoteTriggerTool::new()));
    registry.register(Arc::new(crate::user_interaction::AskUserQuestionTool::new()));
    registry.register(Arc::new(crate::user_interaction::SendUserMessageTool::new()));
    registry.register(Arc::new(crate::user_interaction::SendUserFileTool::new()));
    registry.register(Arc::new(crate::user_interaction::SleepTool::new()));
    registry.register(Arc::new(crate::mcp_tool::McpTool::new()));
    registry.register(Arc::new(crate::mcp_tool::ListMcpResourcesTool::new()));
    registry.register(Arc::new(
        crate::mcp_tool::ListMcpResourceTemplatesTool::new(),
    ));
    registry.register(Arc::new(crate::mcp_tool::ListMcpPromptsTool::new()));
    registry.register(Arc::new(crate::mcp_tool::ReadMcpResourceTool::new()));
    registry.register(Arc::new(crate::mcp_tool::GetMcpPromptTool::new()));
    registry.register(Arc::new(crate::synthetic_output::SyntheticOutputTool::new()));
    registry.register(Arc::new(crate::notebook_edit::NotebookEditTool::new()));
    registry.register(Arc::new(crate::lsp_tool::LspTool::new()));
    registry.register(Arc::new(crate::team_create::TeamCreateTool::new()));
    registry.register(Arc::new(crate::team_create::TeamDeleteTool::new()));
    registry.register(Arc::new(crate::team_create::SendMessageTool::new()));
    registry.register(Arc::new(crate::team_create::ConfigTool::new()));
    registry.register(Arc::new(crate::team_create::WorkflowTool::new()));
    registry.register(Arc::new(crate::team_create::EnterPlanModeTool::new()));
    registry.register(Arc::new(crate::team_create::ExitPlanModeTool::new()));
    registry.register(Arc::new(crate::team_create::EnterWorktreeTool::new()));
    registry.register(Arc::new(crate::team_create::ExitWorktreeTool::new()));
    registry.register(Arc::new(crate::team_create::SkillTool::new()));
    registry.register(Arc::new(crate::discover_skills::DiscoverSkillsTool::new()));
    registry.register(Arc::new(crate::team_create::ToolSearchTool::new()));
    registry.register(Arc::new(crate::team_create::CronCreateTool::new()));
    registry.register(Arc::new(crate::team_create::CronDeleteTool::new()));
    registry.register(Arc::new(crate::team_create::CronListTool::new()));
    registry.register(Arc::new(crate::team_create::MonitorTool::new()));
    registry.register(Arc::new(crate::task_create::TaskCreateTool::new()));
    registry.register(Arc::new(crate::task_create::TaskGetTool::new()));
    registry.register(Arc::new(crate::task_create::TaskListTool::new()));
    registry.register(Arc::new(crate::task_output::TaskOutputTool::new()));
    registry.register(Arc::new(crate::task_create::TaskStopTool::new()));
    registry.register(Arc::new(crate::task_create::TaskUpdateTool::new()));

    registry
}

#[cfg(test)]
mod tests {
    use super::create_default_registry;

    #[test]
    fn default_registry_exposes_delegation_and_task_tools() {
        let registry = create_default_registry();

        for tool_name in [
            "Agent",
            "REPL",
            "TaskCreate",
            "TaskGet",
            "TaskList",
            "TaskOutput",
            "TaskStop",
            "TaskUpdate",
            "PowerShell",
            "WebFetch",
            "WebSearch",
            "RemoteTrigger",
            "AskUserQuestion",
            "SendUserMessage",
            "send_user_file",
            "Sleep",
            "MCP",
            "ListMcpResourcesTool",
            "ListMcpResourceTemplatesTool",
            "ListMcpPromptsTool",
            "ReadMcpResourceTool",
            "GetMcpPromptTool",
            "Delete",
            "StructuredOutput",
            "NotebookEdit",
            "LSP",
            "TeamCreate",
            "TeamDelete",
            "SendMessage",
            "Config",
            "Workflow",
            "EnterPlanMode",
            "ExitPlanMode",
            "EnterWorktree",
            "ExitWorktree",
            "Skill",
            "discover_skills",
            "ToolSearch",
            "CronCreate",
            "CronDelete",
            "CronList",
            "Monitor",
        ] {
            assert!(
                registry.get(tool_name).is_some(),
                "{tool_name} is not registered"
            );
        }
    }

    #[test]
    fn default_registry_schemas_avoid_top_level_combinators() {
        let registry = create_default_registry();

        for schema in registry.get_schemas() {
            let name = schema["name"].as_str().unwrap_or("<unnamed>");
            let input_schema = &schema["input_schema"];
            assert!(
                input_schema.get("anyOf").is_none(),
                "{name} uses top-level anyOf"
            );
            assert!(
                input_schema.get("oneOf").is_none(),
                "{name} uses top-level oneOf"
            );
            assert!(
                input_schema.get("allOf").is_none(),
                "{name} uses top-level allOf"
            );
        }
    }

    #[test]
    fn default_registry_marks_core_read_tools_as_concurrent_read_only() {
        let registry = create_default_registry();

        for tool_name in ["Read", "Grep", "Glob"] {
            let tool = registry
                .get(tool_name)
                .unwrap_or_else(|| panic!("{tool_name} is not registered"));
            assert!(tool.is_read_only(), "{tool_name} should be read-only");
            assert!(
                tool.is_concurrency_safe(),
                "{tool_name} should be concurrency-safe"
            );
        }
    }

    #[test]
    fn default_registry_exposes_lifecycle_workbench_metadata() {
        let registry = create_default_registry();
        let schemas = registry.get_schemas();

        let find_schema = |name: &str| {
            schemas
                .iter()
                .find(|schema| schema["name"] == name)
                .unwrap_or_else(|| panic!("{name} schema is missing"))
        };

        assert_eq!(find_schema("MCP")["workbench"], "mcp");
        assert_eq!(find_schema("ListMcpResourcesTool")["workbench"], "mcp");
        assert_eq!(
            find_schema("ListMcpResourceTemplatesTool")["workbench"],
            "mcp"
        );
        assert_eq!(find_schema("ListMcpPromptsTool")["workbench"], "mcp");
        assert_eq!(find_schema("ReadMcpResourceTool")["workbench"], "mcp");
        assert_eq!(find_schema("GetMcpPromptTool")["workbench"], "mcp");
        assert_eq!(find_schema("NotebookEdit")["workbench"], "notebook");
        assert!(find_schema("Read").get("workbench").is_none());
    }
}
