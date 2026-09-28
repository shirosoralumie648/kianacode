//! DEP-24 recovery objective, rehearsal measurement and rehearsal evidence contract.
//!
//! RPO and RTO are *declared* targets, not observations. A recovery point objective says how much
//! history a restore is allowed to lose; a recovery time objective says how long service is
//! allowed to be unavailable. Neither is a fact: both are numbers somebody wrote down. This
//! module is the place where those numbers are compared against what a rehearsal actually
//! observed, and where a rehearsal that observed nothing worth reporting is refused rather than
//! published.
//!
//! Everything here is a pure function over supplied values. It does not kill a process, restore a
//! root, take a backup, read a file, start a timer or append an event. A "rehearsal" at this
//! layer is a *record* of a run somebody else performed; the decision about whether that record
//! may be published as evidence of an RPO/RTO is the only thing decided here.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, FenceTokenId, RequestId, SchemaVersion,
    SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RECOVERY_OBJECTIVE_SCHEMA: &str = "kiana.recovery-objective.v1";
pub const RECOVERY_EVIDENCE_SCHEMA: &str = "kiana.recovery-evidence.v1";
pub const RECOVERY_MEASUREMENT_SCHEMA: &str = "kiana.recovery-measurement.v1";
pub const RECOVERY_REHEARSAL_REPORT_SCHEMA: &str = "kiana.recovery-rehearsal-report.v1";
pub const RECOVERY_REHEARSAL_LEDGER_SCHEMA: &str = "kiana.recovery-rehearsal-ledger.v1";
pub const RECOVERY_REHEARSAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RECOVERY_TEXT: usize = 256;
pub const MAX_RECOVERY_ARGV: usize = 16;
pub const MAX_RECOVERY_REHEARSALS: usize = 64;

/// The fault a rehearsal injected.
///
/// There is no `Other`. A rehearsal whose fault cannot be named is not a rehearsal, and an open
/// bucket would be a way to report a run nobody can reproduce.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryFaultScenario {
    /// The process was killed mid-write.
    ProcessCrash,
    /// The backup stopped before every chunk was committed.
    BackupPartial,
    /// The restore was interrupted between two roots.
    RestoreInterrupted,
    /// A frame in the backup was structurally damaged.
    CorruptFrame,
    /// The volume filled up partway through the run.
    DiskFull,
    /// The wall clock moved backwards during the run.
    ClockRegression,
    /// A command was dispatched against the restored root after the fault.
    PostRestoreDispatch,
}

impl RecoveryFaultScenario {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProcessCrash => "process_crash",
            Self::BackupPartial => "backup_partial",
            Self::RestoreInterrupted => "restore_interrupted",
            Self::CorruptFrame => "corrupt_frame",
            Self::DiskFull => "disk_full",
            Self::ClockRegression => "clock_regression",
            Self::PostRestoreDispatch => "post_restore_dispatch",
        }
    }
}

/// What the rehearsal actually observed.
///
/// `ResultUnknown` is a terminal observation, not a pending one. A rehearsal that ended Unknown
/// stays Unknown until a *separate* reconciliation decides what the outside world did (DEP-25);
/// it is never resolved here by retrying or by optimism, and it can never be reported as a
/// rehearsal that met its objective.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryObservation {
    /// The rehearsal completed and the restored root served from re-read durable facts.
    Succeeded,
    /// The rehearsal was refused by a guard before it could complete.
    Denied,
    /// The rehearsal could not determine the outcome. Terminal for this slice.
    ResultUnknown,
}

impl RecoveryObservation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Denied => "denied",
            Self::ResultUnknown => "result_unknown",
        }
    }
}

/// Whether a rehearsal met the objective it was measured against. This is a judgment, and it is
/// deliberately a different axis from `RecoveryObservation`, which is a fact.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryObjectiveStatus {
    WithinObjective,
    ObjectiveMissed,
}

impl RecoveryObjectiveStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WithinObjective => "within_objective",
            Self::ObjectiveMissed => "objective_missed",
        }
    }
}

/// The declared recovery targets for one scope.
///
/// A declaration is allowed to be optimistic; that is what a target is. What is not allowed is
/// for the optimism to become a fact, which is why every field here is a bound a measurement is
/// checked against rather than a claim about what happened.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryObjective {
    pub schema: String,
    pub version: SchemaVersion,
    pub objective_id: RequestId,
    /// What the targets cover. A space, a project, a store. Never a path.
    pub scope: String,
    pub owner: String,
    /// Recovery point objective: the most history a restore may lose, in milliseconds.
    pub rpo_max_data_loss_ms: u64,
    /// Recovery time objective: the longest a restore may take, in milliseconds.
    pub rto_max_recovery_ms: u64,
    pub declared_at_unix_ms: u64,
    pub objective_digest: String,
}

impl RecoveryObjective {
    pub fn new(
        objective_id: RequestId,
        scope: impl Into<String>,
        owner: impl Into<String>,
        rpo_max_data_loss_ms: u64,
        rto_max_recovery_ms: u64,
        declared_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut objective = Self {
            schema: RECOVERY_OBJECTIVE_SCHEMA.to_owned(),
            version: RECOVERY_REHEARSAL_VERSION,
            objective_id,
            scope: scope.into(),
            owner: owner.into(),
            rpo_max_data_loss_ms,
            rto_max_recovery_ms,
            declared_at_unix_ms,
            objective_digest: String::new(),
        };
        objective.objective_digest = objective.digest();
        objective.validate()?;
        Ok(objective)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RECOVERY_OBJECTIVE_SCHEMA
            || !self.version.is_compatible_with(&RECOVERY_REHEARSAL_VERSION)
            || self.objective_id.as_uuid().is_nil()
            || self.declared_at_unix_ms == 0
        {
            return Err("recovery_objective_header_invalid".to_owned());
        }
        // A zero budget would make "met the objective" trivially true for any loss at all, which
        // is a target nobody can keep. Both must be a real, positive window.
        if self.rpo_max_data_loss_ms == 0 {
            return Err("recovery_objective_rpo_invalid".to_owned());
        }
        if self.rto_max_recovery_ms == 0 {
            return Err("recovery_objective_rto_invalid".to_owned());
        }
        safe_text(&self.scope, "recovery_objective_scope")?;
        safe_text(&self.owner, "recovery_objective_owner")?;
        valid_digest(&self.objective_digest, "recovery_objective_digest")?;
        if self.objective_digest != self.digest() {
            return Err("recovery_objective_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "objective_id": self.objective_id,
            "scope": self.scope,
            "owner": self.owner,
            "rpo_max_data_loss_ms": self.rpo_max_data_loss_ms,
            "rto_max_recovery_ms": self.rto_max_recovery_ms,
            "declared_at_unix_ms": self.declared_at_unix_ms,
        }))
    }
}

/// The evidence block one rehearsal run must be bound to.
///
/// Without these fields a rehearsal is a self-report: a number with nothing behind it. The exit
/// code and the limitations are deliberately *not* checked by `validate` -- an absent exit code
/// and an unstated limitation are structurally serializable, so they are the business of
/// [`evaluate_evidence_completeness`], which is the one function that decides whether a run
/// counts as evidence at all.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRehearsalEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    /// The worktree state the run started from.
    pub source_worktree_status: String,
    /// The command that produced the run, argv, as it was invoked. Never a shell string.
    pub command_argv: Vec<String>,
    /// The exit status of that command. `None` means the run never captured one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_exit_code: Option<i32>,
    /// A digest over the environment the run saw. Never the environment itself.
    pub environment_digest: String,
    /// The fixture or cassette the run was driven from.
    pub fixture_cassette: String,
    /// What this run did *not* prove. Required, and a placeholder is refused.
    pub limitations: String,
    pub evidence_digest: String,
}

impl RecoveryRehearsalEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_worktree_status: impl Into<String>,
        command_argv: Vec<String>,
        command_exit_code: Option<i32>,
        environment_digest: impl Into<String>,
        fixture_cassette: impl Into<String>,
        limitations: impl Into<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: RECOVERY_EVIDENCE_SCHEMA.to_owned(),
            version: RECOVERY_REHEARSAL_VERSION,
            source_worktree_status: source_worktree_status.into(),
            command_argv,
            command_exit_code,
            environment_digest: environment_digest.into(),
            fixture_cassette: fixture_cassette.into(),
            limitations: limitations.into(),
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RECOVERY_EVIDENCE_SCHEMA
            || !self.version.is_compatible_with(&RECOVERY_REHEARSAL_VERSION)
        {
            return Err("recovery_evidence_header_invalid".to_owned());
        }
        if self.command_argv.is_empty() || self.command_argv.len() > MAX_RECOVERY_ARGV {
            return Err("recovery_evidence_command_argv_invalid".to_owned());
        }
        for argument in &self.command_argv {
            safe_text(argument, "recovery_evidence_command_argv")?;
        }
        safe_text(
            &self.source_worktree_status,
            "recovery_evidence_source_worktree_status",
        )?;
        safe_text(&self.fixture_cassette, "recovery_evidence_fixture_cassette")?;
        valid_digest(
            &self.environment_digest,
            "recovery_evidence_environment_digest",
        )?;
        valid_digest(&self.evidence_digest, "recovery_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("recovery_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "source_worktree_status": self.source_worktree_status,
            "command_argv": self.command_argv,
            "command_exit_code": self.command_exit_code,
            "environment_digest": self.environment_digest,
            "fixture_cassette": self.fixture_cassette,
            "limitations": self.limitations,
        }))
    }
}

/// One rehearsal run, measured.
///
/// Every field is an observation the adapter reports. The domain can require that they are
/// present, well formed, mutually consistent and not flattering; it cannot require that they are
/// true. That limit is the same one DEP-22 and DEP-23 document for their own inventories.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryMeasurement {
    pub schema: String,
    pub version: SchemaVersion,
    /// Stable identity of this rehearsal run.
    pub rehearsal_id: String,
    pub scenario: RecoveryFaultScenario,
    /// The fault fixture that was exercised.
    pub fixture: String,
    /// The objective this measurement claims to test. A measurement taken against a different
    /// declaration may not be attached to this one.
    pub objective_digest: String,
    pub evidence: RecoveryRehearsalEvidence,
    /// Newest cursor the source instance had committed when the fault hit.
    pub source_cursor: u64,
    /// Cursor the quarantined root carried when the restore finished.
    pub restored_root_cursor: u64,
    /// Cursor the restored root actually resumed at.
    pub recovered_cursor: u64,
    /// Measured recovery point: how much history the restore actually lost.
    pub observed_data_loss_ms: u64,
    /// Measured recovery time: how long the restore actually took.
    pub observed_recovery_ms: u64,
    pub observation: RecoveryObservation,
    /// Whether the restored root re-read its facts from durable storage, as opposed to the
    /// process simply coming back up.
    pub durable_recovery: bool,
    /// The pre-fault fence token. A restore may never reissue it.
    pub superseded_fence_token: FenceTokenId,
    /// Authority epoch the restored root came up under.
    pub authority_epoch: u64,
    /// Data epoch the restored root came up under.
    pub data_epoch: u64,
    /// A command was dispatched against the restored root under a pre-fault lease.
    pub stale_dispatch_observed: bool,
    /// The pre-fault lease still effects after the restore.
    pub old_lease_still_effective: bool,
    /// The pre-fault fence token is still accepted after the restore.
    pub old_fence_token_still_effective: bool,
    pub measurement_digest: String,
}

impl RecoveryMeasurement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rehearsal_id: impl Into<String>,
        scenario: RecoveryFaultScenario,
        fixture: impl Into<String>,
        objective_digest: impl Into<String>,
        evidence: RecoveryRehearsalEvidence,
        source_cursor: u64,
        restored_root_cursor: u64,
        recovered_cursor: u64,
        observed_data_loss_ms: u64,
        observed_recovery_ms: u64,
        observation: RecoveryObservation,
        durable_recovery: bool,
        superseded_fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        stale_dispatch_observed: bool,
        old_lease_still_effective: bool,
        old_fence_token_still_effective: bool,
    ) -> Result<Self, String> {
        let mut measurement = Self {
            schema: RECOVERY_MEASUREMENT_SCHEMA.to_owned(),
            version: RECOVERY_REHEARSAL_VERSION,
            rehearsal_id: rehearsal_id.into(),
            scenario,
            fixture: fixture.into(),
            objective_digest: objective_digest.into(),
            evidence,
            source_cursor,
            restored_root_cursor,
            recovered_cursor,
            observed_data_loss_ms,
            observed_recovery_ms,
            observation,
            durable_recovery,
            superseded_fence_token,
            authority_epoch,
            data_epoch,
            stale_dispatch_observed,
            old_lease_still_effective,
            old_fence_token_still_effective,
            measurement_digest: String::new(),
        };
        measurement.measurement_digest = measurement.digest();
        measurement.validate()?;
        Ok(measurement)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RECOVERY_MEASUREMENT_SCHEMA
            || !self.version.is_compatible_with(&RECOVERY_REHEARSAL_VERSION)
            || self.superseded_fence_token.as_uuid().is_nil()
        {
            return Err("recovery_measurement_header_invalid".to_owned());
        }
        safe_text(&self.rehearsal_id, "recovery_measurement_rehearsal_id")?;
        safe_text(&self.fixture, "recovery_measurement_fixture")?;
        valid_digest(
            &self.objective_digest,
            "recovery_measurement_objective_digest",
        )?;
        self.evidence.validate()?;
        // A source with no committed cursor has nothing to lose, so a measurement against one is
        // not a measurement of anything. The other two cursors may legitimately be zero.
        if self.source_cursor == 0 {
            return Err("recovery_measurement_cursor_invalid".to_owned());
        }
        if self.authority_epoch == 0 || self.data_epoch == 0 {
            return Err("recovery_measurement_epoch_invalid".to_owned());
        }
        valid_digest(&self.measurement_digest, "recovery_measurement_digest")?;
        if self.measurement_digest != self.digest() {
            return Err("recovery_measurement_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "rehearsal_id": self.rehearsal_id,
            "scenario": self.scenario,
            "fixture": self.fixture,
            "objective_digest": self.objective_digest,
            "evidence": self.evidence,
            "source_cursor": self.source_cursor,
            "restored_root_cursor": self.restored_root_cursor,
            "recovered_cursor": self.recovered_cursor,
            "observed_data_loss_ms": self.observed_data_loss_ms,
            "observed_recovery_ms": self.observed_recovery_ms,
            "observation": self.observation,
            "durable_recovery": self.durable_recovery,
            "superseded_fence_token": self.superseded_fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "stale_dispatch_observed": self.stale_dispatch_observed,
            "old_lease_still_effective": self.old_lease_still_effective,
            "old_fence_token_still_effective": self.old_fence_token_still_effective,
        }))
    }
}

/// The recovery point the restore actually reached, against the one that was declared.
///
/// A declared RPO is a promise about lost history. A rehearsal that lost more than the promise
/// allows has missed the objective no matter how the promise was written down, and the gap is
/// reported rather than absorbed.
pub fn evaluate_rpo(
    objective: &RecoveryObjective,
    measurement: &RecoveryMeasurement,
) -> Result<(), String> {
    objective.validate()?;
    measurement.validate()?;
    if measurement.observed_data_loss_ms > objective.rpo_max_data_loss_ms {
        return Err("recovery_rpo_objective_exceeded".to_owned());
    }
    Ok(())
}

/// The recovery time the restore actually took, against the one that was declared.
pub fn evaluate_rto(
    objective: &RecoveryObjective,
    measurement: &RecoveryMeasurement,
) -> Result<(), String> {
    objective.validate()?;
    measurement.validate()?;
    if measurement.observed_recovery_ms > objective.rto_max_recovery_ms {
        return Err("recovery_rto_objective_exceeded".to_owned());
    }
    Ok(())
}

/// Where the restored root resumed, against the two cursors it must sit between.
///
/// A restore is allowed to lose history; that is what the RPO budgets. It is not allowed to
/// invent history, so the recovered cursor may never sit past the source's own, and it may never
/// sit behind the position the quarantined root already held -- a resume that goes *backwards*
/// past a verified checkpoint is a second loss that no RPO budget covers.
pub fn evaluate_cursor_monotonicity(measurement: &RecoveryMeasurement) -> Result<(), String> {
    measurement.validate()?;
    if measurement.recovered_cursor > measurement.source_cursor {
        return Err("recovery_cursor_ahead_of_source".to_owned());
    }
    if measurement.recovered_cursor < measurement.restored_root_cursor {
        return Err("recovery_cursor_regressed".to_owned());
    }
    Ok(())
}

/// The single-writer state the restore left behind.
///
/// This is an inventory of what the adapter saw, checked before anything the rehearsal says about
/// itself. A stale dispatch, a live old lease or a still-accepted fence token is a writer that
/// survives the restore, and a run that observed one has already failed regardless of the
/// numbers it goes on to report.
pub fn evaluate_fence_state(measurement: &RecoveryMeasurement) -> Result<(), String> {
    measurement.validate()?;
    if measurement.stale_dispatch_observed {
        return Err("recovery_stale_dispatch_after_restore".to_owned());
    }
    if measurement.old_lease_still_effective {
        return Err("recovery_stale_lease_still_effective".to_owned());
    }
    if measurement.old_fence_token_still_effective {
        return Err("recovery_stale_fence_token_still_effective".to_owned());
    }
    Ok(())
}

/// Whether the run is bound to anything at all.
///
/// A rehearsal that reports its own result, with no command behind it, no exit status and no
/// statement of what it did not prove, is a number rather than evidence. The exit code and the
/// limitations are required here rather than in `validate` so that "the run happened" and "the
/// run is evidence" stay two different questions with two different answers.
pub fn evaluate_evidence_completeness(measurement: &RecoveryMeasurement) -> Result<(), String> {
    measurement.validate()?;
    if measurement.evidence.command_exit_code.is_none() {
        return Err("recovery_evidence_exit_code_missing".to_owned());
    }
    if unstated(&measurement.evidence.limitations) {
        return Err("recovery_evidence_limitations_missing".to_owned());
    }
    Ok(())
}

/// Whether the run measured a durable recovery rather than a process that came back up.
///
/// Coming back up is not a restore. A run that observed success without re-reading its facts from
/// durable storage is a restart wearing a restore's name, and it is refused here. `ResultUnknown`
/// is deliberately *not* promoted by this rule: an unknown outcome stays unknown, because deciding
/// what happened is DEP-25's job and guessing is worse than admitting it.
pub fn evaluate_durability(measurement: &RecoveryMeasurement) -> Result<(), String> {
    measurement.validate()?;
    if measurement.observation == RecoveryObservation::Succeeded && !measurement.durable_recovery {
        return Err("recovery_restart_not_durable".to_owned());
    }
    if measurement.observation != RecoveryObservation::Succeeded {
        return Err("recovery_rehearsal_outcome_not_succeeded".to_owned());
    }
    Ok(())
}

/// The sealed decision: did this rehearsal meet the objective it was measured against?
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRehearsalReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub rehearsal_id: String,
    pub scenario: RecoveryFaultScenario,
    pub status: RecoveryObjectiveStatus,
    pub observation: RecoveryObservation,
    pub durable_recovery: bool,
    pub measured_data_loss_ms: u64,
    pub allowed_data_loss_ms: u64,
    pub measured_recovery_ms: u64,
    pub allowed_recovery_ms: u64,
    pub source_cursor: u64,
    pub recovered_cursor: u64,
    /// Stable denial code, empty on a within-objective decision.
    pub reason: String,
    pub remediation: String,
    pub objective_digest: String,
    pub measurement_digest: String,
    pub report_digest: String,
}

impl RecoveryRehearsalReport {
    pub fn evaluate(
        objective: &RecoveryObjective,
        measurement: &RecoveryMeasurement,
    ) -> Result<Self, String> {
        objective.validate()?;
        measurement.validate()?;
        if measurement.objective_digest != objective.objective_digest {
            return Err("recovery_measurement_objective_binding_invalid".to_owned());
        }
        let (status, reason, remediation) = derive(objective, measurement);
        let mut report = Self {
            schema: RECOVERY_REHEARSAL_REPORT_SCHEMA.to_owned(),
            version: RECOVERY_REHEARSAL_VERSION,
            rehearsal_id: measurement.rehearsal_id.clone(),
            scenario: measurement.scenario,
            status,
            observation: measurement.observation,
            durable_recovery: measurement.durable_recovery,
            measured_data_loss_ms: measurement.observed_data_loss_ms,
            allowed_data_loss_ms: objective.rpo_max_data_loss_ms,
            measured_recovery_ms: measurement.observed_recovery_ms,
            allowed_recovery_ms: objective.rto_max_recovery_ms,
            source_cursor: measurement.source_cursor,
            recovered_cursor: measurement.recovered_cursor,
            reason,
            remediation,
            objective_digest: objective.objective_digest.clone(),
            measurement_digest: measurement.measurement_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(objective, measurement)?;
        Ok(report)
    }

    /// The checks a report can make about itself, with nothing else in hand. The ledger uses this
    /// so a recorded report is at least internally coherent before it is stored.
    pub fn validate_self(&self) -> Result<(), String> {
        if self.schema != RECOVERY_REHEARSAL_REPORT_SCHEMA
            || !self.version.is_compatible_with(&RECOVERY_REHEARSAL_VERSION)
        {
            return Err("recovery_rehearsal_report_header_invalid".to_owned());
        }
        safe_text(&self.rehearsal_id, "recovery_rehearsal_report_rehearsal_id")?;
        // A within-objective decision names no reason and a missed one always names one. Without
        // this a report could present itself as a plain success and hide which rule it hit.
        if (self.status == RecoveryObjectiveStatus::WithinObjective) != self.reason.is_empty() {
            return Err("recovery_rehearsal_report_reason_incoherent".to_owned());
        }
        if self.reason.len() > MAX_RECOVERY_TEXT || self.remediation.len() > MAX_RECOVERY_TEXT {
            return Err("recovery_rehearsal_report_text_too_long".to_owned());
        }
        valid_digest(
            &self.objective_digest,
            "recovery_rehearsal_report_objective_digest",
        )?;
        valid_digest(
            &self.measurement_digest,
            "recovery_rehearsal_report_measurement_digest",
        )?;
        valid_digest(&self.report_digest, "recovery_rehearsal_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("recovery_rehearsal_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        objective: &RecoveryObjective,
        measurement: &RecoveryMeasurement,
    ) -> Result<(), String> {
        self.validate_self()?;
        objective.validate()?;
        measurement.validate()?;
        let (status, reason, remediation) = derive(objective, measurement);
        if self.rehearsal_id != measurement.rehearsal_id
            || self.scenario != measurement.scenario
            || self.status != status
            || self.observation != measurement.observation
            || self.durable_recovery != measurement.durable_recovery
            || self.measured_data_loss_ms != measurement.observed_data_loss_ms
            || self.allowed_data_loss_ms != objective.rpo_max_data_loss_ms
            || self.measured_recovery_ms != measurement.observed_recovery_ms
            || self.allowed_recovery_ms != objective.rto_max_recovery_ms
            || self.source_cursor != measurement.source_cursor
            || self.recovered_cursor != measurement.recovered_cursor
            || self.reason != reason
            || self.remediation != remediation
            || self.objective_digest != objective.objective_digest
            || self.measurement_digest != measurement.measurement_digest
        {
            return Err("recovery_rehearsal_report_binding_invalid".to_owned());
        }
        Ok(())
    }

    /// The single gate a caller should ask. A report that misses its objective is never
    /// publishable, however small the miss was.
    pub fn within_objective(&self) -> bool {
        self.status == RecoveryObjectiveStatus::WithinObjective
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "rehearsal_id": self.rehearsal_id,
            "scenario": self.scenario,
            "status": self.status,
            "observation": self.observation,
            "durable_recovery": self.durable_recovery,
            "measured_data_loss_ms": self.measured_data_loss_ms,
            "allowed_data_loss_ms": self.allowed_data_loss_ms,
            "measured_recovery_ms": self.measured_recovery_ms,
            "allowed_recovery_ms": self.allowed_recovery_ms,
            "source_cursor": self.source_cursor,
            "recovered_cursor": self.recovered_cursor,
            "reason": self.reason,
            "remediation": self.remediation,
            "objective_digest": self.objective_digest,
            "measurement_digest": self.measurement_digest,
        }))
    }
}

/// Decide in a fixed order, so the reported reason is the first violated rule and therefore the
/// same for the same facts.
///
/// The first group is *positive observations* about the restored root: a dispatch that already
/// happened under a pre-fault lease, a writer that still holds its old lease, a fence token that
/// is still accepted, a cursor in a position no source ever had, a resume behind a checkpoint that
/// was already verified. Each is a live writer or an invented fact rather than an absence of
/// proof, so each outranks what follows. Then come the two honesty invariants -- a restart is not
/// a restore, and a run that did not succeed measures nothing -- which no operator intent can
/// waive. Finally the absences of proof, ordered by how cheap the caller's fix is: fill in the
/// exit code, state the limitations, or re-measure against a target the run actually met.
fn derive(
    objective: &RecoveryObjective,
    measurement: &RecoveryMeasurement,
) -> (RecoveryObjectiveStatus, String, String) {
    if let Err(reason) = evaluate_fence_state(measurement) {
        return missed(&reason);
    }
    if let Err(reason) = evaluate_cursor_monotonicity(measurement) {
        return missed(&reason);
    }
    if let Err(reason) = evaluate_durability(measurement) {
        return missed(&reason);
    }
    if let Err(reason) = evaluate_evidence_completeness(measurement) {
        return missed(&reason);
    }
    if let Err(reason) = evaluate_rpo(objective, measurement) {
        return missed(&reason);
    }
    if let Err(reason) = evaluate_rto(objective, measurement) {
        return missed(&reason);
    }
    (
        RecoveryObjectiveStatus::WithinObjective,
        String::new(),
        "none".to_owned(),
    )
}

fn missed(reason: &str) -> (RecoveryObjectiveStatus, String, String) {
    (
        RecoveryObjectiveStatus::ObjectiveMissed,
        reason.to_owned(),
        remediation_for(reason).to_owned(),
    )
}

/// The remediation is tied to the rule, not to the run, so two rehearsals that miss the same
/// objective are handed the same next step.
fn remediation_for(reason: &str) -> &'static str {
    match reason {
        "recovery_stale_dispatch_after_restore" => {
            "reconcile the post-restore dispatch under a fresh lease before recording a rehearsal"
        }
        "recovery_stale_lease_still_effective" => {
            "fence the pre-fault lease before measuring the restore"
        }
        "recovery_stale_fence_token_still_effective" => {
            "burn the pre-fault fence token; a restore may not reissue it"
        }
        "recovery_cursor_ahead_of_source" => {
            "the recovered cursor cannot exceed the source cursor; re-read it from the restored root"
        }
        "recovery_cursor_regressed" => {
            "the restored root resumed behind its own verified checkpoint; resume forward instead"
        }
        "recovery_restart_not_durable" => {
            "re-read the facts from durable storage; a process restart is not a recovery"
        }
        "recovery_rehearsal_outcome_not_succeeded" => {
            "the rehearsal did not succeed, so it measures no RPO and no RTO"
        }
        "recovery_evidence_exit_code_missing" => {
            "capture the command's exit code; a run without one is not reproducible"
        }
        "recovery_evidence_limitations_missing" => {
            "state what the run did not prove; an unstated limitation is not a clean result"
        }
        "recovery_rpo_objective_exceeded" => {
            "the restore lost more history than the RPO allows; tighten the backup cadence or restate the objective"
        }
        "recovery_rto_objective_exceeded" => {
            "the restore took longer than the RTO allows; stage the root ahead of the fault or restate the objective"
        }
        _ => "the rehearsal did not satisfy a recovery objective rule",
    }
}

/// The recorded history of rehearsals, so a periodic run cannot be quietly replaced.
///
/// A rehearsal is repeated evidence, and repeated evidence is exactly where a rewrite hides. One
/// identity therefore carries one decision: replaying the identical report is a no-op, and a
/// different decision under the same identity is a contradiction rather than an update.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRehearsalLedger {
    pub schema: String,
    pub version: SchemaVersion,
    pub reports: Vec<RecoveryRehearsalReport>,
    pub ledger_digest: String,
}

impl RecoveryRehearsalLedger {
    pub fn new(reports: Vec<RecoveryRehearsalReport>) -> Result<Self, String> {
        let mut ledger = Self {
            schema: RECOVERY_REHEARSAL_LEDGER_SCHEMA.to_owned(),
            version: RECOVERY_REHEARSAL_VERSION,
            reports,
            ledger_digest: String::new(),
        };
        ledger.ledger_digest = ledger.digest();
        ledger.validate()?;
        Ok(ledger)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RECOVERY_REHEARSAL_LEDGER_SCHEMA
            || !self.version.is_compatible_with(&RECOVERY_REHEARSAL_VERSION)
            || self.reports.len() > MAX_RECOVERY_REHEARSALS
        {
            return Err("recovery_rehearsal_ledger_header_invalid".to_owned());
        }
        let mut identities = BTreeSet::new();
        for report in &self.reports {
            report.validate_self()?;
            if !identities.insert(report.rehearsal_id.as_str()) {
                return Err("recovery_rehearsal_ledger_duplicate_rehearsal".to_owned());
            }
        }
        valid_digest(&self.ledger_digest, "recovery_rehearsal_ledger_digest")?;
        if self.ledger_digest != self.digest() {
            return Err("recovery_rehearsal_ledger_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn record(&self, report: &RecoveryRehearsalReport) -> Result<Self, String> {
        self.validate()?;
        report.validate_self()?;
        if self.reports.len() >= MAX_RECOVERY_REHEARSALS
            && !self
                .reports
                .iter()
                .any(|held| held.rehearsal_id == report.rehearsal_id)
        {
            return Err("recovery_rehearsal_ledger_full".to_owned());
        }
        if let Some(held) = self
            .reports
            .iter()
            .find(|held| held.rehearsal_id == report.rehearsal_id)
        {
            if held.report_digest != report.report_digest {
                return Err("recovery_rehearsal_identity_digest_conflict".to_owned());
            }
            return Ok(self.clone());
        }
        let mut ledger = self.clone();
        ledger.reports.push(report.clone());
        ledger.ledger_digest = ledger.digest();
        ledger.validate()?;
        Ok(ledger)
    }

    /// The most recently recorded decision, if there is one.
    pub fn latest(&self) -> Option<&RecoveryRehearsalReport> {
        self.reports.last()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "reports": self.reports,
        }))
    }
}

/// Whether a limitations field states anything. A placeholder is not a statement: writing
/// "none" in the one field reserved for what the run did not prove is how an unproven run gets
/// published as a clean one.
fn unstated(value: &str) -> bool {
    let lowered = value.trim().to_ascii_lowercase();
    lowered.is_empty()
        || matches!(
            lowered.as_str(),
            "none" | "n/a" | "na" | "nil" | "tbd" | "unknown" | "-" | "--"
        )
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_RECOVERY_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("://")
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
