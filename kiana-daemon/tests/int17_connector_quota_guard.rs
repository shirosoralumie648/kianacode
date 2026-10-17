const DAEMON_FIXTURES: &str = include_str!("int17_connector_quota.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int17-connector-quota.yml");

#[test]
fn daemon_rechecks_quota_before_fixture_and_preserves_single_spine() {
    let daemon = include_str!("../src/connectors.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let core = include_str!("../../kiana-core/src/connector_quota.rs");
    let provider = include_str!("../../kiana-provider/src/connector_quota.rs");
    for marker in [
        "connector_quota_reservation",
        "connector_quota_claim",
        "connector_quota_policy",
        "validate_connector_quota_boundary",
        "connector_quota_effect_admission",
        "connector_quota_credential_generation_mismatch",
        "ConnectorQuotaReservation",
        "ConnectorQuotaSettlement",
        "ConnectorFixture",
        "provider_receipt",
        "append_idempotent_expected",
    ] {
        assert!(
            daemon.contains(marker)
                || broker.contains(marker)
                || core.contains(marker)
                || provider.contains(marker),
            "INT-17 daemon marker missing: {marker}"
        );
    }
    assert!(daemon.contains(".find_case("));
    assert!(daemon.contains("validate_connector_quota_boundary"));
    assert!(!daemon.contains("tokio::spawn"));
    assert!(!daemon.contains("reqwest::Client"));
}

#[test]
fn daemon_only_forwards_the_server_owned_quota_envelope() {
    let daemon = include_str!("../src/connectors.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    // The daemon never owns the quota ledger, never reserves/settles and never widens a quota
    // decision: it only forwards a server-emitted envelope to the Broker's effect boundary.
    for forbidden in [
        "ConnectorQuotaLedger",
        "connector_quota_reserve",
        "connector_quota_settle",
        "connector_quota_claim(",
    ] {
        assert!(
            !daemon.contains(forbidden),
            "daemon must not own quota accounting: {forbidden}"
        );
    }
    for marker in [
        "connector_quota_reservation_required",
        "connector_quota_claim_required",
        "connector_quota_policy_required",
        "connector_quota_reservation_invalid",
        "connector_quota_claim_invalid",
        "connector_quota_policy_invalid",
        "connector_quota_claim_fence_mismatch",
    ] {
        assert!(
            broker.contains(marker)
                || include_str!("../../kiana-domain/src/connector_quota.rs").contains(marker),
            "INT-17 refusal code missing: {marker}"
        );
    }
    // Wire smuggling and durable restart accounting stay the daemon behaviour fixtures.
    for fixture in [
        "wire_supplied_quota_envelope_is_refused_before_any_effect",
        "rate_accounting_survives_a_restart_and_is_never_reset_to_zero",
        "connector_arguments_invalid",
        "connector_rate_limit_exceeded",
        "with_harness_on_disk_and_project_authority",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "INT-17 daemon fixture missing: {fixture}"
        );
    }
    assert!(MANUAL_LANE.contains("--test int17_connector_quota"));
    assert!(MANUAL_LANE.contains("kiana-daemon"));
}
