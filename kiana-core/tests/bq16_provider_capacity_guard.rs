//! BQ-16 provider capacity source and wiring guard (deny first).
//!
//! The guard only pins source markers, refusal codes, fixture names and CI wiring.  It does not
//! execute the controller, open a socket or mint a permit; `include_str!` hits are not runtime
//! evidence for any behavior.

#[path = "support/source_guard.rs"]
mod source_guard;

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/provider_capacity.rs");
const PROVIDER_CAPACITY_SOURCE: &str = include_str!("../../kiana-provider/src/capacity.rs");
const PROVIDER_TRANSPORT_SOURCE: &str = include_str!("../../kiana-provider/src/transport.rs");
const PROVIDER_CONFIG_SOURCE: &str = include_str!("../../kiana-provider/src/config.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/bq16_provider_capacity.rs");
const PROVIDER_FIXTURES: &str =
    include_str!("../../kiana-provider/tests/bq16_provider_capacity.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq16-provider-capacity.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn bq16_capacity_contract_is_deny_first_and_control_plane_bound() {
    let domain = DOMAIN_SOURCE;
    let provider_capacity = PROVIDER_CAPACITY_SOURCE;
    let provider = source_guard::production_source(PROVIDER_TRANSPORT_SOURCE);
    let config = PROVIDER_CONFIG_SOURCE;
    for marker in [
        "ProviderCapacityPolicy",
        "ProviderCapacityQueue",
        "ProviderCapacityRequest",
        "ProviderCapacityOutcome",
        "ProviderCapacityOutcomeKind",
        "ProviderCapacityUsage",
        "ProviderCapacityController",
        "ProviderCapacityLease",
        "queue_wait_ms",
        "retry_after_ms",
        "is_dispatchable",
        "provider_capacity_quota_group_mismatch",
        "provider_capacity_window_mismatch",
        "provider_capacity_queue_full",
        "provider_capacity_queue_duplicate_attempt",
        "provider_capacity_concurrency_full",
        "provider_capacity_quota_window_exhausted",
        "provider_capacity_deadline_expired",
        "provider_capacity_queue_wait_expired",
        "provider_capacity_lease_unknown",
        "provider_capacity_lease_owner_mismatch",
        "provider_capacity_release_without_permit",
        "provider_capacity_tokens_invalid",
        "deadline_unix_ms",
        "tokens_per_minute",
        "requests_per_minute",
        "CapacityWindow",
        "provider_capacity_policy_invalid",
        "provider_capacity_quota_exhausted",
        "provider_capacity_clock_rollback",
        "provider_capacity_window_request_invalid",
        "provider_capacity_window_overflow",
        "try_acquire_owned",
        "acquire_owned",
        "drop(queue_slot)",
        "provider_capacity_tpm_exceeded",
        "capacity_scope",
        "share_capacity_gates",
        "KIANA_MODEL_QUEUE_LIMIT",
        "KIANA_MODEL_MAX_CONCURRENCY",
        "ControlPlane",
    ] {
        assert!(
            domain.contains(marker)
                || provider_capacity.contains(marker)
                || provider.contains(marker)
                || config.contains(marker)
                || include_str!("../src/lib.rs").contains(marker),
            "BQ-16 source marker missing: {marker}"
        );
    }
    let attempt = source_guard::item_source(&provider, "async fn send_inner_attempt(");
    let circuit = source_guard::compact_code(&source_guard::item_source(
        &provider,
        "async fn send_inner(",
    ));
    assert_eq!(
        circuit
            .matches("send_inner_attempt(connection,prepared,sink)")
            .count(),
        1
    );
    assert!(attempt.contains("try_acquire_owned"));
    assert!(attempt.contains("acquire_owned"));
    for forbidden in [
        "tokio::spawn",
        "fallback.unwrap()",
        "send_inner_attempt(connection, prepared",
        "provider_loop",
    ] {
        assert!(
            !if forbidden == "send_inner_attempt(connection, prepared" {
                attempt.contains(forbidden)
            } else {
                provider.contains(forbidden)
            },
            "BQ-16 provider boundary widened with forbidden marker: {forbidden}"
        );
    }
}

#[test]
fn bq16_refusal_codes_and_fixture_names_stay_pinned_in_the_fixtures() {
    for code in [
        "provider_capacity_policy_invalid",
        "provider_capacity_request_invalid",
        "provider_capacity_clock_rollback",
        "provider_capacity_quota_group_mismatch",
        "provider_capacity_window_mismatch",
        "provider_capacity_outcome_invalid",
        "provider_capacity_accept_fields_invalid",
        "provider_capacity_queue_fields_invalid",
        "provider_capacity_retry_after_missing",
        "provider_capacity_queue_entry_invalid",
        "provider_capacity_queue_duplicate_attempt",
        "provider_capacity_queue_full",
        "provider_capacity_concurrency_full",
        "provider_capacity_quota_window_exhausted",
        "provider_capacity_queue_wait_expired",
        "provider_capacity_lease_invalid",
        "provider_capacity_lease_unknown",
        "provider_capacity_lease_owner_mismatch",
        "provider_capacity_release_without_permit",
        "provider_capacity_tokens_invalid",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-16 domain fixture refusal code missing: {code}"
        );
    }
    for code in [
        "provider_capacity_policy_invalid",
        "provider_capacity_window_request_invalid",
        "provider_capacity_clock_rollback",
        "provider_capacity_window_overflow",
        "provider_capacity_quota_exhausted",
    ] {
        assert!(
            PROVIDER_CAPACITY_SOURCE.contains(code),
            "BQ-16 provider capacity code missing: {code}"
        );
    }
    for code in [
        "provider_capacity_window_request_invalid",
        "provider_capacity_window_overflow",
        "provider_capacity_quota_exhausted",
        "provider_capacity_queue_full",
        "provider_capacity_clock_rollback",
        "provider_capacity_tpm_exceeded",
        "provider_capacity_closed",
    ] {
        assert!(
            PROVIDER_FIXTURES.contains(code),
            "BQ-16 provider fixture refusal code missing: {code}"
        );
    }

    for name in [
        "outcomes_are_typed_and_queue_is_session_fair_and_bounded",
        "queued_sessions_rotate_fairly_after_the_active_slot_is_released",
        "queued_request_deadline_is_not_extended_at_dispatch",
        "quota_delay_does_not_consume_active_slot_and_group_alias_cannot_bypass",
        "accept_queue_delay_and_reject_outcomes_are_typed_and_observable",
        "request_scope_drift_and_window_mismatch_are_refused_before_queueing",
        "queue_is_bounded_fair_and_refuses_duplicate_or_unknown_entries",
        "cancelled_entry_is_never_dispatched_and_active_lease_is_not_cancellable",
        "dispatch_backpressure_delays_without_holding_a_slot_or_losing_the_entry",
        "queue_wait_and_tickets_stay_observable_per_session",
        "lease_release_is_owner_scoped_and_usage_rejects_leaseless_release",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(name),
            "BQ-16 domain fixture missing: {name}"
        );
    }
    for name in [
        "transport_keeps_waiters_bounded_and_releases_queue_before_active_slot",
        "provider_capacity_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards",
    ] {
        assert!(
            PROVIDER_FIXTURES.contains(name),
            "BQ-16 provider fixture missing: {name}"
        );
    }
    for name in [
        "capacity_permits_are_bounded_and_released_on_drop",
        "aliased_connections_cannot_double_spend_the_shared_capacity_budget",
    ] {
        assert!(
            PROVIDER_TRANSPORT_SOURCE.contains(name),
            "BQ-16 transport fixture missing: {name}"
        );
    }
    for name in [
        "reserve_keeps_rpm_and_tpm_in_the_fixed_utc_minute_and_rolls_the_window",
        "reserve_fails_closed_on_invalid_input_clock_rollback_and_overflow",
    ] {
        assert!(
            PROVIDER_CAPACITY_SOURCE.contains(name),
            "BQ-16 capacity fixture missing: {name}"
        );
    }

    for (name, source) in [
        ("domain", DOMAIN_FIXTURES),
        ("provider", PROVIDER_FIXTURES),
        ("provider_capacity", PROVIDER_CAPACITY_SOURCE),
        ("provider_transport", PROVIDER_TRANSPORT_SOURCE),
        ("provider_config", PROVIDER_CONFIG_SOURCE),
    ] {
        assert!(
            !source.contains("#[ignore"),
            "BQ-16 {name} fixtures must not be ignored"
        );
    }
}

#[test]
fn bq16_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq16_provider_capacity --locked -- --test-threads=1",
        "cargo test -p kiana-provider --test bq16_provider_capacity --locked -- --test-threads=1",
        "cargo test -p kiana-provider --lib --locked capacity -- --test-threads=1",
        "cargo test -p kiana-core --test bq16_provider_capacity_guard --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test bq16_provider_capacity_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-16 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-16 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("contents: read"));

    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    let domain_targets = shards["kiana-domain-s1/4"]["targets"]
        .as_array()
        .expect("domain shard targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("bq16_provider_capacity")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("bq16_provider_capacity_guard")));
    // The provider and daemon crates are whole-crate shards, so the new in-crate and integration
    // fixtures plus the daemon guard are discovered automatically.
    assert!(shards["kiana-provider"]["targets"].is_null());
    assert!(shards["kiana-daemon"]["targets"].is_null());
}
