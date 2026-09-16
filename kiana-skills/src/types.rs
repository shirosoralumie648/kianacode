use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::collections::BTreeMap;
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
#[serde(deny_unknown_fields)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub when_to_use: Option<String>,
    #[serde(rename = "argument-hint")]
    pub argument_hint: Option<String>,
    #[serde(
        default,
        rename = "allowed-tools",
        deserialize_with = "deserialize_allowed_tools"
    )]
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    #[serde(rename = "disable-model-invocation")]
    pub disable_model_invocation: Option<bool>,
    #[serde(rename = "user-invocable")]
    pub user_invocable: Option<bool>,
    pub context: Option<String>,
    pub paths: Option<String>,
    pub version: Option<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: Option<BTreeMap<String, YamlValue>>,
    /// Legacy Claude-compatible aliases accepted by the explicit adapter path.
    #[serde(default)]
    pub triggers: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_allowed_tools")]
    pub tools: Option<Vec<String>>,
}

fn deserialize_allowed_tools<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<YamlValue>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(YamlValue::String(value)) => Ok(Some(
            value
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )),
        Some(YamlValue::Sequence(values)) => values
            .into_iter()
            .map(|value| match value {
                YamlValue::String(value) => Ok(value),
                _ => Err(serde::de::Error::custom(
                    "allowed-tools must contain strings",
                )),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(serde::de::Error::custom(
            "allowed-tools must be a string or sequence",
        )),
    }
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
