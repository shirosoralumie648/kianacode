//! Per-connection live provider identity and evidence contracts.
//!
//! These values classify operator-supplied observations. They do not perform a request, grant a
//! model-call permit or turn a synthetic/cassette response into live evidence.

use crate::{json_digest, ModelCapabilities, ModelProtocol, ModelRoute, ModelUsage, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const PROVIDER_LIVE_CONNECTION_SCHEMA: &str = "kiana.provider-live-connection.v1";
pub const PROVIDER_LIVE_EVIDENCE_SCHEMA: &str = "kiana.provider-live-evidence.v1";
pub const PROVIDER_LIVE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderLiveConnectionMetadata {
    pub schema: String,
    pub version: SchemaVersion,
    pub route: ModelRoute,
    pub capabilities: ModelCapabilities,
    pub configuration_digest: String,
    /// A credential revision digest, or `none` for an explicitly credential-free local route.
    pub credential_revision: String,
    /// A non-secret provider/account binding digest.
    pub provider_account: String,
    pub metadata_digest: String,
}

impl ProviderLiveConnectionMetadata {
    pub fn new(
        route: ModelRoute,
        capabilities: ModelCapabilities,
        configuration_digest: impl Into<String>,
        credential_revision: impl Into<String>,
        provider_account: impl Into<String>,
    ) -> Result<Self, String> {
        let mut metadata = Self {
            schema: PROVIDER_LIVE_CONNECTION_SCHEMA.to_owned(),
            version: PROVIDER_LIVE_SCHEMA_VERSION,
            route,
            capabilities,
            configuration_digest: configuration_digest.into(),
            credential_revision: credential_revision.into(),
            provider_account: provider_account.into(),
            metadata_digest: String::new(),
        };
        metadata.metadata_digest = metadata.digest();
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_LIVE_CONNECTION_SCHEMA
            || self.version != PROVIDER_LIVE_SCHEMA_VERSION
            || self.route.provider_id.trim().is_empty()
            || self.route.connection_id.trim().is_empty()
            || self.route.profile.trim().is_empty()
            || self.route.model_id.trim().is_empty()
        {
            return Err("provider_live_connection_metadata_invalid".to_owned());
        }
        digest(
            &self.route.configuration_revision,
            "provider_live_route_revision",
        )?;
        digest(
            &self.configuration_digest,
            "provider_live_configuration_digest",
        )?;
        credential_revision(&self.credential_revision)?;
        digest(&self.provider_account, "provider_live_provider_account")?;
        digest(&self.metadata_digest, "provider_live_metadata_digest")?;
        if self.metadata_digest != self.digest() {
            return Err("provider_live_metadata_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "route": self.route,
            "capabilities": self.capabilities,
            "configuration_digest": self.configuration_digest,
            "credential_revision": self.credential_revision,
            "provider_account": self.provider_account,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConnectionKind {
    Native,
    CompatibleGateway,
    Local,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLiveEvidenceSource {
    LiveNetwork,
    LiveLocal,
    Synthetic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLiveEvidenceStatus {
    Verified,
    Unverified,
    Skipped,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderLiveUsage {
    Known { usage: ModelUsage },
    Unknown { reason: String },
}

impl ProviderLiveUsage {
    fn validate(&self) -> Result<(), String> {
        if let Self::Unknown { reason } = self {
            bounded(reason, "provider_live_usage_unknown_reason", 256)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderLiveConnectionEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub provider_id: String,
    pub connection_id: String,
    pub connection_kind: ProviderConnectionKind,
    pub protocol: ModelProtocol,
    pub profile: String,
    pub requested_model: String,
    pub reported_model: Option<String>,
    pub configuration_digest: String,
    pub credential_revision: String,
    pub budget_digest: String,
    pub source: ProviderLiveEvidenceSource,
    pub status: ProviderLiveEvidenceStatus,
    pub operator_approval_ref: Option<String>,
    pub request_count: u32,
    pub text_round_trip: bool,
    pub tool_round_trip: bool,
    pub delta_observed: bool,
    pub cancel_observed: bool,
    pub usage: ProviderLiveUsage,
    pub receipt_digest: Option<String>,
    pub artifact_digest: Option<String>,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl ProviderLiveConnectionEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: impl Into<String>,
        connection_id: impl Into<String>,
        connection_kind: ProviderConnectionKind,
        protocol: ModelProtocol,
        profile: impl Into<String>,
        requested_model: impl Into<String>,
        reported_model: Option<String>,
        configuration_digest: impl Into<String>,
        credential_revision: impl Into<String>,
        budget_digest: impl Into<String>,
        source: ProviderLiveEvidenceSource,
        status: ProviderLiveEvidenceStatus,
        operator_approval_ref: Option<String>,
        request_count: u32,
        text_round_trip: bool,
        tool_round_trip: bool,
        delta_observed: bool,
        cancel_observed: bool,
        usage: ProviderLiveUsage,
        receipt_digest: Option<String>,
        artifact_digest: Option<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: PROVIDER_LIVE_EVIDENCE_SCHEMA.to_owned(),
            version: PROVIDER_LIVE_SCHEMA_VERSION,
            provider_id: provider_id.into(),
            connection_id: connection_id.into(),
            connection_kind,
            protocol,
            profile: profile.into(),
            requested_model: requested_model.into(),
            reported_model,
            configuration_digest: configuration_digest.into(),
            credential_revision: credential_revision.into(),
            budget_digest: budget_digest.into(),
            source,
            status,
            operator_approval_ref,
            request_count,
            text_round_trip,
            tool_round_trip,
            delta_observed,
            cancel_observed,
            usage,
            receipt_digest,
            artifact_digest,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_LIVE_EVIDENCE_SCHEMA
            || self.version != PROVIDER_LIVE_SCHEMA_VERSION
        {
            return Err("provider_live_evidence_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.provider_id, "provider_live_provider_id", 128),
            (&self.connection_id, "provider_live_connection_id", 128),
            (&self.profile, "provider_live_profile", 128),
            (&self.requested_model, "provider_live_requested_model", 256),
        ] {
            bounded(value, field, max)?;
        }
        if let Some(model) = &self.reported_model {
            bounded(model, "provider_live_reported_model", 256)?;
        }
        digest(
            &self.configuration_digest,
            "provider_live_configuration_digest",
        )?;
        credential_revision(&self.credential_revision)?;
        digest(&self.budget_digest, "provider_live_budget_digest")?;
        if self.request_count > 128 {
            return Err("provider_live_request_count_invalid".to_owned());
        }
        for reference in [
            &self.operator_approval_ref,
            &self.receipt_digest,
            &self.artifact_digest,
        ] {
            if let Some(value) = reference {
                bounded(value, "provider_live_reference", 256)?;
            }
        }
        if let Some(value) = &self.receipt_digest {
            digest(value, "provider_live_receipt_digest")?;
        }
        if let Some(value) = &self.artifact_digest {
            digest(value, "provider_live_artifact_digest")?;
        }
        if self.limitations.len() > MAX_LIMITATIONS {
            return Err("provider_live_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "provider_live_limitation", 256)?;
        }
        self.usage.validate()?;

        if self.status == ProviderLiveEvidenceStatus::Verified {
            if self.source == ProviderLiveEvidenceSource::Synthetic {
                return Err("provider_live_synthetic_evidence".to_owned());
            }
            let Some(approval_ref) = self.operator_approval_ref.as_deref() else {
                return Err("provider_live_operator_approval_required".to_owned());
            };
            if approval_ref.trim() != approval_ref
                || !approval_ref.to_ascii_lowercase().starts_with("approval:")
            {
                return Err("provider_live_operator_approval_ref_invalid".to_owned());
            }
            if self.request_count == 0
                || !self.text_round_trip
                || !self.tool_round_trip
                || !self.delta_observed
                || !self.cancel_observed
                || self.reported_model.is_none()
                || self.receipt_digest.is_none()
                || self.artifact_digest.is_none()
            {
                return Err("provider_live_verified_evidence_incomplete".to_owned());
            }
            if matches!(self.usage, ProviderLiveUsage::Unknown { .. })
                && self.limitations.is_empty()
            {
                return Err("provider_live_unknown_usage_reason_required".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("provider_live_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("provider_live_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "provider_id": self.provider_id,
            "connection_id": self.connection_id,
            "connection_kind": self.connection_kind,
            "protocol": self.protocol,
            "profile": self.profile,
            "requested_model": self.requested_model,
            "reported_model": self.reported_model,
            "configuration_digest": self.configuration_digest,
            "credential_revision": self.credential_revision,
            "budget_digest": self.budget_digest,
            "source": self.source,
            "status": self.status,
            "operator_approval_ref": self.operator_approval_ref,
            "request_count": self.request_count,
            "text_round_trip": self.text_round_trip,
            "tool_round_trip": self.tool_round_trip,
            "delta_observed": self.delta_observed,
            "cancel_observed": self.cancel_observed,
            "usage": self.usage,
            "receipt_digest": self.receipt_digest,
            "artifact_digest": self.artifact_digest,
            "limitations": self.limitations,
        }))
    }
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn credential_revision(value: &str) -> Result<(), String> {
    if value == "none" {
        return Ok(());
    }
    digest(value, "provider_live_credential_revision")
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
