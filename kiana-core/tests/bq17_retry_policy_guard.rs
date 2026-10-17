//! BQ-17 core source and wiring guard (deny first).
//!
//! The guard only pins source markers, refusal codes, fixture names and CI wiring.  It does not
//! execute the classifier or a reservation, and `include_str!` hits are not runtime evidence for
//! any behavior.

#[path = "support/source_guard.rs"]
mod source_guard;

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/retry_policy.rs");
const LIFECYCLE_SOURCE: &str = include_str!("../../kiana-domain/src/model_attempt_lifecycle.rs");
const SETTLEMENT_SOURCE: &str = include_str!("../../kiana-domain/src/billing_settlement_fold.rs");
const PROJECTION_SOURCE: &str = include_str!("../src/model_attempt_projection.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/bq17_retry_policy.rs");
const RUNNER_FIXTURES: &str = include_str!("../../kiana-runner/tests/bq17_retry_policy_guard.rs");
const PROVIDER_FIXTURES: &str = include_str!("../../kiana-provider/tests/bq17_retry_policy.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq17-retry-policy.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn core_retry_projection_keeps_attempt_reservation_and_unknown_fences() {
    let domain = DOMAIN_SOURCE;
    let lifecycle = LIFECYCLE_SOURCE;
    let settlement = SETTLEMENT_SOURCE;
    let projection = PROJECTION_SOURCE;
    for marker in [
        "RetryAttemptReservation",
        "QuotaReservationId",
        "request_count",
        "retry_ordinal",
        "retry_attempt_cancel_race_terminal",
        "RetryAttemptState::Settled",
        "retry_attempt_usage_exceeds_reservation",
        "ModelAttemptState::Unknown",
        "settlement",
    ] {
        assert!(
            domain.contains(marker)
                || lifecycle.contains(marker)
                || settlement.contains(marker)
                || projection.contains(marker),
            "BQ-17 core marker missing: {marker}"
        );
    }
    // The read-only projection restores the retry class from committed facts, keeps a typed
    // 429/408 attempt `Unknown` until settlement and never turns it into a success.
    for marker in [
        "retry_class",
        "provider_http_429",
        "provider_http_408",
        "TraceStatus::Unknown",
        "ModelRetryClass::Never",
    ] {
        assert!(
            projection.contains(marker),
            "BQ-17 projection marker missing: {marker}"
        );
    }
    assert!(
        !projection.contains("provider_http_503") || projection.contains("ModelRetryClass::Never")
    );
}

#[test]
fn core_does_not_authorize_retry_or_create_provider_execution_loop() {
    let core = include_str!("../src/lib.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    for marker in ["ControlPlane", "CapabilityBrokerPort", "dispatch"] {
        assert!(
            core.contains(marker) || capabilities.contains(marker) || dispatch.contains(marker),
            "BQ-17 core authority marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn(provider_retry",
        "tokio::spawn(retry_loop",
        "send_inner_attempt(connection, prepared",
        "auto_retry_unknown",
        "retry_after_cancel",
    ] {
        assert!(
            !core.contains(forbidden)
                && !capabilities.contains(forbidden)
                && !dispatch.contains(forbidden),
            "BQ-17 second authority marker present: {forbidden}"
        );
    }
    // Core never dispatches a retried provider request itself; the replay classification path
    // only labels committed facts and stays a single classifier call site.
    let projection = source_guard::production_source(PROJECTION_SOURCE);
    assert_eq!(
        projection.matches("parse_retry_class(").count(),
        2,
        "BQ-17 projection must keep one classification helper and one call site"
    );
    for forbidden in [
        "reqwest::",
        "retry_delay(",
        "tokio::time::sleep(",
        "RetryPolicy::new(",
    ] {
        assert!(
            !projection.contains(forbidden),
            "BQ-17 core projection must stay read-only: {forbidden}"
        );
    }
}

#[test]
fn bq17_refusal_codes_and_fixture_names_stay_pinned() {
    for code in [
        "recovery_disposition_not_retryable",
        "retry_class_not_allowed",
        "request_already_sent",
        "side_effect_unknown",
        "observed_delta",
        "non_idempotent",
        "cancelled",
        "attempt_budget",
        "request_budget",
        "deadline",
        "retry_after_too_large",
        "retry_policy_invalid",
        "retry_observation_phase_invalid",
        "retry_observation_code_invalid",
        "retry_decision_invalid",
        "retry_denial_invalid",
        "retry_attempt_reservation_invalid",
        "retry_capacity_lease_digest_invalid",
        "retry_attempt_dispatch_not_reserved",
        "retry_attempt_settle_state_invalid",
        "retry_attempt_unknown_state_invalid",
        "retry_attempt_cancel_race_terminal",
        "retry_attempt_usage_exceeds_reservation",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-17 domain fixture refusal code missing: {code}"
        );
    }
    for marker in [
        "retry_policy_invalid",
        "retry_observation_phase",
        "retry_observation_code",
        "retry_attempt_reservation_invalid",
        "retry_capacity_lease_digest",
        "retry_deadline_underflow",
        "retry_attempt_dispatch_not_reserved",
        "retry_attempt_settle_state_invalid",
        "retry_attempt_unknown_state_invalid",
        "retry_attempt_cancel_race_terminal",
        "retry_attempt_usage_exceeds_reservation",
        "retry_attempt_revision_overflow",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker),
            "BQ-17 domain source refusal code missing: {marker}"
        );
    }
    for code in [
        "provider_http_429",
        "provider_http_408",
        "provider_http_503",
        "provider_tls_failed",
        "provider_auth_failed",
    ] {
        assert!(
            PROVIDER_FIXTURES.contains(code) || DOMAIN_FIXTURES.contains(code),
            "BQ-17 provider status code missing from fixtures: {code}"
        );
    }

    for name in [
        "classifier_refuses_permanent_transport_failures_and_post_send_unknown",
        "classifier_allows_only_known_no_effect_429_408_or_pre_send",
        "classifier_fences_delta_unknown_non_idempotent_budget_and_retry_after",
        "refusal_reasons_are_stable_policy_codes",
        "retry_budget_and_deadline_refuse_before_scheduling_another_request",
        "retry_after_above_the_bound_or_the_deadline_never_schedules_an_early_request",
        "pre_send_backoff_is_bounded_growth_and_never_shortens_retry_after",
        "policy_validation_refuses_unbounded_or_forged_policies_before_classification",
        "forged_decision_shapes_are_refused_before_any_scheduling_use",
        "retry_contracts_round_trip_and_reject_unknown_fields",
        "bounded_retry_decisions_advance_one_request_per_attempt_and_stop_at_the_budget",
        "attempt_reservation_has_one_request_and_one_terminal_state",
        "cancellation_race_refuses_a_second_terminal_state",
        "reservation_usage_cannot_exceed_the_reserved_upper_bound",
        "reservation_identity_fields_and_digest_are_fenced",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(name),
            "BQ-17 domain fixture missing: {name}"
        );
    }
    for name in [
        "runner_owns_one_bounded_retry_driver_and_records_each_attempt_reservation",
        "runner_never_retries_unknown_effect_or_after_cancellation",
        "runner_backoff_wait_is_cancellable_and_deadline_bounded",
    ] {
        assert!(
            RUNNER_FIXTURES.contains(name),
            "BQ-17 runner fixture missing: {name}"
        );
    }
    for name in [
        "provider_retry_after_and_typed_status_boundary_is_source_visible",
        "provider_boundary_does_not_add_a_second_retry_loop_or_copy_error_bodies",
        "provider_transport_retry_fixtures_stay_injectable_and_not_ignored",
    ] {
        assert!(
            PROVIDER_FIXTURES.contains(name),
            "BQ-17 provider fixture missing: {name}"
        );
    }
    for (name, source) in [
        ("domain", DOMAIN_FIXTURES),
        ("runner", RUNNER_FIXTURES),
        ("provider", PROVIDER_FIXTURES),
    ] {
        assert!(
            !source.contains("#[ignore"),
            "BQ-17 {name} fixtures must not be ignored"
        );
    }
}

#[test]
fn bq17_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq17_retry_policy --locked -- --test-threads=1",
        "cargo test -p kiana-provider --test bq17_retry_policy --locked -- --test-threads=1",
        "cargo test -p kiana-provider --lib --locked retry -- --test-threads=1",
        "cargo test -p kiana-provider --lib --locked tls_failure -- --test-threads=1",
        "cargo test -p kiana-runner --test bq17_retry_policy_guard --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq17_retry_policy_guard --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test bq17_retry_policy_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-17 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-17 manual lane must stay dispatch-only: {forbidden}"
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
        .any(|target| target.as_str() == Some("bq17_retry_policy")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("bq17_retry_policy_guard")));
    // The provider, runner and daemon crates are whole-crate shards, so the new integration
    // fixtures and guards are discovered automatically.
    for whole in ["kiana-provider", "kiana-runner", "kiana-daemon"] {
        assert!(
            shards[whole]["targets"].is_null(),
            "{whole} must stay a whole-crate shard"
        );
    }
}
