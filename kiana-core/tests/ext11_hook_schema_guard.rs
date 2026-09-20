#[test]
fn hook_schema_normalizes_events_without_changing_decision_semantics() {
    let manifest = include_str!("../../kiana-skills/src/manifest.rs");
    for marker in [
        "NormalizedHookEvent",
        "deny_sticky",
        "ask_preserved",
        "update_requires_reauthorization",
        "required_scope",
        "kiana.hook-input.v1",
        "kiana.hook-output.v1",
    ] {
        assert!(manifest.contains(marker), "missing EXT-11 marker: {marker}");
    }
    assert!(manifest.contains("unsupported hook event"));
}
