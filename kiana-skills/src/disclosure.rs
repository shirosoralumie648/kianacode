//! Progressive disclosure for Skills.
//!
//! Catalog and search return metadata only.  Body and resource reads are explicit, bounded
//! operations.  A budget failure is returned before any truncation so callers cannot mistake a
//! partial body or resource for a complete load.

use crate::source_resolver::{SourceResolveError, SourceResolver, SourceTrust};
use crate::types::{Command, LoadedFrom, SettingSource};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use thiserror::Error;
use tokio::fs;

pub const SKILL_DISCLOSURE_SCHEMA: &str = "kiana.skill-disclosure.v1";
pub const DEFAULT_BODY_MAX_BYTES: usize = 16 * 1024;
pub const DEFAULT_BODY_MAX_TOKENS: usize = 4 * 1024;
pub const DEFAULT_RESOURCE_MAX_BYTES: usize = 256 * 1024;
pub const DEFAULT_RESOURCE_MAX_TOKENS: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisclosureBudget {
    pub max_bytes: usize,
    pub max_tokens: usize,
}

impl DisclosureBudget {
    pub const fn body_default() -> Self {
        Self {
            max_bytes: DEFAULT_BODY_MAX_BYTES,
            max_tokens: DEFAULT_BODY_MAX_TOKENS,
        }
    }

    pub const fn resource_default() -> Self {
        Self {
            max_bytes: DEFAULT_RESOURCE_MAX_BYTES,
            max_tokens: DEFAULT_RESOURCE_MAX_TOKENS,
        }
    }

    fn validate(&self) -> Result<(), DisclosureError> {
        if self.max_bytes == 0 || self.max_tokens == 0 {
            return Err(DisclosureError::BudgetInvalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillDisclosureStatus {
    Eligible,
    Disabled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSourceSummary {
    pub source: String,
    pub trust: SourceTrust,
    pub setting_source: SettingSource,
    pub loaded_from: LoadedFrom,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillCatalogEntry {
    pub schema: String,
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub source: SkillSourceSummary,
    pub status: SkillDisclosureStatus,
    pub package_hash: String,
    pub content_digest: String,
    pub user_invocable: bool,
    pub disable_model_invocation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillCatalogResponse {
    pub schema: String,
    pub query: String,
    pub entries: Vec<SkillCatalogEntry>,
    pub total_matches: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisclosureQuota {
    pub max_bytes: usize,
    pub max_tokens: usize,
    pub used_bytes: usize,
    pub estimated_tokens: usize,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillBody {
    pub schema: String,
    pub name: String,
    pub package_hash: String,
    pub content_digest: String,
    pub body: String,
    pub quota: DisclosureQuota,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillResource {
    pub schema: String,
    pub package_hash: String,
    pub relative_path: String,
    pub content_digest: String,
    pub content: Vec<u8>,
    pub quota: DisclosureQuota,
}

#[derive(Debug, Error)]
pub enum DisclosureError {
    #[error("budget_invalid")]
    BudgetInvalid,
    #[error(
        "over_budget:{kind}:bytes={used_bytes}:max_bytes={max_bytes}:tokens={estimated_tokens}:max_tokens={max_tokens}"
    )]
    OverBudget {
        kind: &'static str,
        used_bytes: usize,
        max_bytes: usize,
        estimated_tokens: usize,
        max_tokens: usize,
    },
    #[error("skill_root_missing")]
    SkillRootMissing,
    #[error("resource_invalid:{0}")]
    ResourceInvalid(String),
    #[error("resource_io:{0}")]
    Io(#[from] std::io::Error),
}

pub fn list_skill_catalog(skills: &[Command]) -> SkillCatalogResponse {
    search_skill_catalog(skills, "", usize::MAX)
}

pub fn search_skill_catalog(
    skills: &[Command],
    query: &str,
    max_results: usize,
) -> SkillCatalogResponse {
    let query = query.trim().to_ascii_lowercase();
    let mut entries = skills
        .iter()
        .filter(|skill| query.is_empty() || matches_skill_query(skill, &query))
        .map(skill_catalog_entry)
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.source.source.cmp(&right.source.source))
    });
    let total_matches = entries.len();
    entries.truncate(max_results);
    SkillCatalogResponse {
        schema: SKILL_DISCLOSURE_SCHEMA.to_owned(),
        query: query.to_owned(),
        entries,
        total_matches,
    }
}

pub fn load_skill_body(
    skill: &Command,
    budget: &DisclosureBudget,
) -> Result<SkillBody, DisclosureError> {
    budget.validate()?;
    let used_bytes = skill.content.as_bytes().len();
    let estimated_tokens = estimate_tokens(used_bytes);
    if used_bytes > budget.max_bytes || estimated_tokens > budget.max_tokens {
        return Err(DisclosureError::OverBudget {
            kind: "skill_body",
            used_bytes,
            max_bytes: budget.max_bytes,
            estimated_tokens,
            max_tokens: budget.max_tokens,
        });
    }
    Ok(SkillBody {
        schema: SKILL_DISCLOSURE_SCHEMA.to_owned(),
        name: skill.name.clone(),
        package_hash: package_hash(skill),
        content_digest: skill_content_digest(skill),
        body: skill.content.clone(),
        quota: DisclosureQuota {
            max_bytes: budget.max_bytes,
            max_tokens: budget.max_tokens,
            used_bytes,
            estimated_tokens,
            complete: true,
        },
    })
}

pub async fn read_skill_resource(
    skill: &Command,
    relative_path: &str,
    budget: &DisclosureBudget,
) -> Result<SkillResource, DisclosureError> {
    budget.validate()?;
    let root = skill
        .skill_root
        .as_ref()
        .ok_or(DisclosureError::SkillRootMissing)?;
    let path = SourceResolver::resolve_resource(root, relative_path).map_err(map_resource_error)?;
    let metadata = fs::metadata(&path).await?;
    if !metadata.is_file() {
        return Err(DisclosureError::ResourceInvalid(
            "resource_not_regular_file".to_owned(),
        ));
    }
    let used_bytes = usize::try_from(metadata.len())
        .map_err(|_| DisclosureError::ResourceInvalid("resource_size_overflow".to_owned()))?;
    let estimated_tokens = estimate_tokens(used_bytes);
    if used_bytes > budget.max_bytes || estimated_tokens > budget.max_tokens {
        return Err(DisclosureError::OverBudget {
            kind: "skill_resource",
            used_bytes,
            max_bytes: budget.max_bytes,
            estimated_tokens,
            max_tokens: budget.max_tokens,
        });
    }
    let content = fs::read(&path).await?;
    let actual_bytes = content.len();
    let actual_tokens = estimate_tokens(actual_bytes);
    if actual_bytes > budget.max_bytes || actual_tokens > budget.max_tokens {
        return Err(DisclosureError::OverBudget {
            kind: "skill_resource",
            used_bytes: actual_bytes,
            max_bytes: budget.max_bytes,
            estimated_tokens: actual_tokens,
            max_tokens: budget.max_tokens,
        });
    }
    Ok(SkillResource {
        schema: SKILL_DISCLOSURE_SCHEMA.to_owned(),
        package_hash: package_hash(skill),
        relative_path: normalize_relative_path(relative_path),
        content_digest: json_digest(&json!({ "bytes": content })),
        content,
        quota: DisclosureQuota {
            max_bytes: budget.max_bytes,
            max_tokens: budget.max_tokens,
            used_bytes: actual_bytes,
            estimated_tokens: actual_tokens,
            complete: true,
        },
    })
}

fn matches_skill_query(skill: &Command, query: &str) -> bool {
    [
        skill.name.as_str(),
        skill.display_name.as_deref().unwrap_or_default(),
        skill.description.as_str(),
    ]
    .into_iter()
    .any(|value| value.to_ascii_lowercase().contains(query))
}

fn skill_catalog_entry(skill: &Command) -> SkillCatalogEntry {
    SkillCatalogEntry {
        schema: SKILL_DISCLOSURE_SCHEMA.to_owned(),
        name: skill.name.clone(),
        display_name: skill.display_name.clone(),
        description: skill.description.clone(),
        source: SkillSourceSummary {
            source: source_id(skill),
            trust: SourceTrust::Trusted,
            setting_source: skill.source,
            loaded_from: skill.loaded_from,
        },
        status: if skill.disable_model_invocation {
            SkillDisclosureStatus::Disabled
        } else {
            SkillDisclosureStatus::Eligible
        },
        package_hash: package_hash(skill),
        content_digest: skill_content_digest(skill),
        user_invocable: skill.user_invocable,
        disable_model_invocation: skill.disable_model_invocation,
    }
}

fn source_id(skill: &Command) -> String {
    format!(
        "skill-source:{}:{}",
        loaded_from_label(skill.loaded_from),
        skill
            .skill_root
            .as_ref()
            .map(|path| json_digest(&json!({ "root": path.to_string_lossy() })))
            .unwrap_or_else(|| "sha256:unknown".to_owned())
    )
}

fn package_hash(skill: &Command) -> String {
    json_digest(&json!({
        "source": source_id(skill),
        "name": skill.name,
        "content_digest": skill_content_digest(skill),
    }))
}

fn skill_content_digest(skill: &Command) -> String {
    json_digest(&json!({ "content": skill.content }))
}

fn loaded_from_label(loaded_from: LoadedFrom) -> &'static str {
    match loaded_from {
        LoadedFrom::CommandsDeprecated => "legacy",
        LoadedFrom::Skills => "skills",
        LoadedFrom::Plugin => "plugin",
        LoadedFrom::Managed => "managed",
        LoadedFrom::Bundled => "bundled",
        LoadedFrom::Mcp => "mcp",
    }
}

fn estimate_tokens(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

fn normalize_relative_path(relative_path: &str) -> String {
    Path::new(relative_path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn map_resource_error(error: SourceResolveError) -> DisclosureError {
    DisclosureError::ResourceInvalid(error.to_string())
}
