//! DEP-24 core facade: a recovery rehearsal decision bound to the lease the restored root is
//! actually serving under.
//!
//! The domain contract in `kiana_domain::recovery_objective` decides whether a measured rehearsal
//! met its declared RPO and RTO. That decision is about numbers, and numbers do not say who may
//! write. This module adds the one thing the domain deliberately leaves out: the single-writer
//! state the restore left behind, taken as a real `OperationLease` rather than as a boolean an
//! adapter set.
//!
//! It is the same read-only shape DEP-22 and DEP-23 use. Nothing here kills a process, restores a
//! root, starts a timer, takes an OS lock, appends an event or dispatches a capability. The lease
//! is carried as a value that a later step must persist and revalidate at the effect boundary,
//! exactly as `OperationLeaseCas` documents for itself.

use kiana_domain::{
    json_digest, FenceTokenId, OperationLease, OperationLeaseState, RecoveryMeasurement,
    RecoveryObjective, RecoveryRehearsalLedger, RecoveryRehearsalReport, SchemaVersion,
    StorageLockId, StorageRootId,
};

pub const RECOVERY_REHEARSAL_OUTCOME_SCHEMA: &str = "kiana.recovery-rehearsal-outcome.v1";
pub const RECOVERY_REHEARSAL_CORE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The single-writer state the restored root came up under, as the adapter observed it.
///
/// DEP-24 never mints this. It is supplied, and the checks below are the ones a rehearsal has to
/// survive before its numbers may be published.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryLeaseObservation {
    pub storage_root: StorageRootId,
    /// The lease the restored root is serving under after the fault.
    pub lease: OperationLease,
    /// Whether the restored root reached the readiness gate the rehearsal measured against.
    pub restored_ready: bool,
}

impl RecoveryLeaseObservation {
    pub fn new(
        storage_root: StorageRootId,
        lease: OperationLease,
        restored_ready: bool,
    ) -> Result<Self, String> {
        let observation = Self {
            storage_root,
            lease,
            restored_ready,
        };
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.storage_root.as_uuid().is_nil() {
            return Err("recovery_rehearsal_observation_header_invalid".to_owned());
        }
        self.lease
            .validate()
            .map_err(|_| "recovery_rehearsal_lease_invalid".to_owned())?;
        // A lease quoted against one root while the observation names another is not evidence
        // about anything, so the two are compared rather than trusted.
        if self.lease.storage_root != self.storage_root {
            return Err("recovery_rehearsal_lease_root_mismatch".to_owned());
        }
        Ok(())
    }
}

/// The single place a caller asks whether a rehearsal met its objective.
pub fn evaluate_recovery_rehearsal(
    objective: &RecoveryObjective,
    measurement: &RecoveryMeasurement,
) -> Result<RecoveryRehearsalReport, String> {
    RecoveryRehearsalReport::evaluate(objective, measurement)
}

/// Whether the restored root is in a state where a rehearsal's numbers could mean anything.
///
/// Ordered so the reported reason is the first violated rule: a root that never became ready, a
/// lease that is not the active one, a fence token the pre-fault writer still recognises, and only
/// then the two epochs that no operator intent can waive. A restore that measures a perfect RPO
/// and leaves no serving lease has not recovered anything.
pub fn evaluate_post_recovery_lease(
    measurement: &RecoveryMeasurement,
    observation: &RecoveryLeaseObservation,
) -> Result<(), String> {
    measurement.validate()?;
    observation.validate()?;
    if !observation.restored_ready {
        return Err("recovery_rehearsal_restored_root_not_ready".to_owned());
    }
    if observation.lease.state != OperationLeaseState::Active {
        return Err("recovery_rehearsal_lease_not_active".to_owned());
    }
    if observation.lease.fence_token == measurement.superseded_fence_token {
        return Err("recovery_rehearsal_fence_token_reused".to_owned());
    }
    if observation.lease.authority_epoch <= measurement.authority_epoch {
        return Err("recovery_rehearsal_authority_epoch_not_advanced".to_owned());
    }
    if observation.lease.data_epoch <= measurement.data_epoch {
        return Err("recovery_rehearsal_data_epoch_not_advanced".to_owned());
    }
    Ok(())
}

/// The sealed record of one rehearsal: the decision, plus the lease the restored root is actually
/// serving under.
///
/// A report on its own is a claim about a run. An outcome is that claim joined to a lease, and it
/// is publishable only when both halves hold: the rehearsal met its declared objectives, and the
/// root it measured is the one now holding the single-writer lease under a fresh fence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryRehearsalOutcome {
    pub schema: String,
    pub version: SchemaVersion,
    pub rehearsal_id: String,
    pub report: RecoveryRehearsalReport,
    pub storage_root: StorageRootId,
    pub lease_id: StorageLockId,
    pub fence_token: FenceTokenId,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    /// Whether this outcome may be published as rehearsal evidence.
    pub publishable: bool,
    pub outcome_digest: String,
}

impl RecoveryRehearsalOutcome {
    /// Seal a rehearsal. The lease checks run first, so a restore that left no serving root is
    /// refused before its RPO and RTO are even derived.
    pub fn seal(
        objective: &RecoveryObjective,
        measurement: &RecoveryMeasurement,
        observation: &RecoveryLeaseObservation,
    ) -> Result<Self, String> {
        evaluate_post_recovery_lease(measurement, observation)?;
        let report = RecoveryRehearsalReport::evaluate(objective, measurement)?;
        let mut outcome = Self {
            schema: RECOVERY_REHEARSAL_OUTCOME_SCHEMA.to_owned(),
            version: RECOVERY_REHEARSAL_CORE_VERSION,
            rehearsal_id: measurement.rehearsal_id.clone(),
            publishable: report.within_objective(),
            report,
            storage_root: observation.storage_root,
            lease_id: observation.lease.lease_id,
            fence_token: observation.lease.fence_token,
            authority_epoch: observation.lease.authority_epoch,
            data_epoch: observation.lease.data_epoch,
            outcome_digest: String::new(),
        };
        outcome.outcome_digest = outcome.digest();
        outcome.validate_against(objective, measurement, observation)?;
        Ok(outcome)
    }

    /// Re-derive the whole outcome from the same three inputs. A stored outcome whose decision,
    /// lease binding or publish flag no longer follows from the facts is refused rather than
    /// trusted, so a rewrite cannot be laundered through a recomputed digest.
    pub fn validate_against(
        &self,
        objective: &RecoveryObjective,
        measurement: &RecoveryMeasurement,
        observation: &RecoveryLeaseObservation,
    ) -> Result<(), String> {
        if self.schema != RECOVERY_REHEARSAL_OUTCOME_SCHEMA
            || !self
                .version
                .is_compatible_with(&RECOVERY_REHEARSAL_CORE_VERSION)
        {
            return Err("recovery_rehearsal_outcome_header_invalid".to_owned());
        }
        evaluate_post_recovery_lease(measurement, observation)?;
        let report = RecoveryRehearsalReport::evaluate(objective, measurement)?;
        if self.rehearsal_id != measurement.rehearsal_id
            || self.report != report
            || self.storage_root != observation.storage_root
            || self.lease_id != observation.lease.lease_id
            || self.fence_token != observation.lease.fence_token
            || self.authority_epoch != observation.lease.authority_epoch
            || self.data_epoch != observation.lease.data_epoch
        {
            return Err("recovery_rehearsal_outcome_binding_invalid".to_owned());
        }
        // The publish flag is derived, never declared. An outcome that misses its objective is
        // not publishable however small the miss was, and an outcome that met it is not withheld.
        if self.publishable != report.within_objective() {
            return Err("recovery_rehearsal_outcome_publish_flag_invalid".to_owned());
        }
        valid_digest(&self.outcome_digest, "recovery_rehearsal_outcome_digest")?;
        if self.outcome_digest != self.digest() {
            return Err("recovery_rehearsal_outcome_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The single gate. Nothing downstream should read the report without asking this first.
    pub fn may_publish(&self) -> bool {
        self.publishable
    }

    /// Append this outcome to a rehearsal ledger. The domain ledger owns replay protection, so a
    /// repeated identity with a different decision is a contradiction rather than an update.
    pub fn record(
        &self,
        ledger: &RecoveryRehearsalLedger,
    ) -> Result<RecoveryRehearsalLedger, String> {
        ledger.record(&self.report)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "rehearsal_id": self.rehearsal_id,
            "report_digest": self.report.report_digest,
            "storage_root": self.storage_root,
            "lease_id": self.lease_id,
            "fence_token": self.fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "publishable": self.publishable,
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
