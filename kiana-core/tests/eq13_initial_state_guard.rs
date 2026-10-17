#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn evaluation_initial_state_uses_digest_bound_controlled_fixture_store() {
    let runtime =
        source_guard::production_source(include_str!("../../kiana-daemon/src/eval_runtime.rs"));
    let boundary = [
        source_guard::item_source(&runtime, "pub struct EvalInitialStateBundle"),
        source_guard::item_source(&runtime, "impl EvalInitialStateBundle"),
        source_guard::item_source(&runtime, "pub struct EvalInitialStateStore"),
        source_guard::item_source(&runtime, "impl EvalInitialStateStore"),
        source_guard::item_source(&runtime, "impl FixtureStore for EvalInitialStateStore"),
        source_guard::item_source(&runtime, "pub struct EvalInitialStateMaterialization"),
        source_guard::item_source(&runtime, "impl EvalInitialStateMaterialization"),
    ]
    .join("\n");
    for marker in [
        "EvalInitialStateBundle",
        "EvalInitialStateStore",
        "EvalInitialStateMaterialization",
        "EVAL_INITIAL_STATE_SCHEMA",
        "EVAL_INITIAL_STATE_MATERIALIZATION_SCHEMA",
        "materialize_initial_state",
        "initial_state_digest",
        "fixture_digests",
        "FixtureStore",
        "scope_digest",
        "canonical_journal_bytes",
        "redact_value",
    ] {
        assert!(
            runtime.contains(marker),
            "initial-state marker missing: {marker}"
        );
    }
    for forbidden in [
        "fs::read",
        "read_to_string",
        "std::env::var",
        "KianaHarness",
        "CapabilityBroker {",
        "tokio::spawn",
        "reqwest",
        "SecretStore",
    ] {
        assert!(
            !boundary.contains(forbidden),
            "uncontrolled fixture path found: {forbidden}"
        );
    }
    // The sandbox materializer must build every path from the owned sandbox KIANA_HOME and must
    // never discover the operator home, the process cwd or a network/secret dependency.
    let materializer = source_guard::item_source(&runtime, "pub fn materialize_initial_state");
    assert!(materializer.contains("self.kiana_home"));
    for forbidden in [
        "std::env::var",
        "current_dir",
        "home_dir",
        "KianaHarness",
        "reqwest",
    ] {
        assert!(
            !materializer.contains(forbidden),
            "materializer escape found: {forbidden}"
        );
    }
}
