//! UI-05 source guard for atomic snapshot projection and deny-first pagination.

#[test]
fn snapshot_projector_keeps_cursor_retention_lag_and_owner_boundaries() {
    let domain = include_str!("../../kiana-domain/src/ui_snapshot.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let baseline = include_str!("../../docs/roadmap/ui05-snapshot-projector-baseline.md");
    for marker in [
        "UiSnapshotQuery",
        "UiSnapshotPageCursor",
        "UiSnapshotPage",
        "project_ui_snapshot",
        "source_cursor",
        "projection_cursor",
        "projection_lag",
        "retention_floor",
        "retention_protected_pending",
        "stable",
        "offset",
        "cursor_digest",
        "ui_snapshot_session_owner_mismatch",
        "ui_snapshot_source_cursor_mismatch",
        "ui_snapshot_cursor_source_mismatch",
        "ui_snapshot_cursor_generation_mismatch",
        "UiSnapshotV1",
        "persisted_events",
    ] {
        assert!(
            domain.contains(marker)
                || daemon.contains(marker)
                || protocol.contains(marker),
            "UI-05 source marker missing: {marker}"
        );
    }
    for marker in [
        "atomic",
        "snapshot",
        "pagination",
        "retention",
        "lag",
        "owner",
        "feature_status",
        "proof_level",
        "source",
    ] {
        assert!(baseline.contains(marker), "UI-05 baseline marker missing: {marker}");
    }
    assert!(daemon.contains("ui_snapshot_projection_unknown"));
    assert!(!daemon.contains("CapabilityBroker::new"));
}
