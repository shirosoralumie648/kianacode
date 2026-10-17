const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/int17_connector_quota.rs");
const BROKER_FIXTURES: &str =
    include_str!("../../kiana-capability-broker/tests/int17_connector_quota.rs");
const PROVIDER_FIXTURES: &str = include_str!("../../kiana-provider/tests/int17_connector_quota.rs");
const QUERY_FIXTURES: &str = include_str!("../../kiana-query/tests/int17_connector_quota.rs");
const DAEMON_FIXTURES: &str = include_str!("../../kiana-daemon/tests/int17_connector_quota.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int17-connector-quota.yml");
const TEST_SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

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
        "connector_quota_idempotency_key_digest",
        "connector_quota_reservation_id_conflict",
        "connector_quota_window_expired",
        "connector_quota_settlement_conflict",
        "connector_quota_settlement_owner_mismatch",
        "connector_quota_reservation_expired",
        "connector_quota_policy_fence_stale",
        "connector_quota_claim_fence_mismatch",
        "admits_without_usage",
        "validate_identity_against",
        "connector_quota_{}_limit_exceeded",
        "commit_connector_quota_reservation",
        "commit_connector_quota_claim",
        "commit_connector_quota_settlement",
        "CONNECTOR_QUOTA_STREAM",
        "commit_confirmed",
        "expected_versions",
        "connector_quota_effect_admission",
        "validate_connector_quota_boundary",
        "connector_quota_reservation_required",
        "connector_quota_claim_required",
        "connector_quota_policy_required",
        "connector_quota_reservation_invalid",
        "connector_quota_claim_invalid",
        "connector_quota_policy_invalid",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || broker.contains(marker),
            "INT-17 marker missing: {marker}"
        );
    }
    assert!(core.contains("self.events"));
    assert!(core.contains("TransitionBatch"));
    assert!(core.contains("CONNECTOR_QUOTA_EVENT_RESERVED"));
    let domain_code = domain
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<String>();
    assert!(!domain_code.contains("EventStore"));
    assert!(!domain.contains("CapabilityBroker"));
}

#[test]
fn quota_claim_and_settlement_never_authorize_or_dispatch() {
    let domain = include_str!("../../kiana-domain/src/connector_quota.rs");
    let core = include_str!("../src/connector_quota.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let query = include_str!("../../kiana-query/src/connector_quota.rs");
    for source in [domain, core, query] {
        assert!(!source.contains("tokio::spawn"));
        assert!(!source.contains("reqwest::Client"));
        assert!(!source.contains("CapabilityBroker"));
    }
    // The Broker only revalidates the server-owned envelope; it never reserves, settles or
    // releases quota and never owns the ledger.
    assert!(!broker.contains("ConnectorQuotaLedger"));
    assert!(!broker.contains("connector_quota_reserve"));
    assert!(!broker.contains("connector_quota_settle"));
}

#[test]
fn connector_quota_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    for fixture in [
        // Domain ledger fixtures.
        "restart_without_persisted_state_cannot_reset_usage_to_zero",
        "over_limit_reservations_are_refused_per_scope_with_stable_codes",
        "forged_policy_reservation_and_settlement_shapes_fail_closed",
        "claim_cas_admits_exactly_one_worker_and_owner_release_is_fenced",
        "settlement_replays_exactly_and_unknown_usage_is_not_laundered_into_success",
        "effect_admission_requires_a_live_claimed_reservation_and_refuses_over_limit_dimensions",
        "idempotency_replay_returns_the_original_reservation_and_conflicts_are_refused",
        // Broker effect-boundary fixtures: deny-first with a recording handler at zero calls.
        "admitted_quota_envelope_reaches_the_handler_exactly_once",
        "over_limit_stale_and_tampered_quota_envelopes_never_reach_the_handler",
        // Provider transport-side lease fixtures.
        "stale_foreign_and_tampered_quota_leases_are_refused_before_transport",
        // Query projection fixtures.
        "settled_then_released_projection_replays_states_and_settlement",
        "unknown_usage_stays_visible_and_unrelated_events_only_advance_the_cursor",
        "tampered_payloads_and_schema_drift_are_refused_without_partial_projection",
        "projection_validate_refuses_duplicate_drifted_and_oversized_headers",
        // Daemon spine fixtures: wire smuggling and durable restart accounting.
        "wire_supplied_quota_envelope_is_refused_before_any_effect",
        "rate_accounting_survives_a_restart_and_is_never_reset_to_zero",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture)
                || BROKER_FIXTURES.contains(fixture)
                || PROVIDER_FIXTURES.contains(fixture)
                || QUERY_FIXTURES.contains(fixture)
                || DAEMON_FIXTURES.contains(fixture),
            "INT-17 fixture missing: {fixture}"
        );
    }
    for step in [
        "cargo test -p kiana-domain --test int17_connector_quota",
        "cargo test -p kiana-capability-broker --test int17_connector_quota",
        "cargo test -p kiana-provider --test int17_connector_quota",
        "cargo test -p kiana-query --test int17_connector_quota",
        "cargo test -p kiana-daemon --test int17_connector_quota",
        "cargo test -p kiana-core --test int17_connector_quota_guard",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-17 lane step: {step}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("contents: read"));
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "--ignored",
        "|| true",
        "cargo test --workspace",
    ] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "INT-17 lane must not contain {forbidden}"
        );
    }
    // Automatic coverage: the chunked domain/core shards claim the INT-17 targets and the
    // whole-crate provider/query/broker/daemon shards discover the behaviour fixtures.
    assert!(TEST_SHARDS.contains("\"int17_connector_quota\""));
    assert!(TEST_SHARDS.contains("\"int17_connector_quota_guard\""));
    // No fixture may silence a case: deny-first ordering means the success column is last.
    for fixture in [
        DOMAIN_FIXTURES,
        BROKER_FIXTURES,
        PROVIDER_FIXTURES,
        QUERY_FIXTURES,
        DAEMON_FIXTURES,
    ] {
        assert!(!fixture.contains("#[ignore"));
        assert!(!fixture.contains("--ignored"));
    }
}
