//! Default-off external notification/webhook contracts.
//!
//! This module validates an envelope, destination allowlist, nonce, epoch and provider receipt;
//! it never opens a socket, signs with a secret, invokes A2A/HTTP, or turns a receipt into a
//! ControlPlane decision. External delivery remains an explicit later adapter.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use url::Url;

pub const EXTERNAL_NOTIFICATION_POLICY_SCHEMA: &str = "kiana.external-notification-policy.v1";
pub const EXTERNAL_NOTIFICATION_ENVELOPE_SCHEMA: &str = "kiana.external-notification-envelope.v1";
pub const EXTERNAL_NOTIFICATION_RECEIPT_SCHEMA: &str = "kiana.external-notification-receipt.v1";
pub const EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA: &str = "kiana.external-notification-admission.v1";
pub const EXTERNAL_NOTIFICATION_MAX_ORIGINS: usize = 32;
pub const EXTERNAL_NOTIFICATION_MAX_SOURCE_IDS: usize = 32;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
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

fn canonical_origin(value: &str, field: &str) -> Result<String, String> {
    let url = Url::parse(value).map_err(|_| format!("{field}_invalid"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
        || (url.path() != "/" && !url.path().is_empty())
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(url.origin().ascii_serialization())
}

fn source_ids_valid(values: &[String]) -> Result<(), String> {
    if values.is_empty() || values.len() > EXTERNAL_NOTIFICATION_MAX_SOURCE_IDS {
        return Err("external_notification_source_ids_invalid".to_owned());
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("external_notification_source_ids_noncanonical".to_owned());
    }
    for value in values {
        required(value, "external_notification_source_id", 512)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalNotificationTransport {
    Webhook,
    A2a,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationPolicy {
    pub schema: String,
    pub enabled: bool,
    pub allowed_origins: BTreeSet<String>,
    #[serde(default)]
    pub signing_key_id: Option<String>,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub policy_digest: String,
}

impl ExternalNotificationPolicy {
    pub fn disabled(authority_epoch: u64, data_epoch: u64) -> Result<Self, String> {
        Self::new(false, Vec::new(), None, authority_epoch, data_epoch)
    }

    pub fn new(
        enabled: bool,
        origins: impl IntoIterator<Item = String>,
        signing_key_id: Option<String>,
        authority_epoch: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        let mut allowed_origins = BTreeSet::new();
        for origin in origins {
            allowed_origins.insert(canonical_origin(&origin, "external_notification_origin")?);
        }
        let mut policy = Self {
            schema: EXTERNAL_NOTIFICATION_POLICY_SCHEMA.to_owned(),
            enabled,
            allowed_origins,
            signing_key_id,
            authority_epoch,
            data_epoch,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTERNAL_NOTIFICATION_POLICY_SCHEMA
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.allowed_origins.len() > EXTERNAL_NOTIFICATION_MAX_ORIGINS
            || self.allowed_origins.iter().any(|origin| {
                canonical_origin(origin, "external_notification_origin")
                    .ok()
                    .as_deref()
                    != Some(origin.as_str())
            })
        {
            return Err("external_notification_policy_invalid".to_owned());
        }
        if self.enabled && (self.allowed_origins.is_empty() || self.signing_key_id.is_none()) {
            return Err("external_notification_policy_enablement_invalid".to_owned());
        }
        if let Some(key_id) = &self.signing_key_id {
            required(key_id, "external_notification_signing_key", 256)?;
        }
        digest(&self.policy_digest, "external_notification_policy_digest")?;
        if self.policy_digest != self.digest() {
            return Err("external_notification_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "enabled": self.enabled,
            "allowed_origins": self.allowed_origins,
            "signing_key_id": self.signing_key_id,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationEnvelope {
    pub schema: String,
    pub transport: ExternalNotificationTransport,
    pub notification_id: String,
    pub delivery_id: String,
    pub recipient_id: String,
    pub destination_origin: String,
    pub nonce: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub payload_digest: String,
    pub signature_digest: String,
    pub source_event_ids: Vec<String>,
    pub envelope_digest: String,
}

impl ExternalNotificationEnvelope {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTERNAL_NOTIFICATION_ENVELOPE_SCHEMA
            || self.created_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.created_at_unix_ms
            || self.authority_epoch == 0
            || self.data_epoch == 0
        {
            return Err("external_notification_envelope_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.notification_id, "external_notification_id", 512),
            (&self.delivery_id, "external_notification_delivery_id", 512),
            (&self.recipient_id, "external_notification_recipient", 512),
            (&self.nonce, "external_notification_nonce", 256),
        ] {
            required(value, field, max)?;
        }
        let canonical = canonical_origin(
            &self.destination_origin,
            "external_notification_destination",
        )?;
        if canonical != self.destination_origin {
            return Err("external_notification_destination_noncanonical".to_owned());
        }
        digest(&self.payload_digest, "external_notification_payload_digest")?;
        digest(
            &self.signature_digest,
            "external_notification_signature_digest",
        )?;
        source_ids_valid(&self.source_event_ids)?;
        digest(
            &self.envelope_digest,
            "external_notification_envelope_digest",
        )?;
        if self.envelope_digest != self.digest() {
            return Err("external_notification_envelope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "transport": self.transport,
            "notification_id": self.notification_id,
            "delivery_id": self.delivery_id,
            "recipient_id": self.recipient_id,
            "destination_origin": self.destination_origin,
            "nonce": self.nonce,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "payload_digest": self.payload_digest,
            "signature_digest": self.signature_digest,
            "source_event_ids": self.source_event_ids,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalNotificationReceiptStatus {
    Acknowledged,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationReceipt {
    pub schema: String,
    pub transport: ExternalNotificationTransport,
    pub notification_id: String,
    pub delivery_id: String,
    pub status: ExternalNotificationReceiptStatus,
    #[serde(default)]
    pub provider_receipt_ref: Option<String>,
    pub payload_digest: String,
    pub nonce: String,
    pub authority_epoch: u64,
    pub observed_at_unix_ms: u64,
    pub receipt_digest: String,
}

impl ExternalNotificationReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTERNAL_NOTIFICATION_RECEIPT_SCHEMA
            || self.authority_epoch == 0
            || self.observed_at_unix_ms == 0
        {
            return Err("external_notification_receipt_invalid".to_owned());
        }
        for (value, field) in [
            (&self.notification_id, "external_receipt_notification"),
            (&self.delivery_id, "external_receipt_delivery"),
            (&self.nonce, "external_receipt_nonce"),
        ] {
            required(value, field, 512)?;
        }
        if let Some(reference) = &self.provider_receipt_ref {
            required(reference, "external_receipt_provider_ref", 512)?;
        }
        digest(&self.payload_digest, "external_receipt_payload_digest")?;
        digest(&self.receipt_digest, "external_receipt_digest")?;
        if self.receipt_digest != self.digest() {
            return Err("external_notification_receipt_digest_mismatch".to_owned());
        }
        if self.status == ExternalNotificationReceiptStatus::Acknowledged
            && self.provider_receipt_ref.is_none()
        {
            return Err("external_receipt_ack_ref_required".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "transport": self.transport,
            "notification_id": self.notification_id,
            "delivery_id": self.delivery_id,
            "status": self.status,
            "provider_receipt_ref": self.provider_receipt_ref,
            "payload_digest": self.payload_digest,
            "nonce": self.nonce,
            "authority_epoch": self.authority_epoch,
            "observed_at_unix_ms": self.observed_at_unix_ms,
        }))
    }
}
