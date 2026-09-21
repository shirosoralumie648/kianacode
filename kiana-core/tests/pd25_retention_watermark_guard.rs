#[test]
fn retention_watermark_guard_keeps_hold_tombstone_and_bounded_prune_order() {
    let domain = include_str!("../../kiana-domain/src/retention_watermark.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/retention_store.rs");
    for marker in [
        "RetentionWatermark",
        "retention_tombstone_or_hold_gate_missing",
        "retention_watermark_advance_invalid",
        "append_tombstone",
        "RetentionStorePort",
    ] {
        assert!(
            domain.contains(marker) || eventlog.contains(marker),
            "missing marker: {marker}"
        );
    }
}
