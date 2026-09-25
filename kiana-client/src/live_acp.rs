//! Explicit opt-in ACP/IDE session boundary.
//!
//! This module records a bounded, local-only host session and delegates every action to the
//! existing [`AcpSessionAdapter`]. It does not open a socket, spawn a process, contact a provider,
//! mint a permit, or turn host/editor capabilities into direct effects.

use crate::{
    AcpAdapterError, AcpConnectionState, AcpInitializeRequest, AcpInitializeResponse,
    AcpPermissionDecision, AcpPermissionRequest, AcpProjection, AcpResumeResult, AcpSessionAdapter,
    AcpSessionReference, AcpUpdateDisposition, ACP_INITIALIZE_SCHEMA, ACP_PROTOCOL_V1,
    ACP_PROTOCOL_V2,
};
use kiana_domain::SessionId;
use kiana_protocol::{
    EvidenceLimitation, UiHostCapability, UiLiveHostEvidence, UiLiveHostStatus, UiSurface,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const LIVE_ACP_OPT_IN_SCHEMA: &str = "kiana.live-acp-opt-in.v1";
pub const LIVE_ACP_MAX_HOST_ID_BYTES: usize = 256;
pub const LIVE_ACP_MAX_HOST_VERSION_BYTES: usize = 128;
pub const LIVE_ACP_MAX_CAPABILITIES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveAcpTransport {
    LocalStdio,
    LocalUnixSocket,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveAcpOptIn {
    pub schema: String,
    pub opt_in: bool,
    pub protocol: String,
    pub surface: UiSurface,
    pub host_id: String,
    pub host_version: String,
    pub environment_digest: String,
    pub workspace_digest: String,
    pub operator_approval_ref: String,
    pub transport: LiveAcpTransport,
    pub redacted_payloads: bool,
    #[serde(default)]
    pub host_capabilities: Vec<UiHostCapability>,
}

impl LiveAcpOptIn {
    pub fn validate(&self) -> Result<(), LiveAcpError> {
        if self.schema != LIVE_ACP_OPT_IN_SCHEMA {
            return Err(LiveAcpError::InvalidOptIn("schema"));
        }
        if !self.opt_in {
            return Err(LiveAcpError::InvalidOptIn("explicit_opt_in"));
        }
        if self.surface != UiSurface::Ide {
            return Err(LiveAcpError::InvalidOptIn("surface"));
        }
        if !matches!(self.protocol.as_str(), ACP_PROTOCOL_V1 | ACP_PROTOCOL_V2) {
            return Err(LiveAcpError::InvalidOptIn("protocol"));
        }
        bounded_text(&self.host_id, LIVE_ACP_MAX_HOST_ID_BYTES, "host_id")?;
        bounded_text(
            &self.host_version,
            LIVE_ACP_MAX_HOST_VERSION_BYTES,
            "host_version",
        )?;
        digest(&self.environment_digest, "environment_digest")?;
        digest(&self.workspace_digest, "workspace_digest")?;
        bounded_text(&self.operator_approval_ref, 256, "operator_approval_ref")?;
        if !self
            .operator_approval_ref
            .to_ascii_lowercase()
            .starts_with("approval:")
        {
            return Err(LiveAcpError::InvalidOptIn("operator_approval_ref"));
        }
        if !self.redacted_payloads {
            return Err(LiveAcpError::InvalidOptIn("redacted_payloads"));
        }
        if self.host_capabilities.len() > LIVE_ACP_MAX_CAPABILITIES {
            return Err(LiveAcpError::InvalidOptIn("host_capabilities"));
        }
        for capability in &self.host_capabilities {
            capability
                .validate()
                .map_err(|_| LiveAcpError::InvalidOptIn("host_capability"))?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveAcpState {
    OptedIn,
    Initialized,
    Attached,
    Unknown,
    Closed,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum LiveAcpError {
    #[error("live_acp_invalid_opt_in:{0}")]
    InvalidOptIn(&'static str),
    #[error("live_acp_protocol_mismatch")]
    ProtocolMismatch,
    #[error("live_acp_session_required")]
    SessionRequired,
    #[error("live_acp_receipt_invalid")]
    ReceiptInvalid,
    #[error("live_acp_adapter:{0}")]
    Adapter(#[from] AcpAdapterError),
    #[error("live_acp_evidence:{0}")]
    Evidence(String),
}

#[derive(Debug)]
pub struct LiveAcpSession {
    opt_in: LiveAcpOptIn,
    adapter: AcpSessionAdapter,
    state: LiveAcpState,
    session: Option<AcpSessionReference>,
    handshake_verified: bool,
    session_verified: bool,
    prompt_routed: bool,
    update_observed: bool,
    permission_routed: bool,
    cancel_fence_verified: bool,
    reconnect_verified: bool,
    receipt_digest: Option<String>,
    limitations: Vec<EvidenceLimitation>,
}

impl LiveAcpSession {
    pub fn start(opt_in: LiveAcpOptIn) -> Result<Self, LiveAcpError> {
        opt_in.validate()?;
        Ok(Self {
            opt_in,
            adapter: AcpSessionAdapter::new(),
            state: LiveAcpState::OptedIn,
            session: None,
            handshake_verified: false,
            session_verified: false,
            prompt_routed: false,
            update_observed: false,
            permission_routed: false,
            cancel_fence_verified: false,
            reconnect_verified: false,
            receipt_digest: None,
            limitations: vec![EvidenceLimitation {
                code: "external_host_receipt_pending".to_owned(),
                detail: "No external ACP/IDE host was executed by this source adapter".to_owned(),
            }],
        })
    }

    pub fn state(&self) -> LiveAcpState {
        self.state
    }

    pub fn transport(&self) -> LiveAcpTransport {
        self.opt_in.transport
    }

    pub fn initialize(
        &mut self,
        request: AcpInitializeRequest,
    ) -> Result<AcpInitializeResponse, LiveAcpError> {
        if request.schema != ACP_INITIALIZE_SCHEMA
            || request.protocol != self.opt_in.protocol
            || request.client_name != self.opt_in.host_id
            || request.client_version != self.opt_in.host_version
        {
            return Err(LiveAcpError::ProtocolMismatch);
        }
        let response = self.adapter.initialize(request)?;
        self.handshake_verified = response.protocol == self.opt_in.protocol;
        self.state = LiveAcpState::Initialized;
        Ok(response)
    }

    pub fn install_feed_handler(&mut self) -> Result<(), LiveAcpError> {
        self.adapter.install_feed_handler()?;
        if self.session.is_some() {
            self.state = LiveAcpState::Attached;
        }
        Ok(())
    }

    pub fn new_session(
        &mut self,
        session_id: SessionId,
        owner_digest: impl Into<String>,
        authority_epoch: impl Into<String>,
    ) -> Result<AcpSessionReference, LiveAcpError> {
        let session = self
            .adapter
            .new_session(session_id, owner_digest, authority_epoch)?;
        self.session_verified = true;
        self.session = Some(session.clone());
        self.state = if self.adapter.connection_state() == AcpConnectionState::Attached {
            LiveAcpState::Attached
        } else {
            LiveAcpState::Initialized
        };
        Ok(session)
    }

    pub fn resume_session(
        &mut self,
        session_id: &SessionId,
        owner_digest: &str,
        authority_epoch: &str,
    ) -> Result<AcpResumeResult, LiveAcpError> {
        let result = self
            .adapter
            .resume_session(session_id, owner_digest, authority_epoch)?;
        self.session = Some(result.session.clone());
        self.reconnect_verified = true;
        self.state = LiveAcpState::Attached;
        Ok(result)
    }

    pub fn prompt(
        &mut self,
        session_id: &SessionId,
        prompt: &str,
        expected_epoch: &str,
        expected_cursor: u64,
        idempotency_key: &str,
    ) -> Result<AcpProjection, LiveAcpError> {
        let projection = self.adapter.prompt(
            session_id,
            prompt,
            expected_epoch,
            expected_cursor,
            idempotency_key,
        )?;
        self.prompt_routed = true;
        Ok(projection)
    }

    pub fn permission_request(
        &mut self,
        request: AcpPermissionRequest,
        now_unix_ms: u64,
    ) -> Result<(), LiveAcpError> {
        self.adapter.permission_request(request, now_unix_ms)?;
        self.permission_routed = true;
        Ok(())
    }

    pub fn resolve_permission(
        &mut self,
        permission_id: &str,
        decision: AcpPermissionDecision,
        now_unix_ms: u64,
        expected_cursor: u64,
        idempotency_key: &str,
    ) -> Result<AcpProjection, LiveAcpError> {
        let projection = self.adapter.resolve_permission(
            permission_id,
            decision,
            now_unix_ms,
            expected_cursor,
            idempotency_key,
        )?;
        self.permission_routed = true;
        Ok(projection)
    }

    pub fn cancel(
        &mut self,
        session_id: &SessionId,
        expected_epoch: &str,
        expected_cursor: u64,
        idempotency_key: &str,
    ) -> Result<AcpProjection, LiveAcpError> {
        let projection =
            self.adapter
                .cancel(session_id, expected_epoch, expected_cursor, idempotency_key)?;
        self.cancel_fence_verified = true;
        Ok(projection)
    }

    pub fn update(
        &mut self,
        frame: kiana_protocol::UiFeedFrameV1,
    ) -> Result<AcpUpdateDisposition, LiveAcpError> {
        let disposition = self.adapter.update(frame)?;
        match disposition {
            AcpUpdateDisposition::Accepted { .. } => {
                self.update_observed = true;
                self.state = LiveAcpState::Attached;
            }
            AcpUpdateDisposition::Replay { .. } => {}
            AcpUpdateDisposition::Gap { .. } => {
                self.state = LiveAcpState::Unknown;
                self.add_limitation(
                    "feed_gap_reconcile_required",
                    "A feed gap requires server snapshot hydration before live continuation",
                );
            }
        }
        Ok(disposition)
    }

    pub fn record_receipt(
        &mut self,
        receipt_digest: impl Into<String>,
    ) -> Result<(), LiveAcpError> {
        let receipt_digest = receipt_digest.into();
        digest(&receipt_digest, "receipt_digest").map_err(|_| LiveAcpError::ReceiptInvalid)?;
        self.receipt_digest = Some(receipt_digest);
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.adapter.disconnect();
        self.state = LiveAcpState::Unknown;
        self.reconnect_verified = false;
        self.add_limitation(
            "host_disconnected_reconcile_required",
            "A disconnected host must reconnect and reconcile through the original session",
        );
    }

    pub fn close(&mut self) {
        self.adapter.close();
        self.state = LiveAcpState::Closed;
    }

    pub fn evidence(&self) -> Result<UiLiveHostEvidence, LiveAcpError> {
        let status = if self.state == LiveAcpState::Unknown {
            UiLiveHostStatus::Unknown
        } else {
            // Source adapters never self-promote to live proof. An operator or external host
            // evidence path must independently promote this record to Verified.
            UiLiveHostStatus::OptedIn
        };
        UiLiveHostEvidence::new(
            self.opt_in.protocol.clone(),
            self.opt_in.surface,
            self.opt_in.host_id.clone(),
            self.opt_in.host_version.clone(),
            self.opt_in.environment_digest.clone(),
            self.opt_in.workspace_digest.clone(),
            self.session
                .as_ref()
                .map(|session| session.session_id.to_string()),
            status,
            Some(self.opt_in.operator_approval_ref.clone()),
            self.opt_in.host_capabilities.clone(),
            self.handshake_verified,
            self.session_verified,
            self.prompt_routed,
            self.update_observed,
            self.permission_routed,
            self.cancel_fence_verified,
            self.reconnect_verified,
            self.receipt_digest.clone(),
            self.limitations.clone(),
        )
        .map_err(LiveAcpError::Evidence)
    }

    fn add_limitation(&mut self, code: &str, detail: &str) {
        if !self
            .limitations
            .iter()
            .any(|limitation| limitation.code == code)
        {
            self.limitations.push(EvidenceLimitation {
                code: code.to_owned(),
                detail: detail.to_owned(),
            });
        }
    }
}

fn bounded_text(value: &str, max: usize, field: &'static str) -> Result<(), LiveAcpError> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(LiveAcpError::InvalidOptIn(field));
    }
    Ok(())
}

fn digest(value: &str, field: &'static str) -> Result<(), LiveAcpError> {
    let valid = value
        .strip_prefix("sha256:")
        .map(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or(false);
    if !valid {
        return Err(LiveAcpError::InvalidOptIn(field));
    }
    Ok(())
}
