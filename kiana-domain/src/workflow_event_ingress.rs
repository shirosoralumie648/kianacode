//! Signed workflow event ingress contracts.
//!
//! An ingress payload is untrusted data used to select an occurrence.  It is never a capability
//! request, prompt, path allow-list or authority snapshot.  The daemon verifier owns key material
//! and dedupe state; this module owns the bounded, digest-bound wire shape.

use crate::{json_digest, AutomationCommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const WORKFLOW_EVENT_INGRESS_SCHEMA: &str = "kiana.workflow-event-ingress.v1";
pub const WORKFLOW_EVENT_SOURCE_POLICY_SCHEMA: &str = "kiana.workflow-event-source-policy.v1";
pub const WORKFLOW_EVENT_OCCURRENCE_SCHEMA: &str = "kiana.workflow-event-occurrence.v1";
pub const WORKFLOW_EVENT_PAYLOAD_MAX_BYTES: usize = 64 * 1024;
pub const WORKFLOW_EVENT_MAX_CLOCK_SKEW_MS: u64 = 86_400_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEventIngress {
    pub schema: String,
    pub source_id: String,
    pub key_id: String,
    pub project_id: String,
    pub trigger_id: String,
    pub event_id: String,
    pub event_kind: String,
    pub occurred_at_unix_ms: u64,
    pub payload: Value,
    pub payload_digest: String,
    pub signature: String,
    pub ingress_digest: String,
}

impl WorkflowEventIngress {
    pub fn new(
        source_id: impl Into<String>,
        key_id: impl Into<String>,
        project_id: impl Into<String>,
        trigger_id: impl Into<String>,
        event_id: impl Into<String>,
        event_kind: impl Into<String>,
        occurred_at_unix_ms: u64,
        payload: Value,
        signature: impl Into<String>,
    ) -> Result<Self, String> {
        validate_payload(&payload)?;
        let payload_digest = json_digest(&payload);
        let mut event = Self {
            schema: WORKFLOW_EVENT_INGRESS_SCHEMA.to_owned(),
            source_id: source_id.into(),
            key_id: key_id.into(),
            project_id: project_id.into(),
            trigger_id: trigger_id.into(),
            event_id: event_id.into(),
            event_kind: event_kind.into(),
            occurred_at_unix_ms,
            payload,
            payload_digest,
            signature: signature.into(),
            ingress_digest: String::new(),
        };
        event.ingress_digest = event.digest();
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_EVENT_INGRESS_SCHEMA || self.occurred_at_unix_ms == 0 {
            return Err("workflow_event_ingress_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.source_id, "workflow_event_source", 128),
            (&self.key_id, "workflow_event_key_id", 128),
            (&self.project_id, "workflow_event_project", 256),
            (&self.trigger_id, "workflow_event_trigger", 128),
            (&self.event_id, "workflow_event_id", 256),
            (&self.event_kind, "workflow_event_kind", 128),
            (&self.signature, "workflow_event_signature", 256),
        ] {
            bounded(value, field, max)?;
        }
        validate_payload(&self.payload)?;
        if self.payload_digest != json_digest(&self.payload) {
            return Err("workflow_event_payload_digest_mismatch".to_owned());
        }
        if !valid_digest(&self.payload_digest)
            || !valid_digest(&self.ingress_digest)
            || self.ingress_digest != self.digest()
        {
            return Err("workflow_event_ingress_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The daemon signs this digest, not a re-serialized arbitrary JSON payload.
    pub fn signing_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "project_id": self.project_id,
            "trigger_id": self.trigger_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "occurred_at_unix_ms": self.occurred_at_unix_ms,
            "payload_digest": self.payload_digest,
        }))
    }

    pub fn occurrence_key(&self) -> String {
        format!("event:{}:{}", self.source_id, self.event_id)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "project_id": self.project_id,
            "trigger_id": self.trigger_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "occurred_at_unix_ms": self.occurred_at_unix_ms,
            "payload_digest": self.payload_digest,
            "signature": self.signature,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEventSourcePolicy {
    pub schema: String,
    pub source_id: String,
    pub key_id: String,
    pub project_id: String,
    pub allowed_event_kinds: BTreeSet<String>,
    pub required_payload_keys: BTreeSet<String>,
    pub allowed_payload_keys: BTreeSet<String>,
    pub max_clock_skew_ms: u64,
    pub policy_digest: String,
}

impl WorkflowEventSourcePolicy {
    pub fn new(
        source_id: impl Into<String>,
        key_id: impl Into<String>,
        project_id: impl Into<String>,
        allowed_event_kinds: impl IntoIterator<Item = String>,
        required_payload_keys: impl IntoIterator<Item = String>,
        allowed_payload_keys: impl IntoIterator<Item = String>,
        max_clock_skew_ms: u64,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: WORKFLOW_EVENT_SOURCE_POLICY_SCHEMA.to_owned(),
            source_id: source_id.into(),
            key_id: key_id.into(),
            project_id: project_id.into(),
            allowed_event_kinds: allowed_event_kinds.into_iter().collect(),
            required_payload_keys: required_payload_keys.into_iter().collect(),
            allowed_payload_keys: allowed_payload_keys.into_iter().collect(),
            max_clock_skew_ms,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_EVENT_SOURCE_POLICY_SCHEMA
            || self.allowed_event_kinds.is_empty()
            || self.allowed_event_kinds.len() > 128
            || self.required_payload_keys.len() > self.allowed_payload_keys.len()
            || self.max_clock_skew_ms == 0
            || self.max_clock_skew_ms > WORKFLOW_EVENT_MAX_CLOCK_SKEW_MS
            || !valid_digest(&self.policy_digest)
            || self.policy_digest != self.digest()
        {
            return Err("workflow_event_source_policy_invalid".to_owned());
        }
        for value in self
            .allowed_event_kinds
            .iter()
            .chain(&self.required_payload_keys)
            .chain(&self.allowed_payload_keys)
        {
            bounded(value, "workflow_event_filter_key", 128)?;
        }
        bounded(&self.source_id, "workflow_event_source", 128)?;
        bounded(&self.key_id, "workflow_event_key_id", 128)?;
        bounded(&self.project_id, "workflow_event_project", 256)?;
        Ok(())
    }

    pub fn matches(&self, event: &WorkflowEventIngress, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        event.validate()?;
        if self.source_id != event.source_id
            || self.key_id != event.key_id
            || self.project_id != event.project_id
            || !self.allowed_event_kinds.contains(&event.event_kind)
        {
            return Err("workflow_event_source_or_kind_denied".to_owned());
        }
        let skew = now_unix_ms.abs_diff(event.occurred_at_unix_ms);
        if now_unix_ms == 0 || skew > self.max_clock_skew_ms {
            return Err("workflow_event_clock_skew".to_owned());
        }
        let object = event
            .payload
            .as_object()
            .ok_or_else(|| "workflow_event_payload_object_required".to_owned())?;
        if !object
            .keys()
            .all(|key| self.allowed_payload_keys.contains(key))
            || !self
                .required_payload_keys
                .iter()
                .all(|key| object.contains_key(key))
        {
            return Err("workflow_event_payload_filter_denied".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "project_id": self.project_id,
            "allowed_event_kinds": self.allowed_event_kinds,
            "required_payload_keys": self.required_payload_keys,
            "allowed_payload_keys": self.allowed_payload_keys,
            "max_clock_skew_ms": self.max_clock_skew_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEventOccurrence {
    pub schema: String,
    pub source_id: String,
    pub event_id: String,
    pub event_kind: String,
    pub project_id: String,
    pub trigger_id: String,
    pub occurrence_key: String,
    pub payload_digest: String,
    pub ingress_digest: String,
    pub policy_digest: String,
    pub occurrence_digest: String,
}

impl WorkflowEventOccurrence {
    pub fn from_verified(
        event: &WorkflowEventIngress,
        policy: &WorkflowEventSourcePolicy,
    ) -> Result<Self, String> {
        policy.matches(event, event.occurred_at_unix_ms)?;
        let mut occurrence = Self {
            schema: WORKFLOW_EVENT_OCCURRENCE_SCHEMA.to_owned(),
            source_id: event.source_id.clone(),
            event_id: event.event_id.clone(),
            event_kind: event.event_kind.clone(),
            project_id: event.project_id.clone(),
            trigger_id: event.trigger_id.clone(),
            occurrence_key: event.occurrence_key(),
            payload_digest: event.payload_digest.clone(),
            ingress_digest: event.ingress_digest.clone(),
            policy_digest: policy.policy_digest.clone(),
            occurrence_digest: String::new(),
        };
        occurrence.occurrence_digest = occurrence.digest();
        occurrence.validate()?;
        Ok(occurrence)
    }

    pub fn to_fire_command(
        &self,
        event_ref: impl Into<String>,
    ) -> Result<AutomationCommand, String> {
        self.validate()?;
        let event_ref = event_ref.into();
        if !event_ref.starts_with("event:") || event_ref.len() > 512 {
            return Err("workflow_event_evidence_ref_invalid".to_owned());
        }
        Ok(AutomationCommand::Fire {
            trigger_id: self.trigger_id.clone(),
            firing_key: self.occurrence_key.clone(),
            event_ref: Some(event_ref),
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_EVENT_OCCURRENCE_SCHEMA
            || self.occurrence_key != format!("event:{}:{}", self.source_id, self.event_id)
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.ingress_digest)
            || !valid_digest(&self.policy_digest)
            || !valid_digest(&self.occurrence_digest)
            || self.occurrence_digest != self.digest()
        {
            return Err("workflow_event_occurrence_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.source_id, "workflow_event_source", 128),
            (&self.event_id, "workflow_event_id", 256),
            (&self.event_kind, "workflow_event_kind", 128),
            (&self.project_id, "workflow_event_project", 256),
            (&self.trigger_id, "workflow_event_trigger", 128),
            (&self.occurrence_key, "workflow_event_occurrence_key", 512),
        ] {
            bounded(value, field, max)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_id": self.source_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "project_id": self.project_id,
            "trigger_id": self.trigger_id,
            "occurrence_key": self.occurrence_key,
            "payload_digest": self.payload_digest,
            "ingress_digest": self.ingress_digest,
            "policy_digest": self.policy_digest,
        }))
    }
}

fn validate_payload(value: &Value) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| "workflow_event_payload_invalid".to_owned())?;
    if bytes.len() > WORKFLOW_EVENT_PAYLOAD_MAX_BYTES {
        return Err("workflow_event_payload_too_large".to_owned());
    }
    fn visit(value: &Value, depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Err("workflow_event_payload_depth_limit".to_owned());
        }
        match value {
            Value::Array(items) if items.len() > 256 => {
                Err("workflow_event_payload_array_limit".to_owned())
            }
            Value::Array(items) => items.iter().try_for_each(|item| visit(item, depth + 1)),
            Value::Object(items) if items.len() > 256 => {
                Err("workflow_event_payload_object_limit".to_owned())
            }
            Value::Object(items) => {
                for (key, item) in items {
                    bounded(key, "workflow_event_payload_key", 128)?;
                    visit(item, depth + 1)?;
                }
                Ok(())
            }
            Value::String(text) if text.len() > 4_096 => {
                Err("workflow_event_payload_string_limit".to_owned())
            }
            _ => Ok(()),
        }
    }
    visit(value, 0)
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
