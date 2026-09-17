//! Stable security reason codes and safe remediation policy.
//!
//! A reason is an explanation value, not an authorization decision.  The wire contract carries a
//! stable code, retry/remediation classification and optional digests/references; it never carries
//! arbitrary provider, prompt, command-line or secret text.  Unknown legacy reasons collapse into
//! an explicit `UNKNOWN_*` code with the most conservative policy.

use crate::{json_digest, EvidenceRefId, OperationId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const SECURITY_REASON_SCHEMA: &str = "kiana.security-reason.v1";
pub const SECURITY_REASON_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SECURITY_REASON_EVIDENCE: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityReasonClass {
    Auth,
    Policy,
    Data,
    Secret,
    Extension,
    Filesystem,
    Network,
    Resource,
    Fact,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityRetryability {
    Never,
    RequiresReauthorization,
    RequiresReconciliation,
    RetryWithBackoff,
    ReadOnlyOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityRemediation {
    None,
    Reauthenticate,
    RequestApproval,
    RefreshAuthority,
    TrustProject,
    ClassifyAndAuthorize,
    RebindSecret,
    RepairPath,
    RecheckEndpoint,
    ReduceRequest,
    ReconcileExternalEffect,
    Quarantine,
    Escalate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityReasonPolicy {
    pub class: SecurityReasonClass,
    pub retryability: SecurityRetryability,
    pub remediation: SecurityRemediation,
}

macro_rules! security_reason_codes {
    ($( $variant:ident => ($code:literal, $class:ident, $retry:ident, $remediation:ident) ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum SecurityReasonCode {
            $( $variant, )+
        }

        impl SecurityReasonCode {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $code,)+ }
            }

            pub const fn policy(self) -> SecurityReasonPolicy {
                match self {
                    $(Self::$variant => SecurityReasonPolicy {
                        class: SecurityReasonClass::$class,
                        retryability: SecurityRetryability::$retry,
                        remediation: SecurityRemediation::$remediation,
                    },)+
                }
            }

            /// Parse a wire/legacy code without retaining arbitrary input. Unknown values always
            /// become the conservative `UNKNOWN_UNCLASSIFIED` bucket.
            pub fn parse(value: &str) -> Self {
                let normalized = value.trim().to_ascii_uppercase();
                Self::ALL
                    .iter()
                    .copied()
                    .find(|code| code.as_str() == normalized)
                    .unwrap_or(Self::UnknownUnclassified)
            }

            pub const fn is_unknown(self) -> bool {
                matches!(
                    self,
                    Self::UnknownUnclassified
                        | Self::UnknownResultUnconfirmed
                        | Self::UnknownTimeout
                        | Self::UnknownReconciliationRequired
                )
            }
        }
    };
}

security_reason_codes! {
    AuthCallerUntrusted => ("AUTH_CALLER_UNTRUSTED", Auth, Never, TrustProject),
    AuthPrincipalMissing => ("AUTH_PRINCIPAL_MISSING", Auth, RequiresReauthorization, Reauthenticate),
    AuthSessionExpired => ("AUTH_SESSION_EXPIRED", Auth, RequiresReauthorization, Reauthenticate),
    AuthProjectMismatch => ("AUTH_PROJECT_MISMATCH", Auth, Never, Escalate),
    AuthRoleMismatch => ("AUTH_ROLE_MISMATCH", Auth, Never, Escalate),
    AuthAudienceMismatch => ("AUTH_AUDIENCE_MISMATCH", Auth, Never, Reauthenticate),
    AuthSessionGenerationStale => ("AUTH_SESSION_GENERATION_STALE", Auth, RequiresReauthorization, Reauthenticate),
    PolicyScopeEmpty => ("POLICY_SCOPE_EMPTY", Policy, Never, RequestApproval),
    PolicyApprovalRequired => ("POLICY_APPROVAL_REQUIRED", Policy, RequiresReauthorization, RequestApproval),
    PolicyRevisionStale => ("POLICY_REVISION_STALE", Policy, RequiresReauthorization, RefreshAuthority),
    PolicyAuthorityEpochStale => ("POLICY_AUTHORITY_EPOCH_STALE", Policy, RequiresReauthorization, RefreshAuthority),
    PolicyGrantWidening => ("POLICY_GRANT_WIDENING", Policy, Never, Escalate),
    PolicyOperationUnregistered => ("POLICY_OPERATION_UNREGISTERED", Policy, Never, RequestApproval),
    PolicyBundleInvalid => ("POLICY_BUNDLE_INVALID", Policy, Never, Quarantine),
    PolicyAuthorityEpochRollback => ("POLICY_AUTHORITY_EPOCH_ROLLBACK", Policy, RequiresReconciliation, Quarantine),
    PolicyConfigRevisionStale => ("POLICY_CONFIG_REVISION_STALE", Policy, RequiresReauthorization, RefreshAuthority),
    PolicySelfApprovalForbidden => ("POLICY_SELF_APPROVAL_FORBIDDEN", Policy, Never, Escalate),
    PolicyApprovalBindingMismatch => ("POLICY_APPROVAL_BINDING_MISMATCH", Policy, Never, Quarantine),
    PolicyApprovalExpired => ("POLICY_APPROVAL_EXPIRED", Policy, RequiresReauthorization, RequestApproval),
    DataClassRequired => ("DATA_CLASS_REQUIRED", Data, RequiresReauthorization, ClassifyAndAuthorize),
    DataPurposeDenied => ("DATA_PURPOSE_DENIED", Data, Never, RequestApproval),
    DataBoundaryMismatch => ("DATA_BOUNDARY_MISMATCH", Data, Never, ClassifyAndAuthorize),
    DataRetentionExpired => ("DATA_RETENTION_EXPIRED", Data, ReadOnlyOnly, ClassifyAndAuthorize),
    SecretReferenceInvalid => ("SECRET_REFERENCE_INVALID", Secret, Never, RebindSecret),
    SecretLeaseExpired => ("SECRET_LEASE_EXPIRED", Secret, RequiresReauthorization, RebindSecret),
    SecretAudienceMismatch => ("SECRET_AUDIENCE_MISMATCH", Secret, Never, RebindSecret),
    SecretRedactionFailed => ("SECRET_REDACTION_FAILED", Secret, Never, Quarantine),
    ExtensionUntrusted => ("EXT_UNTRUSTED", Extension, Never, TrustProject),
    ExtensionDigestMismatch => ("EXT_DIGEST_MISMATCH", Extension, Never, Quarantine),
    ExtensionSignatureInvalid => ("EXT_SIGNATURE_INVALID", Extension, Never, Quarantine),
    FilesystemPathEscape => ("FS_PATH_ESCAPE", Filesystem, Never, RepairPath),
    FilesystemToctou => ("FS_TOCTOU", Filesystem, RequiresReauthorization, RepairPath),
    FilesystemRootDrift => ("FS_ROOT_DRIFT", Filesystem, RequiresReauthorization, RepairPath),
    NetworkEndpointDenied => ("NET_ENDPOINT_DENIED", Network, Never, RecheckEndpoint),
    NetworkAudienceMismatch => ("NET_AUDIENCE_MISMATCH", Network, Never, RecheckEndpoint),
    NetworkDnsRebind => ("NET_DNS_REBIND", Network, Never, Quarantine),
    ResourceQuotaExceeded => ("RESOURCE_QUOTA_EXCEEDED", Resource, RetryWithBackoff, ReduceRequest),
    ResourceInputTooLarge => ("RESOURCE_INPUT_TOO_LARGE", Resource, Never, ReduceRequest),
    ResourceQueueFull => ("RESOURCE_QUEUE_FULL", Resource, RetryWithBackoff, ReduceRequest),
    FactSchemaUnknownMajor => ("FACT_SCHEMA_UNKNOWN_MAJOR", Fact, Never, Quarantine),
    FactCursorGap => ("FACT_CURSOR_GAP", Fact, RequiresReconciliation, ReconcileExternalEffect),
    FactSequenceRollback => ("FACT_SEQUENCE_ROLLBACK", Fact, RequiresReconciliation, ReconcileExternalEffect),
    FactDigestMismatch => ("FACT_DIGEST_MISMATCH", Fact, RequiresReconciliation, Quarantine),
    FactFenceMismatch => ("FACT_FENCE_MISMATCH", Fact, RequiresReconciliation, Quarantine),
    UnknownResultUnconfirmed => ("UNKNOWN_RESULT_UNCONFIRMED", Unknown, RequiresReconciliation, ReconcileExternalEffect),
    UnknownTimeout => ("UNKNOWN_TIMEOUT", Unknown, RequiresReconciliation, ReconcileExternalEffect),
    UnknownReconciliationRequired => ("UNKNOWN_RECONCILIATION_REQUIRED", Unknown, RequiresReconciliation, ReconcileExternalEffect),
    UnknownUnclassified => ("UNKNOWN_UNCLASSIFIED", Unknown, RequiresReconciliation, Quarantine),
    UnknownFenceExpired => ("UNKNOWN_FENCE_EXPIRED", Unknown, RequiresReconciliation, Quarantine),
}

impl std::fmt::Display for SecurityReasonCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Map compatibility reason text to a stable security code without echoing the text.
pub fn classify_security_reason(value: &str) -> SecurityReasonCode {
    let normalized = value.trim().to_ascii_lowercase();
    let mut reason = normalized.as_str();
    loop {
        let Some((prefix, detail)) = reason.split_once(':') else {
            break;
        };
        if matches!(prefix, "port_failed" | "port_conflict" | "port_unavailable") {
            reason = detail.trim();
        } else {
            break;
        }
    }
    let code = reason.split(':').next().unwrap_or(reason).trim();
    match code {
        "permission_denied" | "project_untrusted" | "role_path_denied" => {
            SecurityReasonCode::AuthCallerUntrusted
        }
        "actor_identity_required" | "authority_context_incomplete" | "role_unknown" => {
            SecurityReasonCode::AuthPrincipalMissing
        }
        "role_tool_denied" | "role_department_mismatch" => SecurityReasonCode::AuthRoleMismatch,
        "approval_required" | "approval_expired" | "hook_ask_unattended" => {
            SecurityReasonCode::PolicyApprovalRequired
        }
        "authority_epoch_stale" | "policy_revision_stale" => {
            SecurityReasonCode::PolicyAuthorityEpochStale
        }
        "path_escape" | "path_outside_project" | "apply_patch_path_escape" => {
            SecurityReasonCode::FilesystemPathEscape
        }
        "unknown_schema" | "protocol_schema_unsupported" | "schema_version_incompatible" => {
            SecurityReasonCode::FactSchemaUnknownMajor
        }
        "result_unknown" | "shell_result_unknown" | "capability_result_mismatch" => {
            SecurityReasonCode::UnknownResultUnconfirmed
        }
        "timeout" | "shell_timeout" | "timed_out" => SecurityReasonCode::UnknownTimeout,
        "run_budget_exceeded" | "budget_exceeded" | "context_budget_exceeded" => {
            SecurityReasonCode::ResourceQuotaExceeded
        }
        "secret_redaction_failed" | "redaction_failed" => SecurityReasonCode::SecretRedactionFailed,
        _ => SecurityReasonCode::parse(code),
    }
}

/// Strict, serializable security explanation.  `detail_digest` is deliberately a digest, never
/// the original error or provider response.  Optional IDs are correlation only and confer no
/// authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityReason {
    pub schema: String,
    pub version: SchemaVersion,
    pub code: SecurityReasonCode,
    #[serde(default)]
    pub operation_id: Option<OperationId>,
    #[serde(default)]
    pub detail_digest: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRefId>,
    pub reason_digest: String,
}

impl SecurityReason {
    pub fn new(code: SecurityReasonCode) -> Result<Self, String> {
        Self::with_refs(code, None, None, Vec::new())
    }

    pub fn with_refs(
        code: SecurityReasonCode,
        operation_id: Option<OperationId>,
        detail_digest: Option<String>,
        mut evidence_refs: Vec<EvidenceRefId>,
    ) -> Result<Self, String> {
        evidence_refs.sort_by_key(|id| id.as_uuid());
        let mut reason = Self {
            schema: SECURITY_REASON_SCHEMA.to_owned(),
            version: SECURITY_REASON_VERSION,
            code,
            operation_id,
            detail_digest,
            evidence_refs,
            reason_digest: String::new(),
        };
        reason.reason_digest = reason.digest();
        reason.validate()?;
        Ok(reason)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let reason: Self = serde_json::from_value(value.clone())
            .map_err(|_| "security_reason_decode_failed".to_owned())?;
        reason.validate()?;
        Ok(reason)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "security_reason_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_REASON_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_REASON_VERSION)
            || self.evidence_refs.len() > MAX_SECURITY_REASON_EVIDENCE
        {
            return Err("security_reason_header_invalid".to_owned());
        }
        if let Some(operation_id) = self.operation_id {
            if operation_id.as_uuid().is_nil() {
                return Err("security_reason_operation_id_invalid".to_owned());
            }
        }
        let mut ids = BTreeSet::new();
        for evidence_ref in &self.evidence_refs {
            if evidence_ref.as_uuid().is_nil() || !ids.insert(evidence_ref.as_uuid()) {
                return Err("security_reason_evidence_refs_invalid".to_owned());
            }
        }
        if self
            .evidence_refs
            .windows(2)
            .any(|pair| pair[0].as_uuid() >= pair[1].as_uuid())
        {
            return Err("security_reason_evidence_refs_noncanonical".to_owned());
        }
        if let Some(detail_digest) = &self.detail_digest {
            validate_digest(detail_digest, "security_reason_detail_digest")?;
        }
        validate_digest(&self.reason_digest, "security_reason_digest")?;
        if self.reason_digest != self.digest() {
            return Err("security_reason_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub const fn policy(&self) -> SecurityReasonPolicy {
        self.code.policy()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "code": self.code,
            "operation_id": self.operation_id,
            "detail_digest": self.detail_digest,
            "evidence_refs": self.evidence_refs,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
