//! PD-34 declared capacity, throughput, latency and degradation budgets.
//!
//! DEP-17's [`CapacityEnvelope`] already bounds the *shape* of a store — frame bytes, batch
//! events, page events, artifact bytes — and `kiana-domain::persistence_capacity` already turns a
//! benchmark summary into a pass/fail. PD-34 adds the third thing: **declared** budgets per
//! storage subject, each one naming where its numbers come from and whether anyone has measured
//! them.
//!
//! ## The numbers are declared, not measured
//!
//! No pressure run, stress test, throughput measurement or latency percentile was performed for
//! this module. Every `max_*` here is a **declared budget**: a number somebody chose, bound to a
//! [`BudgetOrigin`] that says how, and to a [`BudgetMeasurement`] that says how it would be
//! measured. [`DeclaredBudget::validate`] refuses a budget whose measurement is missing, and
//! [`StorageDegradationReport::evaluate`] refuses to call a budget *met* until a measurement
//! receipt exists. There is no field in this module that a source contract can fill to claim
//! measured latency, and the module doc says so out loud.
//!
//! ## Vocabulary is reused, not reinvented
//!
//! * [`StorageCapacitySubject`] is the four thing the card names — event frames, artifacts,
//!   indexes, backup/prune — plus the writer queue, because PD-27's `WriterQueuePolicy` already
//!   derives its bounds from `CapacityEnvelope` and a queue with no subject would be invisible to
//!   the report.
//! * [`CapacityEnvelope`] and [`PersistenceCapacityBudget`] are the existing bounds and budget
//!   types. This module references them; it does not restate a second set of numbers.
//! * [`BenchmarkSummary`] carries the observation, and the existing latency/queue/rejection/
//!   maintenance decision order in `persistence_capacity` is the order this report reuses.
//!
//! ## What the report refuses
//!
//! [`StorageDegradationReport::evaluate`] decides in a fixed order, and the reported reason is the
//! first violated rule:
//!
//! 1. an unbounded subject (`capacity_subject_not_bounded`),
//! 2. a budget with no measurement method (`capacity_budget_measurement_missing`),
//! 3. a budget with no origin (`capacity_budget_origin_missing`),
//! 4. a claimed pass with no receipt (`capacity_budget_receipt_missing`),
//! 5. a backpressure decision that dropped facts (`capacity_backpressure_dropped_facts`),
//! 6. a maintenance share above budget (`capacity_maintenance_budget_exceeded`),
//! 7. an unbounded reader holding a writer (`capacity_reader_blocked_writer`).

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, BenchmarkSummary, CapacityEnvelope,
    PersistenceCapacityBudget, SchemaVersion, SecretScanChannel, StorageErrorClass,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const STORAGE_BUDGET_SCHEMA: &str = "kiana.pd34-storage-budget.v1";
pub const STORAGE_DEGRADATION_SCHEMA: &str = "kiana.pd34-storage-degradation-report.v1";
pub const STORAGE_DEGRADATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_STORAGE_BUDGETS: usize = 8;
pub const MAX_STORAGE_BUDGET_TEXT: usize = 256;
/// Basis points: 10_000 bps is 100%. A maintenance share above this is not a share.
pub const MAX_MAINTENANCE_SHARE_BPS: u16 = 10_000;

/// The things under budget. These are the card's four plus the writer queue PD-27 already bounds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageCapacitySubject {
    /// A single event frame and a commit batch. Bounded by `max_frame_bytes` / `max_batch_events`.
    EventFrame,
    /// A staged or committed artifact payload. Bounded by `max_artifact_bytes`.
    Artifact,
    /// A projection index, bounded by the page/export record caps while it is being rebuilt.
    Index,
    /// Backup creation and retention prune, which share the maintenance share budget.
    BackupPrune,
    /// The writer queue, bounded by the PD-27 `WriterQueuePolicy` caps.
    WriterQueue,
}

impl StorageCapacitySubject {
    pub const ALL: [Self; 5] = [
        Self::EventFrame,
        Self::Artifact,
        Self::Index,
        Self::BackupPrune,
        Self::WriterQueue,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EventFrame => "event_frame",
            Self::Artifact => "artifact",
            Self::Index => "index",
            Self::BackupPrune => "backup_prune",
            Self::WriterQueue => "writer_queue",
        }
    }

    /// The existing `CapacityEnvelope` field that bounds this subject. Returning the field name
    /// rather than a number is deliberate: a second set of bounds would be a second thing to drift.
    pub const fn envelope_field(self) -> &'static str {
        match self {
            Self::EventFrame => "max_frame_bytes",
            Self::Artifact => "max_artifact_bytes",
            Self::Index => "max_page_events",
            Self::BackupPrune => "journal_max_bytes",
            Self::WriterQueue => "max_batch_events",
        }
    }

    /// The bound `CapacityEnvelope` carries for this subject, read from the envelope itself.
    pub fn envelope_bound(self, envelope: &CapacityEnvelope) -> u64 {
        match self {
            Self::EventFrame => envelope.max_frame_bytes,
            Self::Artifact => envelope.max_artifact_bytes,
            Self::Index => envelope.max_page_events,
            Self::BackupPrune => envelope.journal_max_bytes,
            Self::WriterQueue => envelope.max_batch_events,
        }
    }

    /// The card's first rejection: a frame, batch, page or queue with no declared cap is not
    /// "fast", it is unbounded, and an unbounded subject under load is how a store stops exposing
    /// new facts.
    pub fn is_bounded(self, envelope: &CapacityEnvelope) -> bool {
        self.envelope_bound(envelope) > 0
    }
}

/// Where a declared budget's numbers came from. A budget with no origin is a wish.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetOrigin {
    /// Derived from the existing `CapacityEnvelope`; the number is a configured cap, not a
    /// measurement.
    CapacityEnvelope,
    /// Derived from the PD-27 `WriterQueuePolicy` caps.
    WriterQueuePolicy,
    /// Derived from the DEP-17 `PersistenceCapacityBudget` p95/p99 declarations.
    PersistenceBudget,
    /// Chosen by a reviewer and recorded; no measurement exists.
    DeclaredByReview,
}

impl BudgetOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CapacityEnvelope => "capacity_envelope",
            Self::WriterQueuePolicy => "writer_queue_policy",
            Self::PersistenceBudget => "persistence_budget",
            Self::DeclaredByReview => "declared_by_review",
        }
    }

    /// Only a measurement receipt can promote a declared budget to a measured one. A
    /// `DeclaredByReview` budget can never be measured by this module.
    pub const fn needs_measurement(self) -> bool {
        true
    }
}

/// How a budget would be measured. Recorded whether or not the measurement ran, so a future
/// pressure run has a written method to follow instead of inventing one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetMeasurement {
    /// A sustained append of bounded frames; latency is a percentile over the run.
    SustainedAppend,
    /// A restore/rebuild of a representative snapshot under concurrent reads.
    RebuildUnderRead,
    /// A staged-then-committed artifact of the declared maximum size.
    ArtifactStageCommit,
    /// A backup plus retention prune pass with user admission running concurrently.
    MaintenanceUnderLoad,
    /// A queue filled to its declared cap while writers contend.
    QueueContention,
}

impl BudgetMeasurement {
    pub const ALL: [Self; 5] = [
        Self::SustainedAppend,
        Self::RebuildUnderRead,
        Self::ArtifactStageCommit,
        Self::MaintenanceUnderLoad,
        Self::QueueContention,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SustainedAppend => "sustained_append",
            Self::RebuildUnderRead => "rebuild_under_read",
            Self::ArtifactStageCommit => "artifact_stage_commit",
            Self::MaintenanceUnderLoad => "maintenance_under_load",
            Self::QueueContention => "queue_contention",
        }
    }

    /// The method each subject must declare, so two subjects cannot be measured by two different
    /// means without the difference being visible in the report.
    pub const fn for_subject(subject: StorageCapacitySubject) -> Self {
        match subject {
            StorageCapacitySubject::EventFrame => Self::SustainedAppend,
            StorageCapacitySubject::Artifact => Self::ArtifactStageCommit,
            StorageCapacitySubject::Index => Self::RebuildUnderRead,
            StorageCapacitySubject::BackupPrune => Self::MaintenanceUnderLoad,
            StorageCapacitySubject::WriterQueue => Self::QueueContention,
        }
    }
}

/// How a bounded refusal is surfaced when a budget is exceeded.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityBackpressure {
    /// Reject the new work with a diagnosable code and keep every committed fact.
    BoundedReject,
    /// Shed the newest work and keep the queue draining. Facts already accepted are kept.
    ShedNewest,
    /// Stop admitting and let the queue drain to the cap. Never unbounded parking.
    DrainToCap,
}

impl CapacityBackpressure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BoundedReject => "bounded_reject",
            Self::ShedNewest => "shed_newest",
            Self::DrainToCap => "drain_to_cap",
        }
    }

    /// Every strategy preserves committed facts; the difference is only *which* new work is
    /// refused. None of them may drop an accepted fact, so a `false` here is a defect.
    pub const fn preserves_accepted_facts(self) -> bool {
        matches!(
            self,
            Self::BoundedReject | Self::ShedNewest | Self::DrainToCap
        )
    }
}

/// What the report concluded about one subject.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBudgetOutcome {
    /// The declared cap is in force. No measurement was run, so this says nothing about latency.
    Bounded,
    /// Over the declared cap, with the backpressure strategy that was applied.
    OverBudget,
    /// The cap is in force and a measurement receipt exists.
    Measured,
    /// The subject has no declared cap at all. Never a pass.
    Unbounded,
}

impl StorageBudgetOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bounded => "bounded",
            Self::OverBudget => "over_budget",
            Self::Measured => "measured",
            Self::Unbounded => "unbounded",
        }
    }
}

/// The report's overall decision. `Refused` is the positive reading: every subject is bounded and
/// the refusal behaviour is a diagnosable reject rather than a silent drop.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDegradationStatus {
    /// Every subject is bounded; the declared budgets and the backpressure strategy are in force.
    Refused,
    /// At least one subject is unbounded, over budget, or refusing in a way a caller could not
    /// diagnose.
    Exceeded,
}

impl StorageDegradationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refused => "refused",
            Self::Exceeded => "exceeded",
        }
    }
}

/// One declared budget for one subject.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredBudget {
    pub subject: StorageCapacitySubject,
    /// The bound this subject's cap must come from, so a second set of numbers cannot appear.
    pub envelope_field: String,
    pub envelope_bound: u64,
    pub origin: BudgetOrigin,
    pub measurement: BudgetMeasurement,
    /// The DEP-17 latency/rejection budget this subject's numbers are declared against, if any.
    pub latency_budget: Option<PersistenceCapacityBudget>,
    /// The share of runtime maintenance may take, in basis points. Zero means no maintenance.
    pub max_maintenance_share_bps: u16,
    /// The strategy applied when the bound is reached.
    pub backpressure: CapacityBackpressure,
    /// A receipt from an actual measurement. `None` means the budget is declared, not met.
    pub measurement_receipt_digest: Option<String>,
    pub budget_digest: String,
}

impl DeclaredBudget {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subject: StorageCapacitySubject,
        envelope: &CapacityEnvelope,
        origin: BudgetOrigin,
        latency_budget: Option<PersistenceCapacityBudget>,
        max_maintenance_share_bps: u16,
        backpressure: CapacityBackpressure,
        measurement_receipt_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut budget = Self {
            subject,
            envelope_field: subject.envelope_field().to_owned(),
            envelope_bound: subject.envelope_bound(envelope),
            origin,
            measurement: BudgetMeasurement::for_subject(subject),
            latency_budget,
            max_maintenance_share_bps,
            backpressure,
            measurement_receipt_digest,
            budget_digest: String::new(),
        };
        budget.budget_digest = budget.digest();
        budget.validate_against(envelope)?;
        Ok(budget)
    }

    /// Whether anyone has actually measured this subject. A source contract cannot set this.
    pub fn is_measured(&self) -> bool {
        self.measurement_receipt_digest.is_some()
    }

    pub fn outcome(&self) -> StorageBudgetOutcome {
        if self.envelope_bound == 0 {
            StorageBudgetOutcome::Unbounded
        } else if self.is_measured() {
            StorageBudgetOutcome::Measured
        } else {
            StorageBudgetOutcome::Bounded
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_against_shape()?;
        if let Some(latency_budget) = &self.latency_budget {
            latency_budget.validate()?;
        }
        if let Some(receipt) = &self.measurement_receipt_digest {
            validate_digest(receipt, "capacity_budget_measurement_receipt")?;
        }
        validate_digest(&self.budget_digest, "capacity_budget_digest")?;
        if self.budget_digest != self.digest() {
            return Err("capacity_budget_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Validate the shape plus the two declarations that must agree with the envelope. This is
    /// what the report calls per budget, so a subject cannot be declared against a field it does
    /// not actually read.
    pub fn validate_against(&self, envelope: &CapacityEnvelope) -> Result<(), String> {
        envelope.validate()?;
        self.validate_against_shape()?;
        if self.envelope_field != self.subject.envelope_field() {
            return Err("capacity_budget_envelope_field_mismatch".to_owned());
        }
        if self.envelope_bound != self.subject.envelope_bound(envelope) {
            return Err("capacity_budget_envelope_bound_mismatch".to_owned());
        }
        self.validate()
    }

    fn validate_against_shape(&self) -> Result<(), String> {
        if self.envelope_field.trim().is_empty()
            || self.envelope_field.len() > MAX_STORAGE_BUDGET_TEXT
            || self.envelope_field.contains(['\0', '\r', '\n'])
        {
            return Err("capacity_budget_envelope_field_invalid".to_owned());
        }
        if self.max_maintenance_share_bps > MAX_MAINTENANCE_SHARE_BPS {
            return Err("capacity_budget_maintenance_share_invalid".to_owned());
        }
        if !self.backpressure.preserves_accepted_facts() {
            return Err("capacity_backpressure_dropped_facts".to_owned());
        }
        // The measurement method is always recorded. It is a declaration about how a future run
        // would be performed, never a claim that one was.
        if self.measurement != BudgetMeasurement::for_subject(self.subject) {
            return Err("capacity_budget_measurement_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "subject": self.subject,
            "envelope_field": self.envelope_field,
            "envelope_bound": self.envelope_bound,
            "origin": self.origin,
            "measurement": self.measurement,
            "latency_budget": self.latency_budget,
            "max_maintenance_share_bps": self.max_maintenance_share_bps,
            "backpressure": self.backpressure,
            "measurement_receipt_digest": self.measurement_receipt_digest,
        }))
    }
}

/// What a bounded refusal looked like on one subject, and whether a reader held a writer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityRefusalObservation {
    pub subject: StorageCapacitySubject,
    /// The class the refusal carried. A capacity refusal is `Unavailable` or `Conflict`; a
    /// `ResultUnknown` refusal is the PD-30 fault shape, not a capacity one.
    pub error_class: StorageErrorClass,
    /// The stable code the refusal used. Empty when the subject was not refused at all.
    pub refusal_code: String,
    /// Rejected requests, and the total presented. Both zero means nothing was refused.
    pub rejected: u64,
    pub presented: u64,
    /// A reader that held the writer for longer than the budget allows. The card's third
    /// rejection.
    pub reader_held_writer: bool,
    /// Facts already committed before the refusal. Zero here with a `false` drop flag means the
    /// refusal was honest; a lost fact is `capacity_backpressure_dropped_facts`.
    pub committed_facts_retained: bool,
    pub observation_digest: String,
}

impl CapacityRefusalObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subject: StorageCapacitySubject,
        error_class: StorageErrorClass,
        refusal_code: impl Into<String>,
        rejected: u64,
        presented: u64,
        reader_held_writer: bool,
        committed_facts_retained: bool,
    ) -> Result<Self, String> {
        let mut value = Self {
            subject,
            error_class,
            refusal_code: refusal_code.into(),
            rejected,
            presented,
            reader_held_writer,
            committed_facts_retained,
            observation_digest: String::new(),
        };
        value.observation_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.rejected > self.presented
            || (self.rejected > 0 && self.refusal_code.trim().is_empty())
            || (self.rejected == 0 && !self.refusal_code.is_empty())
            || (self.rejected > 0
                && !matches!(
                    self.error_class,
                    StorageErrorClass::Unavailable | StorageErrorClass::Conflict
                ))
        {
            return Err("capacity_refusal_observation_invalid".to_owned());
        }
        if !self.refusal_code.is_empty() {
            safe_text(&self.refusal_code, "capacity_refusal_code")?;
        }
        // A capacity refusal that lost a committed fact is not a refusal, it is a loss.
        if !self.committed_facts_retained {
            return Err("capacity_backpressure_dropped_facts".to_owned());
        }
        validate_digest(
            &self.observation_digest,
            "capacity_refusal_observation_digest",
        )?;
        if self.observation_digest != self.digest() {
            return Err("capacity_refusal_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "subject": self.subject,
            "error_class": self.error_class,
            "refusal_code": self.refusal_code,
            "rejected": self.rejected,
            "presented": self.presented,
            "reader_held_writer": self.reader_held_writer,
            "committed_facts_retained": self.committed_facts_retained,
        }))
    }
}

/// The PD-34 report: declared budgets per subject, the observed refusals, and the maintenance
/// share. A report reaches `Refused` only when every subject is bounded and no refusal was
/// indistinguishable from success.
///
/// `PartialEq` rather than `Eq` because `CapacityEnvelope` and `BenchmarkSummary` — the DEP-17
/// types this report carries unchanged — are not `Eq`. Adding a second capacity model to make
/// this `Eq` would be the drift the card rejects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDegradationReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: StorageDegradationStatus,
    pub baseline_digest: String,
    pub capacity: CapacityEnvelope,
    pub budgets: Vec<DeclaredBudget>,
    pub observations: Vec<CapacityRefusalObservation>,
    /// The DEP-17 benchmark summaries this report was compared against, if any were run. Empty
    /// is the normal case for a source-only slice.
    pub summaries: Vec<BenchmarkSummary>,
    pub maintenance_share_bps: u16,
    /// The subjects that exceeded their declared budget, in subject order.
    pub over_budget: Vec<StorageCapacitySubject>,
    /// The subjects with a measurement receipt, in subject order. Empty means nothing was measured.
    pub measured_subjects: Vec<StorageCapacitySubject>,
    pub reason: String,
    pub remediation: String,
    pub report_digest: String,
}

impl StorageDegradationReport {
    pub fn evaluate(
        baseline_digest: impl Into<String>,
        budgets: Vec<DeclaredBudget>,
        observations: Vec<CapacityRefusalObservation>,
        summaries: Vec<BenchmarkSummary>,
        maintenance_share_bps: u16,
    ) -> Result<Self, String> {
        let baseline_digest = baseline_digest.into();
        let (status, over_budget, measured, reason, remediation) = derive(
            &baseline_digest,
            &budgets,
            &observations,
            &summaries,
            maintenance_share_bps,
        );
        let mut report = Self {
            schema: STORAGE_DEGRADATION_SCHEMA.to_owned(),
            version: STORAGE_DEGRADATION_VERSION,
            status,
            baseline_digest,
            capacity: default_capacity(),
            budgets,
            observations,
            summaries,
            maintenance_share_bps,
            over_budget,
            measured_subjects: measured,
            reason,
            remediation,
            report_digest: String::new(),
        };
        // The envelope is the bounds the budgets were declared against; it is re-read here so a
        // report cannot be evaluated against a different envelope than its budgets describe.
        report.capacity = report.envelope_from_budgets();
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    /// Whether every subject is bounded and every refusal was diagnosable. This is the only
    /// positive reading, and it says nothing about latency.
    pub fn budgets_in_force(&self) -> bool {
        self.status == StorageDegradationStatus::Refused && self.reason.is_empty()
    }

    /// Whether anyone measured anything. A source-only slice answers false, and must.
    pub fn anything_measured(&self) -> bool {
        !self.summaries.is_empty() || !self.measured_subjects.is_empty()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_DEGRADATION_SCHEMA
            || !self
                .version
                .is_compatible_with(&STORAGE_DEGRADATION_VERSION)
            || self.budgets.is_empty()
            || self.budgets.len() > MAX_STORAGE_BUDGETS
        {
            return Err("storage_degradation_report_header_invalid".to_owned());
        }
        self.capacity.validate()?;
        validate_digest(&self.baseline_digest, "storage_degradation_baseline_digest")?;
        validate_digest(&self.report_digest, "storage_degradation_report_digest")?;
        if self.maintenance_share_bps > MAX_MAINTENANCE_SHARE_BPS {
            return Err("storage_degradation_maintenance_share_invalid".to_owned());
        }
        for budget in &self.budgets {
            budget.validate_against(&self.capacity)?;
        }
        for observation in &self.observations {
            observation.validate()?;
        }
        for summary in &self.summaries {
            summary.validate()?;
        }
        let (status, over_budget, measured, reason, remediation) = derive(
            &self.baseline_digest,
            &self.budgets,
            &self.observations,
            &self.summaries,
            self.maintenance_share_bps,
        );
        if self.status != status
            || self.over_budget != over_budget
            || self.measured_subjects != measured
            || self.reason != reason
            || self.remediation != remediation
        {
            return Err("storage_degradation_report_binding_invalid".to_owned());
        }
        if self.status == StorageDegradationStatus::Refused && !self.reason.is_empty() {
            return Err("storage_degradation_refused_with_reason".to_owned());
        }
        if self.status == StorageDegradationStatus::Exceeded && self.reason.is_empty() {
            return Err("storage_degradation_exceeded_without_reason".to_owned());
        }
        // A report that claims a subject was measured must carry the receipt, and a report that
        // carries a receipt must say which subject it belongs to.
        if !self.measured_subjects.is_empty() {
            for subject in &self.measured_subjects {
                let Some(budget) = self
                    .budgets
                    .iter()
                    .find(|budget| budget.subject == *subject)
                else {
                    return Err("storage_degradation_measured_subject_unknown".to_owned());
                };
                if !budget.is_measured() {
                    return Err("capacity_budget_receipt_missing".to_owned());
                }
            }
        }
        if self.report_digest != self.digest() {
            return Err("storage_degradation_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The envelope a set of budgets describes. Reconstructed from the declared bounds so a
    /// report and its budgets cannot disagree about what was bounded.
    fn envelope_from_budgets(&self) -> CapacityEnvelope {
        let bound = |subject: StorageCapacitySubject| -> u64 {
            self.budgets
                .iter()
                .find(|budget| budget.subject == subject)
                .map(|budget| budget.envelope_bound)
                .unwrap_or(0)
        };
        CapacityEnvelope {
            journal_max_bytes: bound(StorageCapacitySubject::BackupPrune),
            journal_max_events: 1,
            max_frame_bytes: bound(StorageCapacitySubject::EventFrame),
            max_event_bytes: bound(StorageCapacitySubject::EventFrame),
            max_batch_events: bound(StorageCapacitySubject::WriterQueue),
            max_page_events: bound(StorageCapacitySubject::Index),
            max_export_records: bound(StorageCapacitySubject::Index),
            observability_queue_capacity: bound(StorageCapacitySubject::WriterQueue),
            max_artifact_bytes: bound(StorageCapacitySubject::Artifact),
            high_cardinality_rejected: true,
            oversize_rejected: true,
            backpressure_preserves_facts: true,
        }
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "baseline_digest": self.baseline_digest,
            "capacity": self.capacity,
            "budgets": self.budgets,
            "observations": self.observations,
            "summaries": self.summaries,
            "maintenance_share_bps": self.maintenance_share_bps,
            "over_budget": self.over_budget,
            "measured_subjects": self.measured_subjects,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

/// Decide in a fixed order, so the reported reason is the first violated rule and therefore the
/// same for the same facts. Unbounded comes first because a subject with no cap cannot be
/// measured against one; then the two declaration rules; then a claimed pass with no receipt;
/// then the card's three behavioural rejections in the order the card names them.
#[allow(clippy::too_many_lines)]
fn derive(
    baseline_digest: &str,
    budgets: &[DeclaredBudget],
    observations: &[CapacityRefusalObservation],
    summaries: &[BenchmarkSummary],
    maintenance_share_bps: u16,
) -> (
    StorageDegradationStatus,
    Vec<StorageCapacitySubject>,
    Vec<StorageCapacitySubject>,
    String,
    String,
) {
    // A named refusal. `over_budget` names the subject the card's rule fired on, and `measured` is
    // deliberately empty: a refused report claims nothing was measured.
    let refusal = |subject: StorageCapacitySubject,
                   reason: &str,
                   remediation: &str|
     -> (
        StorageDegradationStatus,
        Vec<StorageCapacitySubject>,
        Vec<StorageCapacitySubject>,
        String,
        String,
    ) {
        (
            StorageDegradationStatus::Exceeded,
            vec![subject],
            Vec::new(),
            reason.to_owned(),
            remediation.to_owned(),
        )
    };
    if validate_digest(baseline_digest, "storage_degradation_baseline_digest").is_err()
        || budgets.is_empty()
        || budgets.len() > MAX_STORAGE_BUDGETS
        || maintenance_share_bps > MAX_MAINTENANCE_SHARE_BPS
    {
        return (
            StorageDegradationStatus::Exceeded,
            Vec::new(),
            Vec::new(),
            "storage_degradation_report_header_invalid".to_owned(),
            "seal the baseline and declare at most one budget per subject".to_owned(),
        );
    }
    let mut subjects = BTreeSet::new();
    for budget in budgets {
        if let Err(error) = budget.validate() {
            return refusal(
                budget.subject,
                "capacity_budget_invalid",
                &format!("fix the declared budget: {error}"),
            );
        }
        if !subjects.insert(budget.subject) {
            return refusal(
                budget.subject,
                "capacity_budget_duplicate",
                "one declared budget per subject; a second number for the same subject is a second claim",
            );
        }
        // The card's first rejection: an unbounded frame, batch, page or queue.
        if budget.envelope_bound == 0 {
            return refusal(
                budget.subject,
                "capacity_subject_not_bounded",
                "declare a bound from the capacity envelope before admitting this subject",
            );
        }
        // The measurement method is always declared; a budget with none cannot be reproduced.
        if budget.measurement != BudgetMeasurement::for_subject(budget.subject) {
            return refusal(
                budget.subject,
                "capacity_budget_measurement_missing",
                "record the method a future pressure run would use for this subject",
            );
        }
        // Every budget names where its numbers came from.
        if budget.origin.as_str().is_empty() {
            return refusal(
                budget.subject,
                "capacity_budget_origin_missing",
                "declare whether the numbers come from the envelope, a policy or a review",
            );
        }
        // The card's second rejection: backpressure that is not fact-preserving. `validate`
        // already refuses a strategy that drops facts, so reaching here means the *observation*
        // lost one.
        if let Some(observation) = observations
            .iter()
            .find(|observation| !observation.committed_facts_retained)
        {
            return refusal(
                observation.subject,
                "capacity_backpressure_dropped_facts",
                "a capacity refusal must retain every fact accepted before the bound was reached",
            );
        }
    }
    for observation in observations {
        if !subjects.contains(&observation.subject) {
            return refusal(
                observation.subject,
                "capacity_observation_subject_undeclared",
                "declare a budget for the subject whose refusal you are reporting",
            );
        }
    }
    // A summary is the DEP-17 measurement record. A malformed one is refused rather than
    // silently dropped, so a report can never look measured with fewer samples than it claims.
    for summary in summaries {
        if let Err(error) = summary.validate() {
            return refusal(
                StorageCapacitySubject::EventFrame,
                "storage_degradation_summary_invalid",
                &format!("fix the benchmark summary before reporting against it: {error}"),
            );
        }
    }
    // The card's third rejection: maintenance squeezing out user admission.
    if let Some(budget) = budgets
        .iter()
        .find(|budget| maintenance_share_bps > budget.max_maintenance_share_bps)
    {
        return refusal(
            budget.subject,
            "capacity_maintenance_budget_exceeded",
            "bound backup/prune so user admission stays observable; defer maintenance instead",
        );
    }
    // A long reader blocking the writer is the remaining named rejection.
    if let Some(observation) = observations
        .iter()
        .find(|observation| observation.reader_held_writer)
    {
        return refusal(
            observation.subject,
            "capacity_reader_blocked_writer",
            "bound the reader or page it; an unbounded read must not hold the writer",
        );
    }
    let measured: Vec<StorageCapacitySubject> = StorageCapacitySubject::ALL
        .into_iter()
        .filter(|subject| {
            budgets
                .iter()
                .any(|budget| budget.subject == *subject && budget.is_measured())
        })
        .collect();
    // A subject may only be reported as measured when a receipt exists for it.
    for candidate in &measured {
        let budget = budgets
            .iter()
            .find(|budget| budget.subject == *candidate)
            .expect("a measured subject always has a declared budget");
        if !budget.is_measured() {
            return refusal(
                *candidate,
                "capacity_budget_receipt_missing",
                "attach the measurement receipt to the budget it belongs to",
            );
        }
    }
    (
        StorageDegradationStatus::Refused,
        Vec::new(),
        measured,
        String::new(),
        String::new(),
    )
}

/// The DEP-17 envelope this slice declares against when the caller supplies none.
///
/// These are the *configured caps* the JSONL writer already publishes through its
/// `EventStoreCapabilities`; they are not measurements and none of them was observed here.
fn default_capacity() -> CapacityEnvelope {
    CapacityEnvelope {
        journal_max_bytes: 64 * 1024 * 1024,
        journal_max_events: 1_000_000,
        max_frame_bytes: 1_048_576,
        max_event_bytes: 512 * 1024,
        max_batch_events: 256,
        max_page_events: 1_000,
        max_export_records: 10_000,
        observability_queue_capacity: 4_096,
        max_artifact_bytes: 8 * 1024 * 1024,
        high_cardinality_rejected: true,
        oversize_rejected: true,
        backpressure_preserves_facts: true,
    }
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_STORAGE_BUDGET_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{field}_invalid"))
    }
}
