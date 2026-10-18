use kiana_domain::{
    plan_billing_recovery, reconcile_attempt, BillingAttemptDisposition, BillingAttemptSnapshot,
    BillingBootFence, BillingReconciliation, BillingReconciliationOutcome, BillingRecoveryAction,
    BillingRecoveryApplyOutcome, BillingRecoveryFact, BillingRecoveryLedger, BillingRecoveryPlan,
    BillingRecoveryRequest, BillingRecoveryState, BILLING_RECOVERY_SCHEMA,
};
use serde_json::json;

fn digest(fact: &BillingRecoveryFact) -> String {
    kiana_domain::json_digest(&json!({
        "schema": fact.schema,
        "attempt_id": fact.attempt_id,
        "reservation_digest": fact.reservation_digest,
        "source_cursor": fact.source_cursor,
        "authority_epoch": fact.authority_epoch,
        "lease_epoch": fact.lease_epoch,
        "state": fact.state,
        "usage_unknown": fact.usage_unknown,
        "continue_resets_usage": fact.continue_resets_usage,
        "settlement_count": fact.settlement_count,
        "reconciliation_ref": fact.reconciliation_ref,
    }))
}

fn fact(state: BillingRecoveryState) -> BillingRecoveryFact {
    let mut fact = BillingRecoveryFact {
        schema: BILLING_RECOVERY_SCHEMA.to_owned(),
        attempt_id: "attempt:billing-1".to_owned(),
        reservation_digest:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        source_cursor: 42,
        authority_epoch: 7,
        lease_epoch: 3,
        state,
        usage_unknown: matches!(state, BillingRecoveryState::NeedsReconciliation),
        continue_resets_usage: false,
        settlement_count: u8::from(matches!(state, BillingRecoveryState::Recovered)),
        reconciliation_ref: match state {
            BillingRecoveryState::NeedsReconciliation | BillingRecoveryState::Recovered => {
                Some("reconcile:billing-1".to_owned())
            }
            BillingRecoveryState::Paused => None,
        },
        recovery_digest: String::new(),
    };
    fact.recovery_digest = digest(&fact);
    fact
}

#[test]
fn recovery_states_preserve_unknown_until_explicit_reconciliation() {
    assert!(fact(BillingRecoveryState::Paused).validate().is_ok());
    assert!(fact(BillingRecoveryState::NeedsReconciliation)
        .validate()
        .is_ok());
    assert!(fact(BillingRecoveryState::Recovered).validate().is_ok());
}

#[test]
fn continue_never_resets_usage_and_unknown_cannot_be_success() {
    let mut invalid = fact(BillingRecoveryState::NeedsReconciliation);
    invalid.continue_resets_usage = true;
    assert_eq!(invalid.validate(), Err("billing_recovery_fact_invalid"));

    let mut invalid = fact(BillingRecoveryState::Recovered);
    invalid.usage_unknown = true;
    invalid.recovery_digest = digest(&invalid);
    assert_eq!(invalid.validate(), Err("billing_recovery_fact_invalid"));
}

#[test]
fn old_epoch_or_digest_tampering_fails_closed_and_unknown_fields_are_denied() {
    let mut invalid = fact(BillingRecoveryState::NeedsReconciliation);
    invalid.lease_epoch = 0;
    assert_eq!(invalid.validate(), Err("billing_recovery_fact_invalid"));

    let mut value = serde_json::to_value(fact(BillingRecoveryState::Paused)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<BillingRecoveryFact>(value).is_err());

    let mut repeated = fact(BillingRecoveryState::Recovered);
    repeated.settlement_count = 2;
    repeated.recovery_digest = digest(&repeated);
    assert_eq!(repeated.validate(), Err("billing_recovery_fact_invalid"));
}

const RESERVATION_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SOURCE_EVENT_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RECONCILED_EVENT_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ATTEMPT: &str = "attempt:billing-1";

fn boot_fence() -> BillingBootFence {
    BillingBootFence::new(9, 7, 3, 42, SOURCE_EVENT_DIGEST).expect("boot fence")
}

/// The fence of the process that boots after the reconciliation was committed: its cursor and
/// source event are the reconciled ones, not the pre-reconciliation page.
fn settled_fence() -> BillingBootFence {
    BillingBootFence::new(9, 7, 3, 43, RECONCILED_EVENT_DIGEST).expect("settled boot fence")
}

fn snapshot(disposition: BillingAttemptDisposition) -> BillingAttemptSnapshot {
    let reconciliation_ref = match disposition {
        BillingAttemptDisposition::Consumed => Some("reconcile:settle-1".to_owned()),
        BillingAttemptDisposition::Released => Some("reconcile:release-1".to_owned()),
        _ => None,
    };
    BillingAttemptSnapshot::new(
        ATTEMPT,
        RESERVATION_DIGEST,
        disposition,
        120,
        reconciliation_ref,
        SOURCE_EVENT_DIGEST,
        42,
    )
    .expect("attempt snapshot")
}

fn request(action: BillingRecoveryAction, accumulated_units: u64) -> BillingRecoveryRequest {
    BillingRecoveryRequest::new(
        action,
        ATTEMPT,
        RESERVATION_DIGEST,
        9,
        7,
        3,
        accumulated_units,
        SOURCE_EVENT_DIGEST,
    )
    .expect("recovery request")
}

/// The same assertion, bound to the reconciled source event a restarted caller reads.
fn settled_request(
    action: BillingRecoveryAction,
    accumulated_units: u64,
) -> BillingRecoveryRequest {
    BillingRecoveryRequest::new(
        action,
        ATTEMPT,
        RESERVATION_DIGEST,
        9,
        7,
        3,
        accumulated_units,
        RECONCILED_EVENT_DIGEST,
    )
    .expect("settled recovery request")
}

fn reconciliation(
    outcome: BillingReconciliationOutcome,
    usage_known: bool,
    consumed_units: u64,
) -> BillingReconciliation {
    BillingReconciliation::new(
        ATTEMPT,
        RESERVATION_DIGEST,
        SOURCE_EVENT_DIGEST,
        42,
        7,
        3,
        "reconcile:billing-1",
        usage_known,
        consumed_units,
        outcome,
        RECONCILED_EVENT_DIGEST,
        43,
    )
    .expect("reconciliation")
}

#[test]
fn restart_never_redispatches_a_completed_attempt() {
    for disposition in [
        BillingAttemptDisposition::Consumed,
        BillingAttemptDisposition::Released,
    ] {
        let completed = snapshot(disposition);
        for action in [
            BillingRecoveryAction::Restart,
            BillingRecoveryAction::Continue,
        ] {
            assert_eq!(
                plan_billing_recovery(&completed, &boot_fence(), &request(action, 120)),
                Err("billing_recovery_attempt_already_terminal".to_owned()),
                "a completed attempt must not be re-dispatched: {action:?}"
            );
        }
        let replay = plan_billing_recovery(
            &completed,
            &boot_fence(),
            &request(BillingRecoveryAction::Replay, 120),
        )
        .expect("replay rebuilds the committed decision");
        assert!(!replay.redispatch_permitted);
        assert_eq!(
            replay.settlements,
            u8::from(matches!(disposition, BillingAttemptDisposition::Consumed))
        );
    }
}

#[test]
fn continue_never_resets_accumulated_usage() {
    let in_flight = snapshot(BillingAttemptDisposition::InFlight);
    assert_eq!(
        plan_billing_recovery(
            &in_flight,
            &boot_fence(),
            &request(BillingRecoveryAction::Continue, 119)
        ),
        Err("billing_recovery_continue_resets_accumulation".to_owned())
    );
    let carried = plan_billing_recovery(
        &in_flight,
        &boot_fence(),
        &request(BillingRecoveryAction::Continue, 120),
    )
    .expect("continue keeps the accumulation");
    assert_eq!(carried.state, BillingRecoveryState::Paused);
    assert_eq!(carried.accumulated_units, 120);
    assert!(carried.reservation_held);
    assert_eq!(carried.settlements, 0);
    assert!(!carried.retry_permitted);
}

#[test]
fn stale_boot_lease_and_authority_epochs_cannot_continue_settlement() {
    let in_flight = snapshot(BillingAttemptDisposition::InFlight);
    let fence = boot_fence();

    let mut stale_boot = request(BillingRecoveryAction::Continue, 120);
    stale_boot.boot_generation = 8;
    stale_boot.request_digest = stale_boot.digest();
    assert_eq!(
        plan_billing_recovery(&in_flight, &fence, &stale_boot),
        Err("billing_recovery_boot_generation_stale".to_owned())
    );

    let mut stale_authority = request(BillingRecoveryAction::Continue, 120);
    stale_authority.authority_epoch = 6;
    stale_authority.request_digest = stale_authority.digest();
    assert_eq!(
        plan_billing_recovery(&in_flight, &fence, &stale_authority),
        Err("billing_recovery_authority_epoch_stale".to_owned())
    );

    let mut stale_lease = request(BillingRecoveryAction::Continue, 120);
    stale_lease.lease_epoch = 2;
    stale_lease.request_digest = stale_lease.digest();
    assert_eq!(
        plan_billing_recovery(&in_flight, &fence, &stale_lease),
        Err("billing_recovery_lease_epoch_stale".to_owned())
    );

    let mut future_epoch = request(BillingRecoveryAction::Continue, 120);
    future_epoch.lease_epoch = 4;
    future_epoch.request_digest = future_epoch.digest();
    assert_eq!(
        plan_billing_recovery(&in_flight, &fence, &future_epoch),
        Err("billing_recovery_epoch_ahead".to_owned())
    );

    let unknown = snapshot(BillingAttemptDisposition::Unknown);
    let mut stale_reconcile = reconciliation(BillingReconciliationOutcome::Consumed, true, 120);
    stale_reconcile.lease_epoch = 2;
    stale_reconcile.reconciliation_digest = stale_reconcile.digest();
    assert_eq!(
        reconcile_attempt(&unknown, &fence, &stale_reconcile),
        Err("billing_recovery_lease_epoch_stale".to_owned())
    );
}

#[test]
fn unknown_attempts_default_to_needs_reconciliation_and_never_auto_succeed() {
    let unknown = snapshot(BillingAttemptDisposition::Unknown);
    let fence = boot_fence();

    let restart = plan_billing_recovery(
        &unknown,
        &fence,
        &request(BillingRecoveryAction::Restart, 120),
    )
    .expect("restart defaults to reconciliation");
    assert_eq!(restart.state, BillingRecoveryState::NeedsReconciliation);
    assert!(restart.usage_unknown);
    assert!(restart.reservation_held);
    assert_eq!(restart.settlements, 0);
    assert!(restart.reconciliation_ref.is_none());
    assert!(!restart.retry_permitted);

    let replay = plan_billing_recovery(
        &unknown,
        &fence,
        &request(BillingRecoveryAction::Replay, 120),
    )
    .expect("replay defaults to reconciliation");
    assert_eq!(replay.state, BillingRecoveryState::NeedsReconciliation);

    assert_eq!(
        plan_billing_recovery(
            &unknown,
            &fence,
            &request(BillingRecoveryAction::Continue, 120)
        ),
        Err("billing_recovery_unknown_requires_reconciliation".to_owned())
    );
    assert_eq!(
        plan_billing_recovery(
            &unknown,
            &fence,
            &request(BillingRecoveryAction::Reconcile, 120)
        ),
        Err("billing_recovery_reconcile_requires_evidence".to_owned())
    );
    assert_eq!(
        reconcile_attempt(
            &unknown,
            &fence,
            &reconciliation(BillingReconciliationOutcome::Consumed, false, 120)
        ),
        Err("billing_recovery_unknown_cannot_be_success".to_owned())
    );
    assert_eq!(
        reconcile_attempt(
            &snapshot(BillingAttemptDisposition::InFlight),
            &fence,
            &reconciliation(BillingReconciliationOutcome::Consumed, true, 120)
        ),
        Err("billing_recovery_reconcile_requires_unknown".to_owned())
    );
    let settled = unknown
        .settled_by(
            &fence,
            &reconciliation(BillingReconciliationOutcome::Consumed, true, 120),
        )
        .expect("settled snapshot");
    assert_eq!(
        reconcile_attempt(
            &settled,
            &settled_fence(),
            &reconciliation(BillingReconciliationOutcome::Consumed, true, 120)
        ),
        Err("billing_recovery_reconcile_already_settled".to_owned())
    );
}

#[test]
fn explicit_reconciliation_settles_exactly_once() {
    let unknown = snapshot(BillingAttemptDisposition::Unknown);
    let fence = boot_fence();
    let evidence = reconciliation(BillingReconciliationOutcome::Consumed, true, 120);

    let plan = reconcile_attempt(&unknown, &fence, &evidence).expect("reconcile plan");
    assert_eq!(plan.state, BillingRecoveryState::Recovered);
    assert_eq!(plan.settlements, 1);
    assert!(!plan.reservation_held);
    assert!(!plan.usage_unknown);
    assert!(!plan.redispatch_permitted);
    assert_eq!(
        plan.reconciliation_ref.as_deref(),
        Some("reconcile:billing-1")
    );
    assert_eq!(plan.accumulated_units, 240);

    let mut ledger = BillingRecoveryLedger::default();
    assert!(matches!(
        ledger.apply(plan.clone()),
        Ok(BillingRecoveryApplyOutcome::Applied(_))
    ));
    assert!(matches!(
        ledger.apply(plan.clone()),
        Ok(BillingRecoveryApplyOutcome::Replayed(_))
    ));
    assert_eq!(ledger.settlement_count(ATTEMPT), 1);
    assert_eq!(ledger.len(), 1);

    let settled = unknown
        .settled_by(&fence, &evidence)
        .expect("settled snapshot");
    assert_eq!(settled.disposition, BillingAttemptDisposition::Consumed);
    assert_eq!(settled.settlement_count, 1);
    assert_eq!(settled.accumulated_units, 240);
    // A restarted caller reads the reconciled page: the old fence is behind that cursor and the
    // pre-reconciliation request no longer matches the committed source event.
    assert_eq!(
        plan_billing_recovery(
            &settled,
            &fence,
            &settled_request(BillingRecoveryAction::Restart, 240)
        ),
        Err("billing_recovery_source_cursor_regression".to_owned())
    );
    assert_eq!(
        plan_billing_recovery(
            &settled,
            &settled_fence(),
            &request(BillingRecoveryAction::Restart, 240)
        ),
        Err("billing_recovery_source_event_mismatch".to_owned())
    );
    assert_eq!(
        plan_billing_recovery(
            &settled,
            &settled_fence(),
            &settled_request(BillingRecoveryAction::Restart, 240)
        ),
        Err("billing_recovery_attempt_already_terminal".to_owned())
    );
    let recovered = plan_billing_recovery(
        &settled,
        &settled_fence(),
        &settled_request(BillingRecoveryAction::Replay, 240),
    )
    .expect("recovered replay");
    assert_eq!(recovered.state, BillingRecoveryState::Recovered);
    assert_eq!(recovered.settlements, 1);

    let mut second = recovered.clone();
    second.action = BillingRecoveryAction::Reconcile;
    second.plan_digest = second.digest();
    assert_eq!(
        ledger.apply(second),
        Err("billing_recovery_second_settlement".to_owned())
    );
}

#[test]
fn in_flight_reservation_stays_held_across_restart_and_replay() {
    let in_flight = snapshot(BillingAttemptDisposition::InFlight);
    let fence = boot_fence();
    let restart = plan_billing_recovery(
        &in_flight,
        &fence,
        &request(BillingRecoveryAction::Restart, 0),
    )
    .expect("restart plan");
    assert_eq!(restart.state, BillingRecoveryState::Paused);
    assert!(restart.reservation_held);
    assert_eq!(restart.settlements, 0);
    assert!(!restart.redispatch_permitted);
    assert!(!restart.retry_permitted);
    assert_eq!(restart.accumulated_units, 120);

    let mut ledger = BillingRecoveryLedger::default();
    assert!(matches!(
        ledger.apply(restart.clone()),
        Ok(BillingRecoveryApplyOutcome::Applied(_))
    ));
    assert!(matches!(
        ledger.apply(restart.clone()),
        Ok(BillingRecoveryApplyOutcome::Replayed(_))
    ));
    assert_eq!(ledger.settlement_count(ATTEMPT), 0);

    let mut released_without_evidence = restart;
    released_without_evidence.reservation_held = false;
    released_without_evidence.retry_permitted = true;
    released_without_evidence.plan_digest = released_without_evidence.digest();
    assert_eq!(
        released_without_evidence.validate(),
        Err("billing_recovery_plan_invalid".to_owned())
    );
}

#[test]
fn recovery_ledger_cannot_skip_reconciliation_or_downgrade_a_decision() {
    let fence = boot_fence();
    let mut ledger = BillingRecoveryLedger::default();

    let needs = plan_billing_recovery(
        &snapshot(BillingAttemptDisposition::Unknown),
        &fence,
        &request(BillingRecoveryAction::Restart, 120),
    )
    .expect("needs reconciliation");
    ledger.apply(needs).expect("first decision");
    let paused = plan_billing_recovery(
        &snapshot(BillingAttemptDisposition::InFlight),
        &fence,
        &request(BillingRecoveryAction::Restart, 120),
    )
    .expect("paused");
    assert_eq!(
        ledger.apply(paused),
        Err("billing_recovery_reconciliation_skipped".to_owned())
    );

    let recovered = reconcile_attempt(
        &snapshot(BillingAttemptDisposition::Unknown),
        &fence,
        &reconciliation(BillingReconciliationOutcome::Consumed, true, 10),
    )
    .expect("recovered");
    ledger.apply(recovered.clone()).expect("settlement");
    let mut downgrade = recovered;
    downgrade.state = BillingRecoveryState::Paused;
    downgrade.settlements = 0;
    downgrade.reservation_held = true;
    downgrade.reconciliation_ref = None;
    downgrade.plan_digest = downgrade.digest();
    assert_eq!(downgrade.validate(), Ok(()));
    assert_eq!(
        ledger.apply(downgrade),
        Err("billing_recovery_decision_downgrade".to_owned())
    );

    let mut other = BillingRecoveryLedger::default();
    let first = plan_billing_recovery(
        &snapshot(BillingAttemptDisposition::InFlight),
        &fence,
        &request(BillingRecoveryAction::Restart, 120),
    )
    .expect("paused");
    other.apply(first).expect("first paused decision");
    let mut conflicting = plan_billing_recovery(
        &snapshot(BillingAttemptDisposition::InFlight),
        &fence,
        &request(BillingRecoveryAction::Restart, 120),
    )
    .expect("paused");
    conflicting.accumulated_units = 121;
    conflicting.plan_digest = conflicting.digest();
    assert_eq!(
        other.apply(conflicting),
        Err("billing_recovery_replay_conflict".to_owned())
    );
}

#[test]
fn recovery_requests_and_snapshots_are_digest_and_binding_fenced() {
    let in_flight = snapshot(BillingAttemptDisposition::InFlight);
    let fence = boot_fence();

    let mut tampered = in_flight.clone();
    tampered.accumulated_units = 121;
    assert_eq!(
        tampered.validate(),
        Err("billing_recovery_snapshot_invalid".to_owned())
    );

    let mut zero_epoch_fence = fence.clone();
    zero_epoch_fence.lease_epoch = 0;
    zero_epoch_fence.fence_digest = zero_epoch_fence.digest();
    assert_eq!(
        zero_epoch_fence.validate(),
        Err("billing_recovery_boot_fence_invalid".to_owned())
    );

    let mut forged = request(BillingRecoveryAction::Restart, 120);
    forged.boot_generation = 10;
    assert_eq!(
        plan_billing_recovery(&in_flight, &fence, &forged),
        Err("billing_recovery_request_invalid".to_owned())
    );

    let mut foreign_attempt = request(BillingRecoveryAction::Restart, 120);
    foreign_attempt.attempt_id = "attempt:billing-2".to_owned();
    foreign_attempt.request_digest = foreign_attempt.digest();
    assert_eq!(
        plan_billing_recovery(&in_flight, &fence, &foreign_attempt),
        Err("billing_recovery_attempt_binding_mismatch".to_owned())
    );

    let mut foreign_source = request(BillingRecoveryAction::Restart, 120);
    foreign_source.source_event_digest = RECONCILED_EVENT_DIGEST.to_owned();
    foreign_source.request_digest = foreign_source.digest();
    assert_eq!(
        plan_billing_recovery(&in_flight, &fence, &foreign_source),
        Err("billing_recovery_source_event_mismatch".to_owned())
    );

    let mut regressed = fence.clone();
    regressed.source_cursor = 41;
    regressed.fence_digest = regressed.digest();
    assert_eq!(
        plan_billing_recovery(
            &in_flight,
            &regressed,
            &request(BillingRecoveryAction::Restart, 120)
        ),
        Err("billing_recovery_source_cursor_regression".to_owned())
    );

    let mut value = serde_json::to_value(request(BillingRecoveryAction::Restart, 120)).unwrap();
    value["auto_success"] = json!(true);
    assert!(serde_json::from_value::<BillingRecoveryRequest>(value).is_err());
}

#[test]
fn reconciliation_evidence_is_bound_and_cannot_claim_a_release_with_consumption() {
    let unknown = snapshot(BillingAttemptDisposition::Unknown);
    let fence = boot_fence();

    let mut foreign_prior = reconciliation(BillingReconciliationOutcome::Consumed, true, 120);
    foreign_prior.prior_source_event_digest = RECONCILED_EVENT_DIGEST.to_owned();
    foreign_prior.reconciliation_digest = foreign_prior.digest();
    assert_eq!(
        reconcile_attempt(&unknown, &fence, &foreign_prior),
        Err("billing_recovery_reconcile_source_mismatch".to_owned())
    );

    let mut claim = reconciliation(BillingReconciliationOutcome::Released, true, 0);
    claim.consumed_units = 5;
    claim.reconciliation_digest = claim.digest();
    assert_eq!(
        claim.validate(),
        Err("billing_recovery_release_claims_consumption".to_owned())
    );
    assert_eq!(
        reconcile_attempt(&unknown, &fence, &claim),
        Err("billing_recovery_release_claims_consumption".to_owned())
    );

    let released = reconcile_attempt(
        &unknown,
        &fence,
        &reconciliation(BillingReconciliationOutcome::Released, true, 0),
    )
    .expect("released plan");
    assert_eq!(released.state, BillingRecoveryState::Paused);
    assert!(!released.reservation_held);
    assert!(released.retry_permitted);
    assert_eq!(released.settlements, 0);
    assert_eq!(released.accumulated_units, 120);
    assert_eq!(
        released.reconciliation_ref.as_deref(),
        Some("reconcile:billing-1")
    );

    let mut released_snapshot = unknown
        .settled_by(
            &fence,
            &reconciliation(BillingReconciliationOutcome::Released, true, 0),
        )
        .expect("released snapshot");
    assert_eq!(
        released_snapshot.disposition,
        BillingAttemptDisposition::Released
    );
    assert_eq!(released_snapshot.settlement_count, 0);
    released_snapshot.accumulated_units = 121;
    assert_eq!(
        released_snapshot.validate(),
        Err("billing_recovery_snapshot_invalid".to_owned())
    );
}

#[test]
fn recovered_facts_and_plans_agree_on_one_settlement() {
    let fence = boot_fence();
    let unknown = snapshot(BillingAttemptDisposition::Unknown);
    let evidence = reconciliation(BillingReconciliationOutcome::Consumed, true, 120);
    let settled = unknown
        .settled_by(&fence, &evidence)
        .expect("settled snapshot");
    let plan = reconcile_attempt(&unknown, &fence, &evidence).expect("reconcile plan");
    assert_eq!(settled.settlement_count, 1);
    assert_eq!(plan.settlements, 1);
    assert_eq!(settled.reconciliation_ref, plan.reconciliation_ref);
    assert_eq!(settled.accumulated_units, plan.accumulated_units);
    assert_eq!(
        plan.matches_fact(&fact(BillingRecoveryState::Recovered)),
        Ok(())
    );
    assert_eq!(
        plan.matches_fact(&fact(BillingRecoveryState::Paused)),
        Err("billing_recovery_plan_fact_settlement_mismatch".to_owned())
    );

    let needs = plan_billing_recovery(
        &unknown,
        &fence,
        &request(BillingRecoveryAction::Restart, 120),
    )
    .expect("needs reconciliation");
    assert_eq!(
        needs.matches_fact(&fact(BillingRecoveryState::NeedsReconciliation)),
        Ok(())
    );
    assert_eq!(
        needs.matches_fact(&fact(BillingRecoveryState::Recovered)),
        Err("billing_recovery_plan_fact_settlement_mismatch".to_owned())
    );

    let mut no_settlement = plan.clone();
    no_settlement.settlements = 0;
    no_settlement.plan_digest = no_settlement.digest();
    assert_eq!(
        no_settlement.validate(),
        Err("billing_recovery_plan_invalid".to_owned())
    );

    let mut unknown_success = plan.clone();
    unknown_success.usage_unknown = true;
    unknown_success.plan_digest = unknown_success.digest();
    assert_eq!(
        unknown_success.validate(),
        Err("billing_recovery_plan_invalid".to_owned())
    );

    let mut redispatch = plan;
    redispatch.redispatch_permitted = true;
    redispatch.plan_digest = redispatch.digest();
    assert_eq!(
        redispatch.validate(),
        Err("billing_recovery_plan_invalid".to_owned())
    );
}
