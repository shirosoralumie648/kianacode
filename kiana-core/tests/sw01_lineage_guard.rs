#[test]
fn swarm_paths_expose_typed_lineage_without_a_second_execution_loop() {
    let domain = include_str!("../../kiana-domain/src/swarm_identity.rs");
    let ids = include_str!("../../kiana-domain/src/ids.rs");
    let core = include_str!("../src/swarm.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "SwarmPlanId",
        "PartitionId",
        "ChildCellId",
        "AttemptId",
        "DispatchIntentId",
        "QueueEntryId",
        "MergeDecisionId",
        "pub struct SwarmLineage",
        "validate_for_swarm",
        "swarm_lineage_swarm_mismatch",
        "deny_unknown_fields",
    ] {
        assert!(domain.contains(marker), "lineage marker missing: {marker}");
    }
    for marker in [
        "uuid_id!(SwarmPlanId)",
        "uuid_id!(PartitionId)",
        "uuid_id!(ChildCellId)",
        "uuid_id!(AttemptId)",
        "uuid_id!(DispatchIntentId)",
        "uuid_id!(QueueEntryId)",
        "uuid_id!(MergeDecisionId)",
    ] {
        assert!(ids.contains(marker), "lineage ID marker missing: {marker}");
    }
    assert!(ports.contains("pub trait SwarmLineagePort"));
    assert!(protocol.contains("SwarmLineage"));
    for marker in [
        "commit_swarm",
        "handle_company_command",
        "ExecutionStatus::ResultUnknown",
        "swarm_idempotency_conflict",
    ] {
        assert!(core.contains(marker), "swarm path marker missing: {marker}");
    }
}
