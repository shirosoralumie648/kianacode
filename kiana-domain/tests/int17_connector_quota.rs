use kiana_domain::{
    json_digest, AttemptId, ClockObservation, ConnectorQuotaClaimOutcome, ConnectorQuotaDimensions,
    ConnectorQuotaKey, ConnectorQuotaLedger, ConnectorQuotaLimit, ConnectorQuotaPolicy,
    ConnectorQuotaReservation, ConnectorQuotaReserveOutcome, ConnectorQuotaSettlement,
    ConnectorQuotaSettlementOutcome, InvocationId, QuotaReservationId, QuotaWindow, RateCardId,
    RunId,
};
use serde_json::json;

fn window() -> QuotaWindow {
    let clock = ClockObservation::observe("int17", 125_000, 125_000, None, 1).unwrap();
    QuotaWindow::from_clock(&clock, 60_000).unwrap()
}

fn policy() -> ConnectorQuotaPolicy {
    let key = ConnectorQuotaKey::new("Connector:Mail", "v1", "Account-A", "Project-A", "Main", 7)
        .unwrap();
    let limit = ConnectorQuotaLimit {
        max_requests: 2,
        max_tokens: 100,
        max_budget_micros: 1_000,
        max_concurrency: 1,
    };
    ConnectorQuotaPolicy::new(
        key,
        window(),
        limit.clone(),
        limit.clone(),
        limit,
        9,
        "config:9",
        Some(RateCardId::new()),
        Some(json_digest(&json!({"capacity_lease":"fixture"}))),
    )
    .unwrap()
}

fn reservation(policy: &ConnectorQuotaPolicy) -> ConnectorQuotaReservation {
    ConnectorQuotaReservation::new(
        QuotaReservationId::new(),
        InvocationId::new(),
        1,
        RunId::new(),
        AttemptId::new(),
        json_digest(&json!({"owner":"project-a"})),
        json_digest(&json!({"command":"mail.send","payload":"sha256:payload"})),
        policy,
        ConnectorQuotaDimensions::invocation(20, 100),
        125_000,
        179_000,
    )
    .unwrap()
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
    let idempotency = json_digest(&json!({"idempotency":"one"}));
    let id = first.reservation_id;
    assert!(matches!(
        ledger
            .reserve(first.clone(), &idempotency, 125_000)
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
    let idem = json_digest(&json!({"idempotency":"same"}));
    ledger.reserve(first.clone(), &idem, 125_000).unwrap();
    let replay = ledger.reserve(first.clone(), &idem, 125_000).unwrap();
    assert!(matches!(replay, ConnectorQuotaReserveOutcome::Replayed(_)));
    let second = reservation(&policy);
    assert_eq!(
        ledger
            .reserve(second, &json_digest(&json!({"idempotency":"two"})), 125_000)
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
