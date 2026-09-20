#[test]
fn hook_lifecycle_dispatch_is_identity_bound_and_non_executing() {
    let dispatch = include_str!("../../kiana-query/src/hook_lifecycle_dispatch.rs");
    for marker in [
        "HookLifecycleEventKind",
        "session_id",
        "run_id",
        "snapshot_id",
        "result_committed",
        "capability_dispatch_allowed: false",
        "hook_post_tool_result_not_committed",
    ] {
        assert!(dispatch.contains(marker), "missing EXT-16 marker: {marker}");
    }
    assert!(!dispatch.contains("CapabilityBroker"));
    assert!(!dispatch.contains("std::process::Command"));
}
