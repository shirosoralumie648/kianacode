//! Versioned notification and messaging contracts.
//!
//! These values are facts or bounded delivery intents. They do not send anything, grant
//! authority, or replace the EventLog. Every action reference must return to ControlPlane for a
//! fresh authorization check before a command is applied.
use crate::{
    canonical_journal_bytes, json_digest, redact_text, ActionRefId, DeliveryAttemptId,
    DeliveryReceiptId, MessageId, NotificationId, ProjectId, SubscriptionId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fmt;

pub const MESSAGE_SCHEMA: &str = "kiana.message.v1";
pub const NOTIFICATION_SCHEMA: &str = "kiana.notification.v1";
pub const SUBSCRIPTION_SCHEMA: &str = "kiana.notification-subscription.v1";
pub const DELIVERY_ATTEMPT_SCHEMA: &str = "kiana.notification-delivery-attempt.v1";
pub const ACTION_REF_SCHEMA: &str = "kiana.notification-action-ref.v1";
pub const DELIVERY_RECEIPT_SCHEMA: &str = "kiana.notification-delivery-receipt.v1";
pub const NOTIFICATION_DEDUP_REQUEST_SCHEMA: &str = "kiana.notification-dedup-request.v1";
pub const NOTIFICATION_DEDUP_RECORD_SCHEMA: &str = "kiana.notification-dedup-record.v1";
pub const MAX_MESSAGE_BODY_BYTES: usize = 64 * 1024;
pub const MAX_NOTIFICATION_TEXT_BYTES: usize = 512;
pub const MAX_NOTIFICATION_SCOPE: usize = 64;
pub const MAX_NOTIFICATION_CHANNELS: usize = 8;
pub const MAX_NOTIFICATION_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

fn required_text(value: &str, field: &str, max_bytes: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.as_bytes().len() > max_bytes
        || value.as_bytes().contains(&0)
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_secret_detected"));
    }
    Ok(())
}

fn validate_id<T>(is_nil: bool, field: &str) -> Result<(), String> {
    let _ = std::marker::PhantomData::<T>;
    if is_nil {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
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

fn canonical_scope(
    mut values: Vec<String>,
    field: &str,
    allow_empty: bool,
) -> Result<Vec<String>, String> {
    if !allow_empty && values.is_empty() {
        return Err(format!("{field}_required"));
    }
    if values.len() > MAX_NOTIFICATION_SCOPE {
        return Err(format!("{field}_too_large"));
    }
    for value in &mut values {
        *value = value.trim().to_owned();
        required_text(value, field, MAX_NOTIFICATION_TEXT_BYTES)?;
    }
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(format!("{field}_duplicate"));
    }
    Ok(values)
}

fn validate_window(
    created_at_unix_ms: u64,
    expires_at_unix_ms: Option<u64>,
    field: &str,
) -> Result<(), String> {
    if let Some(expires_at) = expires_at_unix_ms {
        if expires_at <= created_at_unix_ms
            || expires_at.saturating_sub(created_at_unix_ms) > MAX_NOTIFICATION_TTL_MS
        {
            return Err(format!("{field}_ttl_invalid"));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Chat,
    Command,
    Handoff,
    Decision,
    StatusReport,
    Evidence,
    Incident,
    Reminder,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Message {
    pub schema: String,
    pub message_id: MessageId,
    pub kind: MessageKind,
    pub sender_id: String,
    pub recipient_id: String,
    pub project_id: Option<ProjectId>,
    pub scope: Vec<String>,
    pub body: String,
    pub action_ref_id: Option<ActionRefId>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub message_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageRepr {
    schema: String,
    message_id: MessageId,
    kind: MessageKind,
    sender_id: String,
    recipient_id: String,
    #[serde(default)]
    project_id: Option<ProjectId>,
    #[serde(default)]
    scope: Vec<String>,
    body: String,
    #[serde(default)]
    action_ref_id: Option<ActionRefId>,
    created_at_unix_ms: u64,
    #[serde(default)]
    expires_at_unix_ms: Option<u64>,
    message_digest: String,
}

impl MessageRepr {
    fn from_message(message: &Message) -> Self {
        Self {
            schema: message.schema.clone(),
            message_id: message.message_id,
            kind: message.kind,
            sender_id: message.sender_id.clone(),
            recipient_id: message.recipient_id.clone(),
            project_id: message.project_id,
            scope: message.scope.clone(),
            body: message.body.clone(),
            action_ref_id: message.action_ref_id,
            created_at_unix_ms: message.created_at_unix_ms,
            expires_at_unix_ms: message.expires_at_unix_ms,
            message_digest: message.message_digest.clone(),
        }
    }

    fn into_message(self) -> Message {
        Message {
            schema: self.schema,
            message_id: self.message_id,
            kind: self.kind,
            sender_id: self.sender_id,
            recipient_id: self.recipient_id,
            project_id: self.project_id,
            scope: self.scope,
            body: self.body,
            action_ref_id: self.action_ref_id,
            created_at_unix_ms: self.created_at_unix_ms,
            expires_at_unix_ms: self.expires_at_unix_ms,
            message_digest: self.message_digest,
        }
    }
}

impl Serialize for Message {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.validate().map_err(serde::ser::Error::custom)?;
        MessageRepr::from_message(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let message = MessageRepr::deserialize(deserializer)?.into_message();
        message.validate().map_err(serde::de::Error::custom)?;
        Ok(message)
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let schema = redact_text(&self.schema);
        let sender_id = redact_text(&self.sender_id);
        let recipient_id = redact_text(&self.recipient_id);
        let scope = self
            .scope
            .iter()
            .map(|value| redact_text(value))
            .collect::<Vec<_>>();
        let body = redact_text(&self.body);
        let message_digest = redact_text(&self.message_digest);

        formatter
            .debug_struct("Message")
            .field("schema", &schema)
            .field("message_id", &self.message_id)
            .field("kind", &self.kind)
            .field("sender_id", &sender_id)
            .field("recipient_id", &recipient_id)
            .field("project_id", &self.project_id)
            .field("scope", &scope)
            .field("body", &body)
            .field("action_ref_id", &self.action_ref_id)
            .field("created_at_unix_ms", &self.created_at_unix_ms)
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .field("message_digest", &message_digest)
            .finish()
    }
}

impl Message {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: MessageKind,
        sender_id: impl Into<String>,
        recipient_id: impl Into<String>,
        project_id: Option<ProjectId>,
        scope: Vec<String>,
        body: impl Into<String>,
        action_ref_id: Option<ActionRefId>,
        created_at_unix_ms: u64,
        expires_at_unix_ms: Option<u64>,
    ) -> Result<Self, String> {
        let mut message = Self {
            schema: MESSAGE_SCHEMA.to_owned(),
            message_id: MessageId::new(),
            kind,
            sender_id: sender_id.into(),
            recipient_id: recipient_id.into(),
            project_id,
            scope: canonical_scope(scope, "message_scope", true)?,
            body: body.into(),
            action_ref_id,
            created_at_unix_ms,
            expires_at_unix_ms,
            message_digest: String::new(),
        };
        message.message_digest = message.digest();
        message.validate()?;
        Ok(message)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MESSAGE_SCHEMA {
            return Err("message_schema_invalid".to_owned());
        }
        validate_id::<MessageId>(self.message_id.as_uuid().is_nil(), "message_id")?;
        required_text(
            &self.sender_id,
            "message_sender",
            MAX_NOTIFICATION_TEXT_BYTES,
        )?;
        required_text(
            &self.recipient_id,
            "message_recipient",
            MAX_NOTIFICATION_TEXT_BYTES,
        )?;
        if canonical_scope(self.scope.clone(), "message_scope", true)? != self.scope {
            return Err("message_scope_noncanonical".to_owned());
        }
        required_text(&self.body, "message_body", MAX_MESSAGE_BODY_BYTES)?;
        if self
            .action_ref_id
            .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("message_action_ref_invalid".to_owned());
        }
        validate_window(self.created_at_unix_ms, self.expires_at_unix_ms, "message")?;
        validate_digest(&self.message_digest, "message_digest")?;
        if self.message_digest != self.digest() {
            return Err("message_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "message_id": self.message_id,
            "kind": self.kind,
            "sender_id": self.sender_id,
            "recipient_id": self.recipient_id,
            "project_id": self.project_id,
            "scope": self.scope,
            "body": self.body,
            "action_ref_id": self.action_ref_id,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationDedupRequest {
    pub schema: String,
    pub dedup_key: String,
    pub content_digest: String,
    pub subscription_revision: u64,
    pub notification: Notification,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub request_digest: String,
}

impl NotificationDedupRequest {
    pub fn new(
        dedup_key: impl Into<String>,
        notification: Notification,
        expected_revision: Option<u64>,
    ) -> Result<Self, String> {
        notification.validate()?;
        let mut request = Self {
            schema: NOTIFICATION_DEDUP_REQUEST_SCHEMA.to_owned(),
            dedup_key: dedup_key.into(),
            content_digest: notification.content_digest(),
            subscription_revision: notification.subscription_revision,
            notification,
            expected_revision,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_DEDUP_REQUEST_SCHEMA
            || self.dedup_key.trim().is_empty()
            || self.dedup_key.len() > 512
            || self.dedup_key.contains(['\0', '\n', '\r'])
            || self.subscription_revision == 0
            || self.expected_revision.is_some_and(|revision| revision == 0)
        {
            return Err("notification_dedup_request_invalid".to_owned());
        }
        self.notification.validate()?;
        validate_digest(&self.content_digest, "notification_dedup_content_digest")?;
        if self.content_digest != self.notification.content_digest()
            || self.subscription_revision != self.notification.subscription_revision
        {
            return Err("notification_dedup_content_mismatch".to_owned());
        }
        validate_digest(&self.request_digest, "notification_dedup_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("notification_dedup_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "dedup_key": self.dedup_key,
            "content_digest": self.content_digest,
            "subscription_revision": self.subscription_revision,
            "notification": self.notification,
            "expected_revision": self.expected_revision,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationDedupRecord {
    pub schema: String,
    pub dedup_key: String,
    pub content_digest: String,
    pub subscription_revision: u64,
    pub notification: Notification,
    pub revision: u64,
    pub record_digest: String,
}

impl NotificationDedupRecord {
    pub fn from_request(request: &NotificationDedupRequest, revision: u64) -> Result<Self, String> {
        request.validate()?;
        if revision == 0 {
            return Err("notification_dedup_revision_invalid".to_owned());
        }
        let mut record = Self {
            schema: NOTIFICATION_DEDUP_RECORD_SCHEMA.to_owned(),
            dedup_key: request.dedup_key.clone(),
            content_digest: request.content_digest.clone(),
            subscription_revision: request.subscription_revision,
            notification: request.notification.clone(),
            revision,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_DEDUP_RECORD_SCHEMA
            || self.dedup_key.trim().is_empty()
            || self.dedup_key.len() > 512
            || self.revision == 0
            || self.subscription_revision == 0
        {
            return Err("notification_dedup_record_invalid".to_owned());
        }
        self.notification.validate()?;
        validate_digest(&self.content_digest, "notification_dedup_content_digest")?;
        if self.content_digest != self.notification.content_digest()
            || self.subscription_revision != self.notification.subscription_revision
        {
            return Err("notification_dedup_record_content_mismatch".to_owned());
        }
        validate_digest(&self.record_digest, "notification_dedup_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("notification_dedup_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn advance(
        &self,
        expected_revision: u64,
        notification: Notification,
    ) -> Result<Self, String> {
        self.validate()?;
        if self.revision != expected_revision {
            return Err("notification_dedup_revision_conflict".to_owned());
        }
        if notification.notification_id != self.notification.notification_id {
            return Err("notification_dedup_notification_identity_conflict".to_owned());
        }
        let request = NotificationDedupRequest::new(
            self.dedup_key.clone(),
            notification,
            Some(expected_revision),
        )?;
        if request.content_digest != self.content_digest
            || request.subscription_revision != self.subscription_revision
        {
            return Err("notification_dedup_content_conflict".to_owned());
        }
        let next_revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| "notification_dedup_revision_overflow".to_owned())?;
        Self::from_request(&request, next_revision)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "dedup_key": self.dedup_key,
            "content_digest": self.content_digest,
            "subscription_revision": self.subscription_revision,
            "notification": self.notification,
            "revision": self.revision,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    InApp,
    RunStream,
    Web,
    Workbench,
    Cli,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationStatus {
    Pending,
    Queued,
    Delivered,
    Read,
    Dismissed,
    Expired,
    Failed,
    Unknown,
}

impl NotificationStatus {
    pub fn transition(self, next: Self) -> Result<Self, &'static str> {
        if self == next {
            return Err("notification_duplicate_terminal");
        }
        let allowed = matches!(
            (self, next),
            (Self::Pending, Self::Queued | Self::Failed | Self::Expired)
                | (Self::Queued, Self::Delivered | Self::Failed | Self::Unknown)
                | (Self::Delivered, Self::Read | Self::Dismissed)
        );
        if allowed {
            Ok(next)
        } else {
            Err("notification_transition_invalid")
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Read | Self::Dismissed | Self::Expired | Self::Failed | Self::Unknown
        )
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Notification {
    pub schema: String,
    pub notification_id: NotificationId,
    pub message_id: MessageId,
    pub recipient_id: String,
    pub project_id: Option<ProjectId>,
    pub scope: Vec<String>,
    pub channel: NotificationChannel,
    pub status: NotificationStatus,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub subscription_revision: u64,
    pub action_ref_id: Option<ActionRefId>,
    pub notification_digest: String,
}

impl Notification {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        message_id: MessageId,
        recipient_id: impl Into<String>,
        project_id: Option<ProjectId>,
        scope: Vec<String>,
        channel: NotificationChannel,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        subscription_revision: u64,
        action_ref_id: Option<ActionRefId>,
    ) -> Result<Self, String> {
        let mut notification = Self {
            schema: NOTIFICATION_SCHEMA.to_owned(),
            notification_id: NotificationId::new(),
            message_id,
            recipient_id: recipient_id.into(),
            project_id,
            scope: canonical_scope(scope, "notification_scope", false)?,
            channel,
            status: NotificationStatus::Pending,
            created_at_unix_ms,
            expires_at_unix_ms,
            subscription_revision,
            action_ref_id,
            notification_digest: String::new(),
        };
        notification.notification_digest = notification.digest();
        notification.validate()?;
        Ok(notification)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_SCHEMA {
            return Err("notification_schema_invalid".to_owned());
        }
        validate_id::<NotificationId>(self.notification_id.as_uuid().is_nil(), "notification_id")?;
        validate_id::<MessageId>(self.message_id.as_uuid().is_nil(), "message_id")?;
        required_text(
            &self.recipient_id,
            "notification_recipient",
            MAX_NOTIFICATION_TEXT_BYTES,
        )?;
        if canonical_scope(self.scope.clone(), "notification_scope", false)? != self.scope {
            return Err("notification_scope_noncanonical".to_owned());
        }
        if self.subscription_revision == 0 {
            return Err("notification_subscription_revision_invalid".to_owned());
        }
        validate_window(
            self.created_at_unix_ms,
            Some(self.expires_at_unix_ms),
            "notification",
        )?;
        if self
            .action_ref_id
            .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("notification_action_ref_invalid".to_owned());
        }
        validate_digest(&self.notification_digest, "notification_digest")?;
        if self.notification_digest != self.digest() {
            return Err("notification_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn transition_status(&mut self, next: NotificationStatus) -> Result<(), String> {
        self.status = self.status.transition(next).map_err(str::to_owned)?;
        self.notification_digest = self.digest();
        Ok(())
    }

    pub fn validate_for_subscription(&self, subscription: &Subscription) -> Result<(), String> {
        self.validate()?;
        subscription.validate()?;
        if self.subscription_revision != subscription.revision {
            return Err("notification_subscription_revision_mismatch".to_owned());
        }
        if subscription.status != SubscriptionStatus::Active
            || self.recipient_id != subscription.recipient_id
            || self.project_id != subscription.project_id
            || !scope_is_subset(&self.scope, &subscription.scope)
            || !subscription.channels.contains(&self.channel)
        {
            return Err("notification_scope_exceeds_subscription".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification_id": self.notification_id,
            "message_id": self.message_id,
            "recipient_id": self.recipient_id,
            "project_id": self.project_id,
            "scope": self.scope,
            "channel": self.channel,
            "status": self.status,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "subscription_revision": self.subscription_revision,
            "action_ref_id": self.action_ref_id,
        }))
    }

    /// Digest of the notification content used for deduplication. It intentionally excludes the
    /// generated notification ID and mutable delivery status, so retries with a new in-memory DTO
    /// can still match the original intent.
    pub fn content_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "message_id": self.message_id,
            "recipient_id": self.recipient_id,
            "project_id": self.project_id,
            "scope": self.scope,
            "channel": self.channel,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "subscription_revision": self.subscription_revision,
            "action_ref_id": self.action_ref_id,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Active,
    Paused,
    Revoked,
    Expired,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Subscription {
    pub schema: String,
    pub subscription_id: SubscriptionId,
    pub recipient_id: String,
    pub project_id: Option<ProjectId>,
    pub scope: Vec<String>,
    pub channels: Vec<NotificationChannel>,
    pub revision: u64,
    pub status: SubscriptionStatus,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub subscription_digest: String,
}

impl Subscription {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        recipient_id: impl Into<String>,
        project_id: Option<ProjectId>,
        scope: Vec<String>,
        mut channels: Vec<NotificationChannel>,
        revision: u64,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        channels.sort();
        let mut subscription = Self {
            schema: SUBSCRIPTION_SCHEMA.to_owned(),
            subscription_id: SubscriptionId::new(),
            recipient_id: recipient_id.into(),
            project_id,
            scope: canonical_scope(scope, "subscription_scope", false)?,
            channels,
            revision,
            status: SubscriptionStatus::Active,
            created_at_unix_ms,
            expires_at_unix_ms,
            subscription_digest: String::new(),
        };
        subscription.subscription_digest = subscription.digest();
        subscription.validate()?;
        Ok(subscription)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SUBSCRIPTION_SCHEMA {
            return Err("subscription_schema_invalid".to_owned());
        }
        validate_id::<SubscriptionId>(self.subscription_id.as_uuid().is_nil(), "subscription_id")?;
        required_text(
            &self.recipient_id,
            "subscription_recipient",
            MAX_NOTIFICATION_TEXT_BYTES,
        )?;
        if canonical_scope(self.scope.clone(), "subscription_scope", false)? != self.scope {
            return Err("subscription_scope_noncanonical".to_owned());
        }
        if self.channels.is_empty() || self.channels.len() > MAX_NOTIFICATION_CHANNELS {
            return Err("subscription_channels_invalid".to_owned());
        }
        if self.channels.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("subscription_channels_noncanonical".to_owned());
        }
        if self.revision == 0 {
            return Err("subscription_revision_invalid".to_owned());
        }
        validate_window(
            self.created_at_unix_ms,
            Some(self.expires_at_unix_ms),
            "subscription",
        )?;
        validate_digest(&self.subscription_digest, "subscription_digest")?;
        if self.subscription_digest != self.digest() {
            return Err("subscription_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "subscription_id": self.subscription_id,
            "recipient_id": self.recipient_id,
            "project_id": self.project_id,
            "scope": self.scope,
            "channels": self.channels,
            "revision": self.revision,
            "status": self.status,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryAttemptStatus {
    Pending,
    Claimed,
    Submitted,
    Acknowledged,
    Failed,
    Unknown,
}

impl DeliveryAttemptStatus {
    pub fn transition(self, next: Self) -> Result<Self, &'static str> {
        if self == next {
            return Err("delivery_attempt_duplicate_terminal");
        }
        let allowed = matches!(
            (self, next),
            (Self::Pending, Self::Claimed | Self::Failed)
                | (
                    Self::Claimed,
                    Self::Submitted | Self::Failed | Self::Unknown
                )
                | (
                    Self::Submitted,
                    Self::Acknowledged | Self::Failed | Self::Unknown
                )
        );
        if allowed {
            Ok(next)
        } else {
            Err("delivery_attempt_transition_invalid")
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DeliveryAttempt {
    pub schema: String,
    pub delivery_attempt_id: DeliveryAttemptId,
    pub notification_id: NotificationId,
    pub subscription_id: SubscriptionId,
    pub attempt_number: u32,
    pub status: DeliveryAttemptStatus,
    pub authority_epoch: u64,
    pub lease_expires_at_unix_ms: Option<u64>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub attempt_digest: String,
}

impl DeliveryAttempt {
    pub fn new(
        notification_id: NotificationId,
        subscription_id: SubscriptionId,
        authority_epoch: u64,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut attempt = Self {
            schema: DELIVERY_ATTEMPT_SCHEMA.to_owned(),
            delivery_attempt_id: DeliveryAttemptId::new(),
            notification_id,
            subscription_id,
            attempt_number: 1,
            status: DeliveryAttemptStatus::Pending,
            authority_epoch,
            lease_expires_at_unix_ms: None,
            created_at_unix_ms,
            updated_at_unix_ms: created_at_unix_ms,
            attempt_digest: String::new(),
        };
        attempt.attempt_digest = attempt.digest();
        attempt.validate()?;
        Ok(attempt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DELIVERY_ATTEMPT_SCHEMA
            || self.attempt_number == 0
            || self.authority_epoch == 0
            || self.updated_at_unix_ms < self.created_at_unix_ms
        {
            return Err("delivery_attempt_header_invalid".to_owned());
        }
        validate_id::<DeliveryAttemptId>(
            self.delivery_attempt_id.as_uuid().is_nil(),
            "delivery_attempt_id",
        )?;
        validate_id::<NotificationId>(self.notification_id.as_uuid().is_nil(), "notification_id")?;
        validate_id::<SubscriptionId>(self.subscription_id.as_uuid().is_nil(), "subscription_id")?;
        if let Some(lease) = self.lease_expires_at_unix_ms {
            if lease <= self.updated_at_unix_ms
                || lease.saturating_sub(self.updated_at_unix_ms) > MAX_NOTIFICATION_TTL_MS
            {
                return Err("delivery_attempt_lease_ttl_invalid".to_owned());
            }
        }
        validate_digest(&self.attempt_digest, "delivery_attempt_digest")?;
        if self.attempt_digest != self.digest() {
            return Err("delivery_attempt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn transition_status(
        &mut self,
        next: DeliveryAttemptStatus,
        updated_at_unix_ms: u64,
    ) -> Result<(), String> {
        if updated_at_unix_ms < self.updated_at_unix_ms {
            return Err("delivery_attempt_time_regression".to_owned());
        }
        self.status = self.status.transition(next).map_err(str::to_owned)?;
        self.updated_at_unix_ms = updated_at_unix_ms;
        self.attempt_digest = self.digest();
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "delivery_attempt_id": self.delivery_attempt_id,
            "notification_id": self.notification_id,
            "subscription_id": self.subscription_id,
            "attempt_number": self.attempt_number,
            "status": self.status,
            "authority_epoch": self.authority_epoch,
            "lease_expires_at_unix_ms": self.lease_expires_at_unix_ms,
            "created_at_unix_ms": self.created_at_unix_ms,
            "updated_at_unix_ms": self.updated_at_unix_ms,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryReceiptStatus {
    Acknowledged,
    Failed,
    Unknown,
}

#[derive(Clone, Eq, PartialEq)]
pub struct DeliveryReceipt {
    pub schema: String,
    pub delivery_receipt_id: DeliveryReceiptId,
    pub notification_id: NotificationId,
    pub delivery_attempt_id: DeliveryAttemptId,
    pub recipient_id: String,
    pub status: DeliveryReceiptStatus,
    pub observed_at_unix_ms: u64,
    pub response_digest: Option<String>,
    pub receipt_digest: String,
}

impl DeliveryReceipt {
    pub fn new(
        notification_id: NotificationId,
        delivery_attempt_id: DeliveryAttemptId,
        recipient_id: impl Into<String>,
        status: DeliveryReceiptStatus,
        observed_at_unix_ms: u64,
        response_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut receipt = Self {
            schema: DELIVERY_RECEIPT_SCHEMA.to_owned(),
            delivery_receipt_id: DeliveryReceiptId::new(),
            notification_id,
            delivery_attempt_id,
            recipient_id: recipient_id.into(),
            status,
            observed_at_unix_ms,
            response_digest,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DELIVERY_RECEIPT_SCHEMA {
            return Err("delivery_receipt_schema_invalid".to_owned());
        }
        validate_id::<DeliveryReceiptId>(
            self.delivery_receipt_id.as_uuid().is_nil(),
            "delivery_receipt_id",
        )?;
        validate_id::<NotificationId>(self.notification_id.as_uuid().is_nil(), "notification_id")?;
        validate_id::<DeliveryAttemptId>(
            self.delivery_attempt_id.as_uuid().is_nil(),
            "delivery_attempt_id",
        )?;
        required_text(
            &self.recipient_id,
            "delivery_receipt_recipient",
            MAX_NOTIFICATION_TEXT_BYTES,
        )?;
        if let Some(response) = self.response_digest.as_deref() {
            validate_digest(response, "delivery_receipt_response_digest")?;
        }
        validate_digest(&self.receipt_digest, "delivery_receipt_digest")?;
        if self.receipt_digest != self.digest() {
            return Err("delivery_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "delivery_receipt_id": self.delivery_receipt_id,
            "notification_id": self.notification_id,
            "delivery_attempt_id": self.delivery_attempt_id,
            "recipient_id": self.recipient_id,
            "status": self.status,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "response_digest": self.response_digest,
        }))
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ActionRef {
    pub schema: String,
    pub action_ref_id: ActionRefId,
    pub command: String,
    pub target_revision: u64,
    pub scope: Vec<String>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub action_digest: String,
}

impl ActionRef {
    pub fn new(
        command: impl Into<String>,
        target_revision: u64,
        scope: Vec<String>,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut action = Self {
            schema: ACTION_REF_SCHEMA.to_owned(),
            action_ref_id: ActionRefId::new(),
            command: command.into(),
            target_revision,
            scope: canonical_scope(scope, "action_scope", false)?,
            created_at_unix_ms,
            expires_at_unix_ms,
            action_digest: String::new(),
        };
        action.action_digest = action.digest();
        action.validate()?;
        Ok(action)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ACTION_REF_SCHEMA || self.target_revision == 0 {
            return Err("action_ref_header_invalid".to_owned());
        }
        validate_id::<ActionRefId>(self.action_ref_id.as_uuid().is_nil(), "action_ref_id")?;
        required_text(
            &self.command,
            "action_ref_command",
            MAX_NOTIFICATION_TEXT_BYTES,
        )?;
        if canonical_scope(self.scope.clone(), "action_scope", false)? != self.scope {
            return Err("action_scope_noncanonical".to_owned());
        }
        validate_window(
            self.created_at_unix_ms,
            Some(self.expires_at_unix_ms),
            "action_ref",
        )?;
        validate_digest(&self.action_digest, "action_ref_digest")?;
        if self.action_digest != self.digest() {
            return Err("action_ref_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "action_ref_id": self.action_ref_id,
            "command": self.command,
            "target_revision": self.target_revision,
            "scope": self.scope,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

macro_rules! impl_validated_serde {
    ($value:ty, $repr:ty) => {
        impl Serialize for $value {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                self.validate().map_err(serde::ser::Error::custom)?;
                <$repr>::from(self).serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $value {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value: Self = <$repr>::deserialize(deserializer)?.into();
                value.validate().map_err(serde::de::Error::custom)?;
                Ok(value)
            }
        }
    };
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationRepr {
    schema: String,
    notification_id: NotificationId,
    message_id: MessageId,
    recipient_id: String,
    #[serde(default)]
    project_id: Option<ProjectId>,
    scope: Vec<String>,
    channel: NotificationChannel,
    status: NotificationStatus,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    subscription_revision: u64,
    #[serde(default)]
    action_ref_id: Option<ActionRefId>,
    notification_digest: String,
}

impl From<&Notification> for NotificationRepr {
    fn from(value: &Notification) -> Self {
        Self {
            schema: value.schema.clone(),
            notification_id: value.notification_id,
            message_id: value.message_id,
            recipient_id: value.recipient_id.clone(),
            project_id: value.project_id,
            scope: value.scope.clone(),
            channel: value.channel,
            status: value.status,
            created_at_unix_ms: value.created_at_unix_ms,
            expires_at_unix_ms: value.expires_at_unix_ms,
            subscription_revision: value.subscription_revision,
            action_ref_id: value.action_ref_id,
            notification_digest: value.notification_digest.clone(),
        }
    }
}

impl From<NotificationRepr> for Notification {
    fn from(value: NotificationRepr) -> Self {
        Self {
            schema: value.schema,
            notification_id: value.notification_id,
            message_id: value.message_id,
            recipient_id: value.recipient_id,
            project_id: value.project_id,
            scope: value.scope,
            channel: value.channel,
            status: value.status,
            created_at_unix_ms: value.created_at_unix_ms,
            expires_at_unix_ms: value.expires_at_unix_ms,
            subscription_revision: value.subscription_revision,
            action_ref_id: value.action_ref_id,
            notification_digest: value.notification_digest,
        }
    }
}

impl_validated_serde!(Notification, NotificationRepr);

impl fmt::Debug for Notification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let schema = redact_text(&self.schema);
        let recipient_id = redact_text(&self.recipient_id);
        let scope = self
            .scope
            .iter()
            .map(|value| redact_text(value))
            .collect::<Vec<_>>();
        let notification_digest = redact_text(&self.notification_digest);

        formatter
            .debug_struct("Notification")
            .field("schema", &schema)
            .field("notification_id", &self.notification_id)
            .field("message_id", &self.message_id)
            .field("recipient_id", &recipient_id)
            .field("project_id", &self.project_id)
            .field("scope", &scope)
            .field("channel", &self.channel)
            .field("status", &self.status)
            .field("created_at_unix_ms", &self.created_at_unix_ms)
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .field("subscription_revision", &self.subscription_revision)
            .field("action_ref_id", &self.action_ref_id)
            .field("notification_digest", &notification_digest)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionRepr {
    schema: String,
    subscription_id: SubscriptionId,
    recipient_id: String,
    #[serde(default)]
    project_id: Option<ProjectId>,
    scope: Vec<String>,
    channels: Vec<NotificationChannel>,
    revision: u64,
    status: SubscriptionStatus,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    subscription_digest: String,
}

impl From<&Subscription> for SubscriptionRepr {
    fn from(value: &Subscription) -> Self {
        Self {
            schema: value.schema.clone(),
            subscription_id: value.subscription_id,
            recipient_id: value.recipient_id.clone(),
            project_id: value.project_id,
            scope: value.scope.clone(),
            channels: value.channels.clone(),
            revision: value.revision,
            status: value.status,
            created_at_unix_ms: value.created_at_unix_ms,
            expires_at_unix_ms: value.expires_at_unix_ms,
            subscription_digest: value.subscription_digest.clone(),
        }
    }
}

impl From<SubscriptionRepr> for Subscription {
    fn from(value: SubscriptionRepr) -> Self {
        Self {
            schema: value.schema,
            subscription_id: value.subscription_id,
            recipient_id: value.recipient_id,
            project_id: value.project_id,
            scope: value.scope,
            channels: value.channels,
            revision: value.revision,
            status: value.status,
            created_at_unix_ms: value.created_at_unix_ms,
            expires_at_unix_ms: value.expires_at_unix_ms,
            subscription_digest: value.subscription_digest,
        }
    }
}

impl_validated_serde!(Subscription, SubscriptionRepr);

impl fmt::Debug for Subscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let schema = redact_text(&self.schema);
        let recipient_id = redact_text(&self.recipient_id);
        let scope = self
            .scope
            .iter()
            .map(|value| redact_text(value))
            .collect::<Vec<_>>();
        let subscription_digest = redact_text(&self.subscription_digest);

        formatter
            .debug_struct("Subscription")
            .field("schema", &schema)
            .field("subscription_id", &self.subscription_id)
            .field("recipient_id", &recipient_id)
            .field("project_id", &self.project_id)
            .field("scope", &scope)
            .field("channels", &self.channels)
            .field("revision", &self.revision)
            .field("status", &self.status)
            .field("created_at_unix_ms", &self.created_at_unix_ms)
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .field("subscription_digest", &subscription_digest)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryAttemptRepr {
    schema: String,
    delivery_attempt_id: DeliveryAttemptId,
    notification_id: NotificationId,
    subscription_id: SubscriptionId,
    attempt_number: u32,
    status: DeliveryAttemptStatus,
    authority_epoch: u64,
    #[serde(default)]
    lease_expires_at_unix_ms: Option<u64>,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
    attempt_digest: String,
}

impl From<&DeliveryAttempt> for DeliveryAttemptRepr {
    fn from(value: &DeliveryAttempt) -> Self {
        Self {
            schema: value.schema.clone(),
            delivery_attempt_id: value.delivery_attempt_id,
            notification_id: value.notification_id,
            subscription_id: value.subscription_id,
            attempt_number: value.attempt_number,
            status: value.status,
            authority_epoch: value.authority_epoch,
            lease_expires_at_unix_ms: value.lease_expires_at_unix_ms,
            created_at_unix_ms: value.created_at_unix_ms,
            updated_at_unix_ms: value.updated_at_unix_ms,
            attempt_digest: value.attempt_digest.clone(),
        }
    }
}

impl From<DeliveryAttemptRepr> for DeliveryAttempt {
    fn from(value: DeliveryAttemptRepr) -> Self {
        Self {
            schema: value.schema,
            delivery_attempt_id: value.delivery_attempt_id,
            notification_id: value.notification_id,
            subscription_id: value.subscription_id,
            attempt_number: value.attempt_number,
            status: value.status,
            authority_epoch: value.authority_epoch,
            lease_expires_at_unix_ms: value.lease_expires_at_unix_ms,
            created_at_unix_ms: value.created_at_unix_ms,
            updated_at_unix_ms: value.updated_at_unix_ms,
            attempt_digest: value.attempt_digest,
        }
    }
}

impl_validated_serde!(DeliveryAttempt, DeliveryAttemptRepr);

impl fmt::Debug for DeliveryAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let schema = redact_text(&self.schema);
        let attempt_digest = redact_text(&self.attempt_digest);

        formatter
            .debug_struct("DeliveryAttempt")
            .field("schema", &schema)
            .field("delivery_attempt_id", &self.delivery_attempt_id)
            .field("notification_id", &self.notification_id)
            .field("subscription_id", &self.subscription_id)
            .field("attempt_number", &self.attempt_number)
            .field("status", &self.status)
            .field("authority_epoch", &self.authority_epoch)
            .field("lease_expires_at_unix_ms", &self.lease_expires_at_unix_ms)
            .field("created_at_unix_ms", &self.created_at_unix_ms)
            .field("updated_at_unix_ms", &self.updated_at_unix_ms)
            .field("attempt_digest", &attempt_digest)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryReceiptRepr {
    schema: String,
    delivery_receipt_id: DeliveryReceiptId,
    notification_id: NotificationId,
    delivery_attempt_id: DeliveryAttemptId,
    recipient_id: String,
    status: DeliveryReceiptStatus,
    observed_at_unix_ms: u64,
    #[serde(default)]
    response_digest: Option<String>,
    receipt_digest: String,
}

impl From<&DeliveryReceipt> for DeliveryReceiptRepr {
    fn from(value: &DeliveryReceipt) -> Self {
        Self {
            schema: value.schema.clone(),
            delivery_receipt_id: value.delivery_receipt_id,
            notification_id: value.notification_id,
            delivery_attempt_id: value.delivery_attempt_id,
            recipient_id: value.recipient_id.clone(),
            status: value.status,
            observed_at_unix_ms: value.observed_at_unix_ms,
            response_digest: value.response_digest.clone(),
            receipt_digest: value.receipt_digest.clone(),
        }
    }
}

impl From<DeliveryReceiptRepr> for DeliveryReceipt {
    fn from(value: DeliveryReceiptRepr) -> Self {
        Self {
            schema: value.schema,
            delivery_receipt_id: value.delivery_receipt_id,
            notification_id: value.notification_id,
            delivery_attempt_id: value.delivery_attempt_id,
            recipient_id: value.recipient_id,
            status: value.status,
            observed_at_unix_ms: value.observed_at_unix_ms,
            response_digest: value.response_digest,
            receipt_digest: value.receipt_digest,
        }
    }
}

impl_validated_serde!(DeliveryReceipt, DeliveryReceiptRepr);

impl fmt::Debug for DeliveryReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let schema = redact_text(&self.schema);
        let recipient_id = redact_text(&self.recipient_id);
        let response_digest = self.response_digest.as_deref().map(redact_text);
        let receipt_digest = redact_text(&self.receipt_digest);

        formatter
            .debug_struct("DeliveryReceipt")
            .field("schema", &schema)
            .field("delivery_receipt_id", &self.delivery_receipt_id)
            .field("notification_id", &self.notification_id)
            .field("delivery_attempt_id", &self.delivery_attempt_id)
            .field("recipient_id", &recipient_id)
            .field("status", &self.status)
            .field("observed_at_unix_ms", &self.observed_at_unix_ms)
            .field("response_digest", &response_digest)
            .field("receipt_digest", &receipt_digest)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionRefRepr {
    schema: String,
    action_ref_id: ActionRefId,
    command: String,
    target_revision: u64,
    scope: Vec<String>,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    action_digest: String,
}

impl From<&ActionRef> for ActionRefRepr {
    fn from(value: &ActionRef) -> Self {
        Self {
            schema: value.schema.clone(),
            action_ref_id: value.action_ref_id,
            command: value.command.clone(),
            target_revision: value.target_revision,
            scope: value.scope.clone(),
            created_at_unix_ms: value.created_at_unix_ms,
            expires_at_unix_ms: value.expires_at_unix_ms,
            action_digest: value.action_digest.clone(),
        }
    }
}

impl From<ActionRefRepr> for ActionRef {
    fn from(value: ActionRefRepr) -> Self {
        Self {
            schema: value.schema,
            action_ref_id: value.action_ref_id,
            command: value.command,
            target_revision: value.target_revision,
            scope: value.scope,
            created_at_unix_ms: value.created_at_unix_ms,
            expires_at_unix_ms: value.expires_at_unix_ms,
            action_digest: value.action_digest,
        }
    }
}

impl_validated_serde!(ActionRef, ActionRefRepr);

impl fmt::Debug for ActionRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let schema = redact_text(&self.schema);
        let command = redact_text(&self.command);
        let scope = self
            .scope
            .iter()
            .map(|value| redact_text(value))
            .collect::<Vec<_>>();
        let action_digest = redact_text(&self.action_digest);

        formatter
            .debug_struct("ActionRef")
            .field("schema", &schema)
            .field("action_ref_id", &self.action_ref_id)
            .field("command", &command)
            .field("target_revision", &self.target_revision)
            .field("scope", &scope)
            .field("created_at_unix_ms", &self.created_at_unix_ms)
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .field("action_digest", &action_digest)
            .finish()
    }
}

fn scope_is_subset(requested: &[String], granted: &[String]) -> bool {
    requested.iter().all(|item| {
        granted.iter().any(|candidate| {
            candidate == "*" || candidate == item || item.starts_with(&format!("{candidate}/"))
        })
    })
}

/// Upcast the known v0 message shape without accepting unknown majors or silently dropping fields.
pub fn upcast_message(value: Value) -> Result<Message, String> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| "message_object_required".to_owned())?;
    let schema = object
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| "message_schema_required".to_owned())?;
    let is_v0 = schema == "kiana.message.v0";
    if !is_v0 && schema != MESSAGE_SCHEMA {
        return Err("message_schema_unknown_major".to_owned());
    }
    if is_v0 {
        object.insert(
            "schema".to_owned(),
            Value::String(MESSAGE_SCHEMA.to_owned()),
        );
        rename_if_missing(&mut object, "id", "message_id");
        rename_if_missing(&mut object, "sender", "sender_id");
        rename_if_missing(&mut object, "recipient", "recipient_id");
        rename_if_missing(&mut object, "text", "body");
        object
            .entry("kind".to_owned())
            .or_insert_with(|| Value::String("chat".to_owned()));
        object
            .entry("scope".to_owned())
            .or_insert_with(|| json!([]));
        object
            .entry("created_at_unix_ms".to_owned())
            .or_insert_with(|| json!(0));
        object
            .entry("message_id".to_owned())
            .or_insert_with(|| json!(MessageId::new()));
        object
            .entry("message_digest".to_owned())
            .or_insert_with(|| Value::String(String::new()));
    }
    let value = Value::Object(object);
    let mut message = if is_v0 {
        serde_json::from_value::<MessageRepr>(value)
            .map(MessageRepr::into_message)
            .map_err(|_| "message_upcast_invalid".to_owned())?
    } else {
        serde_json::from_value::<Message>(value).map_err(|_| "message_upcast_invalid".to_owned())?
    };
    if is_v0 {
        message.message_digest = message.digest();
    }
    message.validate()?;
    Ok(message)
}

fn rename_if_missing(object: &mut Map<String, Value>, old: &str, new: &str) {
    if !object.contains_key(new) {
        if let Some(value) = object.remove(old) {
            object.insert(new.to_owned(), value);
        }
    }
}

/// Canonical bytes for an inspectable message/notification contract.
pub fn canonical_notification_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    canonical_journal_bytes(value)
}
