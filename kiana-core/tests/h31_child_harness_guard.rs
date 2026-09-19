#[test]
fn child_harness_contract_reuses_spawn_and_cell_containment() {
    let domain = include_str!("../../kiana-domain/src/child_harness.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let cells = include_str!("../src/cell_registry.rs");
    for marker in [
        "ChildHarnessIntent",
        "ChildHarnessBounds",
        "ChildHarnessOutcome",
        "ChildHarnessCancellation",
        "scope_widening",
        "transcript_forwarded",
        "result_unknown",
        "sandbox_contained",
    ] {
        assert!(
            domain.contains(marker),
            "missing H31 domain marker: {marker}"
        );
    }
    for marker in [
        "SpawnPlan",
        "spawn_from_packet",
        "derive_swarm_child_grant",
        "commit_spawn",
        "runner_start_failed",
    ] {
        assert!(
            collaboration.contains(marker),
            "missing spawn route marker: {marker}"
        );
    }
    for marker in [
        "spawn_parent_mismatch",
        "spawn_depth_exceeded",
        "spawn_grant_not_contained",
        "spawn_budget_not_contained",
        "spawn_children_limit_exceeded",
    ] {
        assert!(
            cells.contains(marker),
            "missing child containment marker: {marker}"
        );
    }
    for forbidden in ["TeamCreate", "SendMessage", "RunnerCommand::Start"] {
        assert!(
            !domain.contains(forbidden),
            "child contract widened into a free bus: {forbidden}"
        );
    }
}

#[test]
fn child_result_is_refs_not_parent_system_prompt() {
    let domain = include_str!("../../kiana-domain/src/child_harness.rs");
    assert!(domain.contains("transcript_forwarded"));
    assert!(domain.contains("artifact_refs"));
    assert!(domain.contains("evidence_refs"));
    assert!(domain.contains("transcript_forwarded: false"));
}
