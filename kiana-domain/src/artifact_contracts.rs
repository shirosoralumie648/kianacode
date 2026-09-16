//! Immutable Artifact, Evidence and Criterion references.
//!
//! References contain identity and provenance only. Artifact bytes are stored behind a read-only
//! port and must be persisted before a reference is admitted to a Company fact.

use crate::{
    journal_sha256, json_digest, ArtifactId, CriterionId, EventId, EvidenceId, InvocationId, RunId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const ARTIFACT_REF_SCHEMA: &str = "kiana.artifact-ref.v1";
pub const ARTIFACT_VERSION_SCHEMA: &str = "kiana.artifact-version.v1";
pub const EVIDENCE_REF_SCHEMA: &str = "kiana.evidence-ref.v1";
pub const CRITERION_SCHEMA: &str = "kiana.criterion.v1";
pub const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactProvenance {
    pub producer_kind: String,
    pub producer_id: String,
    #[serde(default)]
    pub source_event_id: Option<EventId>,
    #[serde(default)]
    pub source_run_id: Option<RunId>,
    pub recorded_by: String,
}

impl ArtifactProvenance {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.producer_kind, "artifact_producer_kind", 128)?;
        required(&self.producer_id, "artifact_producer_id", 256)?;
        required(&self.recorded_by, "artifact_recorded_by", 256)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactVersion {
    pub schema: String,
    pub artifact_id: ArtifactId,
    pub version: u64,
    pub artifact_schema: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub scope_digest: String,
    pub provenance: ArtifactProvenance,
    pub created_at_unix_ms: u64,
    pub immutable: bool,
}

impl ArtifactVersion {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact_id: ArtifactId,
        version: u64,
        artifact_schema: impl Into<String>,
        content: &[u8],
        scope_digest: impl Into<String>,
        provenance: ArtifactProvenance,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        if content.len() > MAX_ARTIFACT_BYTES {
            return Err("artifact_content_too_large".to_owned());
        }
        let artifact = Self {
            schema: ARTIFACT_VERSION_SCHEMA.to_owned(),
            artifact_id,
            version,
            artifact_schema: artifact_schema.into(),
            content_hash: journal_sha256(content),
            size_bytes: content.len() as u64,
            scope_digest: scope_digest.into(),
            provenance,
            created_at_unix_ms,
            immutable: true,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ARTIFACT_VERSION_SCHEMA
            || self.version == 0
            || self.size_bytes > MAX_ARTIFACT_BYTES as u64
            || self.created_at_unix_ms == 0
            || !self.immutable
        {
            return Err("artifact_version_invalid".to_owned());
        }
        required(&self.artifact_schema, "artifact_schema", 128)?;
        valid_digest(&self.content_hash, "artifact_content_hash")?;
        valid_digest(&self.scope_digest, "artifact_scope_digest")?;
        self.provenance.validate()
    }

    pub fn as_ref(&self) -> ArtifactRef {
        ArtifactRef {
            schema: ARTIFACT_REF_SCHEMA.to_owned(),
            artifact_id: self.artifact_id,
            version: self.version,
            artifact_schema: self.artifact_schema.clone(),
            content_hash: self.content_hash.clone(),
            scope_digest: self.scope_digest.clone(),
            provenance: self.provenance.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub schema: String,
    pub artifact_id: ArtifactId,
    pub version: u64,
    pub artifact_schema: String,
    pub content_hash: String,
    pub scope_digest: String,
    pub provenance: ArtifactProvenance,
}

impl ArtifactRef {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ARTIFACT_REF_SCHEMA || self.version == 0 {
            return Err("artifact_ref_invalid".to_owned());
        }
        required(&self.artifact_schema, "artifact_ref_schema", 128)?;
        valid_digest(&self.content_hash, "artifact_ref_content_hash")?;
        valid_digest(&self.scope_digest, "artifact_ref_scope_digest")?;
        self.provenance.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub schema: String,
    pub evidence_id: EvidenceId,
    pub evidence_kind: String,
    pub run_id: RunId,
    #[serde(default)]
    pub invocation_id: Option<InvocationId>,
    pub artifact: ArtifactRef,
    pub scope_digest: String,
    pub provenance: ArtifactProvenance,
    pub created_at_unix_ms: u64,
}

impl EvidenceRef {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        evidence_id: EvidenceId,
        evidence_kind: impl Into<String>,
        run_id: RunId,
        invocation_id: Option<InvocationId>,
        artifact: ArtifactRef,
        scope_digest: impl Into<String>,
        provenance: ArtifactProvenance,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let evidence = Self {
            schema: EVIDENCE_REF_SCHEMA.to_owned(),
            evidence_id,
            evidence_kind: evidence_kind.into(),
            run_id,
            invocation_id,
            scope_digest: scope_digest.into(),
            provenance,
            created_at_unix_ms,
            artifact,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVIDENCE_REF_SCHEMA || self.created_at_unix_ms == 0 {
            return Err("evidence_ref_invalid".to_owned());
        }
        required(&self.evidence_kind, "evidence_kind", 128)?;
        valid_digest(&self.scope_digest, "evidence_scope_digest")?;
        self.artifact.validate()?;
        if self.scope_digest != self.artifact.scope_digest {
            return Err("evidence_scope_mismatch".to_owned());
        }
        self.provenance.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub schema: String,
    pub criterion_id: CriterionId,
    pub version: u64,
    pub source_ref: String,
    pub source_version: u64,
    pub description: String,
    pub verification_method: String,
    pub evidence_kinds: Vec<String>,
    pub required: bool,
    pub scope_digest: String,
    pub criterion_digest: String,
}

impl Criterion {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        criterion_id: CriterionId,
        source_ref: impl Into<String>,
        source_version: u64,
        description: impl Into<String>,
        verification_method: impl Into<String>,
        evidence_kinds: Vec<String>,
        required: bool,
        scope_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut criterion = Self {
            schema: CRITERION_SCHEMA.to_owned(),
            criterion_id,
            version: 1,
            source_ref: source_ref.into(),
            source_version,
            description: description.into(),
            verification_method: verification_method.into(),
            evidence_kinds,
            required,
            scope_digest: scope_digest.into(),
            criterion_digest: String::new(),
        };
        criterion.criterion_digest = criterion.digest();
        criterion.validate()?;
        Ok(criterion)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CRITERION_SCHEMA
            || self.version == 0
            || self.source_version == 0
            || self.evidence_kinds.is_empty()
            || self.evidence_kinds.len() > 32
        {
            return Err("criterion_invalid".to_owned());
        }
        required(&self.source_ref, "criterion_source_ref", 512)?;
        required(&self.description, "criterion_description", 16_384)?;
        required(
            &self.verification_method,
            "criterion_verification_method",
            512,
        )?;
        valid_digest(&self.scope_digest, "criterion_scope_digest")?;
        let mut kinds = BTreeSet::new();
        if self
            .evidence_kinds
            .iter()
            .any(|kind| kind.trim().is_empty() || !kinds.insert(kind.clone()))
        {
            return Err("criterion_evidence_kinds_invalid".to_owned());
        }
        if self.criterion_digest != self.digest() {
            return Err("criterion_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "criterion_id": self.criterion_id,
            "version": self.version,
            "source_ref": self.source_ref,
            "source_version": self.source_version,
            "description": self.description,
            "verification_method": self.verification_method,
            "evidence_kinds": self.evidence_kinds,
            "required": self.required,
            "scope_digest": self.scope_digest,
        }))
    }
}
