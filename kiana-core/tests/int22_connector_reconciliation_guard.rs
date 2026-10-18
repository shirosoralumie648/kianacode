//! INT-22 source and wiring guard: an Unknown connector result is quarantined by a server-owned,
//! digest-bound reconciliation case, and only explicit provider-query/manual evidence can close it
//! by appending a successor fact.  The contract never retries, never dispatches, never writes
//! facts, and the deny-first fixtures stay wired to the manual lane and the automatic CI shards.
//! `include_str!` assertions are source-level evidence only; they are not runtime receipts.

const RECONCILIATION_SOURCE: &str =
    include_str!("../../kiana-domain/src/connector_reconciliation.rs");
const CONNECTOR_SOURCE: &str = include_str!("../../kiana-domain/src/connectors.rs");
const DAEMON_CONNECTOR_SOURCE: &str = include_str!("../../kiana-daemon/src/connectors.rs");
const DOMAIN_FIXTURES: &str =
    include_str!("../../kiana-domain/tests/int22_connector_reconciliation.rs");
const DAEMON_FIXTURES: &str =
    include_str!("../../kiana-daemon/tests/int22_connector_reconciliation.rs");
const MANUAL_LANE: &str =
    include_str!("../../.github/workflows/int22-connector-reconciliation.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");
const BASELINE: &str =
    include_str!("../../docs/roadmap/int22-connector-reconciliation-baseline.md");

#[test]
fn connector_unknowns_are_quarantined_and_reconciled_by_successor_evidence() {
    for marker in [
        "CONNECTOR_RECONCILIATION_SCHEMA",
        "CONNECTOR_RECONCILIATION_EVIDENCE_SCHEMA",
        "CONNECTOR_RECONCILIATION_SUCCESSOR_SCHEMA",
        "CONNECTOR_HUMAN_INBOX_SCHEMA",
        "ConnectorReconciliationCase",
        "ConnectorReconciliationEvidence",
        "ConnectorReconciliationSuccessor",
        "ConnectorHumanInboxItem",
        "ConnectorReconciliationSource",
        "ProviderQuery",
        "ManualEvidence",
        "ConnectorReconciliationAction",
        "AutomaticRetry",
        "ReuseIdempotencyKey",
        "ReplaceOriginalReceipt",
        "from_unknown",
        "attach_evidence",
        "commit_reconciled",
        "successor",
        "human_inbox_item",
        "authorize_automatic_retry",
        "validate_for_case",
        "original_receipt_digest",
        "reconciliation_required",
        "automatic_retry_allowed: false",
        "replaces_original_fact: false",
        "appends_new_fact: true",
        "connector_reconciliation_unknown_receipt_required",
        "connector_reconciliation_unknown_observation_required",
        "connector_reconciliation_unknown_evidence_forbidden",
        "connector_reconciliation_evidence_binding_mismatch",
        "connector_reconciliation_manual_evidence_required",
        "connector_reconciliation_query_evidence_required",
        "connector_reconciliation_automatic_retry_forbidden",
        "connector_reconciliation_case_already_reconciled",
        "connector_reconciliation_case_not_pending",
        "connector_reconciliation_successor_invalid",
        "connector_reconciliation_successor_mismatch",
        "connector_reconciliation_successor_requires_reconciled_case",
        "connector_human_inbox_item_invalid",
        "connector_human_inbox_item_mismatch",
        "connector_reconciliation_case_invalid",
        "connector_reconciliation_case_digest_mismatch",
    ] {
        assert!(
            RECONCILIATION_SOURCE.contains(marker),
            "INT-22 marker missing from the reconciliation contract: {marker}"
        );
    }

    // Deny-first semantics stay explicit: only Unknown inputs may open a case, only evidence may
    // close it, and the retry verdict is a stable refusal in every state.
    assert!(RECONCILIATION_SOURCE.contains("EffectObservationState::Unknown"));
    assert!(RECONCILIATION_SOURCE.contains("ConnectorReconciliationState::Pending"));
    assert!(RECONCILIATION_SOURCE.contains("ConnectorReconciliationState::Reconciled"));
    assert!(RECONCILIATION_SOURCE.contains("ProviderOutcome::Unknown"));
    assert!(RECONCILIATION_SOURCE
        .contains("Err(\"connector_reconciliation_automatic_retry_forbidden\".to_owned())"));
    assert_eq!(
        RECONCILIATION_SOURCE
            .matches("pub fn from_unknown(")
            .count(),
        1
    );
    assert_eq!(
        RECONCILIATION_SOURCE.matches("pub fn successor(").count(),
        1
    );
    assert_eq!(
        RECONCILIATION_SOURCE
            .matches("pub fn authorize_automatic_retry(")
            .count(),
        1
    );

    // The contract is a pure server-owned value workflow: no broker, no event store, no adapter
    // call, no scheduler, no filesystem, no async runtime and no second execution loop.
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
            !RECONCILIATION_SOURCE.contains(forbidden),
            "INT-22 reconciliation contract must not contain {forbidden}"
        );
    }

    // A committed reconciliation fact validates its successor against the committed case, so a
    // fact can never claim to have rewritten the original Unknown invocation.
    assert!(CONNECTOR_SOURCE.contains("reconciliation_successor"));
    assert!(CONNECTOR_SOURCE.contains("validate_for_case(&case)"));

    // The daemon consumes the contract through the same single spine: the quarantine projection is
    // derived from the stored receipt/observation, the unresolved list reuses it, and the explicit
    // reconcile path commits the case plus its successor.
    for marker in [
        "attach_quarantine_projection",
        "unresolved_invocation_entry",
        "ConnectorReconciliationCase::from_unknown",
        "ConnectorReconciliationReason::ProviderUnknown",
        "ConnectorReconciliationSource::ManualEvidence",
        "commit_reconciled",
        "reconciliation_case",
        "reconciliation_successor",
        "human_inbox_item",
        "connector.reconciled",
    ] {
        assert!(
            DAEMON_CONNECTOR_SOURCE.contains(marker),
            "INT-22 daemon marker missing: {marker}"
        );
    }
    assert!(!DAEMON_CONNECTOR_SOURCE.contains("ReuseIdempotencyKey"));
}

#[test]
fn reconciliation_fixtures_stay_wired_to_the_lane_and_the_auto_shards() {
    // Deny-first domain fixtures: refusals first, the success fixture last.
    for fixture in [
        "a_timeout_cannot_be_recorded_as_a_failure_or_open_a_quarantine",
        "a_quarantine_never_authorizes_a_retry_without_evidence",
        "evidence_from_another_binding_account_operation_key_or_attempt_is_refused",
        "reconciliation_appends_a_successor_and_never_rewrites_the_original_receipt",
        "explicit_reconcile_projects_the_human_inbox_and_binds_pending_and_query_evidence",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture),
            "INT-22 domain fixture missing: {fixture}"
        );
    }
    // Daemon boundary fixtures: the wire refusal and the quarantine come first, the committed
    // successor fact last.
    for fixture in [
        "an_unknown_outcome_is_quarantined_with_a_human_inbox_item_and_no_automatic_retry",
        "wire_supplied_reconciliation_material_is_refused_before_any_fact",
        "explicit_reconcile_appends_a_successor_fact_and_keeps_the_original_event",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "INT-22 daemon fixture missing: {fixture}"
        );
    }
    for fixtures in [DOMAIN_FIXTURES, DAEMON_FIXTURES] {
        assert!(!fixtures.contains("#[ignore"));
        assert!(!fixtures.contains("ignore ="));
    }
    assert!(DAEMON_FIXTURES.contains("WIRE_FORGED_RECONCILIATION_KEYS"));
    assert!(DAEMON_FIXTURES.contains("connector_arguments_invalid"));
    assert!(DAEMON_FIXTURES.contains("connector_reconciliation_binding_mismatch"));
    assert!(DAEMON_FIXTURES.contains("connector_reconciliation_not_pending"));
    assert!(DAEMON_FIXTURES.contains("assert_no_replacement_claim"));

    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int22_connector_reconciliation --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test int22_connector_reconciliation --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int22_connector_reconciliation_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-22 lane step: {step}"
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
            "INT-22 lane must not contain {forbidden}"
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
        owner("int22_connector_reconciliation").as_deref(),
        Some("kiana-domain-s3/4"),
        "the domain fixture must stay in its registered shard"
    );
    assert_eq!(
        owner("int22_connector_reconciliation_guard").as_deref(),
        Some("kiana-core-s4/6"),
        "the core guard must stay in its registered shard"
    );
    assert!(
        shards["kiana-daemon"]["targets"].is_null(),
        "the daemon fixture is discovered by the whole-crate shard"
    );

    assert!(BASELINE.contains("2026-10-17 completion slice (CI-only)"));
    assert!(BASELINE.contains("proof_level: source"));
    assert!(BASELINE.contains("connector_reconciliation_automatic_retry_forbidden"));
    assert!(BASELINE.contains("connector_reconciliation_unknown_observation_required"));
    assert!(BASELINE.contains("connector_reconciliation_successor_mismatch"));
    assert!(BASELINE.contains("connector_human_inbox_item_mismatch"));
    assert!(BASELINE.contains("wire_supplied_reconciliation_material_is_refused_before_any_fact"));
    assert!(BASELINE
        .contains("explicit_reconcile_appends_a_successor_fact_and_keeps_the_original_event"));
}
