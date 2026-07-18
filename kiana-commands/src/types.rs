use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandType {
    Local,
    LocalJsx,
    Prompt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub output_type: String,
    pub value: String,
    pub metadata: Option<HashMap<String, String>>,
}

impl CommandResult {
    pub fn text(value: impl Into<String>) -> Self {
        Self {
            output_type: "text".to_string(),
            value: value.into(),
            metadata: None,
        }
    }

    pub fn system(value: impl Into<String>) -> Self {
        let mut result = Self::text(value);
        result.metadata = Some([("display".to_string(), "system".to_string())].into());
        result
    }

    pub fn exit(value: impl Into<String>) -> Self {
        Self {
            output_type: "exit".to_string(),
            value: value.into(),
            metadata: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommandContext {
    pub args: String,
    pub app_state: HashMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommandRoute {
    Local,
    ControlPlane { name: String, arguments: Value },
}

pub const COMMAND_ARGV_APP_STATE_KEY: &str = "__kiana_command_argv";

#[async_trait]
pub trait Command: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn command_type(&self) -> CommandType;
    fn is_enabled(&self) -> bool {
        true
    }
    fn is_hidden(&self) -> bool {
        false
    }
    fn supports_non_interactive(&self) -> bool {
        false
    }
    fn route(&self, _context: &CommandContext) -> anyhow::Result<CommandRoute> {
        Ok(CommandRoute::Local)
    }
    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult>;
}
