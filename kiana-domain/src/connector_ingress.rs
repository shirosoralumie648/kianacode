//! INT-24 Webhook/A2A ingress contracts.
//!
//! Ingress is untrusted occurrence data. The daemon owns signing keys and replay state; this
//! module owns the bounded tenant/project/source/nonce/payload shape and the digest chain. No
//! ingress value is a capability request or an instruction to call a connector.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const CONNECTOR_INGRESS_SCHEMA: &str = "kiana.connector-ingress.v1";
pub const CONNECTOR_INGRESS_POLICY_SCHEMA: &str = "kiana.connector-ingress-policy.v1";
pub const CONNECTOR_INGRESS_OCCURRENCE_SCHEMA: &str = "kiana.connector-ingress-occurrence.v1";
pub const CONNECTOR_INGRESS_PAYLOAD_MAX_BYTES: usize = 64 * 1024;
pub const CONNECTOR_INGRESS_MAX_CLOCK_SKEW_MS: u64 = 86_400_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorIngressProtocol {
    Webhook,
    A2a,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorIngressEvent {
    pub schema: String,
    pub protocol: ConnectorIngressProtocol,
    pub source_id: String,
    pub key_id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub event_id: String,
    pub event_kind: String,
    pub occurred_at_unix_ms: u64,
    pub nonce: String,
    pub payload: Value,
    pub payload_digest: String,
    pub signature: String,
    pub signature_algorithm: String,
    pub ingress_digest: String,
}

impl ConnectorIngressEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        protocol: ConnectorIngressProtocol,
        source_id: impl Into<String>,
        key_id: impl Into<String>,
        tenant_id: impl Into<String>,
        project_id: impl Into<String>,
        event_id: impl Into<String>,
        event_kind: impl Into<String>,
        occurred_at_unix_ms: u64,
        nonce: impl Into<String>,
        payload: Value,
        signature: impl Into<String>,
    ) -> Result<Self, String> {
        validate_payload(&payload, CONNECTOR_INGRESS_PAYLOAD_MAX_BYTES)?;
        let mut event = Self {
            schema: CONNECTOR_INGRESS_SCHEMA.to_owned(),
            protocol,
            source_id: source_id.into(),
            key_id: key_id.into(),
            tenant_id: tenant_id.into(),
            project_id: project_id.into(),
            event_id: event_id.into(),
            event_kind: event_kind.into(),
            occurred_at_unix_ms,
            nonce: nonce.into(),
            payload,
            payload_digest: String::new(),
            signature: signature.into(),
            signature_algorithm: "hmac-sha256".to_owned(),
            ingress_digest: String::new(),
        };
        event.payload_digest = json_digest(&event.payload);
        event.ingress_digest = event.digest();
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_INGRESS_SCHEMA
            || self.occurred_at_unix_ms == 0
            || self.signature_algorithm != "hmac-sha256"
            || !valid_signature(&self.signature)
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.ingress_digest)
            || self.payload_digest != json_digest(&self.payload)
            || self.ingress_digest != self.digest()
        {
            return Err("connector_ingress_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.source_id, "connector_ingress_source", 128),
            (&self.key_id, "connector_ingress_key", 128),
            (&self.tenant_id, "connector_ingress_tenant", 256),
            (&self.project_id, "connector_ingress_project", 256),
            (&self.event_id, "connector_ingress_event", 256),
            (&self.event_kind, "connector_ingress_kind", 128),
            (&self.nonce, "connector_ingress_nonce", 256),
        ] {
            bounded(value, field, max)?;
        }
        validate_payload(&self.payload, CONNECTOR_INGRESS_PAYLOAD_MAX_BYTES)
    }

    pub fn signing_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "protocol": self.protocol,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "tenant_id": self.tenant_id,
            "project_id": self.project_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "occurred_at_unix_ms": self.occurred_at_unix_ms,
            "nonce": self.nonce,
            "payload_digest": self.payload_digest,
        }))
    }

    pub fn occurrence_key(&self) -> String {
        format!("connector-ingress:{}:{}", self.source_id, self.event_id)
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "protocol": self.protocol,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "tenant_id": self.tenant_id,
            "project_id": self.project_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "occurred_at_unix_ms": self.occurred_at_unix_ms,
            "nonce": self.nonce,
            "payload": self.payload,
            "payload_digest": self.payload_digest,
            "signature": self.signature,
            "signature_algorithm": self.signature_algorithm,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorIngressPolicy {
    pub schema: String,
    pub protocol: ConnectorIngressProtocol,
    pub source_id: String,
    pub key_id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub allowed_event_kinds: BTreeSet<String>,
    pub required_payload_keys: BTreeSet<String>,
    pub allowed_payload_keys: BTreeSet<String>,
    pub max_clock_skew_ms: u64,
    pub max_payload_bytes: usize,
    pub policy_digest: String,
}

impl ConnectorIngressPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        protocol: ConnectorIngressProtocol,
        source_id: impl Into<String>,
        key_id: impl Into<String>,
        tenant_id: impl Into<String>,
        project_id: impl Into<String>,
        allowed_event_kinds: impl IntoIterator<Item = String>,
        required_payload_keys: impl IntoIterator<Item = String>,
        allowed_payload_keys: impl IntoIterator<Item = String>,
        max_clock_skew_ms: u64,
        max_payload_bytes: usize,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: CONNECTOR_INGRESS_POLICY_SCHEMA.to_owned(),
            protocol,
            source_id: source_id.into(),
            key_id: key_id.into(),
            tenant_id: tenant_id.into(),
            project_id: project_id.into(),
            allowed_event_kinds: allowed_event_kinds.into_iter().collect(),
            required_payload_keys: required_payload_keys.into_iter().collect(),
            allowed_payload_keys: allowed_payload_keys.into_iter().collect(),
            max_clock_skew_ms,
            max_payload_bytes,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_INGRESS_POLICY_SCHEMA
            || self.allowed_event_kinds.is_empty()
            || self.allowed_event_kinds.len() > 128
            || !self
                .required_payload_keys
                .is_subset(&self.allowed_payload_keys)
            || self.max_clock_skew_ms == 0
            || self.max_clock_skew_ms > CONNECTOR_INGRESS_MAX_CLOCK_SKEW_MS
            || self.max_payload_bytes == 0
            || self.max_payload_bytes > CONNECTOR_INGRESS_PAYLOAD_MAX_BYTES
            || !valid_digest(&self.policy_digest)
            || self.policy_digest != self.digest()
        {
            return Err("connector_ingress_policy_invalid".to_owned());
        }
        for value in self
            .allowed_event_kinds
            .iter()
            .chain(&self.required_payload_keys)
            .chain(&self.allowed_payload_keys)
        {
            bounded(value, "connector_ingress_filter_key", 128)?;
        }
        for (value, field, max) in [
            (&self.source_id, "connector_ingress_source", 128),
            (&self.key_id, "connector_ingress_key", 128),
            (&self.tenant_id, "connector_ingress_tenant", 256),
            (&self.project_id, "connector_ingress_project", 256),
        ] {
            bounded(value, field, max)?;
        }
        Ok(())
    }

    pub fn matches(&self, event: &ConnectorIngressEvent, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        event.validate()?;
        if self.protocol != event.protocol
            || self.source_id != event.source_id
            || self.key_id != event.key_id
            || self.tenant_id != event.tenant_id
            || self.project_id != event.project_id
            || !self.allowed_event_kinds.contains(&event.event_kind)
        {
            return Err("connector_ingress_source_or_tenant_denied".to_owned());
        }
        if now_unix_ms == 0
            || now_unix_ms.abs_diff(event.occurred_at_unix_ms) > self.max_clock_skew_ms
        {
            return Err("connector_ingress_clock_skew".to_owned());
        }
        validate_payload(&event.payload, self.max_payload_bytes)?;
        let object = event
            .payload
            .as_object()
            .ok_or_else(|| "connector_ingress_payload_object_required".to_owned())?;
        if !object
            .keys()
            .all(|key| self.allowed_payload_keys.contains(key))
            || !self
                .required_payload_keys
                .iter()
                .all(|key| object.contains_key(key))
        {
            return Err("connector_ingress_payload_filter_denied".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "protocol": self.protocol,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "tenant_id": self.tenant_id,
            "project_id": self.project_id,
            "allowed_event_kinds": self.allowed_event_kinds,
            "required_payload_keys": self.required_payload_keys,
            "allowed_payload_keys": self.allowed_payload_keys,
            "max_clock_skew_ms": self.max_clock_skew_ms,
            "max_payload_bytes": self.max_payload_bytes,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorIngressOccurrence {
    pub schema: String,
    pub protocol: ConnectorIngressProtocol,
    pub source_id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub event_id: String,
    pub event_kind: String,
    pub nonce: String,
    pub occurrence_key: String,
    pub payload_digest: String,
    pub ingress_digest: String,
    pub signature_digest: String,
    pub policy_digest: String,
    pub occurrence_digest: String,
}

impl ConnectorIngressOccurrence {
    pub fn from_verified(
        event: &ConnectorIngressEvent,
        policy: &ConnectorIngressPolicy,
    ) -> Result<Self, String> {
        policy.matches(event, event.occurred_at_unix_ms)?;
        let mut occurrence = Self {
            schema: CONNECTOR_INGRESS_OCCURRENCE_SCHEMA.to_owned(),
            protocol: event.protocol,
            source_id: event.source_id.clone(),
            tenant_id: event.tenant_id.clone(),
            project_id: event.project_id.clone(),
            event_id: event.event_id.clone(),
            event_kind: event.event_kind.clone(),
            nonce: event.nonce.clone(),
            occurrence_key: event.occurrence_key(),
            payload_digest: event.payload_digest.clone(),
            ingress_digest: event.ingress_digest.clone(),
            signature_digest: json_digest(&serde_json::json!({
                "algorithm": event.signature_algorithm,
                "signature": event.signature,
            })),
            policy_digest: policy.policy_digest.clone(),
            occurrence_digest: String::new(),
        };
        occurrence.occurrence_digest = occurrence.digest();
        occurrence.validate()?;
        Ok(occurrence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_INGRESS_OCCURRENCE_SCHEMA
            || self.occurrence_key
                != format!("connector-ingress:{}:{}", self.source_id, self.event_id)
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.ingress_digest)
            || !valid_digest(&self.signature_digest)
            || !valid_digest(&self.policy_digest)
            || !valid_digest(&self.occurrence_digest)
            || self.occurrence_digest != self.digest()
        {
            return Err("connector_ingress_occurrence_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.source_id, "connector_ingress_source", 128),
            (&self.tenant_id, "connector_ingress_tenant", 256),
            (&self.project_id, "connector_ingress_project", 256),
            (&self.event_id, "connector_ingress_event", 256),
            (&self.event_kind, "connector_ingress_kind", 128),
            (&self.nonce, "connector_ingress_nonce", 256),
            (
                &self.occurrence_key,
                "connector_ingress_occurrence_key",
                512,
            ),
        ] {
            bounded(value, field, max)?;
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "protocol": self.protocol,
            "source_id": self.source_id,
            "tenant_id": self.tenant_id,
            "project_id": self.project_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "nonce": self.nonce,
            "occurrence_key": self.occurrence_key,
            "payload_digest": self.payload_digest,
            "ingress_digest": self.ingress_digest,
            "signature_digest": self.signature_digest,
            "policy_digest": self.policy_digest,
        }))
    }
}

pub fn validate_connector_ingress(event: &ConnectorIngressEvent) -> Result<(), String> {
    event.validate()
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_payload(value: &Value, max_bytes: usize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "connector_ingress_payload_invalid")?;
    if bytes.len() > max_bytes {
        return Err("connector_ingress_payload_too_large".to_owned());
    }
    Ok(())
}

fn valid_signature(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
