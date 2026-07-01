use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoadedFrom {
    #[serde(rename = "commands_DEPRECATED")]
    CommandsDeprecated,
    Skills,
    Plugin,
    Managed,
    Bundled,
    Mcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingSource {
    #[serde(rename = "policySettings")]
    PolicySettings,
    #[serde(rename = "userSettings")]
    UserSettings,
    #[serde(rename = "projectSettings")]
    ProjectSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionContext {
    Inline,
    Fork,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub when_to_use: Option<String>,
    pub argument_hint: Option<String>,
    pub allowed_tools: Vec<String>,
    pub model: Option<String>,
    pub disable_model_invocation: bool,
    pub user_invocable: bool,
    pub source: SettingSource,
    pub loaded_from: LoadedFrom,
    pub skill_root: Option<PathBuf>,
    pub context: Option<ExecutionContext>,
    pub paths: Option<Vec<String>>,
    pub content: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub when_to_use: Option<String>,
    #[serde(rename = "argument-hint")]
    pub argument_hint: Option<String>,
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    #[serde(rename = "disable-model-invocation")]
    pub disable_model_invocation: Option<bool>,
    #[serde(rename = "user-invocable")]
    pub user_invocable: Option<bool>,
    pub context: Option<String>,
    pub paths: Option<String>,
    pub version: Option<String>,
}

impl Frontmatter {
    pub fn parse_allowed_tools(&self) -> Vec<String> {
        self.allowed_tools.clone().unwrap_or_default()
    }

    pub fn parse_context(&self) -> Option<ExecutionContext> {
        match self.context.as_deref() {
            Some("fork") => Some(ExecutionContext::Fork),
            Some("inline") => Some(ExecutionContext::Inline),
            _ => None,
        }
    }

    pub fn parse_paths(&self) -> Option<Vec<String>> {
        self.paths.as_ref().map(|p| {
            p.split('\n')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches("/**").to_string())
                .filter(|s| !s.is_empty() && s != "**")
                .collect()
        })
    }
}
