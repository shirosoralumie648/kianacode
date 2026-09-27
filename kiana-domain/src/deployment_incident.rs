//! Deployment incident, runbook and alert-routing evidence contract.
//!
//! This reducer validates an ordered incident lifecycle and returns a replayable decision. Alert
//! routes are metadata only: they cannot expand authority or trigger compensation, retry or
//! closure. Unknown evidence and missed phase deadlines remain Unknown and require reconciliation.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, OperationId, SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEPLOYMENT_INCIDENT_SCHEMA: &str = "kiana.deployment-incident.v1";
pub const DEPLOYMENT_RUNBOOK_REF_SCHEMA: &str = "kiana.deployment-runbook-ref.v1";
pub const DEPLOYMENT_ALERT_ROUTE_SCHEMA: &str = "kiana.deployment-alert-route.v1";
pub const DEPLOYMENT_RUNBOOK_EVIDENCE_SCHEMA: &str = "kiana.deployment-runbook-evidence.v1";
pub const DEPLOYMENT_INCIDENT_REPORT_SCHEMA: &str = "kiana.deployment-incident-report.v1";
pub const DEPLOYMENT_INCIDENT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_INCIDENT_HISTORY: usize = 6;
pub const MAX_INCIDENT_TEXT: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentPhase {
    Observed,
    Triaged,
    Contained,
    Recovering,
    Verified,
    Closed,
}

impl IncidentPhase {
    pub const ALL: [Self; 6] = [
        Self::Observed,
        Self::Triaged,
        Self::Contained,
        Self::Recovering,
        Self::Verified,
        Self::Closed,
    ];

    const fn ordinal(self) -> u8 {
        match self {
            Self::Observed => 1,
            Self::Triaged => 2,
            Self::Contained => 3,
            Self::Recovering => 4,
            Self::Verified => 5,
            Self::Closed => 6,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Triaged => "triaged",
            Self::Contained => "contained",
            Self::Recovering => "recovering",
            Self::Verified => "verified",
            Self::Closed => "closed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentDecisionStatus {
    Accepted,
    Replayed,
    Blocked,
    Unknown,
}

impl IncidentDecisionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Replayed => "replayed",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunbookRef {
    pub schema: String,
    pub version: SchemaVersion,
    pub runbook_id: String,
    pub revision_digest: String,
    pub phase: IncidentPhase,
    pub summary: String,
    pub ref_digest: String,
}

impl RunbookRef {
    pub fn new(
        runbook_id: impl Into<String>,
        revision_digest: impl Into<String>,
        phase: IncidentPhase,
        summary: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: DEPLOYMENT_RUNBOOK_REF_SCHEMA.to_owned(),
            version: DEPLOYMENT_INCIDENT_VERSION,
            runbook_id: runbook_id.into(),
            revision_digest: revision_digest.into(),
            phase,
            summary: summary.into(),
            ref_digest: String::new(),
        };
        value.ref_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_RUNBOOK_REF_SCHEMA
            || self.version != DEPLOYMENT_INCIDENT_VERSION
        {
            return Err("deployment_runbook_ref_header_invalid".to_owned());
        }
        safe_text(&self.runbook_id, "deployment_runbook_id")?;
        safe_text(&self.summary, "deployment_runbook_summary")?;
        valid_digest(&self.revision_digest, "deployment_runbook_revision_digest")?;
        valid_digest(&self.ref_digest, "deployment_runbook_ref_digest")?;
        if self.ref_digest != self.digest() {
            return Err("deployment_runbook_ref_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "runbook_id": self.runbook_id,
            "revision_digest": self.revision_digest,
            "summary": self.summary,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertRoute {
    pub schema: String,
    pub version: SchemaVersion,
    pub route_id: String,
    pub severity: u8,
    pub channel: String,
    pub requires_approval: bool,
    pub expands_authority: bool,
    pub auto_compensation: bool,
    pub route_digest: String,
}

impl AlertRoute {
    pub fn new(
        route_id: impl Into<String>,
        severity: u8,
        channel: impl Into<String>,
        requires_approval: bool,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: DEPLOYMENT_ALERT_ROUTE_SCHEMA.to_owned(),
            version: DEPLOYMENT_INCIDENT_VERSION,
            route_id: route_id.into(),
            severity,
            channel: channel.into(),
            requires_approval,
            expands_authority: false,
            auto_compensation: false,
            route_digest: String::new(),
        };
        value.route_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_ALERT_ROUTE_SCHEMA
            || self.version != DEPLOYMENT_INCIDENT_VERSION
            || self.severity == 0
            || self.severity > 5
            || self.expands_authority
            || self.auto_compensation
        {
            return Err("deployment_alert_route_header_invalid".to_owned());
        }
        safe_text(&self.route_id, "deployment_alert_route_id")?;
        safe_text(&self.channel, "deployment_alert_channel")?;
        valid_digest(&self.route_digest, "deployment_alert_route_digest")?;
        if self.route_digest != self.digest() {
            return Err("deployment_alert_route_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "route_id": self.route_id,
            "severity": self.severity,
            "channel": self.channel,
            "requires_approval": self.requires_approval,
            "expands_authority": self.expands_authority,
            "auto_compensation": self.auto_compensation,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunbookEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub incident_id: String,
    pub operation_id: OperationId,
    pub revision_digest: String,
    pub phase: IncidentPhase,
    pub actor_id: String,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub runbook_digest: String,
    pub alert_route_digest: String,
    pub observed_at_ms: u64,
    pub deadline_ms: u64,
    pub unknown: bool,
    pub reason: String,
    pub evidence_digest: String,
}

impl RunbookEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        incident_id: impl Into<String>,
        operation_id: OperationId,
        revision_digest: impl Into<String>,
        phase: IncidentPhase,
        actor_id: impl Into<String>,
        source_cursor: u64,
        authority_epoch: u64,
        data_epoch: u64,
        runbook_digest: impl Into<String>,
        alert_route_digest: impl Into<String>,
        observed_at_ms: u64,
        deadline_ms: u64,
        unknown: bool,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: DEPLOYMENT_RUNBOOK_EVIDENCE_SCHEMA.to_owned(),
            version: DEPLOYMENT_INCIDENT_VERSION,
            incident_id: incident_id.into(),
            operation_id,
            revision_digest: revision_digest.into(),
            phase,
            actor_id: actor_id.into(),
            source_cursor,
            authority_epoch,
            data_epoch,
            runbook_digest: runbook_digest.into(),
            alert_route_digest: alert_route_digest.into(),
            observed_at_ms,
            deadline_ms,
            unknown,
            reason: reason.into(),
            evidence_digest: String::new(),
        };
        value.evidence_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_RUNBOOK_EVIDENCE_SCHEMA
            || self.version != DEPLOYMENT_INCIDENT_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.observed_at_ms == 0
            || self.deadline_ms == 0
        {
            return Err("deployment_runbook_evidence_header_invalid".to_owned());
        }
        safe_text(&self.incident_id, "deployment_incident_id")?;
        safe_text(&self.actor_id, "deployment_incident_actor")?;
        safe_text(&self.reason, "deployment_incident_reason")?;
        valid_digest(
            &self.runbook_digest,
            "deployment_runbook_evidence_runbook_digest",
        )?;
        valid_digest(
            &self.revision_digest,
            "deployment_runbook_evidence_revision_digest",
        )?;
        valid_digest(
            &self.alert_route_digest,
            "deployment_runbook_evidence_route_digest",
        )?;
        valid_digest(&self.evidence_digest, "deployment_runbook_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("deployment_runbook_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "incident_id": self.incident_id,
            "operation_id": self.operation_id,
            "revision_digest": self.revision_digest,
            "phase": self.phase,
            "actor_id": self.actor_id,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "runbook_digest": self.runbook_digest,
            "alert_route_digest": self.alert_route_digest,
            "observed_at_ms": self.observed_at_ms,
            "deadline_ms": self.deadline_ms,
            "unknown": self.unknown,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub incident_id: String,
    pub operation_id: OperationId,
    pub revision_digest: String,
    pub actor_id: String,
    pub requested_phase: IncidentPhase,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub runbook: RunbookRef,
    pub alert_route: AlertRoute,
    pub history: Vec<RunbookEvidence>,
    pub evidence: RunbookEvidence,
    pub input_digest: String,
}

impl IncidentInput {
    pub fn new(
        incident_id: impl Into<String>,
        operation_id: OperationId,
        revision_digest: impl Into<String>,
        actor_id: impl Into<String>,
        requested_phase: IncidentPhase,
        source_cursor: u64,
        authority_epoch: u64,
        data_epoch: u64,
        runbook: RunbookRef,
        alert_route: AlertRoute,
        history: Vec<RunbookEvidence>,
        evidence: RunbookEvidence,
    ) -> Result<Self, String> {
        let mut input = Self {
            schema: DEPLOYMENT_INCIDENT_SCHEMA.to_owned(),
            version: DEPLOYMENT_INCIDENT_VERSION,
            incident_id: incident_id.into(),
            operation_id,
            revision_digest: revision_digest.into(),
            actor_id: actor_id.into(),
            requested_phase,
            source_cursor,
            authority_epoch,
            data_epoch,
            runbook,
            alert_route,
            history,
            evidence,
            input_digest: String::new(),
        };
        input.input_digest = input.digest();
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_INCIDENT_SCHEMA
            || self.version != DEPLOYMENT_INCIDENT_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.history.len() > MAX_INCIDENT_HISTORY
        {
            return Err("deployment_incident_input_header_invalid".to_owned());
        }
        safe_text(&self.incident_id, "deployment_incident_id")?;
        safe_text(&self.actor_id, "deployment_incident_actor")?;
        self.runbook.validate()?;
        self.alert_route.validate()?;
        if self.runbook.phase != self.requested_phase
            || self.evidence.phase != self.requested_phase
            || self.evidence.incident_id != self.incident_id
            || self.evidence.operation_id != self.operation_id
            || self.evidence.revision_digest != self.revision_digest
            || self.evidence.actor_id != self.actor_id
            || self.evidence.source_cursor != self.source_cursor
            || self.evidence.authority_epoch != self.authority_epoch
            || self.evidence.data_epoch != self.data_epoch
            || self.evidence.runbook_digest != self.runbook.ref_digest
            || self.evidence.alert_route_digest != self.alert_route.route_digest
        {
            return Err("deployment_incident_evidence_binding_invalid".to_owned());
        }
        self.evidence.validate()?;
        valid_digest(&self.revision_digest, "deployment_incident_revision_digest")?;
        let mut phases = BTreeSet::new();
        let mut expected_ordinal = 1;
        let mut previous_cursor = 0;
        for item in &self.history {
            item.validate()?;
            if item.incident_id != self.incident_id
                || item.operation_id != self.operation_id
                || item.revision_digest != self.revision_digest
                || item.authority_epoch != self.authority_epoch
                || item.data_epoch != self.data_epoch
                || item.runbook_digest != self.runbook.ref_digest
                || item.alert_route_digest != self.alert_route.route_digest
                || item.phase.ordinal() != expected_ordinal
                || item.source_cursor <= previous_cursor
                || !phases.insert(item.phase)
            {
                return Err("deployment_incident_history_invalid".to_owned());
            }
            expected_ordinal += 1;
            previous_cursor = item.source_cursor;
        }
        let expected_next = expected_ordinal;
        if self.requested_phase.ordinal() != expected_next
            && !(self.history.last().is_some_and(|item| {
                item.phase == self.requested_phase
                    && item.evidence_digest == self.evidence.evidence_digest
            }))
        {
            return Err("deployment_incident_phase_order_invalid".to_owned());
        }
        valid_digest(&self.input_digest, "deployment_incident_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("deployment_incident_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "incident_id": self.incident_id,
            "operation_id": self.operation_id,
            "revision_digest": self.revision_digest,
            "actor_id": self.actor_id,
            "requested_phase": self.requested_phase,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "runbook": self.runbook,
            "alert_route": self.alert_route,
            "history": self.history,
            "evidence": self.evidence,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub incident_id: String,
    pub operation_id: OperationId,
    pub phase: IncidentPhase,
    pub status: IncidentDecisionStatus,
    pub alert_route_read_only: bool,
    pub authority_expansion: bool,
    pub auto_compensation: bool,
    pub recovery_required: bool,
    pub source_cursor: u64,
    pub reason: String,
    pub remediation: String,
    pub evidence_digest: String,
    pub report_digest: String,
}

impl IncidentReport {
    pub fn evaluate(input: &IncidentInput) -> Result<Self, String> {
        input.validate()?;
        let (status, reason, remediation) = derive_decision(input);
        let mut report = Self {
            schema: DEPLOYMENT_INCIDENT_REPORT_SCHEMA.to_owned(),
            version: DEPLOYMENT_INCIDENT_VERSION,
            incident_id: input.incident_id.clone(),
            operation_id: input.operation_id,
            phase: input.requested_phase,
            status,
            alert_route_read_only: true,
            authority_expansion: false,
            auto_compensation: false,
            recovery_required: matches!(
                status,
                IncidentDecisionStatus::Unknown | IncidentDecisionStatus::Blocked
            ),
            source_cursor: input.source_cursor,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            evidence_digest: input.evidence.evidence_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(input)?;
        Ok(report)
    }

    pub fn validate_against(&self, input: &IncidentInput) -> Result<(), String> {
        input.validate()?;
        let (status, reason, remediation) = derive_decision(input);
        let replayed = input.history.last().is_some_and(|item| {
            item.phase == input.requested_phase
                && item.evidence_digest == input.evidence.evidence_digest
        });
        if self.schema != DEPLOYMENT_INCIDENT_REPORT_SCHEMA
            || self.version != DEPLOYMENT_INCIDENT_VERSION
            || self.incident_id != input.incident_id
            || self.operation_id != input.operation_id
            || self.phase != input.requested_phase
            || self.status != status
            || !self.alert_route_read_only
            || self.authority_expansion
            || self.auto_compensation
            || self.recovery_required
                != matches!(
                    status,
                    IncidentDecisionStatus::Unknown | IncidentDecisionStatus::Blocked
                )
            || self.source_cursor != input.source_cursor
            || self.reason != reason
            || self.remediation != remediation
            || self.evidence_digest != input.evidence.evidence_digest
            || (self.status == IncidentDecisionStatus::Replayed) != replayed
        {
            return Err("deployment_incident_report_binding_invalid".to_owned());
        }
        valid_digest(&self.report_digest, "deployment_incident_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("deployment_incident_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn render_human(&self) -> Result<String, String> {
        valid_digest(&self.report_digest, "deployment_incident_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("deployment_incident_report_digest_mismatch".to_owned());
        }
        Ok(format!(
            "incident: {}\nphase: {}\nstatus: {}\nrecovery_required: {}\nreason: {}\nremediation: {}",
            self.incident_id,
            self.phase.as_str(),
            self.status.as_str(),
            self.recovery_required,
            self.reason,
            self.remediation
        ))
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "incident_id": self.incident_id,
            "operation_id": self.operation_id,
            "phase": self.phase,
            "status": self.status,
            "alert_route_read_only": self.alert_route_read_only,
            "authority_expansion": self.authority_expansion,
            "auto_compensation": self.auto_compensation,
            "recovery_required": self.recovery_required,
            "source_cursor": self.source_cursor,
            "reason": self.reason,
            "remediation": self.remediation,
            "evidence_digest": self.evidence_digest,
        }))
    }
}

fn derive_decision(input: &IncidentInput) -> (IncidentDecisionStatus, &'static str, &'static str) {
    let replayed = input.history.last().is_some_and(|item| {
        item.phase == input.requested_phase
            && item.evidence_digest == input.evidence.evidence_digest
    });
    if input.evidence.unknown || input.history.iter().any(|item| item.unknown) {
        (
            IncidentDecisionStatus::Unknown,
            "incident_evidence_unknown",
            "obtain verified evidence and reconcile the incident before advancing",
        )
    } else if input.evidence.observed_at_ms >= input.evidence.deadline_ms {
        (
            IncidentDecisionStatus::Unknown,
            "incident_phase_deadline_expired",
            "record a new bounded phase deadline and reconcile the overdue incident",
        )
    } else if input.requested_phase == IncidentPhase::Closed
        && !input
            .history
            .iter()
            .any(|item| item.phase == IncidentPhase::Verified)
    {
        (
            IncidentDecisionStatus::Blocked,
            "incident_close_requires_verified",
            "complete and verify recovery evidence before closing the incident",
        )
    } else if replayed {
        (
            IncidentDecisionStatus::Replayed,
            "incident_phase_replayed",
            "none",
        )
    } else {
        (
            IncidentDecisionStatus::Accepted,
            "incident_phase_accepted",
            "continue only through the next reviewed runbook phase",
        )
    }
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_INCIDENT_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("file://")
        || value
            .split_whitespace()
            .any(|token| token.starts_with('/') || token.starts_with('\\'))
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
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
