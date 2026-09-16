//! Versioned contracts for extension discovery and hook decisions.
//!
//! These values describe an extension snapshot; they do not load a file, execute a hook, or
//! grant a capability.  Trust, scope and effect checks remain owned by ControlPlane/Broker.

use crate::{json_digest, EvidenceStatus, ExtensionId, SourceRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const SKILL_DESCRIPTOR_SCHEMA: &str = "kiana.skill-descriptor.v1";
pub const HOOK_DESCRIPTOR_SCHEMA: &str = "kiana.hook-descriptor.v1";
pub const HOOK_DECISION_SCHEMA: &str = "kiana.hook-decision.v1";
pub const PLUGIN_LIFECYCLE_SCHEMA: &str = "kiana.plugin-lifecycle.v1";
pub const EXTENSION_SNAPSHOT_SCHEMA: &str = "kiana.extension-snapshot.v1";
pub const EXTENSION_ERROR_SCHEMA: &str = "kiana.extension-error.v1";
pub const EXTENSION_SOURCE_RESOLUTION_SCHEMA: &str = "kiana.extension-source-resolution.v1";
pub const PLUGIN_MANIFEST_SCHEMA: &str = "kiana.plugin-manifest.v1";
pub const HOOK_MANIFEST_SCHEMA: &str = "kiana.hook-manifest.v1";

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn hex_digest(value: &str, field: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn bounded_set(values: &BTreeSet<String>, field: &str, max: usize) -> Result<(), String> {
    if values.len() > max
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillLifecycle {
    Discovered,
    Eligible,
    Active,
    Stale,
    Revoked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    Guard,
    Observer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookDecisionKind {
    Allow,
    Block,
    Ask,
    UpdateInput,
    AdditionalContext,
    Timeout,
    Cancelled,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginLifecycleState {
    Inspected,
    Staged,
    Enabled,
    Disabled,
    Revoked,
    RolledBack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionSnapshotState {
    Prepared,
    Used,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionErrorCode {
    InvalidSchema,
    InvalidIdentity,
    UntrustedSource,
    SignatureInvalid,
    ContentHashMismatch,
    DuplicateIdentity,
    DependencyUnsatisfied,
    ScopeDenied,
    SnapshotStale,
    ResourceDenied,
    Unsupported,
    BudgetExceeded,
    Cancelled,
    Unknown,
}

impl ExtensionErrorCode {
    pub const ALL: [Self; 14] = [
        Self::InvalidSchema,
        Self::InvalidIdentity,
        Self::UntrustedSource,
        Self::SignatureInvalid,
        Self::ContentHashMismatch,
        Self::DuplicateIdentity,
        Self::DependencyUnsatisfied,
        Self::ScopeDenied,
        Self::SnapshotStale,
        Self::ResourceDenied,
        Self::Unsupported,
        Self::BudgetExceeded,
        Self::Cancelled,
        Self::Unknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidSchema => "invalid_schema",
            Self::InvalidIdentity => "invalid_identity",
            Self::UntrustedSource => "untrusted_source",
            Self::SignatureInvalid => "signature_invalid",
            Self::ContentHashMismatch => "content_hash_mismatch",
            Self::DuplicateIdentity => "duplicate_identity",
            Self::DependencyUnsatisfied => "dependency_unsatisfied",
            Self::ScopeDenied => "scope_denied",
            Self::SnapshotStale => "snapshot_stale",
            Self::ResourceDenied => "resource_denied",
            Self::Unsupported => "unsupported",
            Self::BudgetExceeded => "budget_exceeded",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDescriptor {
    pub schema: String,
    pub component_id: crate::ComponentId,
    #[serde(default)]
    pub extension_id: Option<ExtensionId>,
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: SourceRef,
    pub content_digest: String,
    pub lifecycle: SkillLifecycle,
    #[serde(default)]
    pub allowed_tools: BTreeSet<String>,
    #[serde(default)]
    pub activation_reason: Option<String>,
    pub snapshot_id: crate::SnapshotId,
}

impl SkillDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SKILL_DESCRIPTOR_SCHEMA {
            return Err("skill_descriptor_schema_invalid".to_owned());
        }
        required(&self.name, "skill_name", 128)?;
        required(&self.version, "skill_version", 64)?;
        required(&self.description, "skill_description", 4_096)?;
        digest(&self.content_digest, "skill_content_digest")?;
        self.source.validate()?;
        bounded_set(&self.allowed_tools, "skill_allowed_tools", 32)?;
        if self
            .activation_reason
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 1_024)
        {
            return Err("skill_activation_reason_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDescriptor {
    pub schema: String,
    pub hook_id: String,
    pub event: String,
    pub matcher: String,
    pub phase: HookPhase,
    pub source: SourceRef,
    pub snapshot_id: crate::SnapshotId,
}

impl HookDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HOOK_DESCRIPTOR_SCHEMA {
            return Err("hook_descriptor_schema_invalid".to_owned());
        }
        required(&self.hook_id, "hook_id", 256)?;
        required(&self.event, "hook_event", 128)?;
        required(&self.matcher, "hook_matcher", 1_024)?;
        self.source.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDecision {
    pub schema: String,
    pub hook_run_id: crate::HookRunId,
    pub hook_id: String,
    pub decision: HookDecisionKind,
    pub phase: HookPhase,
    pub source: SourceRef,
    pub snapshot_id: crate::SnapshotId,
    pub input_digest: String,
    #[serde(default)]
    pub output_digest: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

impl HookDecision {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HOOK_DECISION_SCHEMA {
            return Err("hook_decision_schema_invalid".to_owned());
        }
        required(&self.hook_id, "hook_decision_hook_id", 256)?;
        digest(&self.input_digest, "hook_input_digest")?;
        if self
            .output_digest
            .as_deref()
            .is_some_and(|value| digest(value, "hook_output_digest").is_err())
        {
            return Err("hook_output_digest_invalid".to_owned());
        }
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 2_048)
        {
            return Err("hook_decision_reason_invalid".to_owned());
        }
        self.source.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginLifecycle {
    pub schema: String,
    pub extension_id: ExtensionId,
    pub version: String,
    pub content_hash: String,
    pub state: PluginLifecycleState,
    pub revision: u64,
    #[serde(default)]
    pub reason: Option<String>,
}

impl PluginLifecycle {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PLUGIN_LIFECYCLE_SCHEMA {
            return Err("plugin_lifecycle_schema_invalid".to_owned());
        }
        required(&self.version, "plugin_version", 64)?;
        hex_digest(&self.content_hash, "plugin_content_hash")?;
        if self.revision == 0 {
            return Err("plugin_revision_invalid".to_owned());
        }
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 2_048)
        {
            return Err("plugin_reason_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionError {
    pub schema: String,
    pub code: ExtensionErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default)]
    pub source: Option<SourceRef>,
    #[serde(default)]
    pub snapshot_id: Option<crate::SnapshotId>,
}

impl ExtensionError {
    pub fn new(code: ExtensionErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            schema: EXTENSION_ERROR_SCHEMA.to_owned(),
            code,
            message: message.into(),
            retryable,
            source: None,
            snapshot_id: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_ERROR_SCHEMA {
            return Err("extension_error_schema_invalid".to_owned());
        }
        required(&self.message, "extension_error_message", 2_048)?;
        if let Some(source) = &self.source {
            source.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionSnapshot {
    pub schema: String,
    pub snapshot_id: crate::SnapshotId,
    pub generation: u64,
    pub state: ExtensionSnapshotState,
    pub source_refs: Vec<SourceRef>,
    pub skills: Vec<SkillDescriptor>,
    pub hooks: Vec<HookDescriptor>,
    pub plugins: Vec<PluginLifecycle>,
    pub trust_revision: String,
    pub snapshot_digest: String,
}

impl ExtensionSnapshot {
    pub fn new(
        snapshot_id: crate::SnapshotId,
        generation: u64,
        source_refs: Vec<SourceRef>,
        skills: Vec<SkillDescriptor>,
        hooks: Vec<HookDescriptor>,
        plugins: Vec<PluginLifecycle>,
        trust_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: EXTENSION_SNAPSHOT_SCHEMA.to_owned(),
            snapshot_id,
            generation,
            state: ExtensionSnapshotState::Prepared,
            source_refs,
            skills,
            hooks,
            plugins,
            trust_revision: trust_revision.into(),
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_SNAPSHOT_SCHEMA || self.generation == 0 {
            return Err("extension_snapshot_header_invalid".to_owned());
        }
        digest(&self.trust_revision, "extension_trust_revision")?;
        digest(&self.snapshot_digest, "extension_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("extension_snapshot_digest_mismatch".to_owned());
        }
        if self.source_refs.len() > 256
            || self.skills.len() > 256
            || self.hooks.len() > 256
            || self.plugins.len() > 256
        {
            return Err("extension_snapshot_limit".to_owned());
        }
        let mut sources = BTreeSet::new();
        for source in &self.source_refs {
            source.validate()?;
            if !sources.insert(source.digest()) {
                return Err("extension_snapshot_source_duplicate".to_owned());
            }
        }
        let mut skills = BTreeSet::new();
        for skill in &self.skills {
            skill.validate()?;
            if !skills.insert((skill.name.clone(), skill.version.clone())) {
                return Err("extension_snapshot_skill_duplicate".to_owned());
            }
        }
        let mut hooks = BTreeSet::new();
        for hook in &self.hooks {
            hook.validate()?;
            if !hooks.insert(hook.hook_id.clone()) {
                return Err("extension_snapshot_hook_duplicate".to_owned());
            }
        }
        let mut plugins = BTreeSet::new();
        for plugin in &self.plugins {
            plugin.validate()?;
            if !plugins.insert(plugin.extension_id.to_string()) {
                return Err("extension_snapshot_plugin_duplicate".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("snapshot_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }

    pub fn invalidate(&mut self) -> Result<(), String> {
        self.state = ExtensionSnapshotState::Invalidated;
        self.snapshot_digest = self.digest();
        self.validate()
    }
}

/// A stable trust label for descriptor projections. It intentionally carries no file path.
pub fn extension_trust_label(source: &SourceRef) -> EvidenceStatus {
    source.evidence
}
