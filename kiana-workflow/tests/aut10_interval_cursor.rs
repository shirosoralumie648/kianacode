#[test]
fn durable_trigger_tick_delegates_to_the_pure_interval_cursor() {
    let source = include_str!("../src/durable.rs");
    for marker in [
        "plan_interval_due(",
        "trigger_interval_cursor_invalid",
        "batch.occurrence_keys",
        "next_at = Some(batch.next_at)",
    ] {
        assert!(
            source.contains(marker),
            "AUT-10 workflow marker missing: {marker}"
        );
    }
    assert!(!source.contains("count.min(32)"));
}
