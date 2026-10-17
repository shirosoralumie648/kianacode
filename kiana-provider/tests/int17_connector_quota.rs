//! INT-17 provider-side quota lease fixtures.
//!
//! The provider is the transport-side checkpoint: it cannot reserve, settle or widen quota, but
//! it must refuse a quota envelope whose capacity lease or credential generation no longer match
//! the connection it is about to use.  Deny-first: expired reservations, policy fence drift,
//! claim mismatches, tampered digests and a foreign capacity lease never reach `transport::send`;
//! the success case proves the exact server-owned triple is accepted unchanged.

use kiana_domain::{
    json_digest, AttemptId, ClockObservation, ConnectorQuotaClaim, ConnectorQuotaDimensions,
    ConnectorQuotaKey, ConnectorQuotaLimit, ConnectorQuotaPolicy, ConnectorQuotaReservation,
    ConnectorQuotaReservationState, InvocationId, QuotaReservationId, QuotaWindow, RateCardId,
    RunId,
};
use kiana_provider::validate_connector_quota_lease;

const CAPACITY_LEASE: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn window() -> QuotaWindow {
    let clock = ClockObservation::observe("int17-provider", 125_000, 125_000, None, 1).unwrap();
    QuotaWindow::from_clock(&clock, 60_000).unwrap()
}

fn limit(max_concurrency: u32) -> ConnectorQuotaLimit {
    ConnectorQuotaLimit {
        max_requests: 10,
        max_tokens: 1_000,
        max_budget_micros: 10_000,
        max_concurrency,
    }
}

fn policy(authority_epoch: u64, credential_generation: u64) -> ConnectorQuotaPolicy {
    ConnectorQuotaPolicy::new(
        ConnectorQuotaKey::new(
            "connector-int17-provider",
            "v1",
            "account-int17",
            "project-int17",
            "main",
            credential_generation,
        )
        .unwrap(),
        window(),
        limit(4),
        limit(4),
        limit(4),
        authority_epoch,
        "quota.config.1",
        Some(RateCardId::new()),
        Some(CAPACITY_LEASE.to_owned()),
    )
    .unwrap()
}

fn reservation_with(
    policy: &ConnectorQuotaPolicy,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> ConnectorQuotaReservation {
    let mut reservation = ConnectorQuotaReservation::new(
        QuotaReservationId::new(),
        InvocationId::new(),
        1,
        RunId::new(),
        AttemptId::new(),
        json_digest(&serde_json::json!({"owner": "project-int17"})),
        json_digest(&serde_json::json!({"command": "connector.invoke"})),
        policy,
        ConnectorQuotaDimensions::invocation(20, 100),
        issued_at_unix_ms,
        expires_at_unix_ms,
    )
    .unwrap();
    reservation.state = ConnectorQuotaReservationState::Claimed;
    reservation.claimed_by = Some("quota-worker".to_owned());
    reservation.revision = 2;
    reservation.reservation_digest = reservation.digest();
    reservation
}

fn claim(reservation: &ConnectorQuotaReservation) -> ConnectorQuotaClaim {
    ConnectorQuotaClaim::new(reservation, "quota-worker", 125_001).unwrap()
}

#[test]
fn provider_rechecks_server_quota_and_capacity_without_dispatch() {
    let source = include_str!("../src/connector_quota.rs");
    let transport = include_str!("../src/transport.rs");
    for marker in [
        "connector_quota_effect_admission",
        "capacity_lease_digest",
        "connector_quota_capacity_lease_mismatch",
        "credential_generation",
        "ConnectorQuotaReservation",
        "ConnectorQuotaClaim",
        "ConnectorQuotaPolicy",
    ] {
        assert!(
            source.contains(marker) || transport.contains(marker),
            "INT-17 provider marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn",
        "reqwest::Client",
        "EventStore",
        "authorize_and_execute",
    ] {
        assert!(
            !source.contains(forbidden),
            "provider quota boundary must not dispatch or authorize: {forbidden}"
        );
    }
}

#[test]
fn stale_foreign_and_tampered_quota_leases_are_refused_before_transport() {
    let policy = policy(9, 7);
    let reservation = reservation_with(&policy, 125_000, 179_000);
    let claim = claim(&reservation);

    // Success column: the exact server-owned triple plus the current capacity lease is accepted.
    assert!(validate_connector_quota_lease(
        &reservation,
        &claim,
        &policy,
        Some(CAPACITY_LEASE),
        125_002,
    )
    .is_ok());

    // A reservation whose window already ended is refused.
    assert_eq!(
        validate_connector_quota_lease(
            &reservation,
            &claim,
            &policy,
            Some(CAPACITY_LEASE),
            179_000
        )
        .unwrap_err(),
        "connector_quota_reservation_expired"
    );

    // A policy revision drift (authority epoch or rate card identity) fails the fence.
    let drifted = policy(10, 7);
    assert_eq!(
        validate_connector_quota_lease(
            &reservation,
            &claim,
            &drifted,
            Some(CAPACITY_LEASE),
            125_002
        )
        .unwrap_err(),
        "connector_quota_policy_fence_stale"
    );

    // A claim that does not fence the claimed reservation revision is refused.
    let mut stale_claim = claim.clone();
    stale_claim.expected_revision = 1;
    stale_claim.claim_digest = stale_claim.digest();
    assert_eq!(
        validate_connector_quota_lease(
            &reservation,
            &stale_claim,
            &policy,
            Some(CAPACITY_LEASE),
            125_002
        )
        .unwrap_err(),
        "connector_quota_claim_fence_mismatch"
    );

    // The capacity lease on the wire must be the same lease the policy was minted with.
    assert_eq!(
        validate_connector_quota_lease(
            &reservation,
            &claim,
            &policy,
            Some("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            125_002,
        )
        .unwrap_err(),
        "connector_quota_capacity_lease_mismatch"
    );
    assert_eq!(
        validate_connector_quota_lease(&reservation, &claim, &policy, None, 125_002).unwrap_err(),
        "connector_quota_capacity_lease_mismatch"
    );

    // A tampered reservation digest never reaches the transport.
    let mut tampered = reservation.clone();
    tampered.reservation_digest = json_digest(&serde_json::json!({"forged": true}));
    assert_eq!(
        validate_connector_quota_lease(&tampered, &claim, &policy, Some(CAPACITY_LEASE), 125_002)
            .unwrap_err(),
        "connector_quota_reservation_invalid"
    );
}
