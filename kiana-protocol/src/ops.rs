//! Versioned `ops.*` wire contracts.
//!
//! These DTOs describe an operator intent and its evidence envelope. They never authorize an
//! operation, consume an approval, call a Broker, append EventLog facts or perform a supervisor
//! action. Core must compare the supplied actor/epoch/scope with a server-owned snapshot before
//! dispatch; unknown envelopes remain inspectable and are never executable.

use kiana_domain::{json_digest, OperationId, ProjectId, RequestId, SchemaVersion, StorageRootId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const OPS_PROTOCOL_SCHEMA: &str = "kiana.ops.v1";
pub const OPS_PROTOCOL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const OPS_ENVELOPE_SCHEMA: &str = "kiana.ops-envelope.v1";
pub const OPS_MAX_TEXT: usize = 512;
pub const OPS_MAX_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsCommand {
    Status,
    Doctor,
    Preflight,
    Drain,
    BackupCreate,
    BackupList,
    BackupVerify,
    RestoreVerify,
    RestorePlan,
    RestoreActivate,
    MigratePlan,
    MigratePreflight,
    MigrateApply,
    MigrateResume,
    MigrateVerify,
    ProjectorStatus,
    ProjectorRebuild,
    ProjectorVerify,
    ReconcileList,
    ReconcileShow,
    ReconcileCommit,
    RolloutStatus,
    RolloutPause,
    RolloutResume,
    RolloutPromote,
    RolloutRollback,
    MaintenanceOpen,
    MaintenanceClose,
}

impl OpsCommand {
    pub const ALL: [Self; 28] = [
        Self::Status,
        Self::Doctor,
        Self::Preflight,
        Self::Drain,
        Self::BackupCreate,
        Self::BackupList,
        Self::BackupVerify,
        Self::RestoreVerify,
        Self::RestorePlan,
        Self::RestoreActivate,
        Self::MigratePlan,
        Self::MigratePreflight,
        Self::MigrateApply,
        Self::MigrateResume,
        Self::MigrateVerify,
        Self::ProjectorStatus,
        Self::ProjectorRebuild,
        Self::ProjectorVerify,
        Self::ReconcileList,
        Self::ReconcileShow,
        Self::ReconcileCommit,
        Self::RolloutStatus,
        Self::RolloutPause,
        Self::RolloutResume,
        Self::RolloutPromote,
        Self::RolloutRollback,
        Self::MaintenanceOpen,
        Self::MaintenanceClose,
    ];

    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Status => "ops.status",
            Self::Doctor => "ops.doctor",
            Self::Preflight => "ops.preflight",
            Self::Drain => "ops.drain",
            Self::BackupCreate => "ops.backup.create",
            Self::BackupList => "ops.backup.list",
            Self::BackupVerify => "ops.backup.verify",
            Self::RestoreVerify => "ops.restore.verify",
            Self::RestorePlan => "ops.restore.plan",
            Self::RestoreActivate => "ops.restore.activate",
            Self::MigratePlan => "ops.migrate.plan",
            Self::MigratePreflight => "ops.migrate.preflight",
            Self::MigrateApply => "ops.migrate.apply",
            Self::MigrateResume => "ops.migrate.resume",
            Self::MigrateVerify => "ops.migrate.verify",
            Self::ProjectorStatus => "ops.projector.status",
            Self::ProjectorRebuild => "ops.projector.rebuild",
            Self::ProjectorVerify => "ops.projector.verify",
            Self::ReconcileList => "ops.reconcile.list",
            Self::ReconcileShow => "ops.reconcile.show",
            Self::ReconcileCommit => "ops.reconcile.commit",
            Self::RolloutStatus => "ops.rollout.status",
            Self::RolloutPause => "ops.rollout.pause",
            Self::RolloutResume => "ops.rollout.resume",
            Self::RolloutPromote => "ops.rollout.promote",
            Self::RolloutRollback => "ops.rollout.rollback",
            Self::MaintenanceOpen => "ops.maintenance.open",
            Self::MaintenanceClose => "ops.maintenance.close",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|command| command.wire_name() == value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsQuery {
    Status,
    Doctor,
    Preflight,
    ReconcileList,
    ReconcileShow,
    RolloutStatus,
}

impl OpsQuery {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Status => "ops.status",
            Self::Doctor => "ops.doctor",
            Self::Preflight => "ops.preflight",
            Self::ReconcileList => "ops.reconcile.list",
            Self::ReconcileShow => "ops.reconcile.show",
            Self::RolloutStatus => "ops.rollout.status",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Status,
            Self::Doctor,
            Self::Preflight,
            Self::ReconcileList,
            Self::ReconcileShow,
            Self::RolloutStatus,
        ]
        .into_iter()
        .find(|query| query.wire_name() == value)
    }
}

const OPS_EVENT_NAMES: [&str; 6] = [
    "ops.operation.accepted",
    "ops.operation.state_changed",
    "ops.operation.completed",
    "ops.operation.failed",
    "ops.operation.cancelled",
    "ops.operation.unknown",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsScope {
    pub project_id: Option<ProjectId>,
    pub storage_root_id: Option<StorageRootId>,
    pub scope_digest: String,
}

impl OpsScope {
    pub fn new(project_id: Option<ProjectId>, storage_root_id: Option<StorageRootId>) -> Self {
        let mut scope = Self {
            project_id,
            storage_root_id,
            scope_digest: String::new(),
        };
        scope.scope_digest = scope.digest();
        scope
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.project_id.is_none() && self.storage_root_id.is_none() {
            return Err("ops_scope_empty".to_owned());
        }
        if self.project_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.storage_root_id.is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("ops_scope_id_invalid".to_owned());
        }
        valid_digest(&self.scope_digest, "ops_scope_digest")?;
        if self.scope_digest != self.digest() {
            return Err("ops_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "project_id": self.project_id,
            "storage_root_id": self.storage_root_id,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsAuthoritySnapshot {
    pub actor_id: String,
    pub authority_epoch: u64,
    pub scope: OpsScope,
}

impl OpsAuthoritySnapshot {
    pub fn validate(&self) -> Result<(), String> {
        bounded(&self.actor_id, "ops_actor_id")?;
        if self.authority_epoch == 0 {
            return Err("ops_authority_epoch_invalid".to_owned());
        }
        self.scope.validate()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsReplayDisposition {
    New,
    Replay,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsCommandRequest {
    pub schema: String,
    pub command: String,
    pub operation_id: OperationId,
    pub idempotency_key: String,
    pub authority: OpsAuthoritySnapshot,
    pub payload: Value,
    pub request_digest: String,
}

impl OpsCommandRequest {
    pub fn new(
        command: OpsCommand,
        operation_id: OperationId,
        idempotency_key: impl Into<String>,
        authority: OpsAuthoritySnapshot,
        payload: Value,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            command: command.wire_name().to_owned(),
            operation_id,
            idempotency_key: idempotency_key.into(),
            authority,
            payload,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA || self.operation_id.as_uuid().is_nil() {
            return Err("ops_command_header_invalid".to_owned());
        }
        if OpsCommand::parse(&self.command).is_none() {
            return Err("ops_unknown_command".to_owned());
        }
        bounded(&self.idempotency_key, "ops_idempotency_key")?;
        if self.idempotency_key.contains(['\r', '\n']) {
            return Err("ops_idempotency_key_invalid".to_owned());
        }
        self.authority.validate()?;
        validate_payload(&self.payload, "ops_command_payload_invalid")?;
        valid_digest(&self.request_digest, "ops_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("ops_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_authority(&self, expected: &OpsAuthoritySnapshot) -> Result<(), String> {
        self.validate()?;
        expected.validate()?;
        if self.authority.actor_id != expected.actor_id {
            return Err("ops_actor_mismatch".to_owned());
        }
        if self.authority.authority_epoch != expected.authority_epoch {
            return Err("ops_authority_epoch_mismatch".to_owned());
        }
        if self.authority.scope != expected.scope {
            return Err("ops_scope_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn replay_against(&self, prior: Option<&Self>) -> Result<OpsReplayDisposition, String> {
        self.validate()?;
        let Some(prior) = prior else {
            return Ok(OpsReplayDisposition::New);
        };
        prior.validate()?;
        if self.operation_id == prior.operation_id
            && self.idempotency_key == prior.idempotency_key
            && self.request_digest == prior.request_digest
        {
            Ok(OpsReplayDisposition::Replay)
        } else if self.operation_id == prior.operation_id
            || self.idempotency_key == prior.idempotency_key
        {
            Err("ops_idempotency_conflict".to_owned())
        } else {
            Ok(OpsReplayDisposition::New)
        }
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "command": self.command,
            "operation_id": self.operation_id,
            "idempotency_key": self.idempotency_key,
            "authority": self.authority,
            "payload": self.payload,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsQueryRequest {
    pub schema: String,
    pub query: String,
    pub request_id: RequestId,
    pub authority: OpsAuthoritySnapshot,
    pub cursor: Option<String>,
    pub payload: Value,
    pub query_digest: String,
}

impl OpsQueryRequest {
    pub fn new(
        query: OpsQuery,
        request_id: RequestId,
        authority: OpsAuthoritySnapshot,
        cursor: Option<String>,
        payload: Value,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            query: query.wire_name().to_owned(),
            request_id,
            authority,
            cursor,
            payload,
            query_digest: String::new(),
        };
        request.query_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA || self.request_id.as_uuid().is_nil() {
            return Err("ops_query_header_invalid".to_owned());
        }
        if OpsQuery::parse(&self.query).is_none() {
            return Err("ops_unknown_query".to_owned());
        }
        self.authority.validate()?;
        if let Some(cursor) = &self.cursor {
            bounded(cursor, "ops_cursor")?;
        }
        validate_payload(&self.payload, "ops_query_payload_invalid")?;
        valid_digest(&self.query_digest, "ops_query_digest")?;
        if self.query_digest != self.digest() {
            return Err("ops_query_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "query": self.query,
            "request_id": self.request_id,
            "authority": self.authority,
            "cursor": self.cursor,
            "payload": self.payload,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsEventKind {
    Accepted,
    StateChanged,
    Completed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsEvent {
    pub schema: String,
    pub event: String,
    pub kind: OpsEventKind,
    pub operation_id: OperationId,
    pub source_cursor: u64,
    pub payload: Value,
    pub event_digest: String,
}

impl OpsEvent {
    pub fn new(
        event: impl Into<String>,
        kind: OpsEventKind,
        operation_id: OperationId,
        source_cursor: u64,
        payload: Value,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            event: event.into(),
            kind,
            operation_id,
            source_cursor,
            payload,
            event_digest: String::new(),
        };
        value.event_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA
            || self.operation_id.as_uuid().is_nil()
            || self.source_cursor == 0
        {
            return Err("ops_event_header_invalid".to_owned());
        }
        bounded(&self.event, "ops_event_name")?;
        if !OPS_EVENT_NAMES.contains(&self.event.as_str()) {
            return Err("ops_event_name_invalid".to_owned());
        }
        validate_payload(&self.payload, "ops_event_payload_invalid")?;
        valid_digest(&self.event_digest, "ops_event_digest")?;
        if self.event_digest != self.digest() {
            return Err("ops_event_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "event": self.event,
            "kind": self.kind,
            "operation_id": self.operation_id,
            "source_cursor": self.source_cursor,
            "payload": self.payload,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsErrorCode {
    UnknownCommand,
    UnknownQuery,
    ScopeDenied,
    AuthorityMismatch,
    IdempotencyConflict,
    ResultUnknown,
    Unsupported,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsError {
    pub schema: String,
    pub code: OpsErrorCode,
    pub message: String,
    pub retryable: bool,
    pub operation_id: Option<OperationId>,
    pub error_digest: String,
}

impl OpsError {
    pub fn new(
        code: OpsErrorCode,
        message: impl Into<String>,
        retryable: bool,
        operation_id: Option<OperationId>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            code,
            message: message.into(),
            retryable,
            operation_id,
            error_digest: String::new(),
        };
        value.error_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA {
            return Err("ops_error_header_invalid".to_owned());
        }
        bounded(&self.message, "ops_error_message")?;
        if self
            .operation_id
            .is_some_and(|operation_id| operation_id.as_uuid().is_nil())
        {
            return Err("ops_error_operation_invalid".to_owned());
        }
        valid_digest(&self.error_digest, "ops_error_digest")?;
        if self.error_digest != self.digest() {
            return Err("ops_error_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "code": self.code,
            "message": self.message,
            "retryable": self.retryable,
            "operation_id": self.operation_id,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsUnknownEnvelope {
    pub schema: String,
    pub original_type: String,
    pub payload: Value,
    pub reason: String,
    pub unknown_digest: String,
}

impl OpsUnknownEnvelope {
    pub fn new(
        original_type: impl Into<String>,
        payload: Value,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            original_type: original_type.into(),
            payload,
            reason: reason.into(),
            unknown_digest: String::new(),
        };
        value.unknown_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA {
            return Err("ops_unknown_header_invalid".to_owned());
        }
        bounded(&self.original_type, "ops_unknown_type")?;
        bounded(&self.reason, "ops_unknown_reason")?;
        validate_payload(&self.payload, "ops_unknown_payload_invalid")?;
        valid_digest(&self.unknown_digest, "ops_unknown_digest")?;
        if self.unknown_digest != self.digest() {
            return Err("ops_unknown_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "original_type": self.original_type,
            "payload": self.payload,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", rename_all = "snake_case")]
pub enum OpsEnvelopeBody {
    Command(OpsCommandRequest),
    Query(OpsQueryRequest),
    Event(OpsEvent),
    Error(OpsError),
    Unknown(OpsUnknownEnvelope),
}

impl OpsEnvelopeBody {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Command(value) => value.validate(),
            Self::Query(value) => value.validate(),
            Self::Event(value) => value.validate(),
            Self::Error(value) => value.validate(),
            Self::Unknown(value) => value.validate(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsEnvelope {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub body: OpsEnvelopeBody,
    pub envelope_digest: String,
}

impl OpsEnvelope {
    pub fn new(request_id: RequestId, body: OpsEnvelopeBody) -> Result<Self, String> {
        let mut envelope = Self {
            schema: OPS_ENVELOPE_SCHEMA.to_owned(),
            version: OPS_PROTOCOL_VERSION,
            request_id,
            body,
            envelope_digest: String::new(),
        };
        envelope.envelope_digest = envelope.digest();
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_ENVELOPE_SCHEMA
            || self.version != OPS_PROTOCOL_VERSION
            || self.request_id.as_uuid().is_nil()
        {
            return Err("ops_envelope_header_invalid".to_owned());
        }
        self.body.validate()?;
        valid_digest(&self.envelope_digest, "ops_envelope_digest")?;
        if self.envelope_digest != self.digest() {
            return Err("ops_envelope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "body": self.body,
        }))
    }
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > OPS_MAX_TEXT || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_payload(value: &Value, error: &str) -> Result<(), String> {
    if !value.is_object()
        || serde_json::to_vec(value)
            .map(|encoded| encoded.len() > OPS_MAX_PAYLOAD_BYTES)
            .unwrap_or(true)
    {
        return Err(error.to_owned());
    }
    Ok(())
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
