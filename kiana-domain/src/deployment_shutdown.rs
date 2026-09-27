//! Unified shutdown decision and acknowledgement contract.
//!
//! This reducer consumes already observed cancellation, fencing, drain and flush facts. It never
//! stops a scheduler, kills a process or writes an EventLog. Missing/unknown flush or late-result
//! evidence yields recovery-required/Unknown instead of a false stopped acknowledgement.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SHUTDOWN_PHASE_EVIDENCE_SCHEMA: &str = "kiana.shutdown-phase-evidence.v1";
pub const SHUTDOWN_INPUT_SCHEMA: &str = "kiana.shutdown-input.v1";
pub const SHUTDOWN_REPORT_SCHEMA: &str = "kiana.shutdown-report.v1";
pub const SHUTDOWN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SHUTDOWN_PHASES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownPhase {
    IntakePause,
    Cancellation,
    SchedulerFence,
    RunnerDrain,
    ToolDrain,
    EventStoreFlush,
    ArtifactFlush,
    EventStoreClose,
}

impl ShutdownPhase {
    pub const ALL: [Self; MAX_SHUTDOWN_PHASES] = [
        Self::IntakePause,
        Self::Cancellation,
        Self::SchedulerFence,
        Self::RunnerDrain,
        Self::ToolDrain,
        Self::EventStoreFlush,
        Self::ArtifactFlush,
        Self::EventStoreClose,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownAckStatus {
    Pending,
    Confirmed,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownStatus {
    Stopped,
    NeedsRecovery,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownPhaseEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub phase: ShutdownPhase,
    pub status: ShutdownAckStatus,
    pub source_cursor: u64,
    pub detail_digest: String,
    pub reason: String,
    pub evidence_digest: String,
}

impl ShutdownPhaseEvidence {
    pub fn new(
        phase: ShutdownPhase,
        status: ShutdownAckStatus,
        source_cursor: u64,
        detail_digest: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: SHUTDOWN_PHASE_EVIDENCE_SCHEMA.to_owned(),
            version: SHUTDOWN_VERSION,
            phase,
            status,
            source_cursor,
            detail_digest: detail_digest.into(),
            reason: reason.into(),
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHUTDOWN_PHASE_EVIDENCE_SCHEMA
            || self.version != SHUTDOWN_VERSION
            || self.source_cursor == 0
            || self.reason.trim().is_empty()
            || self.reason.len() > 256
        {
            return Err("shutdown_phase_evidence_header_invalid".to_owned());
        }
        valid_digest(&self.detail_digest, "shutdown_phase_detail_digest")?;
        valid_digest(&self.evidence_digest, "shutdown_phase_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("shutdown_phase_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "phase": self.phase,
            "status": self.status,
            "source_cursor": self.source_cursor,
            "detail_digest": self.detail_digest,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub now_unix_ms: u64,
    pub deadline_unix_ms: u64,
    pub cancellation_requested: bool,
    pub intake_paused: bool,
    pub scheduler_fenced: bool,
    pub runner_drained: bool,
    pub tools_drained: bool,
    pub event_store_flushed: bool,
    pub artifact_flushed: bool,
    pub event_store_closed: bool,
    pub active_work_count: u64,
    pub unknown_effect_count: u64,
    pub late_result_count: u64,
    pub phases: Vec<ShutdownPhaseEvidence>,
    pub input_digest: String,
}

impl ShutdownInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHUTDOWN_INPUT_SCHEMA
            || self.version != SHUTDOWN_VERSION
            || self.now_unix_ms == 0
            || self.deadline_unix_ms == 0
            || self.phases.len() > MAX_SHUTDOWN_PHASES
            || self
                .phases
                .windows(2)
                .any(|pair| pair[0].phase >= pair[1].phase)
        {
            return Err("shutdown_input_header_invalid".to_owned());
        }
        let mut phases = BTreeSet::new();
        for phase in &self.phases {
            phase.validate()?;
            if !phases.insert(phase.phase) {
                return Err("shutdown_phase_duplicate".to_owned());
            }
        }
        valid_digest(&self.input_digest, "shutdown_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("shutdown_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "now_unix_ms": self.now_unix_ms,
            "deadline_unix_ms": self.deadline_unix_ms,
            "cancellation_requested": self.cancellation_requested,
            "intake_paused": self.intake_paused,
            "scheduler_fenced": self.scheduler_fenced,
            "runner_drained": self.runner_drained,
            "tools_drained": self.tools_drained,
            "event_store_flushed": self.event_store_flushed,
            "artifact_flushed": self.artifact_flushed,
            "event_store_closed": self.event_store_closed,
            "active_work_count": self.active_work_count,
            "unknown_effect_count": self.unknown_effect_count,
            "late_result_count": self.late_result_count,
            "phases": self.phases,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub input_digest: String,
    pub status: ShutdownStatus,
    pub unresolved_work_count: u64,
    pub reason: String,
    pub remediation: String,
    pub report_digest: String,
}

impl ShutdownReport {
    pub fn evaluate(input: &ShutdownInput) -> Result<Self, String> {
        input.validate()?;
        let report = build_report(input);
        report.validate_against(input)?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHUTDOWN_REPORT_SCHEMA
            || self.version != SHUTDOWN_VERSION
            || self.reason.trim().is_empty()
            || self.remediation.trim().is_empty()
        {
            return Err("shutdown_report_header_invalid".to_owned());
        }
        valid_digest(&self.input_digest, "shutdown_report_input_digest")?;
        valid_digest(&self.report_digest, "shutdown_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("shutdown_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, input: &ShutdownInput) -> Result<(), String> {
        input.validate()?;
        self.validate()?;
        if self != &build_report(input) {
            return Err("shutdown_report_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "input_digest": self.input_digest,
            "status": self.status,
            "unresolved_work_count": self.unresolved_work_count,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

fn build_report(input: &ShutdownInput) -> ShutdownReport {
    let missing_phase = ShutdownPhase::ALL
        .iter()
        .find(|phase| !input.phases.iter().any(|fact| fact.phase == **phase));
    let unknown_phase = input
        .phases
        .iter()
        .find(|fact| fact.status == ShutdownAckStatus::Unknown);
    let failed_phase = input.phases.iter().find(|fact| {
        matches!(
            fact.status,
            ShutdownAckStatus::Failed | ShutdownAckStatus::Pending
        )
    });
    let unresolved = input
        .active_work_count
        .saturating_add(input.unknown_effect_count)
        .saturating_add(input.late_result_count);
    let deadline_expired = input.now_unix_ms >= input.deadline_unix_ms;
    let (status, reason, remediation) = if unknown_phase.is_some() {
        (
            ShutdownStatus::Unknown,
            "shutdown_phase_unknown".to_owned(),
            "reconcile_shutdown_phase_before_restart".to_owned(),
        )
    } else if missing_phase.is_some() || failed_phase.is_some() {
        (
            ShutdownStatus::NeedsRecovery,
            "shutdown_ack_missing_or_failed".to_owned(),
            "collect_all_shutdown_acknowledgements".to_owned(),
        )
    } else if !input.cancellation_requested
        || !input.intake_paused
        || !input.scheduler_fenced
        || !input.runner_drained
        || !input.tools_drained
        || !input.event_store_flushed
        || !input.artifact_flushed
        || !input.event_store_closed
    {
        (
            ShutdownStatus::NeedsRecovery,
            "shutdown_boundary_unconfirmed".to_owned(),
            "complete_cancellation_drain_and_flush_ack".to_owned(),
        )
    } else if unresolved > 0 || deadline_expired {
        (
            ShutdownStatus::NeedsRecovery,
            if deadline_expired {
                "shutdown_deadline_expired".to_owned()
            } else {
                "shutdown_work_unresolved".to_owned()
            },
            "reconcile_inflight_or_unknown_work".to_owned(),
        )
    } else {
        (ShutdownStatus::Stopped, "ok".to_owned(), "none".to_owned())
    };
    let mut report = ShutdownReport {
        schema: SHUTDOWN_REPORT_SCHEMA.to_owned(),
        version: SHUTDOWN_VERSION,
        input_digest: input.input_digest.clone(),
        status,
        unresolved_work_count: unresolved,
        reason,
        remediation,
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    report
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
