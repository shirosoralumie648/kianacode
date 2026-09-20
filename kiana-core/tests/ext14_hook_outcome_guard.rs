#[test]
fn hook_outcomes_are_closed_and_fail_closed() {
    let outcome = include_str!("../../kiana-query/src/hook_outcome.rs");
    for marker in [
        "HookOutcome",
        "AdditionalContext",
        "Timeout",
        "Cancelled",
        "Unknown",
        "hook_output_unknown_field",
        "hook_update_requires_reauthorization",
        "hook_patch_limit_exceeded",
    ] {
        assert!(outcome.contains(marker), "missing EXT-14 marker: {marker}");
    }
}
