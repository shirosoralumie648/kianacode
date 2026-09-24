//! Versioned UI snapshot/feed/action contracts.
//!
//! These DTOs are projections and intents at the protocol boundary.  They carry no broker
//! authority; every action must be re-authorized by ControlPlane with the supplied CAS/digest.

use crate::UiCursor;
use kiana_domain::{
    json_digest, validate_json_limits, ArtifactId, AuthenticatedPrincipalRef, ExecutionStatus,
    ProjectId, ReceiptId, RunId, SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const UI_SNAPSHOT_SCHEMA: &str = "kiana.ui-snapshot.v1";
pub const UI_FEED_SCHEMA: &str = "kiana.ui-feed.v1";
pub const UI_ACTION_SCHEMA: &str = "kiana.ui-action.v1";
pub const UI_ACTION_RESULT_SCHEMA: &str = "kiana.ui-action-result.v1";
pub const UI_CAPABILITY_SCHEMA: &str = "kiana.ui-capability.v1";
pub const UI_ERROR_SCHEMA: &str = "kiana.ui-error.v1";
pub const UI_HANDSHAKE_REQUEST_SCHEMA: &str = "kiana.ui-handshake-request.v1";
pub const UI_HANDSHAKE_RESPONSE_SCHEMA: &str = "kiana.ui-handshake-response.v1";
pub const UI_HEALTH_SCHEMA: &str = "kiana.ui-health.v1";
pub const UI_INSTANCE_RECORD_SCHEMA: &str = "kiana.ui-instance-record.v1";
pub const UI_HOST_CAPABILITY_SCHEMA: &str = "kiana.ui-host-capability.v1";
pub const UI_LIVE_HOST_EVIDENCE_SCHEMA: &str = "kiana.ui-live-host-evidence.v1";
pub const UI_EVIDENCE_CASE_SCHEMA: &str = "kiana.ui-evidence-case.v1";
pub const UI_EVIDENCE_BUNDLE_SCHEMA: &str = "kiana.ui-evidence-bundle.v1";

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

fn cursor_valid(epoch: &str, sequence: u64) -> Result<(), String> {
    if epoch.trim().is_empty() || epoch.len() > 256 || sequence == 0 {
        return Err("ui_cursor_invalid".to_owned());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiActionDisposition {
    Accepted,
    Applied,
    Rejected,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiRetryDisposition {
    QueryOriginal,
    SafeRetry,
    DoNotRetry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiErrorCode {
    InvalidRequest,
    SchemaUnsupported,
    PermissionDenied,
    ApprovalRequired,
    Conflict,
    Capacity,
    Unavailable,
    Cancelled,
    Failed,
    Unknown,
    Persistence,
}

impl UiErrorCode {
    pub const ALL: [Self; 11] = [
        Self::InvalidRequest,
        Self::SchemaUnsupported,
        Self::PermissionDenied,
        Self::ApprovalRequired,
        Self::Conflict,
        Self::Capacity,
        Self::Unavailable,
        Self::Cancelled,
        Self::Failed,
        Self::Unknown,
        Self::Persistence,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiError {
    pub schema: String,
    pub code: UiErrorCode,
    pub message: String,
    pub retry: UiRetryDisposition,
}

impl UiError {
    pub fn new(code: UiErrorCode, message: impl Into<String>, retry: UiRetryDisposition) -> Self {
        Self {
            schema: UI_ERROR_SCHEMA.to_owned(),
            code,
            message: message.into(),
            retry,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ERROR_SCHEMA {
            return Err("ui_error_schema_invalid".to_owned());
        }
        required(&self.message, "ui_error_message", 2_048)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCapability {
    pub schema: String,
    pub capability_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    pub scope_digest: String,
}

impl UiCapability {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_CAPABILITY_SCHEMA {
            return Err("ui_capability_schema_invalid".to_owned());
        }
        required(&self.capability_id, "ui_capability_id", 128)?;
        digest(&self.scope_digest, "ui_capability_scope_digest")?;
        if self.actions.len() > 64
            || self.actions.iter().any(|action| {
                action.trim().is_empty() || action.len() > 128 || action.contains('\0')
            })
        {
            return Err("ui_capability_actions_invalid".to_owned());
        }
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 1_024)
        {
            return Err("ui_capability_reason_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiHostCapabilityDisposition {
    Advertised,
    Disabled,
    NotSupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHostCapability {
    pub schema: String,
    pub capability_id: String,
    pub actions: Vec<String>,
    pub scope_digest: String,
    pub disposition: UiHostCapabilityDisposition,
    /// Host/editor/terminal APIs never become a Kiana side-effect authority.
    pub direct_effect: bool,
    /// Advertised host capabilities must route actions back through the Kiana protocol.
    pub delegated_to_kiana: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

impl UiHostCapability {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HOST_CAPABILITY_SCHEMA {
            return Err("ui_host_capability_schema_invalid".to_owned());
        }
        required(&self.capability_id, "ui_host_capability_id", 128)?;
        digest(&self.scope_digest, "ui_host_capability_scope_digest")?;
        if self.actions.len() > 64
            || self.actions.iter().any(|action| {
                action.trim().is_empty() || action.len() > 128 || action.contains('\0')
            })
        {
            return Err("ui_host_capability_actions_invalid".to_owned());
        }
        if self.direct_effect {
            return Err("ui_host_capability_direct_effect_forbidden".to_owned());
        }
        if self.disposition == UiHostCapabilityDisposition::Advertised && !self.delegated_to_kiana {
            return Err("ui_host_capability_delegation_required".to_owned());
        }
        if self.disposition != UiHostCapabilityDisposition::Advertised
            && self
                .reason
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err("ui_host_capability_reason_required".to_owned());
        }
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| reason.len() > 1_024 || reason.contains('\0'))
        {
            return Err("ui_host_capability_reason_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiLiveHostStatus {
    NotSupported,
    OptedIn,
    Verified,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiLiveHostEvidence {
    pub schema: String,
    pub protocol_version: String,
    pub surface: UiSurface,
    pub host_id: String,
    pub host_version: String,
    pub environment_digest: String,
    pub workspace_digest: String,
    #[serde(default)]
    pub session_ref: Option<String>,
    pub status: UiLiveHostStatus,
    #[serde(default)]
    pub operator_approval_ref: Option<String>,
    pub host_capabilities: Vec<UiHostCapability>,
    pub handshake_verified: bool,
    pub session_verified: bool,
    pub prompt_routed: bool,
    pub update_observed: bool,
    pub permission_routed: bool,
    pub cancel_fence_verified: bool,
    pub reconnect_verified: bool,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
    pub evidence_digest: String,
}

impl UiLiveHostEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        protocol_version: impl Into<String>,
        surface: UiSurface,
        host_id: impl Into<String>,
        host_version: impl Into<String>,
        environment_digest: impl Into<String>,
        workspace_digest: impl Into<String>,
        session_ref: Option<String>,
        status: UiLiveHostStatus,
        operator_approval_ref: Option<String>,
        host_capabilities: Vec<UiHostCapability>,
        handshake_verified: bool,
        session_verified: bool,
        prompt_routed: bool,
        update_observed: bool,
        permission_routed: bool,
        cancel_fence_verified: bool,
        reconnect_verified: bool,
        receipt_digest: Option<String>,
        limitations: Vec<EvidenceLimitation>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: UI_LIVE_HOST_EVIDENCE_SCHEMA.to_owned(),
            protocol_version: protocol_version.into(),
            surface,
            host_id: host_id.into(),
            host_version: host_version.into(),
            environment_digest: environment_digest.into(),
            workspace_digest: workspace_digest.into(),
            session_ref,
            status,
            operator_approval_ref,
            host_capabilities,
            handshake_verified,
            session_verified,
            prompt_routed,
            update_observed,
            permission_routed,
            cancel_fence_verified,
            reconnect_verified,
            receipt_digest,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_LIVE_HOST_EVIDENCE_SCHEMA {
            return Err("ui_live_host_evidence_schema_invalid".to_owned());
        }
        required(&self.protocol_version, "ui_live_protocol_version", 64)?;
        required(&self.host_id, "ui_live_host_id", 256)?;
        required(&self.host_version, "ui_live_host_version", 128)?;
        digest(&self.environment_digest, "ui_live_environment_digest")?;
        digest(&self.workspace_digest, "ui_live_workspace_digest")?;
        if self
            .session_ref
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("ui_live_session_ref_invalid".to_owned());
        }
        if let Some(value) = &self.operator_approval_ref {
            required(value, "ui_live_operator_approval_ref", 256)?;
        }
        if self.host_capabilities.len() > 128 {
            return Err("ui_live_host_capability_limit".to_owned());
        }
        let mut capability_ids = BTreeSet::new();
        for capability in &self.host_capabilities {
            capability.validate()?;
            if !capability_ids.insert(capability.capability_id.clone()) {
                return Err("ui_live_host_capability_duplicate".to_owned());
            }
        }
        if let Some(receipt) = &self.receipt_digest {
            digest(receipt, "ui_live_host_receipt_digest")?;
        }
        if self.limitations.len() > 128 {
            return Err("ui_live_host_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        match self.status {
            UiLiveHostStatus::NotSupported | UiLiveHostStatus::Unknown => {
                if self.limitations.is_empty() {
                    return Err("ui_live_host_limitation_required".to_owned());
                }
            }
            UiLiveHostStatus::OptedIn => {
                if self.operator_approval_ref.is_none() || self.limitations.is_empty() {
                    return Err("ui_live_host_opt_in_evidence_incomplete".to_owned());
                }
            }
            UiLiveHostStatus::Verified => {
                if self.operator_approval_ref.is_none()
                    || self.session_ref.is_none()
                    || self.receipt_digest.is_none()
                    || self.host_capabilities.is_empty()
                    || !self.handshake_verified
                    || !self.session_verified
                    || !self.prompt_routed
                    || !self.update_observed
                    || !self.permission_routed
                    || !self.cancel_fence_verified
                    || !self.reconnect_verified
                {
                    return Err("ui_live_host_verified_evidence_incomplete".to_owned());
                }
            }
        }
        if self.evidence_digest != self.digest() {
            return Err("ui_live_host_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "protocol_version": self.protocol_version,
            "surface": self.surface,
            "host_id": self.host_id,
            "host_version": self.host_version,
            "environment_digest": self.environment_digest,
            "workspace_digest": self.workspace_digest,
            "session_ref": self.session_ref,
            "status": self.status,
            "operator_approval_ref": self.operator_approval_ref,
            "host_capabilities": self.host_capabilities,
            "handshake_verified": self.handshake_verified,
            "session_verified": self.session_verified,
            "prompt_routed": self.prompt_routed,
            "update_observed": self.update_observed,
            "permission_routed": self.permission_routed,
            "cancel_fence_verified": self.cancel_fence_verified,
            "reconnect_verified": self.reconnect_verified,
            "receipt_digest": self.receipt_digest,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiEvidenceClass {
    Deny,
    Recovery,
    Happy,
    Parity,
    Performance,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiEvidenceOutcome {
    Passed,
    Failed,
    Skipped,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFeatureStatus {
    Implemented,
    Partial,
    Target,
    Deferred,
    NotSupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
    Physical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEvidenceCase {
    pub schema: String,
    pub case_id: String,
    pub surface: UiSurface,
    pub class: UiEvidenceClass,
    pub command_argv: Vec<String>,
    pub source_snapshot: String,
    pub fixture_digest: String,
    pub environment_digest: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub feature_status: UiFeatureStatus,
    pub proof_level: UiProofLevel,
    pub outcome: UiEvidenceOutcome,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    #[serde(default)]
    pub artifact_digests: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
    pub reviewer: String,
    pub case_digest: String,
}

impl UiEvidenceCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case_id: impl Into<String>,
        surface: UiSurface,
        class: UiEvidenceClass,
        command_argv: Vec<String>,
        source_snapshot: impl Into<String>,
        fixture_digest: impl Into<String>,
        environment_digest: impl Into<String>,
        exit_code: Option<i32>,
        feature_status: UiFeatureStatus,
        proof_level: UiProofLevel,
        outcome: UiEvidenceOutcome,
        receipt_digest: Option<String>,
        mut artifact_digests: Vec<String>,
        limitations: Vec<EvidenceLimitation>,
        reviewer: impl Into<String>,
    ) -> Result<Self, String> {
        artifact_digests.sort();
        artifact_digests.dedup();
        let mut case = Self {
            schema: UI_EVIDENCE_CASE_SCHEMA.to_owned(),
            case_id: case_id.into(),
            surface,
            class,
            command_argv,
            source_snapshot: source_snapshot.into(),
            fixture_digest: fixture_digest.into(),
            environment_digest: environment_digest.into(),
            exit_code,
            feature_status,
            proof_level,
            outcome,
            receipt_digest,
            artifact_digests,
            limitations,
            reviewer: reviewer.into(),
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_EVIDENCE_CASE_SCHEMA {
            return Err("ui_evidence_case_schema_invalid".to_owned());
        }
        required(&self.case_id, "ui_evidence_case_id", 128)?;
        required(&self.source_snapshot, "ui_evidence_source_snapshot", 256)?;
        required(&self.reviewer, "ui_evidence_reviewer", 256)?;
        if self.command_argv.is_empty() || self.command_argv.len() > 64 {
            return Err("ui_evidence_command_argv_invalid".to_owned());
        }
        for argument in &self.command_argv {
            required(argument, "ui_evidence_command_argument", 4_096)?;
            let lower = argument.to_ascii_lowercase();
            if lower.contains("api_key=")
                || lower.contains("token=")
                || lower.contains("secret=")
                || lower.contains("bearer ")
            {
                return Err("ui_evidence_secret_in_command_argv".to_owned());
            }
        }
        digest(&self.fixture_digest, "ui_evidence_fixture_digest")?;
        digest(&self.environment_digest, "ui_evidence_environment_digest")?;
        if self.outcome != UiEvidenceOutcome::Skipped && self.exit_code.is_none() {
            return Err("ui_evidence_exit_code_missing".to_owned());
        }
        if self.outcome == UiEvidenceOutcome::Passed && self.exit_code != Some(0) {
            return Err("ui_evidence_pass_exit_code_invalid".to_owned());
        }
        if let Some(receipt) = &self.receipt_digest {
            digest(receipt, "ui_evidence_receipt_digest")?;
        }
        if self
            .artifact_digests
            .iter()
            .any(|value| digest(value, "ui_evidence_artifact_digest").is_err())
            || self
                .artifact_digests
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("ui_evidence_artifact_digests_invalid".to_owned());
        }
        if self.limitations.len() > 128 {
            return Err("ui_evidence_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        if self.feature_status == UiFeatureStatus::Implemented
            && self.proof_level == UiProofLevel::Source
        {
            return Err("ui_evidence_implemented_requires_behavior".to_owned());
        }
        if self.class == UiEvidenceClass::Live
            && self.outcome == UiEvidenceOutcome::Passed
            && !matches!(
                self.proof_level,
                UiProofLevel::Live | UiProofLevel::Physical
            )
        {
            return Err("ui_evidence_live_proof_required".to_owned());
        }
        if matches!(
            self.proof_level,
            UiProofLevel::Durable | UiProofLevel::Live | UiProofLevel::Physical
        ) && self.receipt_digest.is_none()
        {
            return Err("ui_evidence_receipt_required".to_owned());
        }
        if self.outcome != UiEvidenceOutcome::Passed && self.limitations.is_empty() {
            return Err("ui_evidence_outcome_limitation_required".to_owned());
        }
        if self.case_digest != self.digest() {
            return Err("ui_evidence_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "case_id": self.case_id,
            "surface": self.surface,
            "class": self.class,
            "command_argv": self.command_argv,
            "source_snapshot": self.source_snapshot,
            "fixture_digest": self.fixture_digest,
            "environment_digest": self.environment_digest,
            "exit_code": self.exit_code,
            "feature_status": self.feature_status,
            "proof_level": self.proof_level,
            "outcome": self.outcome,
            "receipt_digest": self.receipt_digest,
            "artifact_digests": self.artifact_digests,
            "limitations": self.limitations,
            "reviewer": self.reviewer,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEvidenceBundle {
    pub schema: String,
    pub source_snapshot: String,
    pub cases: Vec<UiEvidenceCase>,
    pub bundle_digest: String,
}

impl UiEvidenceBundle {
    pub fn new(
        source_snapshot: impl Into<String>,
        cases: Vec<UiEvidenceCase>,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            schema: UI_EVIDENCE_BUNDLE_SCHEMA.to_owned(),
            source_snapshot: source_snapshot.into(),
            cases,
            bundle_digest: String::new(),
        };
        bundle.bundle_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_EVIDENCE_BUNDLE_SCHEMA {
            return Err("ui_evidence_bundle_schema_invalid".to_owned());
        }
        required(
            &self.source_snapshot,
            "ui_evidence_bundle_source_snapshot",
            256,
        )?;
        if self.cases.is_empty() || self.cases.len() > 256 {
            return Err("ui_evidence_bundle_case_count_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.source_snapshot != self.source_snapshot {
                return Err("ui_evidence_bundle_source_snapshot_drift".to_owned());
            }
            if !ids.insert(case.case_id.clone()) {
                return Err("ui_evidence_bundle_case_duplicate".to_owned());
            }
        }
        if self.bundle_digest != self.digest() {
            return Err("ui_evidence_bundle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source_snapshot": self.source_snapshot,
            "cases": self.cases,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub title: String,
    pub status: String,
    pub revision: u64,
}

impl SessionSummary {
    fn validate(&self) -> Result<(), String> {
        required(self.session_id.as_str(), "ui_session_id", 256)?;
        required(&self.title, "ui_session_title", 512)?;
        required(&self.status, "ui_session_status", 64)?;
        if self.revision == 0 {
            return Err("ui_session_revision_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptRef {
    pub receipt_id: ReceiptId,
    pub receipt_digest: String,
}

impl ReceiptRef {
    fn validate(&self) -> Result<(), String> {
        digest(&self.receipt_digest, "ui_receipt_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSummary {
    pub artifact_id: ArtifactId,
    pub digest: String,
    pub mime: String,
    pub size_bytes: u64,
}

impl ArtifactSummary {
    fn validate(&self) -> Result<(), String> {
        digest(&self.digest, "ui_artifact_digest")?;
        required(&self.mime, "ui_artifact_mime", 128)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSummary {
    pub run_id: RunId,
    pub status: ExecutionStatus,
    pub revision: u64,
    #[serde(default)]
    pub final_assistant_text: Option<String>,
    #[serde(default)]
    pub receipt: Option<ReceiptRef>,
}

impl RunSummary {
    fn validate(&self) -> Result<(), String> {
        if self.revision == 0 {
            return Err("ui_run_revision_invalid".to_owned());
        }
        if self
            .final_assistant_text
            .as_deref()
            .is_some_and(|text| text.len() > 64 * 1024 || text.contains('\0'))
        {
            return Err("ui_run_text_invalid".to_owned());
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanActionCard {
    pub action_id: String,
    pub command: String,
    pub target_id: String,
    pub expected_revision: Option<u64>,
    pub expires_at_unix_ms: Option<u64>,
    pub allowed_decisions: Vec<String>,
    pub payload_digest: String,
}

impl HumanActionCard {
    fn validate(&self) -> Result<(), String> {
        required(&self.action_id, "ui_action_id", 256)?;
        required(&self.command, "ui_action_command", 128)?;
        required(&self.target_id, "ui_action_target", 256)?;
        digest(&self.payload_digest, "ui_action_payload_digest")?;
        if self.expected_revision == Some(0) || self.expires_at_unix_ms == Some(0) {
            return Err("ui_action_card_revision_invalid".to_owned());
        }
        if self.allowed_decisions.len() > 16
            || self
                .allowed_decisions
                .iter()
                .any(|decision| decision.trim().is_empty() || decision.len() > 64)
        {
            return Err("ui_action_decisions_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiNotice {
    pub notice_id: String,
    pub severity: UiSeverity,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub action_refs: Vec<String>,
}

impl UiNotice {
    fn validate(&self) -> Result<(), String> {
        required(&self.notice_id, "ui_notice_id", 256)?;
        required(&self.title, "ui_notice_title", 512)?;
        required(&self.summary, "ui_notice_summary", 4_096)?;
        if self.action_refs.len() > 32
            || self
                .action_refs
                .iter()
                .any(|reference| reference.trim().is_empty() || reference.len() > 256)
        {
            return Err("ui_notice_actions_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLimitation {
    pub code: String,
    pub detail: String,
}

impl EvidenceLimitation {
    fn validate(&self) -> Result<(), String> {
        required(&self.code, "ui_limitation_code", 128)?;
        required(&self.detail, "ui_limitation_detail", 1_024)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotV1 {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub snapshot_cursor: UiCursorV1,
    pub generated_at_unix_ms: u64,
    pub principal: AuthenticatedPrincipalRef,
    pub workspace_id: ProjectId,
    #[serde(default)]
    pub capabilities: Vec<UiCapability>,
    pub sessions: Vec<SessionSummary>,
    #[serde(default)]
    pub active_session_id: Option<SessionId>,
    #[serde(default)]
    pub runs: Vec<RunSummary>,
    #[serde(default)]
    pub pending_actions: Vec<HumanActionCard>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactSummary>,
    #[serde(default)]
    pub notices: Vec<UiNotice>,
    #[serde(default)]
    pub next_page: Option<String>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
}

impl UiSnapshotV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_SCHEMA
            || self.instance_id.trim().is_empty()
            || self.instance_id.len() > 256
            || self.authority_epoch == 0
            || self.generated_at_unix_ms == 0
        {
            return Err("ui_snapshot_header_invalid".to_owned());
        }
        self.snapshot_cursor.validate()?;
        self.principal.validate()?;
        if self.capabilities.len() > 128
            || self.sessions.len() > 256
            || self.runs.len() > 512
            || self.pending_actions.len() > 256
            || self.artifacts.len() > 512
            || self.notices.len() > 512
            || self.limitations.len() > 128
        {
            return Err("ui_snapshot_limit".to_owned());
        }
        let mut sessions = BTreeSet::new();
        for session in &self.sessions {
            session.validate()?;
            if !sessions.insert(session.session_id.to_string()) {
                return Err("ui_snapshot_session_duplicate".to_owned());
            }
        }
        if let Some(active) = &self.active_session_id {
            if !sessions.contains(active.as_str()) {
                return Err("ui_snapshot_active_session_missing".to_owned());
            }
        }
        let mut runs = BTreeSet::new();
        for run in &self.runs {
            run.validate()?;
            if !runs.insert(run.run_id.to_string()) {
                return Err("ui_snapshot_run_duplicate".to_owned());
            }
        }
        let mut actions = BTreeSet::new();
        for action in &self.pending_actions {
            action.validate()?;
            if !actions.insert(action.action_id.clone()) {
                return Err("ui_snapshot_action_duplicate".to_owned());
            }
        }
        for capability in &self.capabilities {
            capability.validate()?;
        }
        for artifact in &self.artifacts {
            artifact.validate()?;
        }
        for notice in &self.notices {
            notice.validate()?;
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCursorV1 {
    pub epoch: String,
    pub sequence: u64,
}

impl UiCursorV1 {
    pub fn validate(&self) -> Result<(), String> {
        cursor_valid(&self.epoch, self.sequence)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFeedEnvelope {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub feed_sequence: u64,
    pub snapshot_cursor: UiCursorV1,
    pub event_id: String,
    #[serde(default)]
    pub aggregate_type: Option<String>,
    #[serde(default)]
    pub aggregate_id: Option<String>,
    pub object_revision: u64,
    pub event: Value,
}

impl UiFeedEnvelope {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_FEED_SCHEMA
            || self.instance_id.trim().is_empty()
            || self.instance_id.len() > 256
            || self.authority_epoch == 0
            || self.feed_sequence == 0
            || self.object_revision == 0
            || self.event_id.trim().is_empty()
            || self.event_id.len() > 256
        {
            return Err("ui_feed_header_invalid".to_owned());
        }
        self.snapshot_cursor.validate()?;
        if self
            .aggregate_type
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 128)
            || self
                .aggregate_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("ui_feed_aggregate_invalid".to_owned());
        }
        validate_json_limits(&self.event).map_err(|_| "ui_feed_event_invalid".to_owned())
    }
}

/// Cursor for the daemon-owned UI feed.  It is intentionally separate from the EventLog cursor:
/// the feed is a bounded display projection and an old or foreign cursor can only request a
/// snapshot/gap, never authorize a replay or an effect.
pub const UI_FEED_CURSOR_SCHEMA: &str = "kiana.ui-feed-cursor.v1";
pub const UI_FEED_FRAME_SCHEMA: &str = "kiana.ui-feed-frame.v1";
pub const UI_FEED_GAP_SCHEMA: &str = "kiana.ui-feed-gap.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFeedGapReason {
    SequenceGap,
    ReplayExpired,
    OldEpoch,
    InstanceChanged,
    SequenceAhead,
    Backpressure,
    TerminalRetention,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFeedFrameKind {
    SnapshotBoundary,
    Delta,
    Heartbeat,
    Gap,
    Terminal,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFeedCursorV1 {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: String,
    pub feed_sequence: u64,
    pub snapshot_cursor: UiCursor,
    pub cursor_digest: String,
}

impl UiFeedCursorV1 {
    pub fn new(
        instance_id: impl Into<String>,
        authority_epoch: impl Into<String>,
        feed_sequence: u64,
        snapshot_cursor: UiCursor,
    ) -> Result<Self, String> {
        let mut cursor = Self {
            schema: UI_FEED_CURSOR_SCHEMA.to_owned(),
            instance_id: instance_id.into(),
            authority_epoch: authority_epoch.into(),
            feed_sequence,
            snapshot_cursor,
            cursor_digest: String::new(),
        };
        cursor.cursor_digest = cursor.digest();
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_FEED_CURSOR_SCHEMA {
            return Err("ui_feed_cursor_header_invalid".to_owned());
        }
        required(&self.instance_id, "ui_feed_cursor_instance", 256)?;
        required(&self.authority_epoch, "ui_feed_cursor_epoch", 256)?;
        if self.snapshot_cursor.sequence > 0 {
            required(
                &self.snapshot_cursor.epoch,
                "ui_feed_cursor_snapshot_epoch",
                256,
            )?;
            if self.snapshot_cursor.epoch != self.authority_epoch {
                return Err("ui_feed_cursor_snapshot_epoch_mismatch".to_owned());
            }
        } else if !self.snapshot_cursor.epoch.is_empty() {
            return Err("ui_feed_cursor_snapshot_invalid".to_owned());
        }
        digest(&self.cursor_digest, "ui_feed_cursor_digest")?;
        if self.cursor_digest != self.digest() {
            return Err("ui_feed_cursor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| "ui_feed_cursor_encode_failed".to_owned())
    }

    pub fn decode(value: &str) -> Result<Self, String> {
        if value.len() > 2_048 {
            return Err("ui_feed_cursor_too_large".to_owned());
        }
        let cursor: Self = serde_json::from_str(value)
            .map_err(|_| "ui_feed_cursor_decode_failed".to_owned())?;
        cursor.validate()?;
        Ok(cursor)
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "instance_id": self.instance_id,
            "authority_epoch": self.authority_epoch,
            "feed_sequence": self.feed_sequence,
            "snapshot_cursor": self.snapshot_cursor,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFeedGapV1 {
    pub schema: String,
    pub reason: UiFeedGapReason,
    pub from: Option<UiFeedCursorV1>,
    pub to: UiFeedCursorV1,
    pub snapshot_required: bool,
}

impl UiFeedGapV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_FEED_GAP_SCHEMA {
            return Err("ui_feed_gap_schema_invalid".to_owned());
        }
        self.to.validate()?;
        if let Some(from) = &self.from {
            from.validate()?;
            if from.instance_id == self.to.instance_id
                && from.authority_epoch == self.to.authority_epoch
                && from.feed_sequence > self.to.feed_sequence
            {
                return Err("ui_feed_gap_range_invalid".to_owned());
            }
        }
        if !self.snapshot_required {
            return Err("ui_feed_gap_snapshot_required".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFeedFrameV1 {
    pub schema: String,
    pub kind: UiFeedFrameKind,
    pub cursor: UiFeedCursorV1,
    pub event_id: String,
    pub replay: bool,
    pub terminal: bool,
    #[serde(default)]
    pub event: Option<Value>,
    #[serde(default)]
    pub gap: Option<UiFeedGapV1>,
}

impl UiFeedFrameV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_FEED_FRAME_SCHEMA {
            return Err("ui_feed_frame_schema_invalid".to_owned());
        }
        self.cursor.validate()?;
        required(&self.event_id, "ui_feed_frame_event_id", 512)?;
        if self.terminal != matches!(self.kind, UiFeedFrameKind::Terminal) {
            return Err("ui_feed_frame_terminal_mismatch".to_owned());
        }
        if self.kind == UiFeedFrameKind::Gap {
            let gap = self
                .gap
                .as_ref()
                .ok_or_else(|| "ui_feed_frame_gap_missing".to_owned())?;
            gap.validate()?;
        } else if self.gap.is_some() {
            return Err("ui_feed_frame_gap_unexpected".to_owned());
        }
        if let Some(event) = &self.event {
            validate_json_limits(event).map_err(|_| "ui_feed_frame_event_invalid".to_owned())?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionV1 {
    pub schema: String,
    pub command_id: kiana_domain::RequestId,
    pub idempotency_key: String,
    pub target_id: String,
    pub expected_epoch: String,
    pub expected_cursor: u64,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub payload: Value,
    pub payload_digest: String,
    pub submitted_by: String,
    #[serde(default)]
    pub deadline_unix_ms: Option<u64>,
}

impl UiActionV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_SCHEMA {
            return Err("ui_action_schema_invalid".to_owned());
        }
        required(&self.idempotency_key, "ui_action_idempotency_key", 256)?;
        required(&self.target_id, "ui_action_target", 256)?;
        required(&self.submitted_by, "ui_action_submitted_by", 256)?;
        cursor_valid(&self.expected_epoch, self.expected_cursor)?;
        if self.expected_revision == Some(0) || self.deadline_unix_ms == Some(0) {
            return Err("ui_action_revision_invalid".to_owned());
        }
        validate_json_limits(&self.payload).map_err(|_| "ui_action_payload_invalid".to_owned())?;
        digest(&self.payload_digest, "ui_action_payload_digest")?;
        if self.payload_digest != json_digest(&self.payload) {
            return Err("ui_action_payload_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionResult {
    pub schema: String,
    pub command_id: kiana_domain::RequestId,
    pub disposition: UiActionDisposition,
    #[serde(default)]
    pub resulting_cursor: Option<UiCursorV1>,
    #[serde(default)]
    pub resulting_revision: Option<u64>,
    #[serde(default)]
    pub receipt: Option<ReceiptRef>,
    #[serde(default)]
    pub error: Option<UiError>,
    pub retry: UiRetryDisposition,
}

impl UiActionResult {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_RESULT_SCHEMA {
            return Err("ui_action_result_schema_invalid".to_owned());
        }
        if self.resulting_revision == Some(0) {
            return Err("ui_action_result_revision_invalid".to_owned());
        }
        if let Some(cursor) = &self.resulting_cursor {
            cursor.validate()?;
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
        }
        if let Some(error) = &self.error {
            error.validate()?;
        }
        match self.disposition {
            UiActionDisposition::Applied
                if self.resulting_cursor.is_none() && self.receipt.is_none() =>
            {
                Err("ui_action_applied_evidence_missing".to_owned())
            }
            UiActionDisposition::Rejected if self.error.is_none() => {
                Err("ui_action_rejected_error_missing".to_owned())
            }
            UiActionDisposition::Unknown if self.retry != UiRetryDisposition::QueryOriginal => {
                Err("ui_action_unknown_retry_invalid".to_owned())
            }
            _ => Ok(()),
        }
    }
}

// Names used by surface adapters while the legacy UiSnapshot/UiAction remain available.
pub type VersionedUiSnapshot = UiSnapshotV1;
pub type VersionedUiAction = UiActionV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiTransportKind {
    InProcess,
    UnixSocket,
    NamedPipe,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiInstanceRecord {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub protocol_schema: String,
    pub workspace_digest: String,
    pub transport: UiTransportKind,
    pub endpoint_digest: String,
    pub pid: u32,
    pub ready: bool,
    pub record_digest: String,
}

impl UiInstanceRecord {
    pub fn new(
        instance_id: impl Into<String>,
        authority_epoch: u64,
        workspace: &str,
        transport: UiTransportKind,
        endpoint: &str,
        pid: u32,
        ready: bool,
    ) -> Result<Self, String> {
        if workspace.trim().is_empty() || endpoint.trim().is_empty() {
            return Err("ui_instance_endpoint_required".to_owned());
        }
        let mut record = Self {
            schema: UI_INSTANCE_RECORD_SCHEMA.to_owned(),
            instance_id: instance_id.into(),
            authority_epoch,
            protocol_schema: super::PROTOCOL_SCHEMA.to_owned(),
            workspace_digest: json_digest(&json!({"workspace": workspace})),
            transport,
            endpoint_digest: json_digest(&json!({"endpoint": endpoint})),
            pid,
            ready,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_INSTANCE_RECORD_SCHEMA
            || self.protocol_schema != super::PROTOCOL_SCHEMA
            || self.authority_epoch == 0
            || self.pid == 0
            || !self.ready
            || self.instance_id.trim().is_empty()
            || self.instance_id.len() > 256
            || self.instance_id.contains('/')
            || self.instance_id.contains('\\')
            || self.instance_id.contains('\0')
        {
            return Err("ui_instance_record_header_invalid".to_owned());
        }
        digest(&self.workspace_digest, "ui_instance_workspace_digest")?;
        digest(&self.endpoint_digest, "ui_instance_endpoint_digest")?;
        digest(&self.record_digest, "ui_instance_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("ui_instance_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_peer(
        &self,
        workspace: &str,
        protocol_schema: &str,
        expected_epoch: Option<u64>,
    ) -> Result<(), String> {
        self.validate()?;
        if self.protocol_schema != protocol_schema
            || self.workspace_digest != json_digest(&json!({"workspace": workspace}))
            || expected_epoch.is_some_and(|epoch| epoch != self.authority_epoch)
        {
            return Err("ui_instance_peer_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("record_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSurface {
    Cli,
    Workbench,
    Web,
    Desktop,
    Ide,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCapabilityRequest {
    pub capability_id: String,
    pub feature_version: String,
}

impl UiCapabilityRequest {
    fn validate(&self) -> Result<(), String> {
        required(&self.capability_id, "ui_requested_capability", 128)?;
        required(&self.feature_version, "ui_feature_version", 64)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHandshakeRequest {
    pub schema: String,
    pub client_version: String,
    pub surface: UiSurface,
    #[serde(default)]
    pub requested_capabilities: Vec<UiCapabilityRequest>,
    #[serde(default)]
    pub known_instance_id: Option<String>,
    #[serde(default)]
    pub known_authority_epoch: Option<u64>,
}

impl UiHandshakeRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HANDSHAKE_REQUEST_SCHEMA {
            return Err("ui_handshake_request_schema_invalid".to_owned());
        }
        required(&self.client_version, "ui_client_version", 64)?;
        if self.requested_capabilities.len() > 128 {
            return Err("ui_handshake_capability_limit".to_owned());
        }
        let mut capabilities = BTreeSet::new();
        for capability in &self.requested_capabilities {
            capability.validate()?;
            if !capabilities.insert(capability.capability_id.clone()) {
                return Err("ui_handshake_capability_duplicate".to_owned());
            }
        }
        if self.known_authority_epoch == Some(0)
            || self
                .known_instance_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("ui_handshake_known_instance_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHandshakeResponse {
    pub schema: String,
    pub server_version: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub surface: UiSurface,
    pub capabilities: Vec<UiCapability>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
}

impl UiHandshakeResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HANDSHAKE_RESPONSE_SCHEMA || self.authority_epoch == 0 {
            return Err("ui_handshake_response_header_invalid".to_owned());
        }
        required(&self.server_version, "ui_server_version", 64)?;
        required(&self.instance_id, "ui_instance_id", 256)?;
        if self.capabilities.len() > 128 || self.limitations.len() > 128 {
            return Err("ui_handshake_response_limit".to_owned());
        }
        let mut ids = BTreeSet::new();
        for capability in &self.capabilities {
            capability.validate()?;
            if !ids.insert(capability.capability_id.clone()) {
                return Err("ui_handshake_response_capability_duplicate".to_owned());
            }
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHealth {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub status: String,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
}

impl UiHealth {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HEALTH_SCHEMA || self.authority_epoch == 0 {
            return Err("ui_health_header_invalid".to_owned());
        }
        required(&self.instance_id, "ui_health_instance", 256)?;
        required(&self.status, "ui_health_status", 64)?;
        if self.capabilities.len() > 128 || self.limitations.len() > 128 {
            return Err("ui_health_limit".to_owned());
        }
        if self
            .capabilities
            .iter()
            .any(|capability| capability.trim().is_empty() || capability.len() > 128)
        {
            return Err("ui_health_capability_invalid".to_owned());
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        Ok(())
    }
}

/// Intersect server-derived principal and surface capability sets. Client requests can only
/// narrow the result; a scope digest mismatch disables the capability rather than widening it.
pub fn intersect_ui_capabilities(
    principal: &[UiCapability],
    surface: &[UiCapability],
    requested: &[UiCapabilityRequest],
) -> Result<Vec<UiCapability>, String> {
    let mut requested_ids = BTreeSet::new();
    for request in requested {
        request.validate()?;
        if !requested_ids.insert(request.capability_id.clone()) {
            return Err("ui_handshake_capability_duplicate".to_owned());
        }
    }
    let principal = principal
        .iter()
        .map(|capability| (capability.capability_id.clone(), capability))
        .collect::<std::collections::BTreeMap<_, _>>();
    let surface = surface
        .iter()
        .map(|capability| (capability.capability_id.clone(), capability))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut result = Vec::new();
    for request in requested {
        let Some(left) = principal.get(&request.capability_id) else {
            result.push(UiCapability {
                schema: UI_CAPABILITY_SCHEMA.to_owned(),
                capability_id: request.capability_id.clone(),
                enabled: false,
                actions: Vec::new(),
                reason: Some("principal_capability_missing".to_owned()),
                scope_digest: kiana_domain::json_digest(
                    &json!({"capability": request.capability_id}),
                ),
            });
            continue;
        };
        let Some(right) = surface.get(&request.capability_id) else {
            result.push(UiCapability {
                schema: UI_CAPABILITY_SCHEMA.to_owned(),
                capability_id: request.capability_id.clone(),
                enabled: false,
                actions: Vec::new(),
                reason: Some("surface_capability_missing".to_owned()),
                scope_digest: left.scope_digest.clone(),
            });
            continue;
        };
        let enabled = left.enabled && right.enabled && left.scope_digest == right.scope_digest;
        let mut actions = left
            .actions
            .iter()
            .filter(|action| right.actions.contains(action))
            .cloned()
            .collect::<Vec<_>>();
        actions.sort();
        result.push(UiCapability {
            schema: UI_CAPABILITY_SCHEMA.to_owned(),
            capability_id: request.capability_id.clone(),
            enabled,
            actions,
            reason: (!enabled).then(|| {
                if left.scope_digest != right.scope_digest {
                    "ui_scope_intersection_empty".to_owned()
                } else {
                    "capability_disabled_by_intersection".to_owned()
                }
            }),
            scope_digest: left.scope_digest.clone(),
        });
    }
    Ok(result)
}

pub type StableError = UiError;

pub fn stable_error_from_response(response: &super::ResponseEnvelope) -> Option<UiError> {
    if response.status == kiana_domain::ExecutionStatus::Completed && response.error.is_none() {
        return None;
    }
    let code = response
        .failure_code()
        .map(|code| match code {
            kiana_domain::CapabilityErrorCode::InvalidArguments => UiErrorCode::InvalidRequest,
            kiana_domain::CapabilityErrorCode::SchemaUnsupported => UiErrorCode::SchemaUnsupported,
            kiana_domain::CapabilityErrorCode::PermissionDenied
            | kiana_domain::CapabilityErrorCode::ProjectUntrusted => UiErrorCode::PermissionDenied,
            kiana_domain::CapabilityErrorCode::ApprovalRequired => UiErrorCode::ApprovalRequired,
            kiana_domain::CapabilityErrorCode::Conflict => UiErrorCode::Conflict,
            kiana_domain::CapabilityErrorCode::BudgetExceeded => UiErrorCode::Capacity,
            kiana_domain::CapabilityErrorCode::Unavailable => UiErrorCode::Unavailable,
            kiana_domain::CapabilityErrorCode::Cancelled => UiErrorCode::Cancelled,
            kiana_domain::CapabilityErrorCode::ResultUnknown => UiErrorCode::Unknown,
            kiana_domain::CapabilityErrorCode::ExecutionFailed
            | kiana_domain::CapabilityErrorCode::CompensationRequired
            | kiana_domain::CapabilityErrorCode::TimedOut => UiErrorCode::Failed,
            _ => UiErrorCode::Failed,
        })
        .unwrap_or(UiErrorCode::Failed);
    let retry = match code {
        UiErrorCode::Unknown => UiRetryDisposition::QueryOriginal,
        UiErrorCode::Unavailable | UiErrorCode::Capacity => UiRetryDisposition::DoNotRetry,
        _ => UiRetryDisposition::DoNotRetry,
    };
    Some(UiError::new(code, ui_error_message(code), retry))
}

fn ui_error_message(code: UiErrorCode) -> &'static str {
    match code {
        UiErrorCode::InvalidRequest => "Request is invalid",
        UiErrorCode::SchemaUnsupported => "Protocol version is unsupported",
        UiErrorCode::PermissionDenied => "Permission denied",
        UiErrorCode::ApprovalRequired => "Approval is required",
        UiErrorCode::Conflict => "State conflict; refresh and try again",
        UiErrorCode::Capacity => "Capacity limit reached",
        UiErrorCode::Unavailable => "Service is unavailable",
        UiErrorCode::Cancelled => "Request cancelled",
        UiErrorCode::Failed => "Request failed",
        UiErrorCode::Unknown => "Request outcome is unknown; query the original command",
        UiErrorCode::Persistence => "Persistence failed",
    }
}
