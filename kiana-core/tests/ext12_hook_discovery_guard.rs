#[test]
fn hook_discovery_is_read_only_deterministic_and_phase_separated() {
    let discovery = include_str!("../../kiana-skills/src/hook_discovery.rs");
    for marker in [
        "HOOK_DISCOVERY_SCHEMA",
        "source_priority",
        "declaration_order",
        "matcher_invalid",
        "guard_ids",
        "observer_ids",
        "input_digest",
    ] {
        assert!(
            discovery.contains(marker),
            "missing EXT-12 marker: {marker}"
        );
    }
    assert!(!discovery.contains("std::process::Command"));
    assert!(!discovery.contains("tokio::process"));
}
