//! DEP-33 source guard for revision pin and old-revision drain boundaries.

#[test]
fn revision_pins_and_drain_reject_drift_and_active_writers() {
    let source = include_str!("../../kiana-domain/src/revision_compatibility.rs");
    let baseline = include_str!("../../docs/roadmap/dep33-revision-compatibility-baseline.md");
    for marker in [
        "ExecutionRevisionPin",
        "build_digest",
        "workflow_digest",
        "provider_digest",
        "extension_catalog_digest",
        "skill_trust_revision",
        "replay_schema",
        "compatible_replay_with",
        "revision_replay_schema_unknown",
        "revision_workflow_digest_drift",
        "revision_provider_digest_drift",
        "revision_extension_catalog_drift",
        "revision_skill_trust_drift",
        "RevisionDrain",
        "active_run_count",
        "active_writer_count",
        "replacement_ready",
        "revision_drain_not_empty",
        "revision_drain_deadline_exceeded",
    ] {
        assert!(
            source.contains(marker),
            "DEP-33 source marker missing: {marker}"
        );
    }
    for marker in [
        "静默换 definition",
        "provider",
        "extension",
        "replay version",
        "project skill",
        "old revision",
        "drain",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-33 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("std::fs"));
}
