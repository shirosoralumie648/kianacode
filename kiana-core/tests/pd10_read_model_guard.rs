//! PD-10 source guard: the persistence read model stays fact-only and its fixtures stay wired.

#[test]
fn pd10_read_model_is_fact_only_and_has_no_execution_authority() {
    let source = include_str!("../src/persistence_read_model.rs");
    assert!(source.contains("project_run_state"));
    assert!(source.contains("project_invocations"));
    assert!(source.contains("receipt_from_events"));
    assert!(source.contains("source_cursor"));
    assert!(source.contains("data_epoch"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("authorize_and_execute"));
    assert!(!source.contains("EventStorePort::append"));
    assert!(!source.contains("RunnerPort::send"));
}

#[test]
fn pd10_read_model_projects_attempts_without_adding_authority() {
    let source = include_str!("../src/persistence_read_model.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    for marker in [
        "attempts: Vec<CapabilityAttemptRecord>",
        "project_capability_attempts",
        "persistence_read_model_attempt:",
        "persistence_read_model_attempt_scope_invalid",
    ] {
        assert!(
            source.contains(marker),
            "PD-10 attempt marker missing: {marker}"
        );
    }
    // The attempt fold must hand back a stable order: a fresh process cannot reproduce a HashMap
    // iteration order, and the read model compares rebuilds across processes.
    assert!(attempts.contains("records.sort_by_key(|record| (record.request_id, record.attempt))"));
    for forbidden in [
        "CapabilityBrokerPort",
        "authorize_and_execute",
        "RunnerPort",
    ] {
        assert!(
            !source.contains(forbidden),
            "PD-10 read model must not hold execution authority: {forbidden}"
        );
    }
}

#[test]
fn pd10_behavioural_fixtures_are_wired() {
    let fixtures = include_str!("pd10_read_model.rs");
    let workflow = include_str!("../../.github/workflows/pd10-read-model.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "cargo test -p kiana-core --test pd10_read_model --locked",
        "cargo test -p kiana-core --test pd10_read_model_guard --locked",
    ] {
        assert!(
            workflow.contains(target),
            "PD-10 lane target missing: {target}"
        );
    }
    for target in ["pd10_read_model", "pd10_read_model_guard"] {
        assert!(
            shards.contains(target),
            "PD-10 shard target missing: {target}"
        );
    }
    for fixture in [
        "read_model_rebuilds_run_invocations_and_receipt_from_facts",
        "missing_terminal_foreign_run_old_epoch_and_conflict_fail_closed",
        "duplicate_source_event_is_not_projected_twice",
        "terminal_conflict_preserves_stable_error_code_with_both_kinds",
        "missing_terminal_cannot_become_completed_even_with_other_events",
        "read_model_projects_capability_attempts_and_receipt_evidence",
        "foreign_or_conflicting_attempt_facts_fail_closed",
        "read_model_rebuilds_the_same_state_in_a_fresh_process",
    ] {
        assert!(
            fixtures.contains(fixture),
            "PD-10 fixture missing: {fixture}"
        );
    }
    for marker in [
        "JsonlEventLog",
        "current_exe",
        "KIANA_PD10_REBUILD_CHILD",
        "KIANA_PD10_REBUILD_EVENTS",
        "KIANA_PD10_REBUILD_RUN",
        "KIANA_PD10_REBUILD_OUT",
        "persistence_read_model_attempt:capability_attempt_foreign_result:",
        "persistence_read_model_attempt:capability_attempt_terminal_conflict:",
    ] {
        assert!(
            fixtures.contains(marker),
            "PD-10 fixture wiring marker missing: {marker}"
        );
    }
    // The fresh-process fixture must spawn its own test binary by exact name, and the durable
    // acceptance rebuild must not gain a write path or a runner/broker handle of its own.
    assert!(
        fixtures.contains(r#""--exact""#),
        "PD-10 fresh-process fixture must spawn a child with --exact"
    );
    assert!(!fixtures.contains("CapabilityBrokerPort"));
    assert!(!fixtures.contains("RunnerPort"));
}
