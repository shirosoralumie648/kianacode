#[test]
fn connector_quota_stays_server_owned_and_eventstore_cas_bound() {
    let domain = include_str!("../../kiana-domain/src/connector_quota.rs");
    let core = include_str!("../src/connector_quota.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    for marker in [
        "ConnectorQuotaKey",
        "connector_id",
        "account_id",
        "project_id",
        "alias",
        "credential_generation",
        "ConnectorQuotaLimit",
        "max_requests",
        "max_concurrency",
        "budget_micros",
        "connector_quota_rate_card_required",
        "ConnectorQuotaReservation",
        "ConnectorQuotaClaim",
        "ConnectorQuotaSettlement",
        "connector_quota_claim_revision_stale",
        "connector_quota_claim_already_won",
        "connector_quota_owner_mismatch",
        "connector_quota_old_epoch",
        "connector_quota_restart_state_missing",
        "reopen",
        "connector_quota_idempotency_digest_conflict",
        "connector_quota_connector_limit_exceeded",
        "commit_connector_quota_reservation",
        "commit_connector_quota_claim",
        "commit_connector_quota_settlement",
        "CONNECTOR_QUOTA_STREAM",
        "commit_confirmed",
        "expected_versions",
        "connector_quota_effect_admission",
        "validate_connector_quota_boundary",
        "connector_quota_reservation_required",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || broker.contains(marker),
            "INT-17 marker missing: {marker}"
        );
    }
    assert!(core.contains("self.events"));
    assert!(core.contains("TransitionBatch"));
    assert!(core.contains("CONNECTOR_QUOTA_EVENT_RESERVED"));
    assert!(!domain.contains("EventStore"));
    assert!(!domain.contains("CapabilityBroker"));
}

#[test]
fn quota_claim_and_settlement_never_authorize_or_dispatch() {
    let domain = include_str!("../../kiana-domain/src/connector_quota.rs");
    let core = include_str!("../src/connector_quota.rs");
    for source in [domain, core] {
        assert!(!source.contains("tokio::spawn"));
        assert!(!source.contains("reqwest::Client"));
        assert!(!source.contains("CapabilityBroker"));
    }
}
