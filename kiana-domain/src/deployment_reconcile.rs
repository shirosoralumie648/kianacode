//! Read-only projector/index/queue/lease repair and explicit reconcile decisions.
//!
//! The reducer validates a common source cursor, projection generation, lease/fence and
//! operation/evidence binding. It only reports a repair plan or that an explicit commit is
//! ready; it never rewrites EventLog, reruns an Unknown effect, claims a queue lease or mutates a
//! projection store.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, OperationId, SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEPLOYMENT_REPAIR_FACT_SCHEMA: &str = "kiana.deployment-repair-fact.v1";
pub const DEPLOYMENT_RECONCILE_INPUT_SCHEMA: &str = "kiana.deployment-reconcile-input.v1";
pub const DEPLOYMENT_RECONCILE_REPORT_SCHEMA: &str = "kiana.deployment-reconcile-report.v1";
pub const DEPLOYMENT_RECONCILE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RECONCILE_FACTS: usize = 4;
pub const MAX_RECONCILE_TEXT: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairTarget {
    Projector,
    Index,
    Queue,
    Lease,
}

impl RepairTarget {
    pub const ALL: [Self; 4] = [Self::Projector, Self::Index, Self::Queue, Self::Lease];

    const fn ordinal(self) -> u8 {
        match self {
            Self::Projector => 1,
            Self::Index => 2,
            Self::Queue => 3,
            Self::Lease => 4,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Projector => "projector",
            Self::Index => "index",
            Self::Queue => "queue",
            Self::Lease => "lease",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileCommand {
    Inspect,
    PlanRepair,
    CommitRepair,
}

impl ReconcileCommand {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::PlanRepair => "plan_repair",
            Self::CommitRepair => "commit_repair",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairFactStatus {
    Healthy,
    RepairNeeded,
    Blocked,
    Unknown,
}

impl RepairFactStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::RepairNeeded => "repair_needed",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileStatus {
    Healthy,
    RepairNeeded,
    Blocked,
    Unknown,
    CommitReady,
}

impl ReconcileStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::RepairNeeded => "repair_needed",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
            Self::CommitReady => "commit_ready",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub target: RepairTarget,
    pub source_cursor: u64,
    pub projection_cursor: Option<u64>,
    pub projection_generation: u64,
    pub expected_generation: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub lease_active: bool,
    pub fence_valid: bool,
    pub result_unknown: bool,
    pub source_digest: String,
    pub lease_digest: String,
    pub evidence_digest: String,
    pub fact_digest: String,
}

impl RepairFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: RepairTarget,
        source_cursor: u64,
        projection_cursor: Option<u64>,
        projection_generation: u64,
        expected_generation: u64,
        authority_epoch: u64,
        data_epoch: u64,
        lease_active: bool,
        fence_valid: bool,
        result_unknown: bool,
        source_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        evidence_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: DEPLOYMENT_REPAIR_FACT_SCHEMA.to_owned(),
            version: DEPLOYMENT_RECONCILE_VERSION,
            target,
            source_cursor,
            projection_cursor,
            projection_generation,
            expected_generation,
            authority_epoch,
            data_epoch,
            lease_active,
            fence_valid,
            result_unknown,
            source_digest: source_digest.into(),
            lease_digest: lease_digest.into(),
            evidence_digest: evidence_digest.into(),
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn status(&self) -> RepairFactStatus {
        if self.result_unknown || self.projection_cursor.is_none() {
            RepairFactStatus::Unknown
        } else if !self.lease_active
            || !self.fence_valid
            || self
                .projection_cursor
                .is_some_and(|cursor| cursor > self.source_cursor)
            || self.projection_generation > self.expected_generation
        {
            RepairFactStatus::Blocked
        } else if self
            .projection_cursor
            .is_some_and(|cursor| cursor < self.source_cursor)
            || self.projection_generation < self.expected_generation
        {
            RepairFactStatus::RepairNeeded
        } else {
            RepairFactStatus::Healthy
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_REPAIR_FACT_SCHEMA
            || self.version != DEPLOYMENT_RECONCILE_VERSION
            || self.source_cursor == 0
            || self.projection_generation == 0
            || self.expected_generation == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
        {
            return Err("deployment_repair_fact_header_invalid".to_owned());
        }
        if self.projection_cursor.is_some_and(|cursor| cursor == 0) {
            return Err("deployment_repair_fact_cursor_invalid".to_owned());
        }
        for (value, field) in [
            (&self.source_digest, "deployment_repair_source_digest"),
            (&self.lease_digest, "deployment_repair_lease_digest"),
            (&self.evidence_digest, "deployment_repair_evidence_digest"),
        ] {
            valid_digest(value, field)?;
        }
        valid_digest(&self.fact_digest, "deployment_repair_fact_digest")?;
        if self.fact_digest != self.digest() {
            return Err("deployment_repair_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "target": self.target,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "expected_generation": self.expected_generation,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "lease_active": self.lease_active,
            "fence_valid": self.fence_valid,
            "result_unknown": self.result_unknown,
            "source_digest": self.source_digest,
            "lease_digest": self.lease_digest,
            "evidence_digest": self.evidence_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub command: ReconcileCommand,
    pub actor_id: String,
    pub source_cursor: u64,
    pub expected_generation: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub evidence_digest: String,
    pub approval_digest: Option<String>,
    pub facts: Vec<RepairFact>,
    pub input_digest: String,
}

impl ReconcileInput {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        command: ReconcileCommand,
        actor_id: impl Into<String>,
        source_cursor: u64,
        expected_generation: u64,
        authority_epoch: u64,
        data_epoch: u64,
        evidence_digest: impl Into<String>,
        approval_digest: Option<String>,
        facts: Vec<RepairFact>,
    ) -> Result<Self, String> {
        let mut input = Self {
            schema: DEPLOYMENT_RECONCILE_INPUT_SCHEMA.to_owned(),
            version: DEPLOYMENT_RECONCILE_VERSION,
            operation_id,
            command,
            actor_id: actor_id.into(),
            source_cursor,
            expected_generation,
            authority_epoch,
            data_epoch,
            evidence_digest: evidence_digest.into(),
            approval_digest,
            facts,
            input_digest: String::new(),
        };
        input.input_digest = input.digest();
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_RECONCILE_INPUT_SCHEMA
            || self.version != DEPLOYMENT_RECONCILE_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.expected_generation == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.facts.len() != RepairTarget::ALL.len()
            || self.facts.len() > MAX_RECONCILE_FACTS
        {
            return Err("deployment_reconcile_input_header_invalid".to_owned());
        }
        safe_text(&self.actor_id, "deployment_reconcile_actor")?;
        valid_digest(
            &self.evidence_digest,
            "deployment_reconcile_evidence_digest",
        )?;
        match self.command {
            ReconcileCommand::CommitRepair => {
                let approval = self
                    .approval_digest
                    .as_deref()
                    .ok_or_else(|| "deployment_reconcile_approval_required".to_owned())?;
                valid_digest(approval, "deployment_reconcile_approval_digest")?;
            }
            ReconcileCommand::Inspect | ReconcileCommand::PlanRepair => {
                if self.approval_digest.is_some() {
                    return Err("deployment_reconcile_unexpected_approval".to_owned());
                }
            }
        }
        let mut targets = BTreeSet::new();
        let mut expected_ordinal = 1;
        let mut source_digest: Option<&str> = None;
        for fact in &self.facts {
            fact.validate()?;
            if fact.source_cursor != self.source_cursor
                || fact.expected_generation != self.expected_generation
                || fact.authority_epoch != self.authority_epoch
                || fact.data_epoch != self.data_epoch
                || fact.target.ordinal() != expected_ordinal
                || !targets.insert(fact.target)
            {
                return Err("deployment_reconcile_fact_binding_invalid".to_owned());
            }
            if let Some(previous) = source_digest {
                if previous != fact.source_digest {
                    return Err("deployment_reconcile_source_digest_mismatch".to_owned());
                }
            } else {
                source_digest = Some(fact.source_digest.as_str());
            }
            if fact.evidence_digest != self.evidence_digest {
                return Err("deployment_reconcile_evidence_binding_invalid".to_owned());
            }
            expected_ordinal += 1;
        }
        if targets.len() != RepairTarget::ALL.len() {
            return Err("deployment_reconcile_target_set_invalid".to_owned());
        }
        valid_digest(&self.input_digest, "deployment_reconcile_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("deployment_reconcile_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "command": self.command,
            "actor_id": self.actor_id,
            "source_cursor": self.source_cursor,
            "expected_generation": self.expected_generation,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "evidence_digest": self.evidence_digest,
            "approval_digest": self.approval_digest,
            "facts": self.facts,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub command: ReconcileCommand,
    pub status: ReconcileStatus,
    pub read_only: bool,
    pub explicit_commit_required: bool,
    pub actor_id: String,
    pub source_cursor: u64,
    pub planned_projection_generation: Option<u64>,
    pub diagnostics: Vec<RepairFact>,
    pub reason: String,
    pub remediation: String,
    pub evidence_digest: String,
    pub report_digest: String,
}

impl ReconcileReport {
    pub fn evaluate(input: &ReconcileInput) -> Result<Self, String> {
        input.validate()?;
        let (status, planned_generation, reason, remediation, allow_commit) =
            derive_decision(input)?;
        let mut report = Self {
            schema: DEPLOYMENT_RECONCILE_REPORT_SCHEMA.to_owned(),
            version: DEPLOYMENT_RECONCILE_VERSION,
            operation_id: input.operation_id,
            command: input.command,
            status,
            read_only: true,
            explicit_commit_required: !allow_commit,
            actor_id: input.actor_id.clone(),
            source_cursor: input.source_cursor,
            planned_projection_generation: planned_generation,
            diagnostics: input.facts.clone(),
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            evidence_digest: input.evidence_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(input)?;
        Ok(report)
    }

    pub fn validate_against(&self, input: &ReconcileInput) -> Result<(), String> {
        input.validate()?;
        let (status, planned_generation, reason, remediation, allow_commit) =
            derive_decision(input)?;
        if self.schema != DEPLOYMENT_RECONCILE_REPORT_SCHEMA
            || self.version != DEPLOYMENT_RECONCILE_VERSION
            || self.operation_id != input.operation_id
            || self.command != input.command
            || self.status != status
            || !self.read_only
            || self.explicit_commit_required != !allow_commit
            || self.actor_id != input.actor_id
            || self.source_cursor != input.source_cursor
            || self.planned_projection_generation != planned_generation
            || self.diagnostics != input.facts
            || self.reason != reason
            || self.remediation != remediation
            || self.evidence_digest != input.evidence_digest
        {
            return Err("deployment_reconcile_report_binding_invalid".to_owned());
        }
        safe_text(&self.actor_id, "deployment_reconcile_report_actor")?;
        safe_text(&self.reason, "deployment_reconcile_report_reason")?;
        safe_text(&self.remediation, "deployment_reconcile_report_remediation")?;
        valid_digest(
            &self.evidence_digest,
            "deployment_reconcile_report_evidence_digest",
        )?;
        valid_digest(&self.report_digest, "deployment_reconcile_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("deployment_reconcile_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "command": self.command,
            "status": self.status,
            "read_only": self.read_only,
            "explicit_commit_required": self.explicit_commit_required,
            "actor_id": self.actor_id,
            "source_cursor": self.source_cursor,
            "planned_projection_generation": self.planned_projection_generation,
            "diagnostics": self.diagnostics,
            "reason": self.reason,
            "remediation": self.remediation,
            "evidence_digest": self.evidence_digest,
        }))
    }

    pub fn render_human(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        let mut lines = vec![
            format!("command: {}", self.command.as_str()),
            format!("status: {}", self.status.as_str()),
            format!("read_only: {}", self.read_only),
            format!("source_cursor: {}", self.source_cursor),
            format!("reason: {}", self.reason),
            format!("remediation: {}", self.remediation),
        ];
        if let Some(generation) = self.planned_projection_generation {
            lines.push(format!("planned_projection_generation: {generation}"));
        }
        for fact in &self.diagnostics {
            lines.push(format!(
                "{}: {} cursor={} generation={}",
                fact.target.as_str(),
                fact.status().as_str(),
                fact.projection_cursor
                    .map_or_else(|| "unknown".to_owned(), |cursor| cursor.to_string()),
                fact.projection_generation
            ));
        }
        Ok(lines.join("\n"))
    }

    pub fn render_json(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "deployment_reconcile_json_encode_failed".to_owned())
    }

    fn validate_digest_only(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_RECONCILE_REPORT_SCHEMA
            || self.version != DEPLOYMENT_RECONCILE_VERSION
            || !self.read_only
            || self.diagnostics.len() != MAX_RECONCILE_FACTS
        {
            return Err("deployment_reconcile_report_shape_invalid".to_owned());
        }
        for fact in &self.diagnostics {
            fact.validate()?;
        }
        safe_text(&self.actor_id, "deployment_reconcile_report_actor")?;
        safe_text(&self.reason, "deployment_reconcile_report_reason")?;
        safe_text(&self.remediation, "deployment_reconcile_report_remediation")?;
        valid_digest(
            &self.evidence_digest,
            "deployment_reconcile_report_evidence_digest",
        )?;
        valid_digest(&self.report_digest, "deployment_reconcile_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("deployment_reconcile_report_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

fn derive_decision(
    input: &ReconcileInput,
) -> Result<
    (
        ReconcileStatus,
        Option<u64>,
        &'static str,
        &'static str,
        bool,
    ),
    String,
> {
    let statuses = input
        .facts
        .iter()
        .map(RepairFact::status)
        .collect::<Vec<_>>();
    let planned_generation = input
        .expected_generation
        .checked_add(1)
        .ok_or_else(|| "deployment_reconcile_generation_overflow".to_owned())?;
    if statuses.contains(&RepairFactStatus::Unknown) {
        return Ok((
            ReconcileStatus::Unknown,
            None,
            "reconcile_result_unknown",
            "query the original effect and record verified external evidence",
            false,
        ));
    }
    if statuses.contains(&RepairFactStatus::Blocked) {
        return Ok((
            ReconcileStatus::Blocked,
            None,
            "reconcile_fence_or_cursor_blocked",
            "restore the matching lease, fence, epoch and source cursor before repair",
            false,
        ));
    }
    let needs_repair = statuses.contains(&RepairFactStatus::RepairNeeded);
    if !needs_repair {
        if input.command == ReconcileCommand::CommitRepair {
            return Ok((
                ReconcileStatus::Blocked,
                None,
                "reconcile_no_repair_needed",
                "inspect the current projection and do not submit an empty repair",
                false,
            ));
        }
        return Ok((ReconcileStatus::Healthy, None, "ok", "none", false));
    }
    if input.command == ReconcileCommand::CommitRepair {
        return Ok((
            ReconcileStatus::CommitReady,
            Some(planned_generation),
            "reconcile_commit_explicitly_ready",
            "submit the reviewed repair through the server-owned commit boundary",
            true,
        ));
    }
    Ok((
        ReconcileStatus::RepairNeeded,
        Some(planned_generation),
        if input.command == ReconcileCommand::PlanRepair {
            "reconcile_repair_plan_ready"
        } else {
            "reconcile_repair_needed"
        },
        "review actor, evidence, lease and approval before an explicit commit",
        false,
    ))
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_RECONCILE_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("file://")
        || value.split_whitespace().any(|token| {
            token.starts_with('/')
                || token.starts_with('\\')
                || (token.len() >= 3
                    && token.as_bytes()[0].is_ascii_alphabetic()
                    && token.as_bytes()[1] == b':'
                    && matches!(token.as_bytes()[2], b'/' | b'\\'))
        })
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
