//! EQ-45 quality feedback contracts.
//!
//! Feedback is an immutable, evidence-bound observation.  The client can name only a canonical
//! target and a bounded label/comment reference.  Provenance and privacy scope are derived by the
//! trusted server adapter; they are intentionally absent from [`QualityFeedbackSubmission`].
//! A feedback record contains references to policy/receipt facts when the target has them, but no
//! operation in this module can mutate those facts or grant authority.

use crate::{canonical_journal_bytes, json_digest, redact_text, FeedbackId};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub const QUALITY_CANONICAL_TARGET_SCHEMA: &str = "kiana.quality-canonical-target.v1";
pub const QUALITY_FEEDBACK_SUBMISSION_SCHEMA: &str = "kiana.quality-feedback-submission.v1";
pub const QUALITY_FEEDBACK_SCHEMA: &str = "kiana.quality-feedback.v1";
pub const QUALITY_FEEDBACK_PROVENANCE_SCHEMA: &str = "kiana.quality-feedback-provenance.v1";
pub const QUALITY_FEEDBACK_MAX_TEXT: usize = 2_048;
pub const QUALITY_FEEDBACK_MAX_REF: usize = 512;
pub const QUALITY_FEEDBACK_MAX_SOURCE_EVENTS: usize = 64;

/// The only runtime objects to which feedback may point.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityFeedbackTargetType {
    Run,
    Turn,
    ToolCall,
    Memory,
    Workflow,
    Artifact,
    Receipt,
}

impl QualityFeedbackTargetType {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Turn => "turn",
            Self::ToolCall => "tool_call",
            Self::Memory => "memory",
            Self::Workflow => "workflow",
            Self::Artifact => "artifact",
            Self::Receipt => "receipt",
        }
    }
}

/// Server supplied classification used to derive the privacy scope.  It is not accepted from a
/// feedback submission and is not a policy/authority decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityFeedbackPrivacyClass {
    Public,
    Internal,
    Confidential,
    Restricted,
}

/// Scope stamped by the server after target and data-boundary checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityFeedbackPrivacyScope {
    Project,
    Principal,
    Restricted,
}

impl QualityFeedbackPrivacyClass {
    pub const fn derive_scope(self) -> QualityFeedbackPrivacyScope {
        match self {
            Self::Public | Self::Internal => QualityFeedbackPrivacyScope::Project,
            Self::Confidential => QualityFeedbackPrivacyScope::Principal,
            Self::Restricted => QualityFeedbackPrivacyScope::Restricted,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityCanonicalTarget {
    pub schema: String,
    pub target_type: QualityFeedbackTargetType,
    /// A typed reference such as `run:<uuid>` or `memory:<record-id>`.
    pub target_ref: String,
    /// Digest of the immutable target fact, not a digest of mutable policy or receipt state.
    pub target_digest: String,
    /// Optional immutable policy fact reference used to explain the target's evaluation context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_ref: Option<String>,
    /// Optional immutable receipt fact reference.  It is a reference only and cannot be edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_ref: Option<String>,
    pub target_revision: u64,
    pub canonical_digest: String,
}

impl QualityCanonicalTarget {
    pub fn new(
        target_type: QualityFeedbackTargetType,
        target_ref: impl Into<String>,
        target_digest: impl Into<String>,
        policy_ref: Option<String>,
        receipt_ref: Option<String>,
        target_revision: u64,
    ) -> Result<Self, String> {
        let mut target = Self {
            schema: QUALITY_CANONICAL_TARGET_SCHEMA.to_owned(),
            target_type,
            target_ref: target_ref.into(),
            target_digest: target_digest.into(),
            policy_ref,
            receipt_ref,
            target_revision,
            canonical_digest: String::new(),
        };
        target.canonical_digest = target.digest();
        target.validate()?;
        Ok(target)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_CANONICAL_TARGET_SCHEMA
            || self.target_revision == 0
            || !valid_digest(&self.target_digest)
            || !valid_digest(&self.canonical_digest)
            || self.canonical_digest != self.digest()
            || !valid_target_ref(self.target_type, &self.target_ref)
            || !valid_optional_ref(self.policy_ref.as_deref(), "policy:")
            || !valid_optional_ref(self.receipt_ref.as_deref(), "receipt:")
        {
            return Err("quality_canonical_target_invalid".to_owned());
        }
        if self.target_type == QualityFeedbackTargetType::Receipt
            && self.receipt_ref.as_deref() != Some(self.target_ref.as_str())
        {
            return Err("quality_canonical_receipt_ref_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "target_type": self.target_type,
            "target_ref": self.target_ref,
            "target_digest": self.target_digest,
            "policy_ref": self.policy_ref,
            "receipt_ref": self.receipt_ref,
            "target_revision": self.target_revision,
        }))
    }
}

/// Client-owned fields.  In particular, this type has no provenance, privacy scope, policy
/// mutation, grant, approval, or receipt mutation field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityFeedbackSubmission {
    pub schema: String,
    pub feedback_id: FeedbackId,
    pub target: QualityCanonicalTarget,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_ref: Option<String>,
    pub idempotency_key: String,
}

impl QualityFeedbackSubmission {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_FEEDBACK_SUBMISSION_SCHEMA
            || self.feedback_id.as_uuid().is_nil()
            || self.label.trim().is_empty()
            || self.label.len() > QUALITY_FEEDBACK_MAX_TEXT
            || self.label.contains('\0')
            || redact_text(&self.label) != self.label
            || !valid_ref_text(&self.idempotency_key, "idempotency:")
            || self
                .comment_ref
                .as_deref()
                .is_some_and(|value| !valid_ref_text(value, "comment:"))
            || self
                .correction_ref
                .as_deref()
                .is_some_and(|value| !valid_correction_ref(value))
        {
            return Err("quality_feedback_submission_invalid".to_owned());
        }
        self.target.validate()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "feedback_id": self.feedback_id,
            "target": self.target,
            "label": self.label,
            "comment_ref": self.comment_ref,
            "correction_ref": self.correction_ref,
            "idempotency_key": self.idempotency_key,
        }))
    }
}

/// Server-side facts needed to derive provenance and privacy scope.  A client envelope must never
/// be used as this context without the ControlPlane checking it first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualityFeedbackServerContext {
    pub principal_ref: String,
    pub project_ref: String,
    pub session_ref: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<String>,
    pub source_event_digest: String,
    pub target_privacy_class: QualityFeedbackPrivacyClass,
    pub captured_at_unix_ms: u64,
}

impl QualityFeedbackServerContext {
    pub fn validate(&self) -> Result<(), String> {
        for (value, prefix, field) in [
            (&self.principal_ref, "principal:", "principal_ref"),
            (&self.project_ref, "project:", "project_ref"),
            (&self.session_ref, "session:", "session_ref"),
        ] {
            if !valid_ref_text(value, prefix) {
                return Err(format!("quality_feedback_{field}_invalid"));
            }
        }
        if self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > QUALITY_FEEDBACK_MAX_SOURCE_EVENTS
            || self
                .source_event_ids
                .iter()
                .any(|value| !valid_ref_text(value, "event:"))
            || !valid_digest(&self.source_event_digest)
            || self.captured_at_unix_ms == 0
        {
            return Err("quality_feedback_server_context_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityFeedbackProvenance {
    pub schema: String,
    pub principal_ref: String,
    pub project_ref: String,
    pub session_ref: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<String>,
    pub source_event_digest: String,
    pub target_digest: String,
    pub target_revision: u64,
    pub captured_at_unix_ms: u64,
    pub provenance_digest: String,
}

impl QualityFeedbackProvenance {
    fn from_server(
        submission: &QualityFeedbackSubmission,
        context: &QualityFeedbackServerContext,
    ) -> Self {
        let mut provenance = Self {
            schema: QUALITY_FEEDBACK_PROVENANCE_SCHEMA.to_owned(),
            principal_ref: context.principal_ref.clone(),
            project_ref: context.project_ref.clone(),
            session_ref: context.session_ref.clone(),
            source_cursor: context.source_cursor,
            source_event_ids: context.source_event_ids.clone(),
            source_event_digest: context.source_event_digest.clone(),
            target_digest: submission.target.canonical_digest.clone(),
            target_revision: submission.target.target_revision,
            captured_at_unix_ms: context.captured_at_unix_ms,
            provenance_digest: String::new(),
        };
        provenance.provenance_digest = provenance.digest();
        provenance
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_FEEDBACK_PROVENANCE_SCHEMA
            || !valid_ref_text(&self.principal_ref, "principal:")
            || !valid_ref_text(&self.project_ref, "project:")
            || !valid_ref_text(&self.session_ref, "session:")
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > QUALITY_FEEDBACK_MAX_SOURCE_EVENTS
            || self
                .source_event_ids
                .iter()
                .any(|value| !valid_ref_text(value, "event:"))
            || !valid_digest(&self.source_event_digest)
            || !valid_digest(&self.target_digest)
            || self.target_revision == 0
            || self.captured_at_unix_ms == 0
            || !valid_digest(&self.provenance_digest)
            || self.provenance_digest != self.digest()
        {
            return Err("quality_feedback_provenance_invalid".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "principal_ref": self.principal_ref,
            "project_ref": self.project_ref,
            "session_ref": self.session_ref,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "source_event_digest": self.source_event_digest,
            "target_digest": self.target_digest,
            "target_revision": self.target_revision,
            "captured_at_unix_ms": self.captured_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityFeedback {
    pub schema: String,
    pub feedback_id: FeedbackId,
    pub target: QualityCanonicalTarget,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_ref: Option<String>,
    pub provenance: QualityFeedbackProvenance,
    pub privacy_scope: QualityFeedbackPrivacyScope,
    pub created_at_unix_ms: u64,
    pub feedback_digest: String,
}

impl QualityFeedback {
    pub fn derive(
        submission: QualityFeedbackSubmission,
        context: QualityFeedbackServerContext,
    ) -> Result<Self, String> {
        submission.validate()?;
        context.validate()?;
        let provenance = QualityFeedbackProvenance::from_server(&submission, &context);
        let mut feedback = Self {
            schema: QUALITY_FEEDBACK_SCHEMA.to_owned(),
            feedback_id: submission.feedback_id,
            target: submission.target,
            label: submission.label,
            comment_ref: submission.comment_ref,
            correction_ref: submission.correction_ref,
            provenance,
            privacy_scope: context.target_privacy_class.derive_scope(),
            created_at_unix_ms: context.captured_at_unix_ms,
            feedback_digest: String::new(),
        };
        feedback.feedback_digest = feedback.digest();
        feedback.validate()?;
        Ok(feedback)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_FEEDBACK_SCHEMA
            || self.feedback_id.as_uuid().is_nil()
            || self.label.trim().is_empty()
            || self.label.len() > QUALITY_FEEDBACK_MAX_TEXT
            || self.label.contains('\0')
            || redact_text(&self.label) != self.label
            || self.created_at_unix_ms == 0
            || !valid_digest(&self.feedback_digest)
            || self.feedback_digest != self.digest()
        {
            return Err("quality_feedback_invalid".to_owned());
        }
        self.target.validate()?;
        if self
            .comment_ref
            .as_deref()
            .is_some_and(|value| !valid_ref_text(value, "comment:"))
            || self
                .correction_ref
                .as_deref()
                .is_some_and(|value| !valid_correction_ref(value))
        {
            return Err("quality_feedback_reference_invalid".to_owned());
        }
        self.provenance.validate()?;
        if self.provenance.target_digest != self.target.canonical_digest
            || self.provenance.target_revision != self.target.target_revision
        {
            return Err("quality_feedback_target_provenance_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "feedback_id": self.feedback_id,
            "target": self.target,
            "label": self.label,
            "comment_ref": self.comment_ref,
            "correction_ref": self.correction_ref,
            "provenance": self.provenance,
            "privacy_scope": self.privacy_scope,
            "created_at_unix_ms": self.created_at_unix_ms,
        }))
    }

    /// Canonical bytes are exposed for evidence manifests and are deliberately read-only.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        canonical_journal_bytes(&serde_json::json!({
            "schema": self.schema,
            "feedback_id": self.feedback_id,
            "target": self.target,
            "label": self.label,
            "comment_ref": self.comment_ref,
            "correction_ref": self.correction_ref,
            "provenance": self.provenance,
            "privacy_scope": self.privacy_scope,
            "created_at_unix_ms": self.created_at_unix_ms,
            "feedback_digest": self.feedback_digest,
        }))
    }
}

fn valid_target_ref(target_type: QualityFeedbackTargetType, value: &str) -> bool {
    let Some(identifier) = value.strip_prefix(&format!("{}:", target_type.prefix())) else {
        return false;
    };
    if identifier.is_empty()
        || identifier.len() > QUALITY_FEEDBACK_MAX_REF
        || identifier.contains(['\0', '\n', '\r'])
    {
        return false;
    }
    match target_type {
        QualityFeedbackTargetType::Memory => safe_token(identifier),
        QualityFeedbackTargetType::Run
        | QualityFeedbackTargetType::Turn
        | QualityFeedbackTargetType::ToolCall
        | QualityFeedbackTargetType::Workflow
        | QualityFeedbackTargetType::Artifact
        | QualityFeedbackTargetType::Receipt => Uuid::parse_str(identifier).is_ok(),
    }
}

fn valid_optional_ref(value: Option<&str>, prefix: &str) -> bool {
    value.map_or(true, |value| valid_ref_text(value, prefix))
}

fn valid_ref_text(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|identifier| {
        !identifier.is_empty()
            && identifier.len() <= QUALITY_FEEDBACK_MAX_REF
            && safe_token(identifier)
            && redact_text(value) == value
    })
}

fn valid_correction_ref(value: &str) -> bool {
    [
        QualityFeedbackTargetType::Run,
        QualityFeedbackTargetType::Turn,
        QualityFeedbackTargetType::ToolCall,
        QualityFeedbackTargetType::Memory,
        QualityFeedbackTargetType::Workflow,
        QualityFeedbackTargetType::Artifact,
        QualityFeedbackTargetType::Receipt,
    ]
    .into_iter()
    .any(|kind| valid_target_ref(kind, value))
}

fn safe_token(value: &str) -> bool {
    !value.is_empty()
        && !value.bytes().any(|byte| byte.is_ascii_whitespace())
        && !value.contains(['\0', '\n', '\r'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

impl fmt::Display for QualityFeedbackTargetType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.prefix())
    }
}
