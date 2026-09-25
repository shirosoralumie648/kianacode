//! Bounded ACP/IDE session adapter.
//!
//! This module only translates an ACP-shaped peer into the existing UI snapshot/feed/action
//! contracts. It does not own a runner, broker, permission store, or execution loop. Every
//! mutating result is a `UiActionV1` intent for the server-side ControlPlane to authorize.

use kiana_domain::{json_digest, RequestId, SessionId};
use kiana_protocol::{
    UiActionV1, UiFeedFrameV1, UiSnapshotV1, UI_ACTION_SCHEMA, UI_FEED_FRAME_SCHEMA,
    UI_SNAPSHOT_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use thiserror::Error;

pub const ACP_INITIALIZE_SCHEMA: &str = "kiana.acp-initialize.v1";
pub const ACP_PROTOCOL_V1: &str = "acp.v1";
pub const ACP_PROTOCOL_V2: &str = "acp.v2";
pub const ACP_SESSION_SCHEMA: &str = "kiana.acp-session.v1";
pub const ACP_PERMISSION_SCHEMA: &str = "kiana.acp-permission.v1";
pub const ACP_MAX_PROMPT_BYTES: usize = 64 * 1024;
pub const ACP_MAX_CAPABILITIES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpProtocolVersion {
    V1,
    V2,
}

impl AcpProtocolVersion {
    pub fn wire(self) -> &'static str {
        match self {
            Self::V1 => ACP_PROTOCOL_V1,
            Self::V2 => ACP_PROTOCOL_V2,
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            ACP_PROTOCOL_V1 => Some(Self::V1),
            ACP_PROTOCOL_V2 => Some(Self::V2),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpInitializeRequest {
    pub schema: String,
    pub protocol: String,
    pub client_name: String,
    pub client_version: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpInitializeResponse {
    pub schema: String,
    pub protocol: String,
    pub server_name: String,
    pub server_version: String,
    pub capabilities: Vec<String>,
    pub session_owner_required: bool,
    pub direct_effect: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpSessionReference {
    pub schema: String,
    pub session_id: SessionId,
    pub owner_digest: String,
    pub authority_epoch: String,
    pub feed_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpResumeResult {
    pub session: AcpSessionReference,
    pub replay_from_sequence: u64,
    pub snapshot_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpPermissionRequest {
    pub schema: String,
    pub permission_id: String,
    pub session_id: SessionId,
    pub owner_digest: String,
    pub authority_epoch: String,
    pub expires_at_unix_ms: u64,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpPermissionDecision {
    Allow,
    Deny,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpConnectionState {
    Disconnected,
    Initialized,
    Attached,
    Degraded,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcpUpdateDisposition {
    Accepted { sequence: u64, terminal: bool },
    Replay { sequence: u64 },
    Gap { expected: u64, received: u64 },
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum AcpAdapterError {
    #[error("acp_schema_unsupported")]
    SchemaUnsupported,
    #[error("acp_not_initialized")]
    NotInitialized,
    #[error("acp_session_owner_mismatch")]
    SessionOwnerMismatch,
    #[error("acp_session_mismatch")]
    SessionMismatch,
    #[error("acp_epoch_mismatch")]
    EpochMismatch,
    #[error("acp_feed_handler_required")]
    FeedHandlerRequired,
    #[error("acp_reconcile_required")]
    ReconcileRequired,
    #[error("acp_update_after_terminal")]
    UpdateAfterTerminal,
    #[error("acp_permission_unknown")]
    PermissionUnknown,
    #[error("acp_permission_expired")]
    PermissionExpired,
    #[error("acp_invalid_input:{0}")]
    InvalidInput(&'static str),
    #[error("acp_projection_invalid:{0}")]
    ProjectionInvalid(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AcpProjection {
    Snapshot(UiSnapshotV1),
    Feed(UiFeedFrameV1),
    Action(UiActionV1),
}

impl AcpProjection {
    pub fn validate(&self) -> Result<(), AcpAdapterError> {
        match self {
            Self::Snapshot(value) => {
                if value.schema != UI_SNAPSHOT_SCHEMA {
                    return Err(AcpAdapterError::ProjectionInvalid(
                        "snapshot_schema".to_owned(),
                    ));
                }
                value.validate().map_err(AcpAdapterError::ProjectionInvalid)
            }
            Self::Feed(value) => {
                if value.schema != UI_FEED_FRAME_SCHEMA {
                    return Err(AcpAdapterError::ProjectionInvalid("feed_schema".to_owned()));
                }
                value.validate().map_err(AcpAdapterError::ProjectionInvalid)
            }
            Self::Action(value) => {
                if value.schema != UI_ACTION_SCHEMA {
                    return Err(AcpAdapterError::ProjectionInvalid(
                        "action_schema".to_owned(),
                    ));
                }
                value.validate().map_err(AcpAdapterError::ProjectionInvalid)
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct AcpSessionAdapter {
    protocol: Option<AcpProtocolVersion>,
    owner_digest: Option<String>,
    session_id: Option<SessionId>,
    authority_epoch: Option<String>,
    last_sequence: u64,
    terminal: bool,
    handler_installed: bool,
    connection: AcpConnectionState,
    pending_permissions: BTreeMap<String, AcpPermissionRequest>,
}

impl Default for AcpConnectionState {
    fn default() -> Self {
        Self::Disconnected
    }
}

impl AcpSessionAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn connection_state(&self) -> AcpConnectionState {
        self.connection
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    pub fn initialize(
        &mut self,
        request: AcpInitializeRequest,
    ) -> Result<AcpInitializeResponse, AcpAdapterError> {
        if request.schema != ACP_INITIALIZE_SCHEMA
            || request.client_name.trim().is_empty()
            || request.client_version.trim().is_empty()
            || request.capabilities.len() > ACP_MAX_CAPABILITIES
            || request
                .capabilities
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 128)
        {
            return Err(AcpAdapterError::InvalidInput("initialize"));
        }
        let protocol = AcpProtocolVersion::parse(&request.protocol)
            .ok_or(AcpAdapterError::SchemaUnsupported)?;
        self.protocol = Some(protocol);
        self.connection = AcpConnectionState::Initialized;
        self.handler_installed = false;
        Ok(AcpInitializeResponse {
            schema: ACP_INITIALIZE_SCHEMA.to_owned(),
            protocol: protocol.wire().to_owned(),
            server_name: "kiana".to_owned(),
            server_version: env!("CARGO_PKG_VERSION").to_owned(),
            capabilities: vec![
                "session.new".to_owned(),
                "session.resume".to_owned(),
                "turn.prompt".to_owned(),
                "feed.update".to_owned(),
                "permission.request".to_owned(),
                "turn.cancel".to_owned(),
            ],
            session_owner_required: true,
            direct_effect: false,
        })
    }

    pub fn install_feed_handler(&mut self) -> Result<(), AcpAdapterError> {
        self.require_initialized()?;
        self.handler_installed = true;
        if self.session_id.is_some() && self.connection != AcpConnectionState::Degraded {
            self.connection = AcpConnectionState::Attached;
        }
        Ok(())
    }

    pub fn new_session(
        &mut self,
        session_id: SessionId,
        owner_digest: impl Into<String>,
        authority_epoch: impl Into<String>,
    ) -> Result<AcpSessionReference, AcpAdapterError> {
        self.require_initialized()?;
        let owner_digest = bounded_digest(owner_digest.into(), "owner_digest")?;
        let authority_epoch = bounded_id(authority_epoch.into(), "authority_epoch")?;
        self.session_id = Some(session_id.clone());
        self.owner_digest = Some(owner_digest.clone());
        self.authority_epoch = Some(authority_epoch.clone());
        self.last_sequence = 0;
        self.terminal = false;
        self.pending_permissions.clear();
        self.connection = if self.handler_installed {
            AcpConnectionState::Attached
        } else {
            AcpConnectionState::Initialized
        };
        Ok(self.session_reference())
    }

    pub fn resume_session(
        &mut self,
        session_id: &SessionId,
        owner_digest: &str,
        authority_epoch: &str,
    ) -> Result<AcpResumeResult, AcpAdapterError> {
        self.require_initialized()?;
        if !self.handler_installed {
            return Err(AcpAdapterError::FeedHandlerRequired);
        }
        self.require_owner(session_id, owner_digest)?;
        if self.authority_epoch.as_deref() != Some(authority_epoch) {
            return Err(AcpAdapterError::EpochMismatch);
        }
        self.connection = AcpConnectionState::Attached;
        Ok(AcpResumeResult {
            session: self.session_reference(),
            replay_from_sequence: self.last_sequence,
            snapshot_required: true,
        })
    }

    pub fn prompt(
        &self,
        session_id: &SessionId,
        prompt: &str,
        expected_epoch: &str,
        expected_cursor: u64,
        idempotency_key: &str,
    ) -> Result<AcpProjection, AcpAdapterError> {
        self.require_owner(session_id, self.owner_digest.as_deref().unwrap_or_default())?;
        if self.authority_epoch.as_deref() != Some(expected_epoch) {
            return Err(AcpAdapterError::EpochMismatch);
        }
        if prompt.trim().is_empty() || prompt.len() > ACP_MAX_PROMPT_BYTES {
            return Err(AcpAdapterError::InvalidInput("prompt"));
        }
        Ok(AcpProjection::Action(self.action(
            idempotency_key,
            expected_epoch,
            expected_cursor,
            json!({"kind":"prompt", "prompt":prompt}),
        )?))
    }

    pub fn permission_request(
        &mut self,
        request: AcpPermissionRequest,
        now_unix_ms: u64,
    ) -> Result<(), AcpAdapterError> {
        self.require_owner(&request.session_id, &request.owner_digest)?;
        if request.schema != ACP_PERMISSION_SCHEMA
            || request.expires_at_unix_ms <= now_unix_ms
            || request.summary.trim().is_empty()
            || request.summary.len() > 2_048
        {
            return Err(AcpAdapterError::PermissionExpired);
        }
        self.pending_permissions
            .insert(request.permission_id.clone(), request);
        Ok(())
    }

    pub fn resolve_permission(
        &mut self,
        permission_id: &str,
        decision: AcpPermissionDecision,
        now_unix_ms: u64,
        expected_cursor: u64,
        idempotency_key: &str,
    ) -> Result<AcpProjection, AcpAdapterError> {
        let request = self
            .pending_permissions
            .get(permission_id)
            .ok_or(AcpAdapterError::PermissionUnknown)?;
        self.require_owner(&request.session_id, &request.owner_digest)?;
        if request.expires_at_unix_ms <= now_unix_ms {
            self.pending_permissions.remove(permission_id);
            return Err(AcpAdapterError::PermissionExpired);
        }
        let expected_epoch = request.authority_epoch.clone();
        let payload = json!({
            "kind": "permission",
            "permission_id": permission_id,
            "decision": decision,
        });
        let action = self.action(idempotency_key, &expected_epoch, expected_cursor, payload)?;
        self.pending_permissions.remove(permission_id);
        Ok(AcpProjection::Action(action))
    }

    pub fn cancel(
        &self,
        session_id: &SessionId,
        expected_epoch: &str,
        expected_cursor: u64,
        idempotency_key: &str,
    ) -> Result<AcpProjection, AcpAdapterError> {
        self.require_owner(session_id, self.owner_digest.as_deref().unwrap_or_default())?;
        if self.authority_epoch.as_deref() != Some(expected_epoch) {
            return Err(AcpAdapterError::EpochMismatch);
        }
        Ok(AcpProjection::Action(self.action(
            idempotency_key,
            expected_epoch,
            expected_cursor,
            json!({"kind":"cancel"}),
        )?))
    }

    pub fn update(
        &mut self,
        frame: UiFeedFrameV1,
    ) -> Result<AcpUpdateDisposition, AcpAdapterError> {
        self.require_initialized()?;
        if !self.handler_installed {
            return Err(AcpAdapterError::FeedHandlerRequired);
        }
        if self.connection == AcpConnectionState::Degraded {
            return Err(AcpAdapterError::ReconcileRequired);
        }
        if self.connection != AcpConnectionState::Attached {
            return Err(AcpAdapterError::FeedHandlerRequired);
        }
        frame
            .validate()
            .map_err(AcpAdapterError::ProjectionInvalid)?;
        let cursor = &frame.cursor;
        if self.authority_epoch.as_deref() != Some(cursor.authority_epoch.as_str()) {
            return Err(AcpAdapterError::EpochMismatch);
        }
        if self.terminal {
            return Err(AcpAdapterError::UpdateAfterTerminal);
        }
        let sequence = cursor.feed_sequence;
        if sequence <= self.last_sequence {
            return Ok(AcpUpdateDisposition::Replay { sequence });
        }
        if sequence != self.last_sequence + 1 {
            self.connection = AcpConnectionState::Degraded;
            return Ok(AcpUpdateDisposition::Gap {
                expected: self.last_sequence + 1,
                received: sequence,
            });
        }
        self.last_sequence = sequence;
        self.terminal = frame.terminal;
        self.connection = AcpConnectionState::Attached;
        Ok(AcpUpdateDisposition::Accepted {
            sequence,
            terminal: frame.terminal,
        })
    }

    pub fn disconnect(&mut self) {
        self.connection = AcpConnectionState::Degraded;
        self.handler_installed = false;
    }

    pub fn close(&mut self) {
        self.connection = AcpConnectionState::Closed;
        self.handler_installed = false;
    }

    fn require_initialized(&self) -> Result<(), AcpAdapterError> {
        if self.protocol.is_none() || matches!(self.connection, AcpConnectionState::Closed) {
            return Err(AcpAdapterError::NotInitialized);
        }
        Ok(())
    }

    fn require_owner(
        &self,
        session_id: &SessionId,
        owner_digest: &str,
    ) -> Result<(), AcpAdapterError> {
        self.require_initialized()?;
        if self.session_id.as_ref() != Some(session_id) {
            return Err(AcpAdapterError::SessionMismatch);
        }
        if self.owner_digest.as_deref() != Some(owner_digest) {
            return Err(AcpAdapterError::SessionOwnerMismatch);
        }
        if !matches!(self.connection, AcpConnectionState::Attached) {
            return Err(AcpAdapterError::FeedHandlerRequired);
        }
        Ok(())
    }

    fn session_reference(&self) -> AcpSessionReference {
        AcpSessionReference {
            schema: ACP_SESSION_SCHEMA.to_owned(),
            session_id: self.session_id.clone().expect("session is required"),
            owner_digest: self.owner_digest.clone().expect("owner is required"),
            authority_epoch: self.authority_epoch.clone().expect("epoch is required"),
            feed_sequence: self.last_sequence,
        }
    }

    fn action(
        &self,
        idempotency_key: &str,
        expected_epoch: &str,
        expected_cursor: u64,
        payload: Value,
    ) -> Result<UiActionV1, AcpAdapterError> {
        let submitted_by = self
            .owner_digest
            .clone()
            .ok_or(AcpAdapterError::NotInitialized)?;
        let action = UiActionV1 {
            schema: UI_ACTION_SCHEMA.to_owned(),
            command_id: RequestId::new(),
            idempotency_key: bounded_id(idempotency_key.to_owned(), "idempotency_key")?,
            target_id: self
                .session_id
                .as_ref()
                .map(ToString::to_string)
                .ok_or(AcpAdapterError::NotInitialized)?,
            expected_epoch: bounded_id(expected_epoch.to_owned(), "expected_epoch")?,
            expected_cursor,
            expected_revision: None,
            payload: payload.clone(),
            payload_digest: json_digest(&payload),
            submitted_by,
            deadline_unix_ms: None,
        };
        action
            .validate()
            .map_err(AcpAdapterError::ProjectionInvalid)?;
        Ok(action)
    }
}

fn bounded_id(value: String, field: &'static str) -> Result<String, AcpAdapterError> {
    if value.trim().is_empty() || value.len() > 256 || value.contains('\0') {
        return Err(AcpAdapterError::InvalidInput(field));
    }
    Ok(value)
}

fn bounded_digest(value: String, field: &'static str) -> Result<String, AcpAdapterError> {
    let valid = value
        .strip_prefix("sha256:")
        .map(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or(false);
    if !valid {
        return Err(AcpAdapterError::InvalidInput(field));
    }
    Ok(value)
}
