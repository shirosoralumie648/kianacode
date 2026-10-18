//! Default-off external notification/webhook contracts.
//!
//! This module validates an envelope, destination allowlist, nonce, epoch and provider receipt;
//! it never opens a socket, signs with a secret, invokes A2A/HTTP, or turns a receipt into a
//! ControlPlane decision. External delivery remains an explicit later adapter.
//!
//! The policy is bound through a digest-checked [`ExternalNotificationPolicySnapshot`] so a
//! request cannot smuggle in an enabled policy of its own; origins pass the same forbidden-address
//! (SSRF/metadata) classification that the connector HTTPS boundary already uses; a signature is
//! only shape-checked and key-bound, never claimed as verified; and nonce/delivery reuse is refused
//! by a bounded [`ExternalNotificationReplayStore`].

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use url::{Host, Url};

pub const EXTERNAL_NOTIFICATION_POLICY_SCHEMA: &str = "kiana.external-notification-policy.v1";
pub const EXTERNAL_NOTIFICATION_POLICY_SNAPSHOT_SCHEMA: &str =
    "kiana.external-notification-policy-snapshot.v1";
pub const EXTERNAL_NOTIFICATION_ENVELOPE_SCHEMA: &str = "kiana.external-notification-envelope.v1";
pub const EXTERNAL_NOTIFICATION_RECEIPT_SCHEMA: &str = "kiana.external-notification-receipt.v1";
pub const EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA: &str = "kiana.external-notification-admission.v1";
pub const EXTERNAL_NOTIFICATION_MAX_ORIGINS: usize = 32;
pub const EXTERNAL_NOTIFICATION_MAX_SOURCE_IDS: usize = 32;
pub const EXTERNAL_NOTIFICATION_MAX_REPLAY_ENTRIES: usize = 4_096;
pub const EXTERNAL_NOTIFICATION_MAX_REPLAY_WINDOW_MS: u64 = 24 * 60 * 60 * 1_000;

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

/// A host name that can be admitted as an external destination. Single-label names, `localhost`
/// and mDNS/private suffixes are refused before any resolver could map them onto a local or
/// metadata address.
fn public_hostname(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    !name.is_empty()
        && name.contains('.')
        && name != "localhost"
        && !name.ends_with(".localhost")
        && !name.ends_with(".local")
        && !name.ends_with(".internal")
        && !name.ends_with(".home.arpa")
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
    let public = match url.host() {
        Some(Host::Ipv4(address)) => {
            !crate::connector_https::is_forbidden_address(IpAddr::V4(address))
        }
        Some(Host::Ipv6(address)) => {
            !crate::connector_https::is_forbidden_address(IpAddr::V6(address))
        }
        Some(Host::Domain(name)) => public_hostname(name),
        None => false,
    };
    if !public {
        return Err(format!("{field}_ssrf_denied"));
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

/// A server-pinned policy revision. A request names only the policy digest it expects; the
/// snapshot (revision + issued time + digest) is what the server actually holds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationPolicySnapshot {
    pub schema: String,
    pub revision: u64,
    pub issued_at_unix_ms: u64,
    pub policy: ExternalNotificationPolicy,
    pub snapshot_digest: String,
}

impl ExternalNotificationPolicySnapshot {
    /// The explicit disabled default. This is the only snapshot a fresh server has.
    pub fn disabled(
        authority_epoch: u64,
        data_epoch: u64,
        issued_at_unix_ms: u64,
    ) -> Result<Self, String> {
        Self::issue(
            ExternalNotificationPolicy::disabled(authority_epoch, data_epoch)?,
            1,
            issued_at_unix_ms,
        )
    }

    pub fn issue(
        policy: ExternalNotificationPolicy,
        revision: u64,
        issued_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: EXTERNAL_NOTIFICATION_POLICY_SNAPSHOT_SCHEMA.to_owned(),
            revision,
            issued_at_unix_ms,
            policy,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTERNAL_NOTIFICATION_POLICY_SNAPSHOT_SCHEMA
            || self.revision == 0
            || self.issued_at_unix_ms == 0
        {
            return Err("external_notification_policy_snapshot_invalid".to_owned());
        }
        self.policy.validate()?;
        digest(
            &self.snapshot_digest,
            "external_notification_policy_snapshot_digest",
        )?;
        if self.snapshot_digest != self.digest() {
            return Err("external_notification_policy_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "revision": self.revision,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "policy": self.policy,
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
    #[serde(default)]
    pub signing_key_id: Option<String>,
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
        if let Some(key_id) = &self.signing_key_id {
            required(key_id, "external_notification_signing_key", 256)?;
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

    /// Pure key binding: the envelope must name the key the pinned policy uses. This is not
    /// signature verification and must never be reported as such.
    pub fn bind_signing_key(&self, policy: &ExternalNotificationPolicy) -> Result<(), String> {
        let expected = policy
            .signing_key_id
            .as_deref()
            .ok_or_else(|| "external_notification_signature_policy_key_missing".to_owned())?;
        let Some(actual) = self.signing_key_id.as_deref() else {
            return Err("external_notification_signature_key_missing".to_owned());
        };
        if actual != expected {
            return Err("external_notification_signature_binding_mismatch".to_owned());
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
            "signing_key_id": self.signing_key_id,
            "source_event_ids": self.source_event_ids,
        }))
    }
}

/// Bounded nonce/delivery replay window. The identifiers are admitted exactly once and only while
/// the envelope is inside a bounded TTL; anything else fails with a stable refusal code. The store
/// is an owned, explicit value: no process-global state, no clock read, no persistence claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalNotificationReplayWindow {
    pub max_entries: usize,
    pub max_ttl_ms: u64,
}

impl ExternalNotificationReplayWindow {
    pub fn new(max_entries: usize, max_ttl_ms: u64) -> Result<Self, String> {
        let window = Self {
            max_entries,
            max_ttl_ms,
        };
        window.validate()?;
        Ok(window)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.max_entries == 0
            || self.max_entries > EXTERNAL_NOTIFICATION_MAX_REPLAY_ENTRIES
            || self.max_ttl_ms == 0
            || self.max_ttl_ms > EXTERNAL_NOTIFICATION_MAX_REPLAY_WINDOW_MS
        {
            return Err("external_notification_replay_window_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalNotificationReplayStore {
    window: ExternalNotificationReplayWindow,
    nonces: BTreeMap<String, u64>,
    deliveries: BTreeMap<String, u64>,
    admitted: u64,
}

impl ExternalNotificationReplayStore {
    pub fn new(window: ExternalNotificationReplayWindow) -> Result<Self, String> {
        window.validate()?;
        Ok(Self {
            window,
            nonces: BTreeMap::new(),
            deliveries: BTreeMap::new(),
            admitted: 0,
        })
    }

    pub fn window(&self) -> ExternalNotificationReplayWindow {
        self.window
    }

    pub fn admitted_count(&self) -> u64 {
        self.admitted
    }

    pub fn pending_entries(&self) -> usize {
        self.nonces.len().max(self.deliveries.len())
    }

    /// Admit one delivery. Refusals happen before any mutation so a rejected envelope never burns
    /// a fresh nonce or delivery id.
    pub fn admit(
        &mut self,
        delivery_id: &str,
        nonce: &str,
        expires_at_unix_ms: u64,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.window.validate()?;
        required(nonce, "external_notification_nonce", 256)?;
        required(delivery_id, "external_notification_delivery_id", 512)?;
        if expires_at_unix_ms <= now_unix_ms {
            return Err("external_notification_replay_window_invalid".to_owned());
        }
        let ttl = expires_at_unix_ms - now_unix_ms;
        if ttl > self.window.max_ttl_ms {
            return Err("external_notification_replay_window_invalid".to_owned());
        }
        self.purge(now_unix_ms);
        if self.nonces.contains_key(nonce) {
            return Err("external_notification_nonce_replayed".to_owned());
        }
        if self.deliveries.contains_key(delivery_id) {
            return Err("external_notification_delivery_replayed".to_owned());
        }
        if self.pending_entries() >= self.window.max_entries {
            return Err("external_notification_replay_capacity_exceeded".to_owned());
        }
        self.nonces.insert(nonce.to_owned(), expires_at_unix_ms);
        self.deliveries
            .insert(delivery_id.to_owned(), expires_at_unix_ms);
        self.admitted = self.admitted.saturating_add(1);
        Ok(())
    }

    fn purge(&mut self, now_unix_ms: u64) {
        self.nonces
            .retain(|_, expires_at| *expires_at > now_unix_ms);
        self.deliveries
            .retain(|_, expires_at| *expires_at > now_unix_ms);
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
