use kiana_core::{
    explicit_re_admit_credential_recovery, project_credential_recovery,
    CredentialRecoveryReplayRequest,
};
use kiana_domain::{
    CredentialLeaseProjectionState, CredentialRecoveryBlocker, CredentialRecoveryEventKind,
    CredentialRecoveryFact, CredentialRecoveryStatus, RequestId,
};
use serde_json::json;

const PRINCIPAL: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ASSIGNMENT: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CONFIG: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CREDENTIAL: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const AUDIT: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const REDACTION: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn fact(event: CredentialRecoveryEventKind, authority_epoch: u64) -> CredentialRecoveryFact {
    CredentialRecoveryFact::new(
        "run:ci11",
        PRINCIPAL,
        ASSIGNMENT,
        CONFIG,
        CREDENTIAL,
        AUDIT,
        REDACTION,
        2,
        authority_epoch,
        "config:2",
        "credential:2",
        if event == CredentialRecoveryEventKind::CredentialRevoked {
            CredentialLeaseProjectionState::Revoked
        } else {
            CredentialLeaseProjectionState::Active
        },
        event,
    )
    .expect("fixture fact")
}

fn event(sequence: u64, fact: &CredentialRecoveryFact) -> kiana_domain::RuntimeEvent {
    fact.to_runtime_event(RequestId::new(), sequence)
        .expect("fixture event")
}

fn request(
    authority_epoch: u64,
    restart_after_index: Option<usize>,
) -> CredentialRecoveryReplayRequest {
    CredentialRecoveryReplayRequest {
        run_ref: "run:ci11".to_owned(),
        principal_digest: PRINCIPAL.to_owned(),
        assignment_digest: ASSIGNMENT.to_owned(),
        config_snapshot_digest: CONFIG.to_owned(),
        credential_ref_digest: CREDENTIAL.to_owned(),
        audit_projection_digest: AUDIT.to_owned(),
        redaction_profile_digest: REDACTION.to_owned(),
        generation: 2,
        authority_epoch,
        config_revision: "config:2".to_owned(),
        credential_revision: "credential:2".to_owned(),
        restart_after_index,
    }
}

#[test]
fn committed_rotation_then_explicit_admission_is_the_only_ready_path() {
    let rotated = fact(CredentialRecoveryEventKind::CredentialRotated, 3);
    let admitted = fact(CredentialRecoveryEventKind::ReAdmissionAuthorized, 3);
    let projection = project_credential_recovery(
        &[event(1, &rotated), event(2, &admitted)],
        &request(3, None),
    )
    .expect("projection");
    assert_eq!(projection.status, CredentialRecoveryStatus::Ready);
    assert!(projection.resume_authorized);
    projection.validate().expect("validated projection");
}

#[test]
fn restart_forces_re_admission_even_when_the_old_lease_was_active() {
    let rotated = fact(CredentialRecoveryEventKind::CredentialRotated, 3);
    let restart = fact(CredentialRecoveryEventKind::RestartDetected, 3);
    let projection = project_credential_recovery(
        &[event(1, &rotated), event(2, &restart)],
        &request(3, Some(0)),
    )
    .expect("paused projection");
    assert_eq!(
        projection.status,
        CredentialRecoveryStatus::ReAdmissionRequired
    );
    assert!(!projection.resume_authorized);
}

#[test]
fn stale_epoch_and_refresh_failure_are_blockers_not_resume_signals() {
    let failed = fact(CredentialRecoveryEventKind::CredentialRefreshFailed, 3);
    let projection = project_credential_recovery(&[event(1, &failed)], &request(4, None))
        .expect("blocked projection");
    assert_eq!(projection.status, CredentialRecoveryStatus::Blocked);
    assert!(!projection.resume_authorized);
    assert!(projection
        .blockers
        .contains(&CredentialRecoveryBlocker::StaleAuthorityEpoch));
    assert!(projection
        .blockers
        .contains(&CredentialRecoveryBlocker::CredentialRefreshFailed));
}

#[test]
fn result_unknown_remains_unknown_and_unknown_recovery_schema_fails_closed() {
    let unknown = fact(CredentialRecoveryEventKind::ResultUnknown, 3);
    let projection = project_credential_recovery(&[event(1, &unknown)], &request(3, None))
        .expect("unknown projection");
    assert_eq!(projection.status, CredentialRecoveryStatus::Unknown);
    assert!(projection
        .blockers
        .contains(&CredentialRecoveryBlocker::ResultUnknown));
    assert!(!projection.resume_authorized);

    let mut forged = event(2, &unknown);
    forged.data["recovery"]["schema"] = json!("kiana.credential-recovery-event.v99");
    let error = project_credential_recovery(&[forged], &request(3, None)).unwrap_err();
    assert!(error.to_string().contains("event_invalid"));
}

#[test]
fn explicit_re_admission_api_rejects_old_or_blocked_projection() {
    let rotated = fact(CredentialRecoveryEventKind::CredentialRotated, 3);
    let paused = kiana_domain::CredentialRecoveryProjection::paused_from_fact(&rotated)
        .expect("paused projection");
    let admitted = fact(CredentialRecoveryEventKind::ReAdmissionAuthorized, 3);
    let ready = explicit_re_admit_credential_recovery(&paused, &admitted).expect("admitted");
    assert_eq!(ready.status, CredentialRecoveryStatus::Ready);

    let failed = fact(CredentialRecoveryEventKind::CredentialRefreshFailed, 3);
    let blocked = project_credential_recovery(&[event(1, &failed)], &request(3, None))
        .expect("blocked projection");
    assert!(explicit_re_admit_credential_recovery(&blocked, &admitted).is_err());
}
