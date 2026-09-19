//! UI-40 source guard for release/evidence closeout without false completion.

#[test]
fn ui_release_gate_requires_deny_recovery_parity_and_honest_limits() {
    let roadmap = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let current = include_str!("../../CURRENT_STATUS.md");
    let module_map = include_str!("../../docs/module-map.md");
    let cap34 = include_str!("../../docs/roadmap/cap34-conformance-baseline.md");
    let h36 = include_str!("../../docs/roadmap/h36-harness-integration-baseline.md");
    let ui39 = include_str!("../../docs/roadmap/ui39-live-acp-baseline.md");
    let ui_contracts = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let baseline = include_str!("../../docs/roadmap/ui40-release-gate-baseline.md");
    for marker in [
        "UI-32",
        "UI-33",
        "UI-38",
        "UI-39",
        "deny",
        "recovery",
        "performance",
        "CURRENT_STATUS",
        "source_snapshot",
        "exit_code",
        "Unknown",
        "UiEvidenceBundle",
        "UiEvidenceCase",
        "feature_status",
        "proof_level",
        "ui_evidence_secret_in_command_argv",
        "ui_evidence_live_proof_required",
    ] {
        assert!(
            roadmap.contains(marker)
                || current.contains(marker)
                || module_map.contains(marker)
                || cap34.contains(marker)
                || h36.contains(marker)
                || ui39.contains(marker)
                || ui_contracts.contains(marker),
            "UI-40 source marker missing: {marker}"
        );
    }
    for marker in [
        "release gate",
        "evidence bundle",
        "git diff --check",
        "not_supported",
        "partial",
        "live",
        "physical",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-40 baseline marker missing: {marker}"
        );
    }
    assert!(!baseline.contains("all UI cards complete"));
    assert!(!baseline.contains("live verified"));
}
