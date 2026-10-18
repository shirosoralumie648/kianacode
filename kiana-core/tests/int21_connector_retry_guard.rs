//! INT-21 source and wiring guard: the connector retry classifier is a pure, server-bound value
//! contract.  It never dispatches, reserves, schedules or writes facts, the daemon and the
//! ControlPlane normalize route never mint retry material from the wire, and the deny-first
//! fixtures stay wired to the manual lane and the automatic CI shards.  `include_str!` assertions
//! are source-level evidence only; they are not runtime receipts.

const RETRY_SOURCE: &str = include_str!("../../kiana-domain/src/connector_retry.rs");
const OPERATION_SOURCE: &str = include_str!("../../kiana-domain/src/connector_operation.rs");
const RESERVATION_SOURCE: &str = include_str!("../../kiana-domain/src/connector_reservation.rs");
const EFFECT_SOURCE: &str = include_str!("../../kiana-domain/src/effect_observation.rs");
const CORE_CONNECTOR_SOURCE: &str = include_str!("../../kiana-core/src/connectors.rs");
const DAEMON_CONNECTOR_SOURCE: &str = include_str!("../../kiana-daemon/src/connectors.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/int21_connector_retry.rs");
const DAEMON_FIXTURES: &str = include_str!("../../kiana-daemon/tests/int21_connector_retry.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int21-connector-retry.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");
const BASELINE: &str = include_str!("../../docs/roadmap/int21-connector-retry-baseline.md");

#[test]
fn connector_retry_is_a_pure_bounded_server_bound_classifier() {
    for marker in [
        "CONNECTOR_RETRY_POLICY_SCHEMA",
        "CONNECTOR_RETRY_OBSERVATION_SCHEMA",
        "CONNECTOR_RETRY_DECISION_SCHEMA",
        "CONNECTOR_RETRY_BINDING_SCHEMA",
        "ConnectorRetryBinding",
        "from_command",
        "validate_observation",
        "connector_retry_binding_invalid",
        "connector_retry_binding_idempotency_conflict",
        "connector_retry_idempotency_key_mismatch",
        "ConnectorRetryClass",
        "KnownNoEffect",
        "DeclaredIdempotent",
        "UnknownEffect",
        "ApprovalDenied",
        "EpochStale",
        "ScopeDenied",
        "ValidationFailed",
        "Cancelled",
        "ConnectorRetryPolicy",
        "max_attempts",
        "max_backoff_ms",
        "attempt_timeout_ms",
        "MAX_CONNECTOR_RETRY_ATTEMPT_TIMEOUT_MS",
        "CONNECTOR_RETRY_BASE_BACKOFF_MS",
        "deadline_unix_ms",
        "next_attempt",
        "retry_after_ms",
        "idempotency_key_digest",
        "ConnectorRetryDecision",
        "validate_for_binding",
        "connector_retry_observation_invalid",
        "connector_retry_observation_effect_state_mismatch",
        "connector_retry_observation_retry_after_invalid",
        "connector_retry_decision_binding_mismatch",
        "connector_retry_decision_attempt_mismatch",
        "connector_retry_policy_digest_mismatch",
    ] {
        assert!(
            RETRY_SOURCE.contains(marker)
                || OPERATION_SOURCE.contains(marker)
                || RESERVATION_SOURCE.contains(marker)
                || EFFECT_SOURCE.contains(marker),
            "INT-21 marker missing: {marker}"
        );
    }

    // Deny-first semantics stay explicit in the classifier.
    assert!(RETRY_SOURCE.contains("effect_state == EffectObservationState::Unknown"));
    assert!(RETRY_SOURCE.contains("ConnectorRetryDenyReason::NonIdempotent"));
    assert!(RETRY_SOURCE.contains("ConnectorRetryDenyReason::RequestAlreadySent"));
    assert!(RETRY_SOURCE.contains("ConnectorRetryDenyReason::RetryModeDisabled"));
    assert!(RETRY_SOURCE.contains("checked_add(1)"));
    assert!(RETRY_SOURCE.contains("fresh connector reservation/permit"));
    // Exactly one classification entry point, and it always requires the server-owned binding.
    assert_eq!(RETRY_SOURCE.matches("pub fn classify(").count(), 1);
    assert!(RETRY_SOURCE.contains("binding: &ConnectorRetryBinding"));
    assert!(RETRY_SOURCE.contains("binding.validate_observation(observation)?"));

    // The contract is a pure value decision: no broker, no event store, no scheduler, no adapter
    // call, no filesystem, no async runtime and no second execution loop.
    for forbidden in [
        "CapabilityBroker",
        "EventStorePort",
        "authorize_and_execute",
        "ConnectorInvocationReservation",
        "ConnectorEffectPermit",
        "ConnectorDispatchLifecycle",
        "tokio",
        "std::process",
        "std::fs",
        "async fn",
        "fn schedule",
        "spawn",
        "reqwest",
        "unsafe",
    ] {
        assert!(
            !RETRY_SOURCE.contains(forbidden),
            "INT-21 classifier must not contain {forbidden}"
        );
    }

    // The ControlPlane invoke route keeps its own key whitelist (so retry material cannot be
    // smuggled in) and neither the ControlPlane nor the daemon schedules retries.
    assert!(CORE_CONNECTOR_SOURCE.contains("connector_arguments_invalid"));
    assert!(!CORE_CONNECTOR_SOURCE.contains("ConnectorRetryPolicy"));
    assert!(!CORE_CONNECTOR_SOURCE.contains("ConnectorRetryDecision"));
    assert!(!DAEMON_CONNECTOR_SOURCE.contains("ConnectorRetryPolicy"));
    assert!(!DAEMON_CONNECTOR_SOURCE.contains("ConnectorRetryDecision"));
}

#[test]
fn connector_retry_completion_slice_stays_wired_to_fixtures_lane_and_shards() {
    // Deny-first domain fixtures: unknown/authority classes first, the bounded success last.
    for fixture in [
        "unknown_non_idempotent_and_authority_classes_never_retry",
        "attempt_budget_deadline_retry_after_and_the_timeout_window_fail_closed",
        "foreign_or_forged_retry_material_is_refused_before_a_decision",
        "decisions_cannot_be_replayed_across_bindings_attempts_or_policies",
        "known_no_effect_and_declared_idempotent_retry_as_a_new_bound_attempt",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture),
            "INT-21 domain fixture missing: {fixture}"
        );
    }
    // Daemon boundary fixtures: the wire refusal comes first, the single-attempt fact last.
    for fixture in [
        "wire_supplied_retry_material_is_refused_before_any_dispatch",
        "a_completed_attempt_is_never_auto_retried_and_facts_keep_one_attempt",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "INT-21 daemon fixture missing: {fixture}"
        );
    }
    for fixtures in [DOMAIN_FIXTURES, DAEMON_FIXTURES] {
        assert!(!fixtures.contains("#[ignore"));
        assert!(!fixtures.contains("ignore ="));
    }
    assert!(DAEMON_FIXTURES.contains("WIRE_FORGED_RETRY_KEYS"));
    assert!(DAEMON_FIXTURES.contains("connector_arguments_invalid"));

    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int21_connector_retry --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test int21_connector_retry --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int21_connector_retry_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-21 lane step: {step}"
        );
    }
    assert_eq!(MANUAL_LANE.matches("--no-run").count(), 3);
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:\n  contents: read"));
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
            "INT-21 lane must not contain {forbidden}"
        );
    }

    // Automatic coverage: the domain fixture and the core guard stay in their registered shards,
    // and the daemon fixture is discovered by the whole-crate daemon shard.
    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("test shards parse");
    let shard_map = shards.as_object().expect("shard map");
    let owner = |target: &str| -> Option<String> {
        shard_map.iter().find_map(|(name, entry)| {
            let targets = entry["targets"].as_array()?;
            targets
                .iter()
                .any(|value| value.as_str() == Some(target))
                .then(|| name.clone())
        })
    };
    assert_eq!(
        owner("int21_connector_retry").as_deref(),
        Some("kiana-domain-s3/4"),
        "the domain fixture must stay in its registered shard"
    );
    assert_eq!(
        owner("int21_connector_retry_guard").as_deref(),
        Some("kiana-core-s4/6"),
        "the core guard must stay in its registered shard"
    );
    assert!(
        shards["kiana-daemon"]["targets"].is_null(),
        "the daemon fixture is discovered by the whole-crate shard"
    );

    assert!(BASELINE.contains("2026-10-17 completion slice (CI-only)"));
    assert!(BASELINE.contains("proof_level: source"));
    assert!(BASELINE.contains("connector_retry_idempotency_key_mismatch"));
    assert!(BASELINE.contains("connector_retry_decision_attempt_mismatch"));
    assert!(BASELINE.contains("attempt_timeout_ms"));
    assert!(BASELINE.contains("wire_supplied_retry_material_is_refused_before_any_dispatch"));
    assert!(
        BASELINE.contains("a_completed_attempt_is_never_auto_retried_and_facts_keep_one_attempt")
    );
}
