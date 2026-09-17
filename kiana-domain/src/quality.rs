//! Foundational quality object identity and lifecycle contracts.
//!
//! EQ-02 intentionally stops at value objects and state transitions. Dataset/case semantics,
//! trace normalization, stores, judges and promotion authority are later roadmap steps.
use crate::{
    canonical_journal_bytes, json_digest, redact_text, QualityArtifactId, QualityTransitionId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const QUALITY_ARTIFACT_SCHEMA: &str = "kiana.quality-artifact.v1";
pub const QUALITY_TRANSITION_SCHEMA: &str = "kiana.quality-transition.v1";
pub const QUALITY_SCHEMA_VERSION: u32 = 1;
pub const MAX_QUALITY_OBJECT_TYPE: usize = 64;
pub const MAX_QUALITY_OWNER: usize = 256;
pub const MAX_QUALITY_REASON: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityArtifactStatus {
    Draft,
    Admitted,
    Running,
    Completed,
    Passed,
    Failed,
    Blocked,
    Cancelled,
    Deprecated,
    RolledBack,
}

impl QualityArtifactStatus {
    pub fn transition(self, next: Self) -> Result<Self, &'static str> {
        if self == next {
            return Err("quality_transition_duplicate");
        }
        let allowed = matches!(
            (self, next),
            (Self::Draft, Self::Admitted | Self::Cancelled)
                | (Self::Admitted, Self::Running | Self::Cancelled)
                | (
                    Self::Running,
                    Self::Completed | Self::Failed | Self::Blocked | Self::Cancelled
                )
                | (Self::Completed, Self::Passed | Self::Failed | Self::Blocked)
                | (Self::Passed, Self::Deprecated | Self::RolledBack)
                | (Self::Failed, Self::Deprecated)
                | (Self::Blocked, Self::Deprecated)
        );
        if allowed {
            Ok(next)
        } else {
            Err("quality_transition_invalid")
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Passed
                | Self::Failed
                | Self::Blocked
                | Self::Cancelled
                | Self::Deprecated
                | Self::RolledBack
        )
    }
}

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_secret_detected"));
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

fn object_type(value: &str) -> bool {
    matches!(
        value,
        "dataset"
            | "suite"
            | "case"
            | "golden_trace"
            | "experiment"
            | "result"
            | "candidate"
            | "gate"
            | "gate_decision"
            | "feedback"
            | "drift_alert"
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityArtifact {
    pub schema: String,
    pub quality_version: u32,
    pub artifact_id: QualityArtifactId,
    pub object_type: String,
    pub owner_id: String,
    pub status: QualityArtifactStatus,
    pub revision: u64,
    pub source_digest: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub artifact_digest: String,
}

impl QualityArtifact {
    pub fn new(
        object_type: impl Into<String>,
        owner_id: impl Into<String>,
        source_digest: impl Into<String>,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut artifact = Self {
            schema: QUALITY_ARTIFACT_SCHEMA.to_owned(),
            quality_version: QUALITY_SCHEMA_VERSION,
            artifact_id: QualityArtifactId::new(),
            object_type: object_type.into(),
            owner_id: owner_id.into(),
            status: QualityArtifactStatus::Draft,
            revision: 1,
            source_digest: source_digest.into(),
            created_at_unix_ms,
            updated_at_unix_ms: created_at_unix_ms,
            artifact_digest: String::new(),
        };
        artifact.artifact_digest = artifact.digest();
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_ARTIFACT_SCHEMA
            || self.quality_version != QUALITY_SCHEMA_VERSION
            || self.artifact_id.as_uuid().is_nil()
            || self.revision == 0
            || self.updated_at_unix_ms < self.created_at_unix_ms
            || !object_type(&self.object_type)
        {
            return Err("quality_artifact_header_invalid".to_owned());
        }
        required(
            &self.object_type,
            "quality_object_type",
            MAX_QUALITY_OBJECT_TYPE,
        )?;
        required(&self.owner_id, "quality_owner", MAX_QUALITY_OWNER)?;
        digest(&self.source_digest, "quality_source_digest")?;
        digest(&self.artifact_digest, "quality_artifact_digest")?;
        if self.artifact_digest != self.digest() {
            return Err("quality_artifact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn transition(
        &mut self,
        next: QualityArtifactStatus,
        updated_at_unix_ms: u64,
    ) -> Result<QualityStateTransition, String> {
        self.validate()?;
        if updated_at_unix_ms < self.updated_at_unix_ms {
            return Err("quality_transition_time_regression".to_owned());
        }
        let from = self.status;
        self.status = from.transition(next).map_err(str::to_owned)?;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "quality_revision_overflow".to_owned())?;
        self.updated_at_unix_ms = updated_at_unix_ms;
        self.artifact_digest = self.digest();
        QualityStateTransition::new(
            self.artifact_id,
            self.object_type.clone(),
            from,
            next,
            self.revision,
            "quality_state_transition",
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "quality_version": self.quality_version,
            "artifact_id": self.artifact_id,
            "object_type": self.object_type,
            "owner_id": self.owner_id,
            "status": self.status,
            "revision": self.revision,
            "source_digest": self.source_digest,
            "created_at_unix_ms": self.created_at_unix_ms,
            "updated_at_unix_ms": self.updated_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityStateTransition {
    pub schema: String,
    pub transition_id: QualityTransitionId,
    pub artifact_id: QualityArtifactId,
    pub object_type: String,
    pub from: QualityArtifactStatus,
    pub to: QualityArtifactStatus,
    pub revision: u64,
    pub reason: String,
    pub transition_digest: String,
}

impl QualityStateTransition {
    pub fn new(
        artifact_id: QualityArtifactId,
        object_type: impl Into<String>,
        from: QualityArtifactStatus,
        to: QualityArtifactStatus,
        revision: u64,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut transition = Self {
            schema: QUALITY_TRANSITION_SCHEMA.to_owned(),
            transition_id: QualityTransitionId::new(),
            artifact_id,
            object_type: object_type.into(),
            from,
            to,
            revision,
            reason: reason.into(),
            transition_digest: String::new(),
        };
        transition.transition_digest = transition.digest();
        transition.validate()?;
        Ok(transition)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_TRANSITION_SCHEMA
            || self.transition_id.as_uuid().is_nil()
            || self.artifact_id.as_uuid().is_nil()
            || self.revision == 0
            || !object_type(&self.object_type)
        {
            return Err("quality_transition_header_invalid".to_owned());
        }
        required(
            &self.object_type,
            "quality_object_type",
            MAX_QUALITY_OBJECT_TYPE,
        )?;
        required(
            &self.reason,
            "quality_transition_reason",
            MAX_QUALITY_REASON,
        )?;
        self.from.transition(self.to).map_err(str::to_owned)?;
        digest(&self.transition_digest, "quality_transition_digest")?;
        if self.transition_digest != self.digest() {
            return Err("quality_transition_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "transition_id": self.transition_id,
            "artifact_id": self.artifact_id,
            "object_type": self.object_type,
            "from": self.from,
            "to": self.to,
            "revision": self.revision,
            "reason": self.reason,
        }))
    }
}

pub fn canonical_quality_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    canonical_journal_bytes(value)
}
