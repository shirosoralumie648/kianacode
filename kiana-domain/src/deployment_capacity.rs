//! Bounded deployment capacity, backpressure and shutdown-limit contract.
//!
//! This reducer consumes adapter-supplied capacity observations for the append, artifact,
//! operation, log, diagnostic, migration and shutdown resources. It never resizes a queue,
//! drops a committed fact, writes an EventLog or changes shutdown state.

use crate::{json_digest, OperationId, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEPLOYMENT_CAPACITY_FACT_SCHEMA: &str = "kiana.deployment-capacity-fact.v1";
pub const DEPLOYMENT_CAPACITY_INPUT_SCHEMA: &str = "kiana.deployment-capacity-input.v1";
pub const DEPLOYMENT_CAPACITY_REPORT_SCHEMA: &str = "kiana.deployment-capacity-report.v1";
pub const DEPLOYMENT_CAPACITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_CAPACITY_FACTS: usize = 7;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityResource {
    Append,
    Artifact,
    Operation,
    Log,
    Diagnostic,
    Migration,
    Shutdown,
}

impl CapacityResource {
    pub const ALL: [Self; MAX_CAPACITY_FACTS] = [
        Self::Append,
        Self::Artifact,
        Self::Operation,
        Self::Log,
        Self::Diagnostic,
        Self::Migration,
        Self::Shutdown,
    ];

    const fn ordinal(self) -> u8 {
        match self {
            Self::Append => 1,
            Self::Artifact => 2,
            Self::Operation => 3,
            Self::Log => 4,
            Self::Diagnostic => 5,
            Self::Migration => 6,
            Self::Shutdown => 7,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Artifact => "artifact",
            Self::Operation => "operation",
            Self::Log => "log",
            Self::Diagnostic => "diagnostic",
            Self::Migration => "migration",
            Self::Shutdown => "shutdown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityFactStatus {
    Available,
    Backpressure,
    Rejected,
    Unknown,
}

impl CapacityFactStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Backpressure => "backpressure",
            Self::Rejected => "rejected",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityReportStatus {
    Ready,
    Backpressure,
    Blocked,
    Unknown,
}

impl CapacityReportStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Backpressure => "backpressure",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub resource: CapacityResource,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub generation: u64,
    pub hard_limit: u64,
    pub used: u64,
    pub requested: u64,
    pub queue_limit: u64,
    pub queue_depth: u64,
    pub bounded: bool,
    pub facts_preserved: bool,
    pub result_unknown: bool,
    pub shutdown_deadline_ms: Option<u64>,
    pub shutdown_elapsed_ms: Option<u64>,
    pub source_digest: String,
    pub evidence_digest: String,
    pub fact_digest: String,
}

impl CapacityFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resource: CapacityResource,
        source_cursor: u64,
        authority_epoch: u64,
        data_epoch: u64,
        generation: u64,
        hard_limit: u64,
        used: u64,
        requested: u64,
        queue_limit: u64,
        queue_depth: u64,
        bounded: bool,
        facts_preserved: bool,
        result_unknown: bool,
        shutdown_deadline_ms: Option<u64>,
        shutdown_elapsed_ms: Option<u64>,
        source_digest: impl Into<String>,
        evidence_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: DEPLOYMENT_CAPACITY_FACT_SCHEMA.to_owned(),
            version: DEPLOYMENT_CAPACITY_VERSION,
            resource,
            source_cursor,
            authority_epoch,
            data_epoch,
            generation,
            hard_limit,
            used,
            requested,
            queue_limit,
            queue_depth,
            bounded,
            facts_preserved,
            result_unknown,
            shutdown_deadline_ms,
            shutdown_elapsed_ms,
            source_digest: source_digest.into(),
            evidence_digest: evidence_digest.into(),
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn status(&self) -> CapacityFactStatus {
        if self.result_unknown
            || self.resource == CapacityResource::Shutdown
                && (self.shutdown_deadline_ms.is_none() || self.shutdown_elapsed_ms.is_none())
            || self.shutdown_deadline_ms.is_some() != self.shutdown_elapsed_ms.is_some()
        {
            return CapacityFactStatus::Unknown;
        }
        if !self.bounded || !self.facts_preserved || self.hard_limit == 0 || self.queue_limit == 0 {
            return CapacityFactStatus::Rejected;
        }
        let Some(total) = self.used.checked_add(self.requested) else {
            return CapacityFactStatus::Rejected;
        };
        if total > self.hard_limit
            || self.queue_depth >= self.queue_limit
            || self
                .shutdown_deadline_ms
                .zip(self.shutdown_elapsed_ms)
                .is_some_and(|(deadline, elapsed)| elapsed >= deadline)
        {
            return CapacityFactStatus::Rejected;
        }
        if self.queue_depth > 0 {
            CapacityFactStatus::Backpressure
        } else {
            CapacityFactStatus::Available
        }
    }

    pub fn stable_code(&self) -> &'static str {
        match self.status() {
            CapacityFactStatus::Available => "ok",
            CapacityFactStatus::Backpressure => "capacity_backpressure",
            CapacityFactStatus::Unknown => "capacity_evidence_unknown",
            CapacityFactStatus::Rejected
                if self
                    .shutdown_deadline_ms
                    .zip(self.shutdown_elapsed_ms)
                    .is_some_and(|(deadline, elapsed)| elapsed >= deadline) =>
            {
                "shutdown_deadline_exceeded"
            }
            CapacityFactStatus::Rejected if !self.bounded => "capacity_unbounded",
            CapacityFactStatus::Rejected if !self.facts_preserved => "capacity_facts_not_preserved",
            CapacityFactStatus::Rejected => "capacity_exceeded",
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_CAPACITY_FACT_SCHEMA
            || self.version != DEPLOYMENT_CAPACITY_VERSION
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
        {
            return Err("deployment_capacity_fact_header_invalid".to_owned());
        }
        if self.shutdown_deadline_ms == Some(0) {
            return Err("deployment_capacity_shutdown_deadline_invalid".to_owned());
        }
        if self.resource != CapacityResource::Shutdown
            && (self.shutdown_deadline_ms.is_some() || self.shutdown_elapsed_ms.is_some())
        {
            return Err("deployment_capacity_shutdown_fields_invalid".to_owned());
        }
        for (value, field) in [
            (&self.source_digest, "deployment_capacity_source_digest"),
            (&self.evidence_digest, "deployment_capacity_evidence_digest"),
            (&self.fact_digest, "deployment_capacity_fact_digest"),
        ] {
            valid_digest(value, field)?;
        }
        if self.fact_digest != self.digest() {
            return Err("deployment_capacity_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "resource": self.resource,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
            "hard_limit": self.hard_limit,
            "used": self.used,
            "requested": self.requested,
            "queue_limit": self.queue_limit,
            "queue_depth": self.queue_depth,
            "bounded": self.bounded,
            "facts_preserved": self.facts_preserved,
            "result_unknown": self.result_unknown,
            "shutdown_deadline_ms": self.shutdown_deadline_ms,
            "shutdown_elapsed_ms": self.shutdown_elapsed_ms,
            "source_digest": self.source_digest,
            "evidence_digest": self.evidence_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub generation: u64,
    pub source_digest: String,
    pub evidence_digest: String,
    pub facts: Vec<CapacityFact>,
    pub input_digest: String,
}

impl CapacityInput {
    pub fn new(
        operation_id: OperationId,
        source_cursor: u64,
        authority_epoch: u64,
        data_epoch: u64,
        generation: u64,
        source_digest: impl Into<String>,
        evidence_digest: impl Into<String>,
        facts: Vec<CapacityFact>,
    ) -> Result<Self, String> {
        let mut input = Self {
            schema: DEPLOYMENT_CAPACITY_INPUT_SCHEMA.to_owned(),
            version: DEPLOYMENT_CAPACITY_VERSION,
            operation_id,
            source_cursor,
            authority_epoch,
            data_epoch,
            generation,
            source_digest: source_digest.into(),
            evidence_digest: evidence_digest.into(),
            facts,
            input_digest: String::new(),
        };
        input.input_digest = input.digest();
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_CAPACITY_INPUT_SCHEMA
            || self.version != DEPLOYMENT_CAPACITY_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
            || self.facts.len() != CapacityResource::ALL.len()
            || self.facts.len() > MAX_CAPACITY_FACTS
        {
            return Err("deployment_capacity_input_header_invalid".to_owned());
        }
        valid_digest(
            &self.source_digest,
            "deployment_capacity_input_source_digest",
        )?;
        valid_digest(
            &self.evidence_digest,
            "deployment_capacity_input_evidence_digest",
        )?;
        let mut resources = BTreeSet::new();
        let mut expected_ordinal = 1;
        for fact in &self.facts {
            fact.validate()?;
            if fact.source_cursor != self.source_cursor
                || fact.authority_epoch != self.authority_epoch
                || fact.data_epoch != self.data_epoch
                || fact.generation != self.generation
                || fact.source_digest != self.source_digest
                || fact.evidence_digest != self.evidence_digest
                || fact.resource.ordinal() != expected_ordinal
                || !resources.insert(fact.resource)
            {
                return Err("deployment_capacity_fact_binding_invalid".to_owned());
            }
            expected_ordinal += 1;
        }
        if resources.len() != CapacityResource::ALL.len() {
            return Err("deployment_capacity_resource_set_invalid".to_owned());
        }
        valid_digest(&self.input_digest, "deployment_capacity_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("deployment_capacity_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
            "source_digest": self.source_digest,
            "evidence_digest": self.evidence_digest,
            "facts": self.facts,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub status: CapacityReportStatus,
    pub allow_new_work: bool,
    pub allow_existing_work: bool,
    pub facts_preserved: bool,
    pub source_cursor: u64,
    pub diagnostics: Vec<CapacityFact>,
    pub reason: String,
    pub remediation: String,
    pub report_digest: String,
}

impl CapacityReport {
    pub fn evaluate(input: &CapacityInput) -> Result<Self, String> {
        input.validate()?;
        let status = derive_status(&input.facts);
        let (reason, remediation) = derive_explanation(status, &input.facts);
        let mut report = Self {
            schema: DEPLOYMENT_CAPACITY_REPORT_SCHEMA.to_owned(),
            version: DEPLOYMENT_CAPACITY_VERSION,
            operation_id: input.operation_id,
            status,
            allow_new_work: matches!(status, CapacityReportStatus::Ready),
            allow_existing_work: !matches!(
                status,
                CapacityReportStatus::Blocked | CapacityReportStatus::Unknown
            ),
            facts_preserved: input.facts.iter().all(|fact| fact.facts_preserved),
            source_cursor: input.source_cursor,
            diagnostics: input.facts.clone(),
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(input)?;
        Ok(report)
    }

    pub fn validate_against(&self, input: &CapacityInput) -> Result<(), String> {
        input.validate()?;
        let status = derive_status(&input.facts);
        let (reason, remediation) = derive_explanation(status, &input.facts);
        if self.schema != DEPLOYMENT_CAPACITY_REPORT_SCHEMA
            || self.version != DEPLOYMENT_CAPACITY_VERSION
            || self.operation_id != input.operation_id
            || self.status != status
            || self.allow_new_work != matches!(status, CapacityReportStatus::Ready)
            || self.allow_existing_work
                != !matches!(
                    status,
                    CapacityReportStatus::Blocked | CapacityReportStatus::Unknown
                )
            || self.facts_preserved != input.facts.iter().all(|fact| fact.facts_preserved)
            || self.source_cursor != input.source_cursor
            || self.diagnostics != input.facts
            || self.reason != reason
            || self.remediation != remediation
        {
            return Err("deployment_capacity_report_binding_invalid".to_owned());
        }
        valid_digest(&self.report_digest, "deployment_capacity_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("deployment_capacity_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn render_json(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "deployment_capacity_json_encode_failed".to_owned())
    }

    pub fn render_human(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        let mut lines = vec![
            format!("status: {}", self.status.as_str()),
            format!("allow_new_work: {}", self.allow_new_work),
            format!("allow_existing_work: {}", self.allow_existing_work),
            format!("facts_preserved: {}", self.facts_preserved),
            format!("source_cursor: {}", self.source_cursor),
            format!("reason: {}", self.reason),
            format!("remediation: {}", self.remediation),
        ];
        for fact in &self.diagnostics {
            lines.push(format!(
                "{}: {} code={}",
                fact.resource.as_str(),
                fact.status().as_str(),
                fact.stable_code()
            ));
        }
        Ok(lines.join("\n"))
    }

    fn validate_digest_only(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_CAPACITY_REPORT_SCHEMA
            || self.version != DEPLOYMENT_CAPACITY_VERSION
            || self.diagnostics.len() != MAX_CAPACITY_FACTS
        {
            return Err("deployment_capacity_report_shape_invalid".to_owned());
        }
        for fact in &self.diagnostics {
            fact.validate()?;
        }
        let status = derive_status(&self.diagnostics);
        let (reason, remediation) = derive_explanation(status, &self.diagnostics);
        if self.status != status
            || self.allow_new_work != matches!(status, CapacityReportStatus::Ready)
            || self.allow_existing_work
                != !matches!(
                    status,
                    CapacityReportStatus::Blocked | CapacityReportStatus::Unknown
                )
            || self.facts_preserved != self.diagnostics.iter().all(|fact| fact.facts_preserved)
            || self.reason != reason
            || self.remediation != remediation
        {
            return Err("deployment_capacity_report_shape_invalid".to_owned());
        }
        valid_digest(&self.report_digest, "deployment_capacity_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("deployment_capacity_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "status": self.status,
            "allow_new_work": self.allow_new_work,
            "allow_existing_work": self.allow_existing_work,
            "facts_preserved": self.facts_preserved,
            "source_cursor": self.source_cursor,
            "diagnostics": self.diagnostics,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

fn derive_status(facts: &[CapacityFact]) -> CapacityReportStatus {
    if facts
        .iter()
        .any(|fact| fact.status() == CapacityFactStatus::Unknown)
    {
        CapacityReportStatus::Unknown
    } else if facts
        .iter()
        .any(|fact| fact.status() == CapacityFactStatus::Rejected)
    {
        CapacityReportStatus::Blocked
    } else if facts
        .iter()
        .any(|fact| fact.status() == CapacityFactStatus::Backpressure)
    {
        CapacityReportStatus::Backpressure
    } else {
        CapacityReportStatus::Ready
    }
}

fn derive_explanation(
    status: CapacityReportStatus,
    facts: &[CapacityFact],
) -> (&'static str, &'static str) {
    let wanted = match status {
        CapacityReportStatus::Unknown => CapacityFactStatus::Unknown,
        CapacityReportStatus::Blocked => CapacityFactStatus::Rejected,
        CapacityReportStatus::Backpressure => CapacityFactStatus::Backpressure,
        CapacityReportStatus::Ready => CapacityFactStatus::Available,
    };
    if let Some(fact) = facts.iter().find(|fact| fact.status() == wanted) {
        return match fact.status() {
            CapacityFactStatus::Backpressure => (
                "capacity_backpressure",
                "bound producers and drain the queue before admitting new work",
            ),
            CapacityFactStatus::Rejected => (
                fact.stable_code(),
                "preserve committed facts and resolve the reviewed capacity limit before retrying",
            ),
            CapacityFactStatus::Unknown => (
                "capacity_evidence_unknown",
                "obtain verified capacity and shutdown observations before retrying",
            ),
            CapacityFactStatus::Available => ("ok", "within bounded capacity"),
        };
    }
    match status {
        CapacityReportStatus::Ready => ("ok", "within bounded capacity"),
        CapacityReportStatus::Backpressure => (
            "capacity_backpressure",
            "bound producers and drain the queue before admitting new work",
        ),
        CapacityReportStatus::Blocked => (
            "capacity_exceeded",
            "preserve committed facts and resolve the reviewed capacity limit before retrying",
        ),
        CapacityReportStatus::Unknown => (
            "capacity_evidence_unknown",
            "obtain verified capacity and shutdown observations before retrying",
        ),
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
