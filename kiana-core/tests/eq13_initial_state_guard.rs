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
    ]
    .join("\n");
    for marker in [
        "EvalInitialStateBundle",
        "EvalInitialStateStore",
        "EVAL_INITIAL_STATE_SCHEMA",
        "initial_state_digest",
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
}
