//! UI-41 source/documentation handoff guard.

#[test]
fn ui_handoff_keeps_partial_and_not_supported_boundaries_visible() {
    let roadmap = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let current = include_str!("../../CURRENT_STATUS.md");
    let module_map = include_str!("../../docs/module-map.md");
    let ui40 = include_str!("../../docs/roadmap/ui40-release-gate-baseline.md");
    let ui39 = include_str!("../../docs/roadmap/ui39-live-acp-baseline.md");
    let ui40_gate = include_str!("../../scripts/verify-ui40-release-evidence.sh");
    let ui_contracts = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let baseline = include_str!("../../docs/roadmap/ui41-handoff-baseline.md");
    for marker in [
        "UI-39",
        "UI-40",
        "UI-41",
        "UI-38",
        "CURRENT_STATUS",
        "module-map",
        "cross-process",
        "physical",
        "live provider",
        "scale",
        "proof-level",
        "UiEvidenceBundle",
        "UiEvidenceCase",
        "feature_status",
        "proof_level",
        "receipt_digest",
        "source_snapshot",
        "next_action",
        "limitations",
    ] {
        assert!(
            roadmap.contains(marker)
                || current.contains(marker)
                || module_map.contains(marker)
                || ui40.contains(marker)
                || ui39.contains(marker)
                || ui40_gate.contains(marker)
                || ui_contracts.contains(marker)
                || baseline.contains(marker),
            "UI-41 handoff marker missing: {marker}"
        );
    }
    for marker in [
        "implemented",
        "partial",
        "deferred",
        "not_supported",
        "reviewer",
        "source snapshot",
        "next",
        "source_snapshot",
        "feature_status",
        "proof_level",
        "receipt_digest",
        "next_action",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-41 handoff status marker missing: {marker}"
        );
    }
    assert!(!baseline.contains("overall project complete"));
    assert!(!baseline.contains("all UI cards are complete"));
}
