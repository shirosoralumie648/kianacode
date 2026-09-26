#[test]
fn company_parallel_keeps_swarm_graph_cell_and_control_plane_boundaries() {
    let parallel = include_str!("../../kiana-domain/src/company_parallel.rs");
    let swarm = include_str!("../../kiana-domain/src/swarm.rs");
    let graph = include_str!("../../kiana-domain/src/swarm_graph.rs");
    let cell = include_str!("../src/cell_registry.rs");
    let core = include_str!("../src/company_parallel.rs");
    for marker in [
        "COMPANY_PARALLEL_SCHEMA",
        "CompanyParallelPlan",
        "CompanyParallelSettlement",
        "CompanyPartitionOutcome",
        "merge_owner",
        "partition_ids",
        "authority_epoch",
        "isolation_digest",
        "merge_allowed",
        "ResultUnknown",
        "SwarmPlan",
        "SwarmWorkGraph",
        "WorkFingerprint",
        "CellRegistry",
        "ControlPlane",
        "validate_company_parallel_plan",
    ] {
        assert!(
            parallel.contains(marker)
                || swarm.contains(marker)
                || graph.contains(marker)
                || cell.contains(marker)
                || core.contains(marker),
            "CO-43 marker missing: {marker}"
        );
    }
    for forbidden in [
        "TeamCreate",
        "SendMessage",
        "ModelClient::new",
        "Command::new",
    ] {
        assert!(
            !parallel.contains(forbidden),
            "CO-43 bypass marker present: {forbidden}"
        );
    }
}
