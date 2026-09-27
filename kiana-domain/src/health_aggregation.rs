//! Conservative health/readiness aggregation over server-owned evidence.
//!
//! This reducer builds the existing `HealthSnapshot` projection without probing a provider,
//! opening an EventLog or granting admission. Provider health is ignored for readiness unless a
//! separate verified evidence flag is present; lag, lease conflict, Unknown and drain/maintenance
//! remain visible and keep admission closed.

use crate::{
    json_digest, ComponentHealth, ComponentHealthState, EventId, HealthProbeKind, HealthSnapshot,
    OperationPhase, OperationState, SchemaVersion, SignalStatus, StartupCoordinatorStatus,
    MAX_HEALTH_LIMITATIONS, MAX_SOURCE_EVENT_IDS,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const HEALTH_AGGREGATION_SCHEMA: &str = "kiana.health-aggregation.v1";
pub const HEALTH_AGGREGATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_HEALTH_COMPONENT: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthAggregationInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub probe: HealthProbeKind,
    pub component: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub observed_at_ms: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub generation: u64,
    pub startup_status: StartupCoordinatorStatus,
    pub startup_report_digest: String,
    pub eventlog_ready: bool,
    pub projection_cursor: u64,
    pub unknown_count: u64,
    pub lease_active: bool,
    pub lease_authority_epoch: u64,
    pub lease_data_epoch: u64,
    pub lease_fence_valid: bool,
    pub provider_healthy: bool,
    pub provider_evidence_verified: bool,
    pub drain_active: bool,
    pub maintenance_active: bool,
    pub operation_state: OperationState,
    pub operation_phase: OperationPhase,
    pub operation_deadline_expired: bool,
    pub component_evidence_digest: String,
    pub input_digest: String,
}

impl HealthAggregationInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HEALTH_AGGREGATION_SCHEMA
            || self.version != HEALTH_AGGREGATION_VERSION
            || self.component.trim().is_empty()
            || self.component.len() > MAX_HEALTH_COMPONENT
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || self.observed_at_ms == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
            || self.projection_cursor == 0
            || self.lease_authority_epoch == 0
            || self.lease_data_epoch == 0
        {
            return Err("health_aggregation_input_header_invalid".to_owned());
        }
        let mut ids = std::collections::BTreeSet::new();
        if self
            .source_event_ids
            .iter()
            .any(|event_id| !ids.insert(event_id.to_string()))
        {
            return Err("health_aggregation_source_event_duplicate".to_owned());
        }
        valid_digest(
            &self.component_evidence_digest,
            "health_aggregation_component_evidence_digest",
        )?;
        valid_digest(
            &self.startup_report_digest,
            "health_aggregation_startup_report_digest",
        )?;
        if self.operation_phase != operation_phase(self.operation_state) {
            return Err("health_aggregation_operation_phase_mismatch".to_owned());
        }
        valid_digest(&self.input_digest, "health_aggregation_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("health_aggregation_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "probe": self.probe,
            "component": self.component,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "observed_at_ms": self.observed_at_ms,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
            "startup_status": self.startup_status,
            "eventlog_ready": self.eventlog_ready,
            "projection_cursor": self.projection_cursor,
            "unknown_count": self.unknown_count,
            "lease_active": self.lease_active,
            "lease_authority_epoch": self.lease_authority_epoch,
            "lease_data_epoch": self.lease_data_epoch,
            "lease_fence_valid": self.lease_fence_valid,
            "provider_healthy": self.provider_healthy,
            "provider_evidence_verified": self.provider_evidence_verified,
            "drain_active": self.drain_active,
            "maintenance_active": self.maintenance_active,
            "operation_state": self.operation_state,
            "operation_phase": self.operation_phase,
            "operation_deadline_expired": self.operation_deadline_expired,
            "startup_report_digest": self.startup_report_digest,
            "component_evidence_digest": self.component_evidence_digest,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthAggregationReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub input_digest: String,
    pub probe: HealthProbeKind,
    pub status: SignalStatus,
    pub admission_allowed: bool,
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub projection_lag: u64,
    pub unknown_count: u64,
    pub lease_conflict: bool,
    pub snapshot: HealthSnapshot,
    pub report_digest: String,
}

impl HealthAggregationReport {
    pub fn evaluate(input: &HealthAggregationInput) -> Result<Self, String> {
        input.validate()?;
        let report = build_report(input)?;
        report.validate_against(input)?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HEALTH_AGGREGATION_SCHEMA
            || self.version != HEALTH_AGGREGATION_VERSION
            || self.snapshot.probe != self.probe
            || self.snapshot.status != self.status
            || self.admission_allowed
                != (self.probe == HealthProbeKind::Readiness && self.status == SignalStatus::Ok)
            || self.source_cursor == 0
            || self.projection_cursor == 0
            || self.projection_lag != self.source_cursor.saturating_sub(self.projection_cursor)
        {
            return Err("health_aggregation_report_header_invalid".to_owned());
        }
        valid_digest(&self.input_digest, "health_aggregation_report_input_digest")?;
        self.snapshot.validate()?;
        valid_digest(&self.report_digest, "health_aggregation_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("health_aggregation_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, input: &HealthAggregationInput) -> Result<(), String> {
        input.validate()?;
        self.validate()?;
        let expected = build_report(input)?;
        if self != &expected {
            return Err("health_aggregation_report_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "input_digest": self.input_digest,
            "probe": self.probe,
            "status": self.status,
            "admission_allowed": self.admission_allowed,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_lag": self.projection_lag,
            "unknown_count": self.unknown_count,
            "lease_conflict": self.lease_conflict,
            "snapshot": self.snapshot,
        }))
    }
}

fn build_report(input: &HealthAggregationInput) -> Result<HealthAggregationReport, String> {
    let projection_lag = input.source_cursor.saturating_sub(input.projection_cursor);
    let projection_ahead = input.projection_cursor > input.source_cursor;
    let status = status_for(input, projection_lag, projection_ahead);
    let mut snapshot = HealthSnapshot::new(
        input.component.clone(),
        status,
        input.source_cursor,
        input.source_event_ids.clone(),
        input.observed_at_ms,
    )?;
    snapshot.probe = input.probe;
    let lease_conflict = effective_lease_conflict(input);
    snapshot.capabilities = BTreeMap::from([
        (
            "startup_complete".to_owned(),
            input.startup_status == StartupCoordinatorStatus::Ready,
        ),
        ("eventlog_ready".to_owned(), input.eventlog_ready),
        (
            "provider_evidence_verified".to_owned(),
            input.provider_evidence_verified,
        ),
        ("lease_conflict".to_owned(), lease_conflict),
        ("drain_active".to_owned(), input.drain_active),
        ("maintenance_active".to_owned(), input.maintenance_active),
    ]);
    let eventlog_state = if input.eventlog_ready {
        ComponentHealthState::Healthy
    } else {
        ComponentHealthState::Unavailable
    };
    let projector_state = if projection_ahead || projection_lag > 0 {
        ComponentHealthState::Degraded
    } else {
        ComponentHealthState::Healthy
    };
    let provider_state = if !input.provider_evidence_verified {
        ComponentHealthState::Unknown
    } else if input.provider_healthy {
        ComponentHealthState::Healthy
    } else {
        ComponentHealthState::Degraded
    };
    let lease_state = if lease_conflict {
        ComponentHealthState::Unknown
    } else {
        ComponentHealthState::Healthy
    };
    snapshot.components = BTreeMap::from([
        (
            "eventlog".to_owned(),
            ComponentHealth::new(
                "eventlog",
                "health.v1",
                eventlog_state,
                Some(input.source_cursor),
                (!input.eventlog_ready).then_some("eventlog_not_ready".to_owned()),
            )?,
        ),
        (
            "projector".to_owned(),
            ComponentHealth::new(
                "projector",
                "health.v1",
                projector_state,
                Some(input.projection_cursor),
                (projection_ahead || projection_lag > 0)
                    .then_some("projection_lag_or_cursor_ahead".to_owned()),
            )?,
        ),
        (
            "provider".to_owned(),
            ComponentHealth::new(
                "provider",
                "health.v1",
                provider_state,
                None,
                if !input.provider_evidence_verified {
                    Some("provider_health_unverified".to_owned())
                } else if !input.provider_healthy {
                    Some("provider_unhealthy".to_owned())
                } else {
                    None
                },
            )?,
        ),
        (
            "lease".to_owned(),
            ComponentHealth::new(
                "lease",
                "health.v1",
                lease_state,
                None,
                lease_conflict.then_some("lease_conflict".to_owned()),
            )?,
        ),
    ]);
    let mut limitations = Vec::new();
    if input.startup_status != StartupCoordinatorStatus::Ready {
        limitations.push("startup_incomplete".to_owned());
    }
    if !input.eventlog_ready {
        limitations.push("eventlog_not_ready".to_owned());
    }
    if projection_ahead || projection_lag > 0 {
        limitations.push("projection_lag_or_cursor_ahead".to_owned());
    }
    if input.unknown_count > 0 {
        limitations.push("unknown_effect_present".to_owned());
    }
    if lease_conflict {
        limitations.push("lease_conflict".to_owned());
    }
    if input.operation_state == OperationState::Unknown {
        limitations.push("operation_unknown".to_owned());
    }
    if input.operation_deadline_expired {
        limitations.push("operation_deadline_expired".to_owned());
    }
    if !input.provider_evidence_verified {
        limitations.push("provider_health_unverified".to_owned());
    } else if !input.provider_healthy {
        limitations.push("provider_unhealthy".to_owned());
    }
    if input.drain_active {
        limitations.push("draining_no_new_admission".to_owned());
    }
    if input.maintenance_active {
        limitations.push("maintenance_no_new_admission".to_owned());
    }
    limitations.truncate(MAX_HEALTH_LIMITATIONS);
    snapshot.limitations = limitations;
    snapshot.snapshot_digest = snapshot.digest();
    snapshot.validate()?;
    let mut report = HealthAggregationReport {
        schema: HEALTH_AGGREGATION_SCHEMA.to_owned(),
        version: HEALTH_AGGREGATION_VERSION,
        input_digest: input.input_digest.clone(),
        probe: input.probe,
        status,
        admission_allowed: input.probe == HealthProbeKind::Readiness && status == SignalStatus::Ok,
        source_cursor: input.source_cursor,
        projection_cursor: input.projection_cursor,
        projection_lag,
        unknown_count: input.unknown_count,
        lease_conflict,
        snapshot,
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    Ok(report)
}

fn status_for(
    input: &HealthAggregationInput,
    projection_lag: u64,
    projection_ahead: bool,
) -> SignalStatus {
    let lease_conflict = effective_lease_conflict(input);
    if input.unknown_count > 0 || lease_conflict {
        return SignalStatus::Unknown;
    }
    if input.operation_state == OperationState::Unknown || input.operation_deadline_expired {
        return SignalStatus::Unknown;
    }
    if matches!(
        input.probe,
        HealthProbeKind::Drain | HealthProbeKind::Maintenance
    ) {
        return SignalStatus::Degraded;
    }
    if projection_ahead || projection_lag > 0 || !input.eventlog_ready {
        return SignalStatus::Degraded;
    }
    match input.probe {
        HealthProbeKind::Startup => {
            if input.startup_status == StartupCoordinatorStatus::Ready {
                SignalStatus::Ok
            } else {
                SignalStatus::Degraded
            }
        }
        HealthProbeKind::Readiness => {
            if input.startup_status == StartupCoordinatorStatus::Ready
                && input.provider_evidence_verified
                && input.provider_healthy
                && !input.drain_active
                && !input.maintenance_active
                && input.operation_state != OperationState::Unknown
                && !input.operation_deadline_expired
                && !lease_conflict
            {
                SignalStatus::Ok
            } else {
                SignalStatus::Degraded
            }
        }
        HealthProbeKind::Liveness => SignalStatus::Ok,
        HealthProbeKind::Drain | HealthProbeKind::Maintenance => SignalStatus::Degraded,
    }
}

fn effective_lease_conflict(input: &HealthAggregationInput) -> bool {
    input.lease_authority_epoch != input.authority_epoch
        || input.lease_data_epoch != input.data_epoch
        || !input.lease_active
        || !input.lease_fence_valid
}

fn operation_phase(state: OperationState) -> OperationPhase {
    match state {
        OperationState::Created | OperationState::Preflight => OperationPhase::Preflight,
        OperationState::Draining => OperationPhase::Draining,
        OperationState::Executing => OperationPhase::Executing,
        OperationState::Completed
        | OperationState::Failed
        | OperationState::Cancelled
        | OperationState::Unknown => OperationPhase::Terminal,
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
