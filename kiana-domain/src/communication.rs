//! Typed communication records. Messages are facts or requests for attention, never grants.
use crate::{canonical_journal_bytes, json_digest};
use serde::{Deserialize, Serialize};

pub const COMMUNICATION_MESSAGE_SCHEMA: &str = "kiana.communication-message.v1";
const MAX_MESSAGE_ID: usize = 256;
const MAX_PARTY_ID: usize = 256;
const MAX_SUBJECT: usize = 512;
const MAX_BODY: usize = 64 * 1024;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunicationMessageKind {
    Chat,
    Command,
    Handoff,
    Decision,
    StatusReport,
    Evidence,
    Incident,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommunicationMessage {
    pub schema: String,
    pub message_id: String,
    pub kind: CommunicationMessageKind,
    pub sender_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_id: Option<String>,
    pub subject: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_ref: Option<String>,
    pub ack_required: bool,
    pub payload_digest: String,
}

impl CommunicationMessage {
    pub fn new(
        message_id: impl Into<String>,
        kind: CommunicationMessageKind,
        sender_id: impl Into<String>,
        recipient_id: Option<String>,
        subject: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<Self, String> {
        let mut message = Self {
            schema: COMMUNICATION_MESSAGE_SCHEMA.to_owned(),
            message_id: message_id.into(),
            kind,
            sender_id: sender_id.into(),
            recipient_id,
            subject: subject.into(),
            body: body.into(),
            action_ref: None,
            ack_required: matches!(kind, CommunicationMessageKind::Handoff),
            payload_digest: String::new(),
        };
        message.payload_digest = message.digest();
        message.validate()?;
        Ok(message)
    }

    pub fn chat(
        message_id: impl Into<String>,
        sender_id: impl Into<String>,
        recipient_id: Option<String>,
        body: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(
            message_id,
            CommunicationMessageKind::Chat,
            sender_id,
            recipient_id,
            "chat",
            body,
        )
    }

    pub fn with_action_ref(mut self, action_ref: impl Into<String>) -> Result<Self, String> {
        self.action_ref = Some(action_ref.into());
        self.payload_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMMUNICATION_MESSAGE_SCHEMA {
            return Err("communication_message_schema_invalid".to_owned());
        }
        required(&self.message_id, "communication_message_id", MAX_MESSAGE_ID)?;
        required(&self.sender_id, "communication_sender", MAX_PARTY_ID)?;
        required(&self.subject, "communication_subject", MAX_SUBJECT)?;
        required(&self.body, "communication_body", MAX_BODY)?;
        if self
            .recipient_id
            .as_deref()
            .is_some_and(|recipient| recipient.trim().is_empty() || recipient.len() > MAX_PARTY_ID)
        {
            return Err("communication_recipient_invalid".to_owned());
        }
        if self
            .action_ref
            .as_deref()
            .is_some_and(|action| action.trim().is_empty() || action.len() > MAX_MESSAGE_ID)
        {
            return Err("communication_action_ref_invalid".to_owned());
        }
        if matches!(self.kind, CommunicationMessageKind::Chat)
            && (self.ack_required || self.action_ref.is_some())
        {
            return Err("communication_chat_authority_fields_forbidden".to_owned());
        }
        if matches!(self.kind, CommunicationMessageKind::Handoff)
            && (self.recipient_id.is_none() || !self.ack_required)
        {
            return Err("communication_handoff_ack_required".to_owned());
        }
        digest(&self.payload_digest, "communication_payload_digest")?;
        if self.payload_digest != self.digest() {
            return Err("communication_payload_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Communication can request a later ControlPlane command, but never carries authority.
    pub const fn grants_authority(&self) -> bool {
        false
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "payload_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
