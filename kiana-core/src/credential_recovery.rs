//! EventLog-backed credential/config recovery projection.
//!
//! Recovery is a read-only fold over committed, redacted facts.  It does not refresh a token,
//! consume a lease or dispatch a provider call.  A caller must append a fresh
//! `ReAdmissionAuthorized` fact after the fold has established that all bindings are current.

use kiana_domain::{
    validate_runtime_event, CredentialLeaseProjectionState, CredentialRecoveryBlocker,
    CredentialRecoveryEventKind, CredentialRecoveryFact, CredentialRecoveryProjection,
    CredentialRecoveryStatus, EventId, RuntimeEvent,
};
use std::collections::{BTreeSet, HashSet};
use thiserror::Error;

pub const CREDENTIAL_RECOVERY_PROJECTION_VERSION: u64 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialRecoveryReplayRequest {
    pub run_ref: String,
    pub principal_digest: String,
    pub assignment_digest: String,
    pub config_snapshot_digest: String,
    pub credential_ref_digest: String,
    pub audit_projection_digest: String,
    pub redaction_profile_digest: String,
    pub generation: u64,
    pub authority_epoch: u64,
    pub config_revision: String,
    pub credential_revision: String,
    /// If set, facts at or before this position are from the pre-restart process and cannot
    /// authorize resume.  The index is deliberately caller-supplied and not treated as an
    /// authority value from the event payload.
    pub restart_after_index: Option<usize>,
}

impl CredentialRecoveryReplayRequest {
    pub fn validate(&self) -> Result<(), CredentialRecoveryProjectionError> {
        if self.run_ref.trim().is_empty() || self.run_ref.len() > 256 {
            return Err(CredentialRecoveryProjectionError::RequestInvalid(
                "run_ref".to_owned(),
            ));
        }
        if self.generation == 0 || self.authority_epoch == 0 {
            return Err(CredentialRecoveryProjectionError::RequestInvalid(
                "epoch".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum CredentialRecoveryProjectionError {
    #[error("credential_recovery_source_empty")]
    SourceEmpty,
    #[error("credential_recovery_event_id_duplicate:{0}")]
    EventIdDuplicate(EventId),
    #[error("credential_recovery_event_invalid:{0}")]
    EventInvalid(String),
    #[error("credential_recovery_request_invalid:{0}")]
    RequestInvalid(String),
    #[error("credential_recovery_run_mismatch")]
    RunMismatch,
    #[error("credential_recovery_replay_conflict:{0}")]
    ReplayConflict(String),
    #[error("credential_recovery_projection_invalid:{0}")]
    ProjectionInvalid(String),
    #[error("credential_recovery_admission_invalid:{0}")]
    AdmissionInvalid(String),
}

fn add_binding_blockers(
    blockers: &mut BTreeSet<CredentialRecoveryBlocker>,
    fact: &CredentialRecoveryFact,
    request: &CredentialRecoveryReplayRequest,
) {
    if fact.authority_epoch != request.authority_epoch {
        blockers.insert(CredentialRecoveryBlocker::StaleAuthorityEpoch);
    }
    if fact.config_revision != request.config_revision
        || fact.config_snapshot_digest != request.config_snapshot_digest
    {
        blockers.insert(CredentialRecoveryBlocker::StaleConfigRevision);
    }
    if fact.credential_revision != request.credential_revision {
        blockers.insert(CredentialRecoveryBlocker::StaleCredentialRevision);
    }
    if fact.generation != request.generation {
        blockers.insert(CredentialRecoveryBlocker::StaleCredentialGeneration);
    }
    if fact.credential_ref_digest != request.credential_ref_digest {
        blockers.insert(CredentialRecoveryBlocker::StaleCredentialRevision);
    }
    if fact.audit_projection_digest != request.audit_projection_digest {
        blockers.insert(CredentialRecoveryBlocker::AuditBindingInvalid);
    }
    if fact.redaction_profile_digest != request.redaction_profile_digest {
        blockers.insert(CredentialRecoveryBlocker::RedactionBindingInvalid);
    }
}

fn source_fact(
    event: &RuntimeEvent,
) -> Result<Option<CredentialRecoveryFact>, CredentialRecoveryProjectionError> {
    if event.kind == kiana_domain::CREDENTIAL_RECOVERY_EVENT_KIND {
        return CredentialRecoveryFact::from_runtime_event(event)
            .map(Some)
            .map_err(CredentialRecoveryProjectionError::EventInvalid);
    }
    if event.kind.starts_with("recovery.") {
        validate_runtime_event(event).map_err(CredentialRecoveryProjectionError::EventInvalid)?;
    }
    Ok(None)
}

/// Fold the committed recovery facts in EventLog order.
pub fn project_credential_recovery(
    events: &[RuntimeEvent],
    request: &CredentialRecoveryReplayRequest,
) -> Result<CredentialRecoveryProjection, CredentialRecoveryProjectionError> {
    request.validate()?;
    let mut seen = HashSet::new();
    let mut facts = Vec::new();
    for event in events {
        let Some(fact) = source_fact(event)? else {
            continue;
        };
        if !seen.insert(event.event_id) {
            return Err(CredentialRecoveryProjectionError::EventIdDuplicate(
                event.event_id,
            ));
        }
        facts.push((facts.len(), fact));
    }
    if facts.is_empty() {
        return Err(CredentialRecoveryProjectionError::SourceEmpty);
    }

    let (_, first) = &facts[0];
    if first.run_ref != request.run_ref {
        return Err(CredentialRecoveryProjectionError::RunMismatch);
    }
    let mut previous = first;
    for (_, fact) in facts.iter().skip(1) {
        if fact.run_ref != request.run_ref {
            return Err(CredentialRecoveryProjectionError::RunMismatch);
        }
        if fact.principal_digest != first.principal_digest
            || fact.assignment_digest != first.assignment_digest
        {
            return Err(CredentialRecoveryProjectionError::ReplayConflict(
                "recovery_identity_binding_changed".to_owned(),
            ));
        }
        if fact.generation < previous.generation {
            return Err(CredentialRecoveryProjectionError::ReplayConflict(
                "credential_generation_regressed".to_owned(),
            ));
        }
        let transition = matches!(
            fact.event,
            CredentialRecoveryEventKind::CredentialRotated
                | CredentialRecoveryEventKind::CredentialRevoked
                | CredentialRecoveryEventKind::ReAdmissionAuthorized
        );
        if !transition
            && (fact.config_snapshot_digest != previous.config_snapshot_digest
                || fact.credential_ref_digest != previous.credential_ref_digest
                || fact.audit_projection_digest != previous.audit_projection_digest
                || fact.redaction_profile_digest != previous.redaction_profile_digest
                || fact.config_revision != previous.config_revision
                || fact.credential_revision != previous.credential_revision)
        {
            return Err(CredentialRecoveryProjectionError::ReplayConflict(
                "recovery_binding_changed_without_transition".to_owned(),
            ));
        }
        previous = fact;
    }

    let (latest_index, latest) = facts.last().expect("facts is non-empty");
    let mut blockers = BTreeSet::new();
    add_binding_blockers(&mut blockers, latest, request);
    if latest.lease_state != CredentialLeaseProjectionState::Active {
        blockers.insert(CredentialRecoveryBlocker::LeaseMissing);
    }
    if latest.event == CredentialRecoveryEventKind::CredentialRefreshFailed {
        blockers.insert(CredentialRecoveryBlocker::CredentialRefreshFailed);
    }
    if latest.result_unknown || latest.event == CredentialRecoveryEventKind::ResultUnknown {
        blockers.insert(CredentialRecoveryBlocker::ResultUnknown);
    }

    let restart_index = request.restart_after_index;
    let last_restart = facts
        .iter()
        .rev()
        .find(|(_, fact)| fact.event == CredentialRecoveryEventKind::RestartDetected)
        .map(|(index, _)| *index);
    let last_admission = facts
        .iter()
        .rev()
        .find(|(_, fact)| fact.event == CredentialRecoveryEventKind::ReAdmissionAuthorized)
        .map(|(index, _)| *index);
    let restart_boundary = restart_index.or(last_restart);
    let explicit_re_admission = latest.explicit_re_admission
        && last_admission
            .is_some_and(|admission| restart_boundary.is_none_or(|restart| admission > restart));
    let resume_authorized = blockers.is_empty()
        && explicit_re_admission
        && latest.lease_state == CredentialLeaseProjectionState::Active;
    let status = if resume_authorized {
        CredentialRecoveryStatus::Ready
    } else if blockers.contains(&CredentialRecoveryBlocker::ResultUnknown) {
        CredentialRecoveryStatus::Unknown
    } else if !blockers.is_empty() {
        CredentialRecoveryStatus::Blocked
    } else {
        CredentialRecoveryStatus::ReAdmissionRequired
    };

    let projection = CredentialRecoveryProjection::new(
        latest.run_ref.clone(),
        latest.principal_digest.clone(),
        latest.assignment_digest.clone(),
        latest.config_snapshot_digest.clone(),
        latest.credential_ref_digest.clone(),
        latest.audit_projection_digest.clone(),
        latest.redaction_profile_digest.clone(),
        latest.generation,
        latest.authority_epoch,
        latest.config_revision.clone(),
        latest.credential_revision.clone(),
        latest.lease_state,
        blockers.into_iter().collect(),
        status,
        explicit_re_admission && resume_authorized,
        resume_authorized,
        latest.result_unknown,
    )
    .map_err(CredentialRecoveryProjectionError::ProjectionInvalid)?;

    if let Some(restart_index) = restart_index {
        if restart_index > *latest_index {
            return Err(CredentialRecoveryProjectionError::ReplayConflict(
                "restart_boundary_after_replay_source".to_owned(),
            ));
        }
    }
    Ok(projection)
}

/// Explicitly admit a recovered projection after a fresh server-owned fact was committed.
pub fn explicit_re_admit_credential_recovery(
    projection: &CredentialRecoveryProjection,
    fact: &CredentialRecoveryFact,
) -> Result<CredentialRecoveryProjection, CredentialRecoveryProjectionError> {
    projection
        .explicit_re_admit(fact)
        .map_err(CredentialRecoveryProjectionError::AdmissionInvalid)
}
