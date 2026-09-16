//! Shared source and context/memory scope values.
//!
//! These contracts describe which material may be read and how fresh/evidenced it is. They do
//! not read files, query Memory, grant capability authority or infer a principal from a path or
//! model argument.

use crate::{
    json_digest, AuthenticatedPrincipalRef, MemoryCollection, ProjectIdentity, Purpose, SessionId,
};
use serde::{Deserialize, Serialize};

pub const SOURCE_REF_SCHEMA: &str = "kiana.source-ref.v1";
pub const SOURCE_SNAPSHOT_SCHEMA: &str = "kiana.source-snapshot.v1";
pub const MEMORY_SCOPE_SCHEMA: &str = "kiana.memory-scope.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    WorkspaceFile,
    Artifact,
    Event,
    Memory,
    Prompt,
    UserImport,
    Connector,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Current,
    Stale,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Attributed,
    Verified,
    #[default]
    Unverifiable,
    Missing,
}

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub schema: String,
    pub source_id: String,
    pub kind: SourceKind,
    pub locator: String,
    pub revision: String,
    pub content_digest: String,
    #[serde(default)]
    pub source_cursor: Option<u64>,
    pub evidence: EvidenceStatus,
}

impl SourceRef {
    pub fn new(
        source_id: impl Into<String>,
        kind: SourceKind,
        locator: impl Into<String>,
        revision: impl Into<String>,
        content_digest: impl Into<String>,
        source_cursor: Option<u64>,
        evidence: EvidenceStatus,
    ) -> Result<Self, String> {
        let source = Self {
            schema: SOURCE_REF_SCHEMA.to_owned(),
            source_id: source_id.into(),
            kind,
            locator: locator.into(),
            revision: revision.into(),
            content_digest: content_digest.into(),
            source_cursor,
            evidence,
        };
        source.validate()?;
        Ok(source)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SOURCE_REF_SCHEMA {
            return Err("source_ref_schema_invalid".to_owned());
        }
        required(&self.source_id, "source_id", 256)?;
        required(&self.locator, "source_locator", 4_096)?;
        required(&self.revision, "source_revision", 256)?;
        digest(&self.content_digest, "source_content_digest")?;
        if self.source_cursor == Some(0) {
            return Err("source_cursor_invalid".to_owned());
        }
        Ok(())
    }

    /// Convert a trusted PromptSection into a source reference without copying its text into the
    /// reference. The prompt body remains in the model input boundary; only a digest is shared.
    pub fn from_prompt_section(section: &crate::PromptSection) -> Result<Self, String> {
        Self::new(
            format!("prompt:{}", section.name),
            SourceKind::Prompt,
            section.source.clone(),
            format!("order:{}", section.order),
            json_digest(&serde_json::json!({"text": section.text})),
            None,
            if section.authority == crate::PromptAuthority::Product {
                EvidenceStatus::Verified
            } else {
                EvidenceStatus::Attributed
            },
        )
    }

    /// Convert an existing MemoryRecord to a source reference while preserving its provenance
    /// status. A legacy/empty content hash is replaced by a digest of the text and remains
    /// `Unverifiable` rather than becoming authority.
    pub fn from_memory_record(record: &crate::MemoryRecord) -> Result<Self, String> {
        let content_digest = if record.content_hash.starts_with("sha256:") {
            record.content_hash.clone()
        } else {
            json_digest(&serde_json::json!({"text": record.text}))
        };
        Self::new(
            format!("memory:{}", record.id),
            SourceKind::Memory,
            format!("{}:{}", record.collection, record.id),
            format!("revision:{}", record.revision),
            content_digest,
            None,
            if record.origin == crate::MemoryOrigin::Unknown {
                EvidenceStatus::Unverifiable
            } else {
                EvidenceStatus::Attributed
            },
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::to_value(self).unwrap_or(serde_json::Value::Null))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSnapshot {
    pub schema: String,
    pub source: SourceRef,
    pub freshness: Freshness,
    pub evidence: EvidenceStatus,
    #[serde(default)]
    pub observed_at_ms: Option<u64>,
    pub snapshot_digest: String,
}

impl SourceSnapshot {
    pub fn new(
        source: SourceRef,
        freshness: Freshness,
        evidence: EvidenceStatus,
        observed_at_ms: Option<u64>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: SOURCE_SNAPSHOT_SCHEMA.to_owned(),
            source,
            freshness,
            evidence,
            observed_at_ms,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SOURCE_SNAPSHOT_SCHEMA {
            return Err("source_snapshot_schema_invalid".to_owned());
        }
        self.source.validate()?;
        if self.observed_at_ms == Some(0) {
            return Err("source_snapshot_observed_at_invalid".to_owned());
        }
        digest(&self.snapshot_digest, "source_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("source_snapshot_digest_mismatch".to_owned());
        }
        if self.evidence == EvidenceStatus::Verified
            && self.source.evidence != EvidenceStatus::Verified
        {
            return Err("source_snapshot_verified_without_source_evidence".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "snapshot_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryScope {
    pub schema: String,
    pub principal: AuthenticatedPrincipalRef,
    pub project: ProjectIdentity,
    pub session_id: SessionId,
    pub collections: Vec<MemoryCollection>,
    pub purpose: Purpose,
    #[serde(default)]
    pub allow_write: bool,
    pub scope_digest: String,
}

impl MemoryScope {
    pub fn new(
        principal: AuthenticatedPrincipalRef,
        project: ProjectIdentity,
        session_id: impl Into<String>,
        collections: Vec<MemoryCollection>,
        purpose: Purpose,
        allow_write: bool,
    ) -> Result<Self, String> {
        let mut scope = Self {
            schema: MEMORY_SCOPE_SCHEMA.to_owned(),
            principal,
            project,
            session_id: SessionId::new(session_id),
            collections,
            purpose,
            allow_write,
            scope_digest: String::new(),
        };
        scope.scope_digest = scope.digest();
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_SCOPE_SCHEMA
            || self.session_id.is_empty()
            || self.collections.is_empty()
            || self.collections.len() > 64
        {
            return Err("memory_scope_header_invalid".to_owned());
        }
        self.principal.validate()?;
        self.project.validate()?;
        self.purpose.validate()?;
        let mut seen = std::collections::HashSet::new();
        for collection in &self.collections {
            if collection.layer.trim().is_empty()
                || collection.collection.trim().is_empty()
                || MemoryCollection::parse(&collection.collection).is_none()
                || !seen.insert((&collection.layer, &collection.collection))
            {
                return Err("memory_scope_collection_invalid".to_owned());
            }
        }
        if self.scope_digest != self.digest() {
            return Err("memory_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn allows_collection(&self, requested: &MemoryCollection) -> bool {
        self.collections.iter().any(|grant| grant.covers(requested))
    }

    /// Derive a memory scope from the already server-owned ExecutionScope. Caller/model
    /// arguments are not consulted for principal, project or session identity.
    pub fn from_execution_scope(
        execution: &crate::ExecutionScope,
        purpose: Purpose,
        allow_write: bool,
    ) -> Result<Self, String> {
        let collections = execution
            .memory_scopes
            .iter()
            .filter_map(|value| MemoryCollection::parse(value))
            .collect::<Vec<_>>();
        if collections.is_empty() {
            return Err("memory_scope_unavailable".to_owned());
        }
        Self::new(
            execution.principal.clone(),
            execution.project.clone(),
            execution.session_id.as_str().to_owned(),
            collections,
            purpose,
            allow_write,
        )
    }

    /// Intersect two independently derived scopes. Identity and purpose must match; collection
    /// visibility can only narrow and write permission is the logical conjunction.
    pub fn intersect(&self, other: &Self) -> Result<Self, String> {
        self.validate()?;
        other.validate()?;
        if self.principal != other.principal
            || self.project != other.project
            || self.session_id != other.session_id
            || self.purpose != other.purpose
        {
            return Err("memory_scope_identity_mismatch".to_owned());
        }
        let mut collections = Vec::new();
        for left in &self.collections {
            for right in &other.collections {
                let narrower = if left.covers(right) {
                    Some(right)
                } else if right.covers(left) {
                    Some(left)
                } else {
                    None
                };
                if let Some(collection) = narrower {
                    if !collections.contains(collection) {
                        collections.push(collection.clone());
                    }
                }
            }
        }
        if collections.is_empty() {
            return Err("memory_scope_intersection_empty".to_owned());
        }
        Self::new(
            self.principal.clone(),
            self.project.clone(),
            self.session_id.as_str().to_owned(),
            collections,
            self.purpose.clone(),
            self.allow_write && other.allow_write,
        )
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "scope_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
