//! INT-17 connector/account/project quota fixtures.
//!
//! Deny-first: an in-memory restart without the persisted ledger, per-scope over-limit
//! dimensions (including concurrency), forged digests, stale epochs, claim CAS races, expired
//! windows and unclaimed settlements are all refused with stable codes before any usage is
//! charged. The success columns prove one-winner worker claims, owner-bound release and exact
//! settlement replay over the same pure ledger the ControlPlane CAS stream persists.

use kiana_domain::{
    connector_quota_effect_admission, json_digest, AttemptId, ClockObservation,
    ConnectorQuotaClaim, ConnectorQuotaClaimOutcome, ConnectorQuotaDimensions, ConnectorQuotaKey,
    ConnectorQuotaLedger, ConnectorQuotaLimit, ConnectorQuotaPolicy, ConnectorQuotaReservation,
    ConnectorQuotaReservationState, ConnectorQuotaReserveOutcome, ConnectorQuotaScope,
    ConnectorQuotaSettlement, ConnectorQuotaSettlementOutcome, InvocationId, QuotaReservationId,
    QuotaWindow, RateCardId, RunId,
};
use serde_json::json;

fn window() -> QuotaWindow {
    let clock = ClockObservation::observe("int17", 125_000, 125_000, None, 1).unwrap();
    QuotaWindow::from_clock(&clock, 60_000).unwrap()
}

fn key() -> ConnectorQuotaKey {
    ConnectorQuotaKey::new("Connector:Mail", "v1", "Account-A", "Project-A", "Main", 7).unwrap()
}

fn limit(
    max_requests: u64,
    max_tokens: u64,
    max_budget_micros: u64,
    max_concurrency: u32,
) -> ConnectorQuotaLimit {
    ConnectorQuotaLimit {
        max_requests,
        max_tokens,
        max_budget_micros,
        max_concurrency,
    }
}

fn policy_with(
    connector: ConnectorQuotaLimit,
    account: ConnectorQuotaLimit,
    project: ConnectorQuotaLimit,
) -> ConnectorQuotaPolicy {
    ConnectorQuotaPolicy::new(
        key(),
        window(),
        connector,
        account,
        project,
        9,
        "config:9",
        Some(RateCardId::new()),
        Some(json_digest(&json!({"capacity_lease":"fixture"}))),
    )
    .unwrap()
}

fn policy() -> ConnectorQuotaPolicy {
    let limit = limit(2, 100, 1_000, 1);
    policy_with(limit.clone(), limit.clone(), limit)
}

fn owner_digest() -> String {
    json_digest(&json!({"owner":"project-a"}))
}

fn reservation_with(
    policy: &ConnectorQuotaPolicy,
    requested: ConnectorQuotaDimensions,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> ConnectorQuotaReservation {
    ConnectorQuotaReservation::new(
        QuotaReservationId::new(),
        InvocationId::new(),
        1,
        RunId::new(),
        AttemptId::new(),
        owner_digest(),
        json_digest(&json!({"command":"mail.send","payload":"sha256:payload"})),
        policy,
        requested,
        issued_at_unix_ms,
        expires_at_unix_ms,
    )
    .unwrap()
}

fn reservation(policy: &ConnectorQuotaPolicy) -> ConnectorQuotaReservation {
    reservation_with(
        policy,
        ConnectorQuotaDimensions::invocation(20, 100),
        125_000,
        179_000,
    )
}

/// A reservation in the state the ControlPlane CAS claim would leave behind, with every digest
/// recomputed so only the requested dimensions can explain a later denial.
fn claimed(reservation: &ConnectorQuotaReservation, worker_id: &str) -> ConnectorQuotaReservation {
    let mut claimed = reservation.clone();
    claimed.state = ConnectorQuotaReservationState::Claimed;
    claimed.claimed_by = Some(worker_id.to_owned());
    claimed.revision = 2;
    claimed.reservation_digest = claimed.digest();
    claimed
}

fn idempotency(seed: &str) -> String {
    json_digest(&json!({"idempotency": seed}))
}

#[test]
fn alias_is_canonical_and_credential_generation_is_quota_identity() {
    let lower = ConnectorQuotaKey::new("mail", "v1", "account", "project", "main", 7).unwrap();
    let upper = ConnectorQuotaKey::new("MAIL", "v1", "ACCOUNT", "project", "MAIN", 7).unwrap();
    assert_eq!(lower, upper);
    let rotated = ConnectorQuotaKey::new("mail", "v1", "account", "project", "main", 8).unwrap();
    assert_ne!(lower.key_digest, rotated.key_digest);
    assert!(ConnectorQuotaKey {
        alias: "MAIN".to_owned(),
        key_digest: lower.key_digest.clone(),
        ..lower
    }
    .validate()
    .is_err());
}

#[test]
fn quota_limits_claim_single_worker_and_owner_release() {
    let policy = policy();
    let mut ledger = ConnectorQuotaLedger::new(policy.clone(), 9).unwrap();
    let first = reservation(&policy);
    let idempotency_key = idempotency("one");
    let id = first.reservation_id;
    assert!(matches!(
        ledger
            .reserve(first.clone(), &idempotency_key, 125_000)
            .unwrap(),
        ConnectorQuotaReserveOutcome::Reserved(_)
    ));
    let (claim, outcome) = ledger.claim(id, 1, "worker-a", 125_001).unwrap();
    assert_eq!(outcome, ConnectorQuotaClaimOutcome::Claimed);
    assert!(ledger.claim(id, 1, "worker-b", 125_002).is_err());
    assert!(ledger
        .release(id, &first.owner_digest, Some("worker-b"))
        .is_err());
    let settlement = ConnectorQuotaSettlement::new(
        &ledger.reservation(id).unwrap().clone(),
        "worker-a",
        None,
        None,
        None,
        125_003,
    )
    .unwrap();
    assert_eq!(
        ledger.settle(settlement.clone()).unwrap(),
        ConnectorQuotaSettlementOutcome::Settled
    );
    assert_eq!(
        ledger.settle(settlement).unwrap(),
        ConnectorQuotaSettlementOutcome::Replayed
    );
    assert!(claim.validate().is_ok());
}

#[test]
fn over_limit_replay_and_old_epoch_fail_closed() {
    let policy = policy();
    let mut ledger = ConnectorQuotaLedger::new(policy.clone(), 9).unwrap();
    let first = reservation(&policy);
    let idem = idempotency("same");
    ledger.reserve(first.clone(), &idem, 125_000).unwrap();
    let replay = ledger.reserve(first.clone(), &idem, 125_000).unwrap();
    assert!(matches!(replay, ConnectorQuotaReserveOutcome::Replayed(_)));
    let second = reservation(&policy);
    assert_eq!(
        ledger
            .reserve(second, &idempotency("two"), 125_000)
            .unwrap_err(),
        "connector_quota_connector_limit_exceeded"
    );
    assert_eq!(
        ledger.validate_reopen(8).unwrap_err(),
        "connector_quota_old_epoch"
    );
    assert!(ledger.validate_reopen(9).is_ok());
    assert_eq!(
        ConnectorQuotaLedger::reopen(9, None).unwrap_err(),
        "connector_quota_restart_state_missing"
    );
}

#[test]
fn restart_without_persisted_state_cannot_reset_usage_to_zero() {
    let policy = policy();
    let mut ledger = ConnectorQuotaLedger::new(policy.clone(), 9).unwrap();
    ledger
        .reserve(reservation(&policy), &idempotency("restart"), 125_000)
        .unwrap();
    assert_eq!(ledger.usage(ConnectorQuotaScope::Connector).concurrency, 1);

    // A restarted worker without the persisted ledger must fail closed instead of reopening an
    // empty window whose counters start at zero; an old server epoch is refused as well.
    assert_eq!(
        ConnectorQuotaLedger::reopen(9, None).unwrap_err(),
        "connector_quota_restart_state_missing"
    );
    assert_eq!(
        ConnectorQuotaLedger::reopen(0, Some(ledger.clone())).unwrap_err(),
        "connector_quota_old_epoch"
    );
    assert_eq!(
        ConnectorQuotaLedger::reopen(8, Some(ledger.clone())).unwrap_err(),
        "connector_quota_old_epoch"
    );
    assert!(ledger.validate_reopen(9).is_ok());

    // The same epoch may reopen the persisted ledger, and the earlier usage is still charged so
    // the concurrency ceiling cannot be bypassed by a restart.
    let mut reopened = ConnectorQuotaLedger::reopen(9, Some(ledger)).unwrap();
    assert_eq!(
        reopened.usage(ConnectorQuotaScope::Connector).concurrency,
        1
    );
    assert_eq!(
        reopened
            .reserve(reservation(&policy), &idempotency("after-restart"), 125_000)
            .unwrap_err(),
        "connector_quota_connector_limit_exceeded"
    );
}

#[test]
fn over_limit_reservations_are_refused_per_scope_with_stable_codes() {
    let wide = limit(10, 1_000, 10_000, 4);
    for (scope, narrow, expected) in [
        (
            ConnectorQuotaScope::Connector,
            limit(10, 1_000, 10_000, 1),
            "connector_quota_connector_limit_exceeded",
        ),
        (
            ConnectorQuotaScope::Account,
            limit(10, 1_000, 10_000, 1),
            "connector_quota_account_limit_exceeded",
        ),
        (
            ConnectorQuotaScope::Project,
            limit(10, 1_000, 10_000, 1),
            "connector_quota_project_limit_exceeded",
        ),
    ] {
        let policy = match scope {
            ConnectorQuotaScope::Connector => policy_with(narrow, wide.clone(), wide.clone()),
            ConnectorQuotaScope::Account => policy_with(wide.clone(), narrow, wide.clone()),
            ConnectorQuotaScope::Project => policy_with(wide.clone(), wide.clone(), narrow),
        };
        let mut ledger = ConnectorQuotaLedger::new(policy.clone(), 9).unwrap();
        ledger
            .reserve(
                reservation(&policy),
                &idempotency(&format!("{}:1", scope.as_str())),
                125_000,
            )
            .unwrap();
        let used = ledger.usage(ConnectorQuotaScope::Connector);
        // A second in-flight reservation exceeds the narrow concurrency ceiling. The denial
        // keeps the recorded usage unchanged and records no reservation.
        let denied = reservation(&policy);
        let denied_id = denied.reservation_id;
        assert_eq!(
            ledger
                .reserve(
                    denied,
                    &idempotency(&format!("{}:2", scope.as_str())),
                    125_000
                )
                .unwrap_err(),
            expected
        );
        assert!(ledger.reservation(denied_id).is_none());
        assert_eq!(ledger.usage(ConnectorQuotaScope::Connector), used);
    }

    // Requests, tokens and budget each hit the same stable per-scope code.
    for narrow in [
        limit(1, 1_000, 10_000, 4),
        limit(10, 30, 10_000, 4),
        limit(10, 1_000, 150, 4),
    ] {
        let policy = policy_with(narrow, wide.clone(), wide.clone());
        let mut ledger = ConnectorQuotaLedger::new(policy.clone(), 9).unwrap();
        ledger
            .reserve(reservation(&policy), &idempotency("dimension:1"), 125_000)
            .unwrap();
        assert_eq!(
            ledger
                .reserve(reservation(&policy), &idempotency("dimension:2"), 125_000)
                .unwrap_err(),
            "connector_quota_connector_limit_exceeded"
        );
    }
}

#[test]
fn forged_policy_reservation_and_settlement_shapes_fail_closed() {
    let policy = policy();
    let good = reservation(&policy);

    // A reservation whose recorded digest no longer matches its content is refused.
    let mut tampered = good.clone();
    tampered.reservation_digest = json_digest(&json!({"forged": true}));
    assert_eq!(
        tampered
            .validate_against(&policy, Some(125_000))
            .unwrap_err(),
        "connector_quota_reservation_invalid"
    );

    // A reservation from another policy revision (different rate card identity in this fixture)
    // can never pass the fence, and the window's own bounds are enforced too.
    let other = policy_with(
        limit(2, 100, 1_000, 1),
        limit(2, 100, 1_000, 1),
        limit(2, 100, 1_000, 1),
    );
    assert_eq!(
        good.validate_against(&other, Some(125_000)).unwrap_err(),
        "connector_quota_policy_fence_stale"
    );
    assert_eq!(
        good.validate_against(&policy, Some(124_999)).unwrap_err(),
        "connector_quota_reservation_expired"
    );
    assert_eq!(
        good.validate_against(&policy, Some(179_000)).unwrap_err(),
        "connector_quota_reservation_expired"
    );

    // A tampered claim digest is refused before it can fence anything.
    let claimed_good = claimed(&good, "worker-a");
    let mut claim = ConnectorQuotaClaim::new(&claimed_good, "worker-a", 125_005).unwrap();
    claim.claim_digest = json_digest(&json!({"forged": true}));
    assert_eq!(
        claim.validate().unwrap_err(),
        "connector_quota_claim_invalid"
    );

    // A settlement bound to another reservation identity never validates against this one.
    let other_reservation = claimed(&reservation(&policy), "worker-b");
    let settlement = ConnectorQuotaSettlement::new(
        &other_reservation,
        "worker-b",
        Some("receipt-x".to_owned()),
        Some(5),
        Some(5),
        125_010,
    )
    .unwrap();
    assert_eq!(
        settlement.validate_against(&claimed_good).unwrap_err(),
        "connector_quota_settlement_reservation_mismatch"
    );

    // A settlement that charges more than the reservation requested is refused even when its own
    // digest is recomputed.
    let mut overcharged = ConnectorQuotaSettlement::new(
        &claimed_good,
        "worker-a",
        Some("receipt-a".to_owned()),
        Some(5),
        Some(5),
        125_010,
    )
    .unwrap();
    overcharged.charged_tokens = Some(999);
    overcharged.usage_known = true;
    overcharged.settlement_digest = overcharged.digest();
    assert_eq!(
        overcharged.validate_against(&claimed_good).unwrap_err(),
        "connector_quota_settlement_reservation_mismatch"
    );

    // Policy/ledger shapes: a rate card is required, zero limits and zero dimensions are refused
    // and a ledger opened under a foreign epoch cannot start.
    assert_eq!(
        ConnectorQuotaPolicy::new(
            key(),
            window(),
            limit(2, 100, 1_000, 1),
            limit(2, 100, 1_000, 1),
            limit(2, 100, 1_000, 1),
            9,
            "config:9",
            None,
            None,
        )
        .unwrap_err(),
        "connector_quota_rate_card_required"
    );
    assert_eq!(
        ConnectorQuotaPolicy::new(
            key(),
            window(),
            limit(0, 100, 1_000, 1),
            limit(2, 100, 1_000, 1),
            limit(2, 100, 1_000, 1),
            9,
            "config:9",
            Some(RateCardId::new()),
            None,
        )
        .unwrap_err(),
        "connector_quota_limit_invalid"
    );
    assert_eq!(
        ConnectorQuotaReservation::new(
            QuotaReservationId::new(),
            InvocationId::new(),
            1,
            RunId::new(),
            AttemptId::new(),
            owner_digest(),
            json_digest(&json!({"command":"mail.send"})),
            &policy,
            ConnectorQuotaDimensions {
                requests: 0,
                tokens: 1,
                budget_micros: 1,
                concurrency: 1,
            },
            125_000,
            179_000,
        )
        .unwrap_err(),
        "connector_quota_dimensions_invalid"
    );
    assert_eq!(
        ConnectorQuotaLedger::new(policy.clone(), 8).unwrap_err(),
        "connector_quota_epoch_invalid"
    );
    assert_eq!(
        ConnectorQuotaLedger::new(policy, 0).unwrap_err(),
        "connector_quota_epoch_invalid"
    );

    // Model text or unknown fields can never be decoded into a quota fact.
    let mut value = serde_json::to_value(&good).unwrap();
    value["transcript"] = json!("model text");
    assert!(serde_json::from_value::<ConnectorQuotaReservation>(value).is_err());
}

#[test]
fn claim_cas_admits_exactly_one_worker_and_owner_release_is_fenced() {
    let policy = policy();
    let mut ledger = ConnectorQuotaLedger::new(policy.clone(), 9).unwrap();
    let first = reservation(&policy);
    let id = first.reservation_id;
    ledger
        .reserve(first.clone(), &idempotency("claim-cas"), 125_000)
        .unwrap();

    let (claim, outcome) = ledger.claim(id, 1, "worker-a", 125_001).unwrap();
    assert_eq!(outcome, ConnectorQuotaClaimOutcome::Claimed);
    assert_eq!(claim.worker_id, "worker-a");
    assert_eq!(claim.expected_revision, 2);
    assert_eq!(
        ledger.reservation(id).unwrap().state,
        ConnectorQuotaReservationState::Claimed
    );

    // A second worker presenting the old revision loses the CAS race before the state check.
    assert_eq!(
        ledger.claim(id, 1, "worker-b", 125_002).unwrap_err(),
        "connector_quota_claim_revision_stale"
    );
    // A second worker presenting the current revision sees the reservation is already won.
    assert_eq!(
        ledger.claim(id, 2, "worker-b", 125_002).unwrap_err(),
        "connector_quota_claim_already_won"
    );
    // The winner's own retry is an idempotent replay, never a second claim.
    let (replay_claim, replay) = ledger.claim(id, 2, "worker-a", 125_003).unwrap();
    assert_eq!(replay, ConnectorQuotaClaimOutcome::Replayed);
    assert_eq!(replay_claim.claim_digest, claim.claim_digest);

    // Release is owner-bound: another worker and another owner digest are both refused.
    assert_eq!(
        ledger
            .release(id, &first.owner_digest, Some("worker-b"))
            .unwrap_err(),
        "connector_quota_release_worker_mismatch"
    );
    assert_eq!(
        ledger
            .release(
                id,
                &json_digest(&json!({"owner":"project-b"})),
                Some("worker-a")
            )
            .unwrap_err(),
        "connector_quota_owner_mismatch"
    );
    assert!(ledger
        .release(id, &first.owner_digest, Some("worker-a"))
        .is_ok());
    assert_eq!(
        ledger.reservation(id).unwrap().state,
        ConnectorQuotaReservationState::Released
    );
    // Released usage returns to zero and a second release is not pending anymore.
    assert_eq!(ledger.usage(ConnectorQuotaScope::Connector).requests, 0);
    assert_eq!(
        ledger.release(id, &first.owner_digest, None).unwrap_err(),
        "connector_quota_release_not_pending"
    );
}

#[test]
fn settlement_replays_exactly_and_unknown_usage_is_not_laundered_into_success() {
    let policy = policy();
    let mut ledger = ConnectorQuotaLedger::new(policy.clone(), 9).unwrap();
    let first = reservation(&policy);
    let id = first.reservation_id;
    ledger
        .reserve(first.clone(), &idempotency("settle"), 125_000)
        .unwrap();
    assert_eq!(
        ledger.claim(id, 1, "worker-a", 125_001).unwrap().1,
        ConnectorQuotaClaimOutcome::Claimed
    );
    let claimed_state = ledger.reservation(id).unwrap().clone();

    // A settlement from a worker that does not own the claim is refused before usage release.
    let foreign = ConnectorQuotaSettlement::new(
        &claimed_state,
        "worker-b",
        Some("receipt-b".to_owned()),
        Some(10),
        Some(50),
        125_010,
    )
    .unwrap();
    assert_eq!(
        ledger.settle(foreign).unwrap_err(),
        "connector_quota_settlement_owner_mismatch"
    );

    let settlement = ConnectorQuotaSettlement::new(
        &claimed_state,
        "worker-a",
        Some("receipt-a".to_owned()),
        Some(10),
        Some(50),
        125_010,
    )
    .unwrap();
    assert_eq!(
        ledger.settle(settlement.clone()).unwrap(),
        ConnectorQuotaSettlementOutcome::Settled
    );
    assert_eq!(
        ledger.reservation(id).unwrap().state,
        ConnectorQuotaReservationState::Settled
    );
    // Settlement releases the concurrency slot but keeps the consumed rate/budget charged.
    assert_eq!(ledger.usage(ConnectorQuotaScope::Connector).concurrency, 0);
    assert_eq!(ledger.usage(ConnectorQuotaScope::Connector).requests, 1);
    // Exact replay is idempotent; a conflicting settlement for the same reservation is refused.
    assert_eq!(
        ledger.settle(settlement).unwrap(),
        ConnectorQuotaSettlementOutcome::Replayed
    );
    let settled = ledger.reservation(id).unwrap().clone();
    let conflicting = ConnectorQuotaSettlement::new(
        &settled,
        "worker-a",
        Some("receipt-a2".to_owned()),
        Some(9),
        Some(50),
        125_011,
    )
    .unwrap();
    assert_eq!(
        ledger.settle(conflicting).unwrap_err(),
        "connector_quota_settlement_conflict"
    );

    // An unknown outcome stays visible as Unknown and is never converted into success, but it can
    // still be released explicitly.
    let unknown_reservation = reservation(&policy);
    let unknown_id = unknown_reservation.reservation_id;
    ledger
        .reserve(unknown_reservation, &idempotency("unknown"), 125_000)
        .unwrap();
    assert_eq!(
        ledger.claim(unknown_id, 1, "worker-c", 125_002).unwrap().1,
        ConnectorQuotaClaimOutcome::Claimed
    );
    let unknown_claimed = ledger.reservation(unknown_id).unwrap().clone();
    let unknown =
        ConnectorQuotaSettlement::new(&unknown_claimed, "worker-c", None, None, None, 125_020)
            .unwrap();
    assert!(!unknown.usage_known);
    assert_eq!(
        ledger.settle(unknown).unwrap(),
        ConnectorQuotaSettlementOutcome::Settled
    );
    assert_eq!(
        ledger.reservation(unknown_id).unwrap().state,
        ConnectorQuotaReservationState::Unknown
    );
    assert!(ledger
        .release(unknown_id, &unknown_claimed.owner_digest, None)
        .is_ok());
    assert_eq!(
        ledger.reservation(unknown_id).unwrap().state,
        ConnectorQuotaReservationState::Released
    );
}

#[test]
fn effect_admission_requires_a_live_claimed_reservation_and_refuses_over_limit_dimensions() {
    let wide = limit(10, 1_000, 10_000, 4);
    for (scope, ceiling, expected) in [
        (
            ConnectorQuotaScope::Connector,
            8u32,
            "connector_quota_connector_limit_exceeded",
        ),
        (
            ConnectorQuotaScope::Account,
            6u32,
            "connector_quota_account_limit_exceeded",
        ),
        (
            ConnectorQuotaScope::Project,
            4u32,
            "connector_quota_project_limit_exceeded",
        ),
    ] {
        let narrow = limit(10, 1_000, 10_000, ceiling);
        let policy = match scope {
            ConnectorQuotaScope::Connector => policy_with(narrow, wide.clone(), wide.clone()),
            ConnectorQuotaScope::Account => policy_with(wide.clone(), narrow, wide.clone()),
            ConnectorQuotaScope::Project => policy_with(wide.clone(), wide.clone(), narrow),
        };

        // A self-consistent but over-limit reservation (forged concurrency above the policy
        // ceiling) is refused at the effect boundary with the same stable scope code.
        let over_limit = reservation_with(
            &policy,
            ConnectorQuotaDimensions {
                requests: 1,
                tokens: 20,
                budget_micros: 100,
                concurrency: ceiling + 1,
            },
            125_000,
            179_000,
        );
        let over_claimed = claimed(&over_limit, "worker-a");
        let over_claim = ConnectorQuotaClaim::new(&over_claimed, "worker-a", 125_005).unwrap();
        assert_eq!(
            connector_quota_effect_admission(&over_limit, &over_claim, &policy, 125_006)
                .unwrap_err(),
            "connector_quota_claim_fence_mismatch"
        );
        assert_eq!(
            connector_quota_effect_admission(&over_claimed, &over_claim, &policy, 125_006)
                .unwrap_err(),
            expected
        );

        // The same shape within the ceiling is admitted and keeps its claim fence.
        let admitted = reservation_with(
            &policy,
            ConnectorQuotaDimensions {
                requests: 1,
                tokens: 20,
                budget_micros: 100,
                concurrency: ceiling,
            },
            125_000,
            179_000,
        );
        let admitted_claim = claimed(&admitted, "worker-a");
        let claim = ConnectorQuotaClaim::new(&admitted_claim, "worker-a", 125_005).unwrap();
        assert!(
            connector_quota_effect_admission(&admitted_claim, &claim, &policy, 125_006).is_ok()
        );
        // Expiry is still refused at the effect boundary, and a claim cannot switch owners.
        assert_eq!(
            connector_quota_effect_admission(&admitted_claim, &claim, &policy, 179_000)
                .unwrap_err(),
            "connector_quota_reservation_expired"
        );
        let foreign_owner = claimed(&admitted, "worker-b");
        let foreign_claim = ConnectorQuotaClaim::new(&foreign_owner, "worker-b", 125_005).unwrap();
        assert_eq!(
            connector_quota_effect_admission(&admitted_claim, &foreign_claim, &policy, 125_006)
                .unwrap_err(),
            "connector_quota_claim_fence_mismatch"
        );
    }
}

#[test]
fn idempotency_replay_returns_the_original_reservation_and_conflicts_are_refused() {
    let policy = policy();
    let mut ledger = ConnectorQuotaLedger::new(policy.clone(), 9).unwrap();
    let first = reservation(&policy);
    let id = first.reservation_id;
    let key_digest = idempotency("one");
    match ledger.reserve(first, &key_digest, 125_000).unwrap() {
        ConnectorQuotaReserveOutcome::Reserved(reserved) => {
            assert_eq!(reserved.reservation_id, id);
        }
        ConnectorQuotaReserveOutcome::Replayed(_) => panic!("first reservation must be admitted"),
    }

    // The same key over the same canonical command returns the original reservation identity.
    let replay = reservation(&policy);
    let replay_id = replay.reservation_id;
    match ledger.reserve(replay, &key_digest, 125_000).unwrap() {
        ConnectorQuotaReserveOutcome::Replayed(original) => {
            assert_eq!(original.reservation_id, id);
            assert_ne!(original.reservation_id, replay_id);
        }
        ConnectorQuotaReserveOutcome::Reserved(_) => {
            panic!("an identical command must replay, not reserve twice")
        }
    }

    // The same key over a different command digest is a conflict, never a second reservation.
    let mut conflicting = reservation(&policy);
    conflicting.command_digest = json_digest(&json!({"command":"mail.other"}));
    conflicting.reservation_digest = conflicting.digest();
    assert_eq!(
        ledger
            .reserve(conflicting, &key_digest, 125_000)
            .unwrap_err(),
        "connector_quota_idempotency_digest_conflict"
    );

    // A new key cannot reuse an existing reservation id.
    let mut reused = reservation(&policy);
    reused.reservation_id = id;
    reused.reservation_digest = reused.digest();
    assert_eq!(
        ledger
            .reserve(reused, &idempotency("two"), 125_000)
            .unwrap_err(),
        "connector_quota_reservation_id_conflict"
    );

    // Malformed idempotency keys and now-outside-the-window reservations fail closed.
    assert_eq!(
        ledger
            .reserve(reservation(&policy), "idempotency-not-a-digest", 125_000)
            .unwrap_err(),
        "connector_quota_idempotency_key_digest_invalid"
    );
    let long_lived = reservation_with(
        &policy,
        ConnectorQuotaDimensions::invocation(20, 100),
        125_000,
        200_000,
    );
    assert_eq!(
        ledger
            .reserve(long_lived, &idempotency("window"), 180_000)
            .unwrap_err(),
        "connector_quota_window_expired"
    );
}
