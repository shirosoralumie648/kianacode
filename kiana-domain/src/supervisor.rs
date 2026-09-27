//! Lease-fenced supervisor request and observation contracts.
//!
//! A supervisor controls a server-owned service reference, never a caller-provided pid. The
//! request binds the operation to an operation lease, fence token, deployment/root identity and
//! authority/data epochs. An adapter may return an observation or an explicit Unknown/TimedOut
//! result; it must not turn a signal into a stopped/restarted claim without confirmation.

use crate::{
    json_digest, FenceTokenId, InstanceId, OperationId, SchemaVersion, StopMethod, StopReport,
};
use serde::{Deserialize, Serialize};

pub const SUPERVISOR_REQUEST_SCHEMA: &str = "kiana.supervisor-request.v1";
pub const SUPERVISOR_OBSERVATION_SCHEMA: &str = "kiana.supervisor-observation.v1";
pub const SUPERVISOR_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SUPERVISOR_TARGET_BYTES: usize = 256;
pub const MAX_SUPERVISOR_EVIDENCE_BYTES: usize = 512;
pub const MAX_SUPERVISOR_DEADLINE_MS: u64 = 120_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorBackend {
    Systemd,
    Launchd,
    WindowsService,
    Container,
    Fake,
}

impl SupervisorBackend {
    pub const fn target_prefix(self) -> &'static str {
        match self {
            Self::Systemd => "systemd:",
            Self::Launchd => "launchd:",
            Self::WindowsService => "windows:",
            Self::Container => "container:",
            Self::Fake => "fake:",
        }
    }

    pub const fn adapter_name(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::Launchd => "launchd",
            Self::WindowsService => "windows-service",
            Self::Container => "container",
            Self::Fake => "fake",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorAction {
    Start,
    Stop,
    Restart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorSignal {
    None,
    Term,
    Kill,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorOutcome {
    Started,
    Stopped,
    Restarted,
    TimedOut,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub instance_id: InstanceId,
    pub deployment_revision_digest: String,
    pub storage_root_digest: String,
    pub lease_digest: String,
    pub fence_token: FenceTokenId,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub backend: SupervisorBackend,
    pub action: SupervisorAction,
    pub target_ref: String,
    pub expected_generation: u64,
    pub deadline_ms: u64,
    pub request_digest: String,
}

impl SupervisorRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        instance_id: InstanceId,
        deployment_revision_digest: impl Into<String>,
        storage_root_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        backend: SupervisorBackend,
        action: SupervisorAction,
        target_ref: impl Into<String>,
        expected_generation: u64,
        deadline_ms: u64,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: SUPERVISOR_REQUEST_SCHEMA.to_owned(),
            version: SUPERVISOR_VERSION,
            operation_id,
            instance_id,
            deployment_revision_digest: deployment_revision_digest.into(),
            storage_root_digest: storage_root_digest.into(),
            lease_digest: lease_digest.into(),
            fence_token,
            authority_epoch,
            data_epoch,
            backend,
            action,
            target_ref: target_ref.into(),
            expected_generation,
            deadline_ms,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SUPERVISOR_REQUEST_SCHEMA
            || self.version != SUPERVISOR_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.instance_id.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.deadline_ms == 0
            || self.deadline_ms > MAX_SUPERVISOR_DEADLINE_MS
            || self.target_ref.len() > MAX_SUPERVISOR_TARGET_BYTES
            || self.target_ref.trim().is_empty()
            || self.target_ref.contains(['\0', '\r', '\n'])
            || self.target_ref.contains("pid:")
            || !self.target_ref.starts_with(self.backend.target_prefix())
        {
            return Err("supervisor_request_header_invalid".to_owned());
        }
        let target_suffix = self
            .target_ref
            .strip_prefix(self.backend.target_prefix())
            .ok_or_else(|| "supervisor_request_target_backend_mismatch".to_owned())?;
        if target_suffix.trim().is_empty() {
            return Err("supervisor_request_target_empty".to_owned());
        }
        for (digest, field) in [
            (
                &self.deployment_revision_digest,
                "supervisor_request_revision_digest",
            ),
            (
                &self.storage_root_digest,
                "supervisor_request_storage_root_digest",
            ),
            (&self.lease_digest, "supervisor_request_lease_digest"),
            (&self.request_digest, "supervisor_request_digest"),
        ] {
            valid_digest(digest, field)?;
        }
        if self.request_digest != self.digest() {
            return Err("supervisor_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "instance_id": self.instance_id,
            "deployment_revision_digest": self.deployment_revision_digest,
            "storage_root_digest": self.storage_root_digest,
            "lease_digest": self.lease_digest,
            "fence_token": self.fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "backend": self.backend,
            "action": self.action,
            "target_ref": self.target_ref,
            "expected_generation": self.expected_generation,
            "deadline_ms": self.deadline_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_digest: String,
    pub operation_id: OperationId,
    pub instance_id: InstanceId,
    pub deployment_revision_digest: String,
    pub storage_root_digest: String,
    pub lease_digest: String,
    pub fence_token: FenceTokenId,
    pub backend: SupervisorBackend,
    pub action: SupervisorAction,
    pub outcome: SupervisorOutcome,
    pub signal: SupervisorSignal,
    pub stop_report: Option<StopReport>,
    pub observed_generation: u64,
    pub evidence_ref: String,
    pub observation_digest: String,
}

impl SupervisorObservation {
    pub fn new(
        request: &SupervisorRequest,
        outcome: SupervisorOutcome,
        signal: SupervisorSignal,
        stop_report: Option<StopReport>,
        observed_generation: u64,
        evidence_ref: impl Into<String>,
    ) -> Result<Self, String> {
        request.validate()?;
        let mut observation = Self {
            schema: SUPERVISOR_OBSERVATION_SCHEMA.to_owned(),
            version: SUPERVISOR_VERSION,
            request_digest: request.request_digest.clone(),
            operation_id: request.operation_id,
            instance_id: request.instance_id,
            deployment_revision_digest: request.deployment_revision_digest.clone(),
            storage_root_digest: request.storage_root_digest.clone(),
            lease_digest: request.lease_digest.clone(),
            fence_token: request.fence_token,
            backend: request.backend,
            action: request.action,
            outcome,
            signal,
            stop_report,
            observed_generation,
            evidence_ref: evidence_ref.into(),
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate_against(request)?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SUPERVISOR_OBSERVATION_SCHEMA
            || self.version != SUPERVISOR_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.instance_id.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
            || self.evidence_ref.len() > MAX_SUPERVISOR_EVIDENCE_BYTES
            || !self.evidence_ref.starts_with("observation:")
            || self.evidence_ref.contains(['\0', '\r', '\n'])
        {
            return Err("supervisor_observation_header_invalid".to_owned());
        }
        for (digest, field) in [
            (
                &self.deployment_revision_digest,
                "supervisor_observation_revision_digest",
            ),
            (
                &self.storage_root_digest,
                "supervisor_observation_storage_root_digest",
            ),
            (
                &self.request_digest,
                "supervisor_observation_request_digest",
            ),
            (&self.lease_digest, "supervisor_observation_lease_digest"),
            (&self.observation_digest, "supervisor_observation_digest"),
        ] {
            valid_digest(digest, field)?;
        }
        if let Some(report) = &self.stop_report {
            report.validate()?;
            let expected_signal = match report.method {
                StopMethod::None => SupervisorSignal::None,
                StopMethod::Term => SupervisorSignal::Term,
                StopMethod::Kill => SupervisorSignal::Kill,
            };
            if self.signal != expected_signal {
                return Err("supervisor_observation_signal_mismatch".to_owned());
            }
        } else if self.signal != SupervisorSignal::None {
            return Err("supervisor_observation_signal_without_report".to_owned());
        }
        if self.observation_digest != self.digest() {
            return Err("supervisor_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, request: &SupervisorRequest) -> Result<(), String> {
        request.validate()?;
        self.validate()?;
        if self.request_digest != request.request_digest
            || self.operation_id != request.operation_id
            || self.instance_id != request.instance_id
            || self.deployment_revision_digest != request.deployment_revision_digest
            || self.storage_root_digest != request.storage_root_digest
            || self.lease_digest != request.lease_digest
            || self.fence_token != request.fence_token
            || self.backend != request.backend
            || self.action != request.action
        {
            return Err("supervisor_observation_request_binding_mismatch".to_owned());
        }
        let confirmed = self
            .stop_report
            .as_ref()
            .is_some_and(|report| report.confirmed);
        match (request.action, self.outcome) {
            (SupervisorAction::Start, SupervisorOutcome::Started) => {
                if self.stop_report.is_some()
                    || self.signal != SupervisorSignal::None
                    || self.observed_generation <= request.expected_generation
                {
                    return Err("supervisor_start_observation_invalid".to_owned());
                }
            }
            (SupervisorAction::Stop, SupervisorOutcome::Stopped) => {
                if !confirmed
                    || self.observed_generation != request.expected_generation
                    || self.stop_report.is_none()
                {
                    return Err("supervisor_stop_observation_required".to_owned());
                }
            }
            (SupervisorAction::Restart, SupervisorOutcome::Restarted) => {
                if !confirmed
                    || self.observed_generation <= request.expected_generation
                    || self.stop_report.is_none()
                {
                    return Err("supervisor_restart_fence_invalid".to_owned());
                }
            }
            (SupervisorAction::Start, SupervisorOutcome::Unknown | SupervisorOutcome::TimedOut)
            | (SupervisorAction::Stop, SupervisorOutcome::Unknown | SupervisorOutcome::TimedOut)
            | (
                SupervisorAction::Restart,
                SupervisorOutcome::Unknown | SupervisorOutcome::TimedOut,
            ) => {
                if confirmed || self.observed_generation != request.expected_generation {
                    return Err("supervisor_unknown_observation_invalid".to_owned());
                }
            }
            _ => return Err("supervisor_outcome_action_mismatch".to_owned()),
        }
        if matches!(
            self.outcome,
            SupervisorOutcome::Unknown | SupervisorOutcome::TimedOut
        ) && confirmed
        {
            return Err("supervisor_unknown_confirmed_conflict".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_digest": self.request_digest,
            "operation_id": self.operation_id,
            "instance_id": self.instance_id,
            "deployment_revision_digest": self.deployment_revision_digest,
            "storage_root_digest": self.storage_root_digest,
            "lease_digest": self.lease_digest,
            "fence_token": self.fence_token,
            "backend": self.backend,
            "action": self.action,
            "outcome": self.outcome,
            "signal": self.signal,
            "stop_report": self.stop_report,
            "observed_generation": self.observed_generation,
            "evidence_ref": self.evidence_ref,
        }))
    }
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
