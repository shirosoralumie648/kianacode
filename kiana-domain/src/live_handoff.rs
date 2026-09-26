//! Explicit live/physical handoff evidence contract.
//!
//! A manifest is an operator-controlled checklist. It never contains a credential and cannot
//! enable a provider, connector, OTLP backend or physical effect by itself.

use crate::{canonical_journal_bytes, json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const LIVE_HANDOFF_SCHEMA: &str = "kiana.live-handoff.v1";
pub const LIVE_HANDOFF_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_LIVE_HANDOFF_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveHandoffTarget {
    Provider,
    Connector,
    OtlpBackend,
    OperatingSystem,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveHandoffStatus {
    NotSupported,
    OptedIn,
    Verified,
    Unknown,
}

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
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
pub struct LiveHandoffManifest {
    pub schema: String,
    pub version: SchemaVersion,
    pub target: LiveHandoffTarget,
    pub target_id: String,
    pub environment: String,
    /// A non-secret reference such as `secret-ref:provider/account`; raw credentials are denied.
    pub credential_ref: String,
    pub configuration_digest: String,
    pub source_snapshot: String,
    #[serde(default)]
    pub provider_receipt_ref: Option<String>,
    #[serde(default)]
    pub operator_approval_ref: Option<String>,
    pub retention_class: String,
    #[serde(default)]
    pub incident_ref: Option<String>,
    pub cleanup_plan: String,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub status: LiveHandoffStatus,
    pub manifest_digest: String,
}

impl LiveHandoffManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: LiveHandoffTarget,
        target_id: impl Into<String>,
        environment: impl Into<String>,
        credential_ref: impl Into<String>,
        configuration_digest: impl Into<String>,
        source_snapshot: impl Into<String>,
        provider_receipt_ref: Option<String>,
        operator_approval_ref: Option<String>,
        retention_class: impl Into<String>,
        incident_ref: Option<String>,
        cleanup_plan: impl Into<String>,
        limitations: Vec<String>,
        status: LiveHandoffStatus,
    ) -> Result<Self, String> {
        let mut manifest = Self {
            schema: LIVE_HANDOFF_SCHEMA.to_owned(),
            version: LIVE_HANDOFF_SCHEMA_VERSION,
            target,
            target_id: target_id.into(),
            environment: environment.into(),
            credential_ref: credential_ref.into(),
            configuration_digest: configuration_digest.into(),
            source_snapshot: source_snapshot.into(),
            provider_receipt_ref,
            operator_approval_ref,
            retention_class: retention_class.into(),
            incident_ref,
            cleanup_plan: cleanup_plan.into(),
            limitations,
            status,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LIVE_HANDOFF_SCHEMA
            || !self
                .version
                .is_compatible_with(&LIVE_HANDOFF_SCHEMA_VERSION)
        {
            return Err("live_handoff_schema_invalid".to_owned());
        }
        nonempty(&self.target_id, "live_handoff_target_id", 256)?;
        nonempty(&self.environment, "live_handoff_environment", 128)?;
        nonempty(&self.credential_ref, "live_handoff_credential_ref", 256)?;
        if !self.credential_ref.starts_with("secret-ref:")
            || self.credential_ref.contains(['\n', '\r'])
            || self.credential_ref.to_ascii_lowercase().contains("bearer ")
        {
            return Err("live_handoff_credential_ref_secret_or_invalid".to_owned());
        }
        digest(
            &self.configuration_digest,
            "live_handoff_configuration_digest",
        )?;
        nonempty(&self.source_snapshot, "live_handoff_source_snapshot", 256)?;
        nonempty(&self.retention_class, "live_handoff_retention_class", 64)?;
        nonempty(&self.cleanup_plan, "live_handoff_cleanup_plan", 2_048)?;
        for (value, field) in [
            (
                &self.provider_receipt_ref,
                "live_handoff_provider_receipt_ref",
            ),
            (
                &self.operator_approval_ref,
                "live_handoff_operator_approval_ref",
            ),
            (&self.incident_ref, "live_handoff_incident_ref"),
        ] {
            if let Some(value) = value {
                nonempty(value, field, 256)?;
            }
        }
        if self.limitations.len() > MAX_LIVE_HANDOFF_LIMITATIONS {
            return Err("live_handoff_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            nonempty(limitation, "live_handoff_limitation", 256)?;
        }
        match self.status {
            LiveHandoffStatus::NotSupported => {
                if self.limitations.is_empty() {
                    return Err("live_handoff_not_supported_reason_required".to_owned());
                }
            }
            LiveHandoffStatus::OptedIn => {
                let Some(approval_ref) = self.operator_approval_ref.as_deref() else {
                    return Err("live_handoff_operator_approval_required".to_owned());
                };
                if approval_ref.trim() != approval_ref
                    || !approval_ref.to_ascii_lowercase().starts_with("approval:")
                {
                    return Err("live_handoff_operator_approval_ref_invalid".to_owned());
                }
            }
            LiveHandoffStatus::Verified => {
                if self.provider_receipt_ref.is_none()
                    || self.operator_approval_ref.is_none()
                    || self.cleanup_plan.trim().len() < 8
                {
                    return Err("live_handoff_verified_evidence_incomplete".to_owned());
                }
                let approval_ref = self.operator_approval_ref.as_deref().unwrap_or_default();
                if approval_ref.trim() != approval_ref
                    || !approval_ref.to_ascii_lowercase().starts_with("approval:")
                {
                    return Err("live_handoff_operator_approval_ref_invalid".to_owned());
                }
            }
            LiveHandoffStatus::Unknown => {
                if self.limitations.is_empty() {
                    return Err("live_handoff_unknown_reason_required".to_owned());
                }
            }
        }
        digest(&self.manifest_digest, "live_handoff_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("live_handoff_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "manifest_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
