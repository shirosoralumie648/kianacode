//! Ready admission, maintenance and drain decision contracts.
//!
//! The reducer consumes typed health/lease/operation evidence. It never pauses a scheduler,
//! changes an EventLog, extends a maintenance window or stops a running operation. A decision
//! explicitly distinguishes new-work admission from work already in flight.

use crate::{json_digest, SchemaVersion, SignalStatus};
use serde::{Deserialize, Serialize};

pub const MAINTENANCE_WINDOW_SCHEMA: &str = "kiana.maintenance-window.v1";
pub const DEPLOYMENT_ADMISSION_INPUT_SCHEMA: &str = "kiana.deployment-admission-input.v1";
pub const DEPLOYMENT_ADMISSION_DECISION_SCHEMA: &str = "kiana.deployment-admission-decision.v1";
pub const DEPLOYMENT_ADMISSION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_ADMISSION_TEXT: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceWindow {
    pub schema: String,
    pub version: SchemaVersion,
    pub window_id: String,
    pub starts_at_unix_ms: u64,
    pub ends_at_unix_ms: u64,
    pub revision: u64,
    pub window_digest: String,
}

impl MaintenanceWindow {
    pub fn new(
        window_id: impl Into<String>,
        starts_at_unix_ms: u64,
        ends_at_unix_ms: u64,
        revision: u64,
    ) -> Result<Self, String> {
        let mut window = Self {
            schema: MAINTENANCE_WINDOW_SCHEMA.to_owned(),
            version: DEPLOYMENT_ADMISSION_VERSION,
            window_id: window_id.into(),
            starts_at_unix_ms,
            ends_at_unix_ms,
            revision,
            window_digest: String::new(),
        };
        window.window_digest = window.digest();
        window.validate()?;
        Ok(window)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MAINTENANCE_WINDOW_SCHEMA
            || self.version != DEPLOYMENT_ADMISSION_VERSION
            || self.window_id.trim().is_empty()
            || self.window_id.len() > MAX_ADMISSION_TEXT
            || self.window_id.contains(['\0', '\r', '\n'])
            || self.starts_at_unix_ms == 0
            || self.ends_at_unix_ms <= self.starts_at_unix_ms
            || self.revision == 0
        {
            return Err("maintenance_window_header_invalid".to_owned());
        }
        valid_digest(&self.window_digest, "maintenance_window_digest")?;
        if self.window_digest != self.digest() {
            return Err("maintenance_window_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "window_id": self.window_id,
            "starts_at_unix_ms": self.starts_at_unix_ms,
            "ends_at_unix_ms": self.ends_at_unix_ms,
            "revision": self.revision,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentAdmissionStatus {
    Ready,
    Paused,
    Draining,
    Maintenance,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentAdmissionInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub health_digest: String,
    pub health_status: SignalStatus,
    pub startup_ready: bool,
    pub maintenance_window: Option<MaintenanceWindow>,
    pub now_unix_ms: u64,
    pub intake_paused: bool,
    pub drain_active: bool,
    pub drain_deadline_unix_ms: Option<u64>,
    pub migration_active: bool,
    pub backup_active: bool,
    pub lease_valid: bool,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub lease_authority_epoch: u64,
    pub lease_data_epoch: u64,
    pub operation_unknown: bool,
    pub source_cursor: u64,
    pub input_digest: String,
}

impl DeploymentAdmissionInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_ADMISSION_INPUT_SCHEMA
            || self.version != DEPLOYMENT_ADMISSION_VERSION
            || self.now_unix_ms == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.lease_authority_epoch == 0
            || self.lease_data_epoch == 0
            || self.source_cursor == 0
            || (self.drain_active
                && self
                    .drain_deadline_unix_ms
                    .is_none_or(|deadline| deadline == 0))
            || (!self.drain_active && self.drain_deadline_unix_ms.is_some())
        {
            return Err("deployment_admission_input_header_invalid".to_owned());
        }
        valid_digest(&self.health_digest, "deployment_admission_health_digest")?;
        if let Some(window) = &self.maintenance_window {
            window.validate()?;
        }
        valid_digest(&self.input_digest, "deployment_admission_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("deployment_admission_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "health_digest": self.health_digest,
            "health_status": self.health_status,
            "startup_ready": self.startup_ready,
            "maintenance_window": self.maintenance_window,
            "now_unix_ms": self.now_unix_ms,
            "intake_paused": self.intake_paused,
            "drain_active": self.drain_active,
            "drain_deadline_unix_ms": self.drain_deadline_unix_ms,
            "migration_active": self.migration_active,
            "backup_active": self.backup_active,
            "lease_valid": self.lease_valid,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "lease_authority_epoch": self.lease_authority_epoch,
            "lease_data_epoch": self.lease_data_epoch,
            "operation_unknown": self.operation_unknown,
            "source_cursor": self.source_cursor,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentAdmissionDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub input_digest: String,
    pub status: DeploymentAdmissionStatus,
    pub allow_new_work: bool,
    pub allow_existing_work: bool,
    pub reason: String,
    pub remediation: String,
    pub decision_digest: String,
}

impl DeploymentAdmissionDecision {
    pub fn evaluate(input: &DeploymentAdmissionInput) -> Result<Self, String> {
        input.validate()?;
        let decision = build_decision(input);
        decision.validate_against(input)?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_ADMISSION_DECISION_SCHEMA
            || self.version != DEPLOYMENT_ADMISSION_VERSION
            || self.reason.trim().is_empty()
            || self.remediation.trim().is_empty()
            || self.reason.len() > MAX_ADMISSION_TEXT
            || self.remediation.len() > MAX_ADMISSION_TEXT
        {
            return Err("deployment_admission_decision_header_invalid".to_owned());
        }
        valid_digest(
            &self.input_digest,
            "deployment_admission_decision_input_digest",
        )?;
        valid_digest(
            &self.decision_digest,
            "deployment_admission_decision_digest",
        )?;
        if self.decision_digest != self.digest() {
            return Err("deployment_admission_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, input: &DeploymentAdmissionInput) -> Result<(), String> {
        input.validate()?;
        self.validate()?;
        let expected = build_decision(input);
        if self != &expected {
            return Err("deployment_admission_decision_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "input_digest": self.input_digest,
            "status": self.status,
            "allow_new_work": self.allow_new_work,
            "allow_existing_work": self.allow_existing_work,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

fn build_decision(input: &DeploymentAdmissionInput) -> DeploymentAdmissionDecision {
    let (status, allow_new_work, allow_existing_work, reason, remediation) = derive_decision(input);
    let mut decision = DeploymentAdmissionDecision {
        schema: DEPLOYMENT_ADMISSION_DECISION_SCHEMA.to_owned(),
        version: DEPLOYMENT_ADMISSION_VERSION,
        input_digest: input.input_digest.clone(),
        status,
        allow_new_work,
        allow_existing_work,
        reason,
        remediation,
        decision_digest: String::new(),
    };
    decision.decision_digest = decision.digest();
    decision
}

fn derive_decision(
    input: &DeploymentAdmissionInput,
) -> (DeploymentAdmissionStatus, bool, bool, String, String) {
    if !input.lease_valid
        || input.authority_epoch != input.lease_authority_epoch
        || input.data_epoch != input.lease_data_epoch
    {
        return (
            DeploymentAdmissionStatus::Unknown,
            false,
            false,
            "admission_lease_conflict".to_owned(),
            "reacquire_current_lease_before_admission".to_owned(),
        );
    }
    if input.operation_unknown || input.health_status == SignalStatus::Unknown {
        return (
            DeploymentAdmissionStatus::Unknown,
            false,
            false,
            "admission_result_unknown".to_owned(),
            "reconcile_unknown_facts_before_admission".to_owned(),
        );
    }
    if input.health_status != SignalStatus::Ok || !input.startup_ready {
        return (
            DeploymentAdmissionStatus::Blocked,
            false,
            false,
            "admission_health_not_ready".to_owned(),
            "complete_startup_and_health_preflight".to_owned(),
        );
    }
    if input.migration_active || input.backup_active {
        return (
            DeploymentAdmissionStatus::Blocked,
            false,
            false,
            "admission_maintenance_conflict".to_owned(),
            "finish_migration_or_backup_before_ready".to_owned(),
        );
    }
    if input.drain_active {
        if input
            .drain_deadline_unix_ms
            .is_some_and(|deadline| input.now_unix_ms >= deadline)
        {
            return (
                DeploymentAdmissionStatus::Unknown,
                false,
                false,
                "drain_deadline_expired".to_owned(),
                "reconcile_inflight_work_after_drain_deadline".to_owned(),
            );
        }
        return (
            DeploymentAdmissionStatus::Draining,
            false,
            true,
            "draining_no_new_admission".to_owned(),
            "wait_for_drain_or_reconcile_inflight_work".to_owned(),
        );
    }
    if let Some(window) = &input.maintenance_window {
        if input.now_unix_ms < window.starts_at_unix_ms {
            return (
                DeploymentAdmissionStatus::Blocked,
                false,
                true,
                "maintenance_window_not_started".to_owned(),
                "wait_for_explicit_window_start".to_owned(),
            );
        }
        if input.now_unix_ms >= window.ends_at_unix_ms {
            return (
                DeploymentAdmissionStatus::Blocked,
                false,
                false,
                "maintenance_window_expired".to_owned(),
                "create_a_new_explicit_maintenance_window".to_owned(),
            );
        }
        return (
            DeploymentAdmissionStatus::Maintenance,
            false,
            true,
            "maintenance_window_active".to_owned(),
            "close_maintenance_window_before_new_admission".to_owned(),
        );
    }
    if input.intake_paused {
        return (
            DeploymentAdmissionStatus::Paused,
            false,
            true,
            "intake_paused".to_owned(),
            "resume_intake_with_explicit_control_plane_action".to_owned(),
        );
    }
    (
        DeploymentAdmissionStatus::Ready,
        true,
        true,
        "ok".to_owned(),
        "none".to_owned(),
    )
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
