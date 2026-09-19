//! DEP-41 source guard for runbook, operator handoff and capability/proof matrix integrity.

#[test]
fn dep41_keeps_evidence_fields_and_proof_ceiling_explicit() {
    let runbook = include_str!("../../docs/roadmap/dep41-operator-runbook.md");
    let matrix = include_str!("../../docs/roadmap/dep41-capability-proof-matrix.md");
    let status = include_str!("../../CURRENT_STATUS.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    let script = include_str!("../../scripts/validate-dep41-release-gate.sh");
    let workflow = include_str!("../../.github/workflows/dep41-release-gate.yml");

    for marker in [
        "DaemonHost",
        "ControlPlane",
        "result_unknown",
        "reconcile",
        "live",
        "physical",
        "operator approval",
        "source_snapshot",
        "worktree_status",
        "command_argv",
        "cwd·environment",
        "fixture·cassette",
        "exit_code",
        "status change",
        "proof-level change",
        "limitations",
        "reviewer",
    ] {
        assert!(
            runbook.contains(marker),
            "DEP-41 runbook marker missing: {marker}"
        );
    }
    for marker in [
        "feature_status",
        "proof_level",
        "implemented",
        "partial",
        "target",
        "deferred",
        "not_supported",
        "source",
        "local_behavior",
        "durable",
        "live",
        "physical",
        "CAP-33",
        "DEP-40",
        "H36",
        "CM-39",
        "UI-41",
        "CO-48",
        "ProviderLiveConnectionEvidence",
        "ContextMemoryGoldenPathEvidence",
        "UiEvidenceBundle",
        "CompanyLiveCloseoutEvidence",
        "ContainerLifecycleEvidence",
        "OrchestratedRolloutEvidence",
        "RolloutLifecycleEvidence",
        "SupplyChainReleaseEvidence",
        "ReleaseUatEvidence",
    ] {
        assert!(
            matrix.contains(marker),
            "DEP-41 matrix marker missing: {marker}"
        );
    }
    for marker in [
        "validate-dep41-release-gate.sh",
        "cargo fmt --all --check",
        "cargo check --workspace --tests --locked",
        "cargo test -p kiana-core --test dep41_release_gate_guard",
        "git diff --check",
    ] {
        assert!(
            workflow.contains(marker),
            "DEP-41 workflow marker missing: {marker}"
        );
    }
    for marker in [
        "### DEP-41",
        "source_snapshot:",
        "fixture or cassette:",
        "proof-level_change:",
        "limitations:",
        "reviewer:",
    ] {
        assert!(
            status.contains(marker),
            "CURRENT_STATUS marker missing: {marker}"
        );
    }
    assert!(roadmap.contains("<a id=\"step-dep-41\"></a>`DEP-41`"));
    assert!(script.contains("feature_status"));
    assert!(script.contains("result_unknown"));
    assert!(matrix.contains("| DEP-41 handoff gate | partial | source |"));
    assert!(!matrix.contains("DEP-41 handoff gate | implemented | durable"));
}
