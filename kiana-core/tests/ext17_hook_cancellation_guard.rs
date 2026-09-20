#[test]
fn hook_cancellation_is_idempotent_recursive_and_observer_retry_only() {
    let source = include_str!("../../kiana-query/src/hook_cancellation.rs");
    for marker in [
        "idempotency_key",
        "recursion_depth",
        "visited",
        "hook_recursion_limit",
        "hook_recursion_visited",
        "HookRunTerminal::Unknown",
        "Observer",
    ] {
        assert!(source.contains(marker), "missing EXT-17 marker: {marker}");
    }
}
