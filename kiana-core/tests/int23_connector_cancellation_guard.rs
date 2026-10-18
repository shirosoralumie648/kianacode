//! INT-23 source and wiring guard: connector cancellation is a pure domain settlement over
//! server-owned stop evidence.  An unconfirmed stop can never be recorded as `not_executed`, a
//! started effect can never release its lease, a late provider result can only be kept as fenced
//! evidence, and the three terminal outcomes stay reconstructible from the durable facts.  The
//! contract never dispatches, never writes facts, never authorizes and never sends a signal; the
//! deny-first fixtures stay wired to the manual lane and the automatic CI shards.
//! `include_str!` assertions are source-level evidence only; they are not runtime receipts.

const CANCELLATION_SOURCE: &str = include_str!("../../kiana-domain/src/connector_cancellation.rs");
const DISPATCH_SOURCE: &str = include_str!("../../kiana-domain/src/connector_dispatch.rs");
const STOP_SOURCE: &str = include_str!("../../kiana-domain/src/process_supervisor.rs");
const PORTS_SOURCE: &str = include_str!("../../kiana-ports/src/connector.rs");
const DOMAIN_FIXTURES: &str =
    include_str!("../../kiana-domain/tests/int23_connector_cancellation.rs");
const DAEMON_FIXTURES: &str =
    include_str!("../../kiana-daemon/tests/int23_connector_cancellation.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int23-connector-cancellation.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");
const BASELINE: &str = include_str!("../../docs/roadmap/int23-connector-cancellation-baseline.md");

#[test]
fn connector_cancellation_settlement_fences_late_results_and_lease_release() {
    for marker in [
        "ConnectorCancellationSettlement",
        "ConnectorCancellationState",
        "NotExecuted",
        "StopConfirmed",
        "Unknown",
        "HeldForReconciliation",
        "connector_cancellation_late_result_fenced",
        "connector_cancellation_effect_already_observed",
        "late_result_fenced",
        "lease_settlement",
        "StopReport",
        "cancel_checked",
        "connector_cancel_request_invalid",
        "CONNECTOR_CANCELLATION_RECORD_SCHEMA",
        "ConnectorCancellationRecord",
        "ConnectorLateResultDisposition",
        "Fenced",
        "reconstruct_outcome",
        "record_late_result",
        "late_result_disposition",
        "fenced_late_results",
        "expected_settlement",
        "validate_for_stop_report",
        "connector_cancellation_stop_attempt_missing",
        "connector_cancellation_started_effect_cannot_release_lease",
        "connector_cancellation_settlement_mismatch",
        "connector_cancellation_stop_report_mismatch",
        "connector_cancellation_late_result_not_late",
        "connector_cancellation_late_result_invocation_mismatch",
        "connector_cancellation_late_result_attempt_mismatch",
        "connector_cancellation_lease_settlement_invalid",
        "connector_cancellation_record_invalid",
        "connector_cancellation_record_digest_mismatch",
    ] {
        assert!(
            CANCELLATION_SOURCE.contains(marker)
                || DISPATCH_SOURCE.contains(marker)
                || STOP_SOURCE.contains(marker)
                || PORTS_SOURCE.contains(marker),
            "INT-23 marker missing: {marker}"
        );
    }

    assert!(CANCELLATION_SOURCE.contains("ConnectorLeaseSettlement::Released"));
    assert!(CANCELLATION_SOURCE.contains("ConnectorLeaseSettlement::HeldForReconciliation"));
    assert!(CANCELLATION_SOURCE.contains("self.late_result_fenced"));
    assert!(CANCELLATION_SOURCE.contains("ConnectorDispatchStage::Dispatching"));

    // The record has exactly one constructor per server-owned derivation, and the reconstruction
    // is the single entry point that re-derives the terminal outcome from the stop report.
    assert_eq!(
        CANCELLATION_SOURCE
            .matches("pub fn from_stop_report(")
            .count(),
        2,
        "the settlement and the record each derive from a StopReport"
    );
    assert_eq!(
        CANCELLATION_SOURCE
            .matches("pub fn reconstruct_outcome(")
            .count(),
        1
    );
    assert_eq!(
        CANCELLATION_SOURCE
            .matches("pub fn record_late_result(")
            .count(),
        1
    );
    assert_eq!(
        CANCELLATION_SOURCE
            .matches("pub fn late_result_disposition(")
            .count(),
        1
    );

    // The contract is a pure server-owned value workflow: no broker, no event store, no adapter
    // call, no signal, no filesystem, no async runtime and no second execution loop.
    for forbidden in [
        "CapabilityBroker",
        "EventStorePort",
        "authorize_and_execute",
        "tokio",
        "std::process",
        "std::fs",
        "async fn",
        "spawn",
        "reqwest",
        "unsafe",
        "kill(",
        "Command::new",
    ] {
        assert!(
            !CANCELLATION_SOURCE.contains(forbidden),
            "the cancellation contract must not contain {forbidden}"
        );
    }
}

#[test]
fn cancellation_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    // Deny-first domain fixtures: the card-named refusals first, the reconstruction last.
    for fixture in [
        "a_cancellation_without_confirmed_stop_evidence_never_reports_not_executed",
        "a_started_effect_cannot_release_its_lease_or_be_recorded_as_not_executed",
        "a_late_result_cannot_resurrect_a_settled_attempt",
        "the_three_cancellation_outcomes_reconstruct_from_stop_evidence",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture),
            "INT-23 domain fixture missing: {fixture}"
        );
    }
    let success = DOMAIN_FIXTURES
        .find("the_three_cancellation_outcomes_reconstruct_from_stop_evidence")
        .expect("success fixture");
    for denial in [
        "a_cancellation_without_confirmed_stop_evidence_never_reports_not_executed",
        "a_started_effect_cannot_release_its_lease_or_be_recorded_as_not_executed",
        "a_late_result_cannot_resurrect_a_settled_attempt",
    ] {
        let position = DOMAIN_FIXTURES
            .find(denial)
            .unwrap_or_else(|| panic!("deny fixture missing: {denial}"));
        assert!(
            position < success,
            "deny-first ordering broken: {denial} must run before the success fixture"
        );
    }

    // Daemon boundary fixtures: the wire refusal comes first, the durable reconstruction last.
    for fixture in [
        "wire_supplied_cancellation_material_is_refused_before_any_fact",
        "a_settled_attempt_reconstructs_from_durable_facts_and_fences_late_results",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "INT-23 daemon fixture missing: {fixture}"
        );
    }
    let wire_refusal = DAEMON_FIXTURES
        .find("wire_supplied_cancellation_material_is_refused_before_any_fact")
        .expect("wire refusal fixture");
    let durable_reconstruction = DAEMON_FIXTURES
        .find("a_settled_attempt_reconstructs_from_durable_facts_and_fences_late_results")
        .expect("durable reconstruction fixture");
    assert!(wire_refusal < durable_reconstruction);

    for fixtures in [DOMAIN_FIXTURES, DAEMON_FIXTURES] {
        assert!(!fixtures.contains("#[ignore"));
        assert!(!fixtures.contains("ignore ="));
    }
    assert!(DAEMON_FIXTURES.contains("WIRE_FORGED_CANCELLATION_KEYS"));
    assert!(DAEMON_FIXTURES.contains("connector_arguments_invalid"));
    assert!(DAEMON_FIXTURES.contains("connector_action_invalid"));
    assert!(DAEMON_FIXTURES.contains("connector_cancellation_effect_already_observed"));
    assert!(DAEMON_FIXTURES.contains("connector_cancellation_stop_attempt_missing"));
    assert!(DAEMON_FIXTURES.contains("CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND"));

    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int23_connector_cancellation --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test int23_connector_cancellation --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int23_connector_cancellation_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-23 lane step: {step}"
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
            "INT-23 lane must not contain {forbidden}"
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
        owner("int23_connector_cancellation").as_deref(),
        Some("kiana-domain-s3/4"),
        "the domain fixtures must stay in their registered shard"
    );
    assert_eq!(
        owner("int23_connector_cancellation_guard").as_deref(),
        Some("kiana-core-s4/6"),
        "the core guard must stay in its registered shard"
    );
    assert!(
        shards["kiana-daemon"]["targets"].is_null(),
        "the daemon fixture is discovered by the whole-crate shard"
    );

    assert!(BASELINE.contains("2026-10-17 completion slice (CI-only)"));
    assert!(BASELINE.contains("proof_level: source"));
    assert!(BASELINE.contains("connector_cancellation_stop_attempt_missing"));
    assert!(BASELINE.contains("connector_cancellation_late_result_fenced"));
    assert!(BASELINE.contains("ConnectorCancellationRecord"));
    assert!(BASELINE.contains("a_late_result_cannot_resurrect_a_settled_attempt"));
    assert!(BASELINE.contains("the_three_cancellation_outcomes_reconstruct_from_stop_evidence"));
    assert!(BASELINE.contains("wire_supplied_cancellation_material_is_refused_before_any_fact"));
}
