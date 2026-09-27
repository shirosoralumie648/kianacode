//! DEP-20 source guard: the quiesce gate is a read-only consistency decision.

#[test]
fn dep20_quiesce_gate_covers_every_component_and_refuses_incoherent_stores() {
    let domain = include_str!("../../kiana-domain/src/quiesce_gate.rs");
    for marker in [
        "QuiesceComponent",
        "EventLogMain",
        "EventLogWal",
        "ArtifactStore",
        "ProjectionCheckpoint",
        "MigrationRegistry",
        "QuiesceWriteState",
        "quiesce_active_writer",
        "quiesce_unflushed_remainder",
        "quiesce_wal_main_divergence",
        "quiesce_target_not_durable",
        "quiesce_projector_cursor_ahead_of_source",
        "quiesce_state_unknown",
        "quiesce_observation_wal_consistency_missing",
        "quiesce_report_cursor_leak",
    ] {
        assert!(domain.contains(marker), "DEP-20 marker missing: {marker}");
    }
    // The gate decides; it must not perform the snapshot effects it is gating.
    for forbidden in [
        "std::fs",
        "std::process",
        "File::",
        "sync_all",
        "tokio::",
        "EventStorePort",
        "ArtifactStorePort",
        "copy(",
    ] {
        assert!(
            !domain.contains(forbidden),
            "DEP-20 quiesce gate crossed effect boundary: {forbidden}"
        );
    }
}

#[test]
fn dep20_a_refused_decision_never_hands_back_a_snapshot_cursor() {
    let domain = include_str!("../../kiana-domain/src/quiesce_gate.rs");
    assert!(
        domain.contains("if self.status != QuiesceStatus::Quiesced && self.snapshot_cursor != 0"),
        "DEP-20 must not return a cursor for a store it refused to quiesce"
    );
    // Unknown is checked before Inconsistent: an unestablished state cannot be called inconsistent.
    let unknown_at = domain.find("quiesce_state_unknown");
    let draining_at = domain.find("quiesce_active_writer");
    let inconsistent_at = domain.find("quiesce_target_not_durable");
    assert!(unknown_at < draining_at && draining_at < inconsistent_at);
}
