#[test]
fn bounded_swarm_keeps_authority_before_child_dispatch_and_merge() {
    let domain = include_str!("../../kiana-domain/src/swarm.rs");
    let graph = include_str!("../../kiana-domain/src/swarm_graph.rs");
    let reducer = include_str!("../../kiana-domain/src/swarm_reducer.rs");
    let core = include_str!("../src/swarm.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let company = include_str!("../src/company.rs");
    let fixture = include_str!("../../kiana-domain/tests/p4_j6_01_bounded_swarm.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j6-01-bounded-swarm-baseline.md");

    for marker in [
        "SwarmPlan",
        "BoundedSwarm",
        "SwarmCommand::Create",
        "SwarmCommand::StartChild",
        "SwarmCommand::Reconcile",
        "SwarmCommand::Merge",
        "SwarmCommand::Cancel",
        "WorkFingerprint",
        "max_concurrency",
        "max_depth",
        "expires_at",
        "max_tokens",
        "max_model_calls",
        "swarm_partition_overlap",
        "swarm_duplicate_fingerprint",
        "swarm_concurrency_exhausted",
        "swarm_dispatch_denied",
        "swarm_merge_review_invalid",
        "swarm_merge_partition_coverage_required",
        "SwarmTransitionReducer",
        "ReadyToMerge",
        "ResultUnknown",
        "cancel_swarm_children",
        "commit_swarm",
        "handle_company_command",
        "self.commit_swarm(&context, event)",
        "ensure_swarm_controller",
        "retire_cell",
        "CompanyCommand::StartRun",
        "event-before-dispatch",
    ] {
        assert!(
            domain.contains(marker)
                || graph.contains(marker)
                || reducer.contains(marker)
                || core.contains(marker)
                || collaboration.contains(marker)
                || company.contains(marker)
                || fixture.contains(marker)
                || baseline.contains(marker),
            "bounded swarm marker missing: {marker}"
        );
    }

    let committed = core
        .find("let committed = self.commit_swarm(&context, event)")
        .expect("swarm commit boundary");
    let dispatch = core
        .find("self.handle_company_command(")
        .expect("Company child dispatch boundary");
    assert!(committed < dispatch);
    assert!(core.contains("SwarmCommand::StartChild"));
    assert!(core.contains("SwarmCommand::Reconcile"));
    assert!(core.contains("SwarmCommand::Merge"));
    assert!(core.contains("ExecutionStatus::ResultUnknown"));
    assert!(core.contains("swarm_sibling_failed"));
    assert!(core.contains("swarm_merged"));
    assert!(domain.contains("swarm_transition_illegal"));
    assert!(domain.contains("review_complete"));
    assert!(!core.contains("broadcast::channel"));
    assert!(!core.contains("ProviderGateway"));
    assert!(!core.contains("ModelClient"));
}
