//! NM-10 ControlPlane admission and single-consume fixtures (deny first, success last).
//!
//! The gate is pure: it returns a typed admission and never mutates the target, appends a fact or
//! dispatches a capability. The ledger is the in-process ControlPlane-side authority that consumes
//! one admission once, records applied/failed/unknown separately from delivery ACK, and wakes at
//! most one wait key. Every fact below is synthesized in memory; no provider/channel is involved.

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{
    ControlPlane, HumanTaskActionLedger, HumanTaskActionLedgerError, NotificationActionGate,
    NotificationActionGateError, NotificationActionTarget,
};
use kiana_domain::{
    ActionRef, ApprovalChallenge, ApprovalId, CapabilityRequest, MessageId, Notification,
    NotificationActionAuthorityEffect, NotificationActionCommand, NotificationActionDecision,
    NotificationActionKind, NotificationActionOutcome, NotificationActionWaitKey,
    NotificationChannel, PendingApproval, RequestContext,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::Arc;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn fixture_for(action: NotificationActionKind) -> (NotificationActionCommand, ActionRef) {
    let notification = Notification::new(
        MessageId::new(),
        "actor-10",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        3,
        None,
    )
    .unwrap();
    let action_ref = ActionRef::new(
        action.command(),
        3,
        vec![format!("notification:{}", notification.notification_id)],
        100,
        200,
    )
    .unwrap();
    let command = NotificationActionCommand::new(
        action_ref.clone(),
        action,
        notification.notification_id,
        "actor-10",
        3,
        notification.notification_digest,
        7,
        11,
        json!({"reason":"fixture"}),
        "actor-10",
        "notification-action-10",
    )
    .unwrap();
    (command, action_ref)
}

fn fixture() -> (NotificationActionCommand, ActionRef) {
    fixture_for(NotificationActionKind::Review)
}

fn server_target(command: &NotificationActionCommand) -> NotificationActionTarget {
    NotificationActionTarget::new(
        3,
        command.expected_target_digest.clone(),
        command.expected_criteria_digest.clone(),
        7,
        11,
    )
    .unwrap()
}

fn admit(
    command: &NotificationActionCommand,
    action_ref: &ActionRef,
) -> kiana_core::NotificationActionAdmission {
    NotificationActionGate::admit(
        command,
        action_ref,
        "actor-10",
        &server_target(command),
        150,
    )
    .unwrap()
}

#[test]
fn gate_refuses_stale_target_wrong_decider_criteria_epoch_cursor_and_unknown_action() {
    let (command, action_ref) = fixture();
    let good = server_target(&command);
    assert!(NotificationActionGate::admit(&command, &action_ref, "actor-10", &good, 150).is_ok());

    // Wrong decider: the server-derived recipient is the only accepted submitter.
    assert_eq!(
        NotificationActionGate::admit(&command, &action_ref, "actor-11", &good, 150),
        Err(NotificationActionGateError::RecipientMismatch)
    );
    let mut foreign = command.clone();
    foreign.submitted_by = "actor-11".to_owned();
    foreign.command_digest = foreign.digest();
    assert_eq!(
        NotificationActionGate::admit(&foreign, &action_ref, "actor-10", &good, 150),
        Err(NotificationActionGateError::RecipientMismatch)
    );

    // A different but valid ref id can never stand in for the authoritative ref.
    let other_ref = ActionRef::new(
        "notification.review",
        3,
        command.action_ref.scope.clone(),
        100,
        200,
    )
    .unwrap();
    assert_eq!(
        NotificationActionGate::admit(&command, &other_ref, "actor-10", &good, 150),
        Err(NotificationActionGateError::RefNotAuthoritative)
    );

    // An unregistered action name is refused before any target comparison.
    let unknown_ref = ActionRef::new(
        "notification.bogus",
        3,
        command.action_ref.scope.clone(),
        100,
        200,
    )
    .unwrap();
    assert_eq!(
        NotificationActionGate::admit(&command, &unknown_ref, "actor-10", &good, 150),
        Err(NotificationActionGateError::UnknownAction)
    );

    let stale_revision =
        NotificationActionTarget::new(4, command.expected_target_digest.clone(), None, 7, 11)
            .unwrap();
    assert_eq!(
        NotificationActionGate::admit(&command, &action_ref, "actor-10", &stale_revision, 150),
        Err(NotificationActionGateError::RevisionConflict)
    );
    let stale_digest = NotificationActionTarget::new(3, digest('c'), None, 7, 11).unwrap();
    assert_eq!(
        NotificationActionGate::admit(&command, &action_ref, "actor-10", &stale_digest, 150),
        Err(NotificationActionGateError::DigestConflict)
    );
    let stale_criteria = NotificationActionTarget::new(
        3,
        command.expected_target_digest.clone(),
        Some(digest('d')),
        7,
        11,
    )
    .unwrap();
    assert_eq!(
        NotificationActionGate::admit(&command, &action_ref, "actor-10", &stale_criteria, 150),
        Err(NotificationActionGateError::CriteriaConflict)
    );
    let stale_epoch =
        NotificationActionTarget::new(3, command.expected_target_digest.clone(), None, 8, 11)
            .unwrap();
    assert_eq!(
        NotificationActionGate::admit(&command, &action_ref, "actor-10", &stale_epoch, 150),
        Err(NotificationActionGateError::AuthorityConflict)
    );
    let stale_cursor =
        NotificationActionTarget::new(3, command.expected_target_digest.clone(), None, 7, 12)
            .unwrap();
    assert_eq!(
        NotificationActionGate::admit(&command, &action_ref, "actor-10", &stale_cursor, 150),
        Err(NotificationActionGateError::CursorConflict)
    );

    // Freshness window is [created_at, expires_at): both edges are refused.
    assert_eq!(
        NotificationActionGate::admit(&command, &action_ref, "actor-10", &good, 99),
        Err(NotificationActionGateError::Expired)
    );
    assert_eq!(
        NotificationActionGate::admit(&command, &action_ref, "actor-10", &good, 200),
        Err(NotificationActionGateError::Expired)
    );

    // Malformed target/command input fails closed before any admission exists.
    assert_eq!(
        NotificationActionTarget::new(0, digest('a'), None, 7, 11).unwrap_err(),
        "notification_action_target_header_invalid"
    );
    assert_eq!(
        NotificationActionTarget::new(3, "not-a-digest".to_owned(), None, 7, 11).unwrap_err(),
        "notification_action_target_digest_invalid"
    );
    let mut blank_submitter = command.clone();
    blank_submitter.submitted_by = " ".to_owned();
    assert_eq!(
        blank_submitter.validate().unwrap_err(),
        "notification_action_submitted_by_invalid"
    );
}

#[test]
fn gate_accepts_only_authoritative_fresh_target_and_returns_no_effect() {
    let (command, action_ref) = fixture();
    let admission = NotificationActionGate::admit(
        &command,
        &action_ref,
        "actor-10",
        &server_target(&command),
        150,
    )
    .unwrap();
    assert!(admission.control_plane_required);
    assert!(!admission.direct_effect);
    admission.validate().unwrap();
    assert_eq!(admission.action, NotificationActionKind::Review);
    assert_eq!(admission.action_ref_id, action_ref.action_ref_id);
    assert_eq!(admission.command_digest, command.command_digest);

    let mut forged = admission.clone();
    forged.direct_effect = true;
    assert_eq!(
        forged.validate().unwrap_err(),
        "notification_action_admission_invalid"
    );

    // A criteria-bound command passes only when the server target serves the same criteria.
    let criteria = digest('e');
    let bound = command.with_criteria_digest(criteria.clone()).unwrap();
    let bound_target = NotificationActionTarget::new(
        3,
        bound.expected_target_digest.clone(),
        Some(criteria),
        7,
        11,
    )
    .unwrap();
    let bound_admission =
        NotificationActionGate::admit(&bound, &action_ref, "actor-10", &bound_target, 150).unwrap();
    assert_eq!(bound_admission.command_digest, bound.command_digest);
}

#[test]
fn ledger_consumes_once_and_replays_the_first_decision() {
    let (command, action_ref) = fixture();
    let target = server_target(&command);
    let admission = admit(&command, &action_ref);
    let mut ledger = HumanTaskActionLedger::new();

    let first = ledger
        .consume(
            &admission,
            &command,
            &action_ref,
            "actor-10",
            &target,
            NotificationActionOutcome::Applied,
            None,
            150,
        )
        .unwrap();
    assert!(!first.replayed);
    assert!(first.wake.is_none());
    assert_eq!(first.decision.outcome, NotificationActionOutcome::Applied);
    assert_eq!(first.decision.source_cursor, 11);
    assert_eq!(first.decision.decider_principal_id, "actor-10");
    assert_eq!(ledger.decision_count(), 1);

    // Replaying the same command digest returns the first outcome and never a second decision.
    let replay = ledger
        .consume(
            &admission,
            &command,
            &action_ref,
            "actor-10",
            &target,
            NotificationActionOutcome::Unknown,
            Some("notification_action_result_unknown".to_owned()),
            151,
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.decision, first.decision);
    assert!(replay.wake.is_none());
    assert_eq!(ledger.decision_count(), 1);

    // Same idempotency key with a different command digest is a conflict, not a replay.
    let conflicting = NotificationActionCommand::new(
        command.action_ref.clone(),
        NotificationActionKind::Review,
        command.notification_id,
        "actor-10",
        3,
        command.expected_target_digest.clone(),
        7,
        11,
        json!({"reason":"different"}),
        "actor-10",
        command.idempotency_key.clone(),
    )
    .unwrap();
    let conflicting_admission = admit(&conflicting, &action_ref);
    assert_eq!(
        ledger.consume(
            &conflicting_admission,
            &conflicting,
            &action_ref,
            "actor-10",
            &target,
            NotificationActionOutcome::Applied,
            None,
            150,
        ),
        Err(HumanTaskActionLedgerError::IdempotencyConflict)
    );

    // Any other second command for the same action ref is a double consume.
    let another = NotificationActionCommand::new(
        command.action_ref.clone(),
        NotificationActionKind::Review,
        command.notification_id,
        "actor-10",
        3,
        command.expected_target_digest.clone(),
        7,
        11,
        json!({"reason":"another"}),
        "actor-10",
        "notification-action-other",
    )
    .unwrap();
    let another_admission = admit(&another, &action_ref);
    assert_eq!(
        ledger.consume(
            &another_admission,
            &another,
            &action_ref,
            "actor-10",
            &target,
            NotificationActionOutcome::Applied,
            None,
            150,
        ),
        Err(HumanTaskActionLedgerError::AlreadyConsumed)
    );

    // A forged admission that does not match the freshly recomputed one is refused.
    let mut forged = another_admission.clone();
    forged.command_digest = command.command_digest.clone();
    assert_eq!(
        ledger.consume(
            &forged,
            &another,
            &action_ref,
            "actor-10",
            &target,
            NotificationActionOutcome::Applied,
            None,
            150,
        ),
        Err(HumanTaskActionLedgerError::AdmissionMismatch)
    );

    // A stale target or an impossible outcome leaves a fresh ledger untouched.
    let (fresh_command, fresh_ref) = fixture();
    let fresh_target = server_target(&fresh_command);
    let fresh_admission = admit(&fresh_command, &fresh_ref);
    let mut fresh = HumanTaskActionLedger::new();
    let stale =
        NotificationActionTarget::new(4, fresh_command.expected_target_digest.clone(), None, 7, 11)
            .unwrap();
    assert_eq!(
        fresh.consume(
            &fresh_admission,
            &fresh_command,
            &fresh_ref,
            "actor-10",
            &stale,
            NotificationActionOutcome::Applied,
            None,
            150,
        ),
        Err(HumanTaskActionLedgerError::Gate(
            NotificationActionGateError::RevisionConflict
        ))
    );
    assert_eq!(fresh.decision_count(), 0);
    assert_eq!(
        fresh.consume(
            &fresh_admission,
            &fresh_command,
            &fresh_ref,
            "actor-10",
            &fresh_target,
            NotificationActionOutcome::Applied,
            Some("notification_action_failed".to_owned()),
            150,
        ),
        Err(HumanTaskActionLedgerError::Invalid(
            "notification_action_decision_applied_with_code".to_owned()
        ))
    );
    assert_eq!(fresh.decision_count(), 0);
    assert_eq!(fresh.wait_key_count(), 0);
}

#[test]
fn ledger_wakes_exactly_one_wait_key_per_decision() {
    let (command, action_ref) = fixture();
    let target = server_target(&command);
    let key = NotificationActionWaitKey::new(
        "wait-activation-1",
        action_ref.action_ref_id,
        command.notification_id,
        3,
        command.expected_target_digest.clone(),
        7,
        11,
        120,
    )
    .unwrap();
    let mut ledger = HumanTaskActionLedger::new();
    ledger
        .register_wait_key(key.clone(), &action_ref, &target, 130)
        .unwrap();
    // Re-registering the identical binding is idempotent.
    ledger
        .register_wait_key(key.clone(), &action_ref, &target, 131)
        .unwrap();
    assert_eq!(ledger.wait_key_count(), 1);
    assert!(ledger.pending_wait_key(action_ref.action_ref_id).is_some());

    let admission = admit(&command, &action_ref);
    let first = ledger
        .consume(
            &admission,
            &command,
            &action_ref,
            "actor-10",
            &target,
            NotificationActionOutcome::Applied,
            None,
            130,
        )
        .unwrap();
    let wake = first.wake.expect("exactly one wake");
    assert_eq!(wake.wait_key, "wait-activation-1");
    assert_eq!(wake.action_ref_id, action_ref.action_ref_id);
    assert_eq!(wake.notification_id, command.notification_id);
    assert_eq!(wake.source_cursor, 11);
    assert!(ledger.pending_wait_key(action_ref.action_ref_id).is_none());
    assert_eq!(ledger.wait_key_count(), 0);

    // The replay returns the same decision but wakes nothing a second time.
    let replay = ledger
        .consume(
            &admission,
            &command,
            &action_ref,
            "actor-10",
            &target,
            NotificationActionOutcome::Applied,
            None,
            131,
        )
        .unwrap();
    assert!(replay.replayed);
    assert!(replay.wake.is_none());

    // A consumed ref cannot register another wait key.
    assert_eq!(
        ledger.register_wait_key(key, &action_ref, &target, 140),
        Err(HumanTaskActionLedgerError::WaitKeyConsumed)
    );

    // Two distinct wait keys for one ref are ambiguous before any decision exists.
    let (command2, ref2) = fixture();
    let target2 = server_target(&command2);
    let key_a = NotificationActionWaitKey::new(
        "wait-a",
        ref2.action_ref_id,
        command2.notification_id,
        3,
        command2.expected_target_digest.clone(),
        7,
        11,
        120,
    )
    .unwrap();
    let key_b = NotificationActionWaitKey::new(
        "wait-b",
        ref2.action_ref_id,
        command2.notification_id,
        3,
        command2.expected_target_digest.clone(),
        7,
        11,
        121,
    )
    .unwrap();
    let mut ledger2 = HumanTaskActionLedger::new();
    ledger2
        .register_wait_key(key_a, &ref2, &target2, 130)
        .unwrap();
    assert_eq!(
        ledger2.register_wait_key(key_b, &ref2, &target2, 131),
        Err(HumanTaskActionLedgerError::WaitKeyAmbiguous)
    );

    // The same wait key cannot bind a second action ref.
    let (command3, ref3) = fixture();
    let target3 = server_target(&command3);
    let reused = NotificationActionWaitKey::new(
        "wait-a",
        ref3.action_ref_id,
        command3.notification_id,
        3,
        command3.expected_target_digest.clone(),
        7,
        11,
        122,
    )
    .unwrap();
    assert_eq!(
        ledger2.register_wait_key(reused, &ref3, &target3, 132),
        Err(HumanTaskActionLedgerError::WaitKeyConflict)
    );

    // A wait key bound to a stale target refuses the decision even though the command is fresh.
    let (command4, ref4) = fixture();
    let key4 = NotificationActionWaitKey::new(
        "wait-d",
        ref4.action_ref_id,
        command4.notification_id,
        3,
        command4.expected_target_digest.clone(),
        7,
        11,
        120,
    )
    .unwrap();
    let target4 = server_target(&command4);
    let mut ledger4 = HumanTaskActionLedger::new();
    ledger4
        .register_wait_key(key4, &ref4, &target4, 130)
        .unwrap();
    let drifted_digest = digest('f');
    let drifted_command = NotificationActionCommand::new(
        ref4.clone(),
        NotificationActionKind::Review,
        command4.notification_id,
        "actor-10",
        3,
        drifted_digest.clone(),
        7,
        11,
        json!({"reason":"drifted"}),
        "actor-10",
        "notification-action-drift",
    )
    .unwrap();
    let drifted_target = NotificationActionTarget::new(3, drifted_digest, None, 7, 11).unwrap();
    let drifted_admission =
        NotificationActionGate::admit(&drifted_command, &ref4, "actor-10", &drifted_target, 150)
            .unwrap();
    assert_eq!(
        ledger4.consume(
            &drifted_admission,
            &drifted_command,
            &ref4,
            "actor-10",
            &drifted_target,
            NotificationActionOutcome::Applied,
            None,
            150,
        ),
        Err(HumanTaskActionLedgerError::WaitKeyBindingMismatch)
    );
    assert_eq!(ledger4.decision_count(), 0);
    assert_eq!(ledger4.wait_key_count(), 1);

    // Clock regression and registration after expiry fail closed without writing a key.
    let (command5, ref5) = fixture();
    let target5 = server_target(&command5);
    let key5 = NotificationActionWaitKey::new(
        "wait-e",
        ref5.action_ref_id,
        command5.notification_id,
        3,
        command5.expected_target_digest.clone(),
        7,
        11,
        120,
    )
    .unwrap();
    let mut ledger5 = HumanTaskActionLedger::new();
    assert_eq!(
        ledger5.register_wait_key(key5.clone(), &ref5, &target5, 119),
        Err(HumanTaskActionLedgerError::Invalid(
            "notification_action_wait_key_clock_regression".to_owned()
        ))
    );
    assert_eq!(
        ledger5.register_wait_key(key5, &ref5, &target5, 200),
        Err(HumanTaskActionLedgerError::Invalid(
            "notification_action_wait_key_expired".to_owned()
        ))
    );
    assert_eq!(ledger5.wait_key_count(), 0);
}

#[test]
fn delivery_and_read_ack_never_forge_an_authority_decision() {
    let (ack, action_ref) = fixture_for(NotificationActionKind::Acknowledge);
    let target = server_target(&ack);
    let admission = admit(&ack, &action_ref);
    let mut ledger = HumanTaskActionLedger::new();
    let consumption = ledger
        .consume(
            &admission,
            &ack,
            &action_ref,
            "actor-10",
            &target,
            NotificationActionOutcome::Applied,
            None,
            150,
        )
        .unwrap();
    assert_eq!(
        consumption.decision.authority_effect,
        NotificationActionAuthorityEffect::Presentation
    );
    assert_eq!(
        consumption.decision.outcome,
        NotificationActionOutcome::Applied
    );
    assert!(consumption.wake.is_none());

    let encoded = serde_json::to_value(&consumption.decision).unwrap();
    let keys = encoded.as_object().unwrap().keys().collect::<Vec<_>>();
    for forbidden in [
        "approved",
        "decision",
        "delivered",
        "acknowledged",
        "read",
        "task_status",
    ] {
        assert!(
            !keys.iter().any(|key| key.as_str() == forbidden),
            "decision carries a delivery/read field: {forbidden}"
        );
    }
    for (key, value) in [
        ("approved", json!(true)),
        ("delivered", json!(true)),
        ("acknowledged", json!(true)),
        ("read", json!(true)),
        ("task_status", json!("approved")),
    ] {
        let mut forged = encoded.clone();
        forged[key] = value;
        assert!(
            serde_json::from_value::<NotificationActionDecision>(forged).is_err(),
            "forged field must not decode into a decision: {key}"
        );
    }
}

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

struct UnavailableApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnavailableApprovalStore {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }
}

fn plane() -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        Arc::new(MemoryEventLog::new()),
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnavailableApprovalStore),
        Arc::new(UnavailableRunner),
    )
}

#[test]
fn control_plane_facade_admits_only_bound_notification_actions() {
    let plane = plane();
    let (command, action_ref) = fixture();
    let target = server_target(&command);

    let admission = plane
        .admit_notification_action(&command, &action_ref, "actor-10", &target, 150)
        .unwrap();
    assert_eq!(
        admission,
        NotificationActionGate::admit(&command, &action_ref, "actor-10", &target, 150).unwrap()
    );
    assert!(admission.control_plane_required);
    assert!(!admission.direct_effect);

    assert_eq!(
        plane.admit_notification_action(&command, &action_ref, "actor-11", &target, 150),
        Err(NotificationActionGateError::RecipientMismatch)
    );
    let stale =
        NotificationActionTarget::new(4, command.expected_target_digest.clone(), None, 7, 11)
            .unwrap();
    assert_eq!(
        plane.admit_notification_action(&command, &action_ref, "actor-10", &stale, 150),
        Err(NotificationActionGateError::RevisionConflict)
    );
    let unknown_ref = ActionRef::new(
        "notification.bogus",
        3,
        command.action_ref.scope.clone(),
        100,
        200,
    )
    .unwrap();
    assert_eq!(
        plane.admit_notification_action(&command, &unknown_ref, "actor-10", &target, 150),
        Err(NotificationActionGateError::UnknownAction)
    );
}
