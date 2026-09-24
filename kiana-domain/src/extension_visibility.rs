//! Server-owned, redacted extension visibility projections.
//!
//! A visibility snapshot is a read model only.  It deliberately contains metadata that a
//! surface may render (identity, status, source trust, risk and allowed read/action labels),
//! while omitting skill bodies, package paths, secrets and internal command/entrypoint data.
//! The snapshot carries its generation and digest so a client cannot reuse a stale projection
//! as an authority.  Any action derived from this module is an intent; ControlPlane/Broker must
//! re-check the current registry before it can have an effect.

use crate::{json_digest, ExtensionSnapshot, ExtensionSnapshotState, SnapshotId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const EXTENSION_VISIBILITY_SCHEMA: &str = "kiana.extension-visibility.v1";
pub const EXTENSION_VISIBILITY_ENTRY_SCHEMA: &str = "kiana.extension-visibility-entry.v1";
pub const EXTENSION_VISIBILITY_ACTION_SCHEMA: &str = "kiana.extension-visibility-action.v1";
pub const MAX_EXTENSION_VISIBILITY_ENTRIES: usize = 512;
pub const MAX_EXTENSION_VISIBILITY_QUERY_BYTES: usize = 256;
pub const MAX_EXTENSION_VISIBILITY_SUMMARY_BYTES: usize = 2_048;

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

fn safe_public_identifier(value: &str, field: &str, max: usize) -> Result<(), String> {
    required(value, field, max)?;
    // A projection must never turn a filesystem locator or an internal command into a public
    // identifier.  The daemon should provide a digest-derived source/extension id instead.
    if value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(format!("{field}_path_like"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionVisibilityKind {
    Skill,
    Hook,
    Plugin,
    Mcp,
    Capability,
    Workflow,
    Memory,
    Provider,
    Ui,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionVisibilityStatus {
    Discovered,
    Eligible,
    Active,
    Disabled,
    Stale,
    Revoked,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionVisibilityTrust {
    Trusted,
    Untrusted,
    Unknown,
    Denied,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionVisibilityRisk {
    ReadOnly,
    WorkspaceWrite,
    Network,
    Secret,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionVisibilityActionKind {
    List,
    Search,
    Inspect,
    Activate,
    Revoke,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionVisibilitySource {
    pub source_id: String,
    pub trust: ExtensionVisibilityTrust,
    pub content_digest: String,
    pub revision: String,
}

impl ExtensionVisibilitySource {
    pub fn new(
        source_id: impl Into<String>,
        trust: ExtensionVisibilityTrust,
        content_digest: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, String> {
        let source = Self {
            source_id: source_id.into(),
            trust,
            content_digest: content_digest.into(),
            revision: revision.into(),
        };
        source.validate()?;
        Ok(source)
    }

    pub fn validate(&self) -> Result<(), String> {
        safe_public_identifier(&self.source_id, "visibility_source_id", 256)?;
        digest(&self.content_digest, "visibility_source_content_digest")?;
        safe_public_identifier(&self.revision, "visibility_source_revision", 256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionVisibilityEntry {
    pub schema: String,
    pub extension_id: String,
    pub component_id: String,
    pub kind: ExtensionVisibilityKind,
    pub version: String,
    pub summary: String,
    pub status: ExtensionVisibilityStatus,
    pub source: ExtensionVisibilitySource,
    pub risk: ExtensionVisibilityRisk,
    #[serde(default)]
    pub package_digest: Option<String>,
    #[serde(default)]
    pub actions: BTreeSet<ExtensionVisibilityActionKind>,
}

impl ExtensionVisibilityEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        extension_id: impl Into<String>,
        component_id: impl Into<String>,
        kind: ExtensionVisibilityKind,
        version: impl Into<String>,
        summary: impl Into<String>,
        status: ExtensionVisibilityStatus,
        source: ExtensionVisibilitySource,
        risk: ExtensionVisibilityRisk,
        package_digest: Option<String>,
        actions: BTreeSet<ExtensionVisibilityActionKind>,
    ) -> Result<Self, String> {
        let entry = Self {
            schema: EXTENSION_VISIBILITY_ENTRY_SCHEMA.to_owned(),
            extension_id: extension_id.into(),
            component_id: component_id.into(),
            kind,
            version: version.into(),
            summary: summary.into(),
            status,
            source,
            risk,
            package_digest,
            actions,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_VISIBILITY_ENTRY_SCHEMA {
            return Err("extension_visibility_entry_schema_invalid".to_owned());
        }
        safe_public_identifier(&self.extension_id, "visibility_extension_id", 256)?;
        safe_public_identifier(&self.component_id, "visibility_component_id", 256)?;
        safe_public_identifier(&self.version, "visibility_version", 128)?;
        required(
            &self.summary,
            "visibility_summary",
            MAX_EXTENSION_VISIBILITY_SUMMARY_BYTES,
        )?;
        if self.summary.contains('\n') || self.summary.contains('\r') {
            return Err("visibility_summary_multiline".to_owned());
        }
        self.source.validate()?;
        if let Some(package_digest) = &self.package_digest {
            digest(package_digest, "visibility_package_digest")?;
        }
        if self.actions.len() > 8 {
            return Err("extension_visibility_actions_limit".to_owned());
        }
        if self.source.trust != ExtensionVisibilityTrust::Trusted
            && !matches!(
                self.status,
                ExtensionVisibilityStatus::Revoked
                    | ExtensionVisibilityStatus::Stale
                    | ExtensionVisibilityStatus::Unsupported
            )
        {
            return Err("extension_visibility_untrusted_entry".to_owned());
        }
        if self.status == ExtensionVisibilityStatus::Unsupported
            && self.actions.contains(&ExtensionVisibilityActionKind::Activate)
        {
            return Err("extension_visibility_unsupported_action".to_owned());
        }
        Ok(())
    }

    pub fn matches_query(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        query.is_empty()
            || self.extension_id.to_ascii_lowercase().contains(&query)
            || self.component_id.to_ascii_lowercase().contains(&query)
            || self.summary.to_ascii_lowercase().contains(&query)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionVisibilitySnapshot {
    pub schema: String,
    pub snapshot_id: SnapshotId,
    pub generation: u64,
    pub state: ExtensionSnapshotState,
    pub query: String,
    pub source_snapshot_digest: String,
    pub entries: Vec<ExtensionVisibilityEntry>,
    pub snapshot_digest: String,
}

impl ExtensionVisibilitySnapshot {
    pub fn new(
        snapshot_id: SnapshotId,
        generation: u64,
        state: ExtensionSnapshotState,
        query: impl Into<String>,
        source_snapshot_digest: impl Into<String>,
        mut entries: Vec<ExtensionVisibilityEntry>,
    ) -> Result<Self, String> {
        if generation == 0 {
            return Err("extension_visibility_generation_invalid".to_owned());
        }
        let query = query.into();
        if query.len() > MAX_EXTENSION_VISIBILITY_QUERY_BYTES || query.contains('\0') {
            return Err("extension_visibility_query_invalid".to_owned());
        }
        let source_snapshot_digest = source_snapshot_digest.into();
        digest(&source_snapshot_digest, "extension_visibility_source_snapshot")?;
        entries.sort_by(|left, right| {
            left.extension_id
                .cmp(&right.extension_id)
                .then_with(|| left.component_id.cmp(&right.component_id))
                .then_with(|| left.version.cmp(&right.version))
        });
        if entries.len() > MAX_EXTENSION_VISIBILITY_ENTRIES {
            return Err("extension_visibility_entry_limit".to_owned());
        }
        let mut snapshot = Self {
            schema: EXTENSION_VISIBILITY_SCHEMA.to_owned(),
            snapshot_id,
            generation,
            state,
            query,
            source_snapshot_digest,
            entries,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Build the public projection from a server-owned source snapshot.  This deliberately copies
    /// only descriptor metadata; source locators, body text and internal package/entrypoint data
    /// never cross this boundary.
    pub fn from_source_snapshot(
        snapshot: &ExtensionSnapshot,
        mut entries: Vec<ExtensionVisibilityEntry>,
        query: impl Into<String>,
        max_results: usize,
    ) -> Result<Self, String> {
        snapshot.validate()?;
        let query = query.into();
        let mut projected = entries
            .drain(..)
            .filter(|entry| entry.matches_query(&query))
            .collect::<Vec<_>>();
        projected.truncate(max_results.min(MAX_EXTENSION_VISIBILITY_ENTRIES));
        Self::new(
            snapshot.snapshot_id,
            snapshot.generation,
            snapshot.state,
            query,
            snapshot.snapshot_digest.clone(),
            projected,
        )
    }

    pub fn inspect(&self, extension_id: &str) -> Result<Option<&ExtensionVisibilityEntry>, String> {
        safe_public_identifier(extension_id, "visibility_extension_id", 256)?;
        Ok(self
            .entries
            .iter()
            .find(|entry| entry.extension_id == extension_id))
    }

    pub fn action(
        &self,
        extension_id: &str,
        action: ExtensionVisibilityActionKind,
        reason: impl Into<String>,
    ) -> Result<ExtensionVisibilityAction, String> {
        let entry = self
            .inspect(extension_id)?
            .ok_or_else(|| "extension_visibility_not_found".to_owned())?;
        if !entry.actions.contains(&action) {
            return Err("extension_visibility_action_denied".to_owned());
        }
        ExtensionVisibilityAction::new(
            self.snapshot_id,
            self.generation,
            entry.extension_id.clone(),
            action,
            reason,
            self.snapshot_digest.clone(),
        )
    }

    /// Check a caller's cached projection before it is sent back as an action precondition.
    /// This is intentionally only a stale-cache check; ControlPlane/Broker still owns the final
    /// authorization and lifecycle recheck.
    pub fn require_generation(
        &self,
        snapshot_id: SnapshotId,
        generation: u64,
    ) -> Result<(), String> {
        if snapshot_id != self.snapshot_id || generation != self.generation {
            return Err("extension_visibility_snapshot_stale".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_VISIBILITY_SCHEMA || self.generation == 0 {
            return Err("extension_visibility_header_invalid".to_owned());
        }
        if self.snapshot_id.as_uuid().is_nil() {
            return Err("extension_visibility_snapshot_id_invalid".to_owned());
        }
        if self.query.len() > MAX_EXTENSION_VISIBILITY_QUERY_BYTES || self.query.contains('\0') {
            return Err("extension_visibility_query_invalid".to_owned());
        }
        digest(
            &self.source_snapshot_digest,
            "extension_visibility_source_snapshot",
        )?;
        digest(&self.snapshot_digest, "extension_visibility_snapshot_digest")?;
        if self.entries.len() > MAX_EXTENSION_VISIBILITY_ENTRIES {
            return Err("extension_visibility_entry_limit".to_owned());
        }
        let mut identities = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !identities.insert(format!("{}\u{1f}{}", entry.extension_id, entry.component_id)) {
                return Err("extension_visibility_duplicate_identity".to_owned());
            }
        }
        if self.snapshot_digest != self.digest() {
            return Err("extension_visibility_snapshot_digest_mismatch".to_owned());
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionVisibilityAction {
    pub schema: String,
    pub snapshot_id: SnapshotId,
    pub generation: u64,
    pub extension_id: String,
    pub action: ExtensionVisibilityActionKind,
    pub reason: String,
    pub snapshot_digest: String,
    pub action_digest: String,
}

impl ExtensionVisibilityAction {
    pub fn new(
        snapshot_id: SnapshotId,
        generation: u64,
        extension_id: impl Into<String>,
        action: ExtensionVisibilityActionKind,
        reason: impl Into<String>,
        snapshot_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: EXTENSION_VISIBILITY_ACTION_SCHEMA.to_owned(),
            snapshot_id,
            generation,
            extension_id: extension_id.into(),
            action,
            reason: reason.into(),
            snapshot_digest: snapshot_digest.into(),
            action_digest: String::new(),
        };
        value.action_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_VISIBILITY_ACTION_SCHEMA
            || self.snapshot_id.as_uuid().is_nil()
            || self.generation == 0
        {
            return Err("extension_visibility_action_header_invalid".to_owned());
        }
        safe_public_identifier(&self.extension_id, "visibility_extension_id", 256)?;
        required(&self.reason, "extension_visibility_reason", 1_024)?;
        digest(&self.snapshot_digest, "extension_visibility_snapshot_digest")?;
        digest(&self.action_digest, "extension_visibility_action_digest")?;
        if self.action_digest != self.digest() {
            return Err("extension_visibility_action_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "snapshot_id": self.snapshot_id,
            "generation": self.generation,
            "extension_id": self.extension_id,
            "action": self.action,
            "reason": self.reason,
            "snapshot_digest": self.snapshot_digest,
        }))
    }
}
