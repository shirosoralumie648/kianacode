use kiana_domain::*;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const E: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fact(
    target: RepairTarget,
    projection_cursor: u64,
    projection_generation: u64,
    result_unknown: bool,
    lease_active: bool,
    fence_valid: bool,
) -> RepairFact {
    RepairFact::new(
        target,
        9,
        Some(projection_cursor),
        projection_generation,
        2,
        3,
        4,
        lease_active,
        fence_valid,
        result_unknown,
        D,
        D,
        E,
    )
    .unwrap()
}

fn facts(projection_cursor: u64, generation: u64) -> Vec<RepairFact> {
    RepairTarget::ALL
        .into_iter()
        .map(|target| fact(target, projection_cursor, generation, false, true, true))
        .collect()
}

fn input(command: ReconcileCommand, facts: Vec<RepairFact>) -> ReconcileInput {
    ReconcileInput::new(
        OperationId::new(),
        command,
        "operator-1",
        9,
        2,
        3,
        4,
        E,
        (command == ReconcileCommand::CommitRepair).then(|| D.to_owned()),
        facts,
    )
    .unwrap()
}

#[test]
fn reconcile_schema_and_healthy_inspection_are_read_only() {
    for schema in [
        DEPLOYMENT_REPAIR_FACT_SCHEMA,
        DEPLOYMENT_RECONCILE_INPUT_SCHEMA,
        DEPLOYMENT_RECONCILE_REPORT_SCHEMA,
    ] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    let input = input(ReconcileCommand::Inspect, facts(9, 2));
    let report = ReconcileReport::evaluate(&input).unwrap();
    assert_eq!(report.status, ReconcileStatus::Healthy);
    assert!(report.read_only);
    assert!(report.planned_projection_generation.is_none());
    assert!(report.render_human().unwrap().contains("status: healthy"));
}

#[test]
fn repair_plan_and_explicit_commit_bind_generation_actor_and_approval() {
    let planned =
        ReconcileReport::evaluate(&input(ReconcileCommand::PlanRepair, facts(8, 1))).unwrap();
    assert_eq!(planned.status, ReconcileStatus::RepairNeeded);
    assert_eq!(planned.planned_projection_generation, Some(3));
    assert!(planned.read_only);
    assert!(planned.explicit_commit_required);

    let committed =
        ReconcileReport::evaluate(&input(ReconcileCommand::CommitRepair, facts(8, 1))).unwrap();
    assert_eq!(committed.status, ReconcileStatus::CommitReady);
    assert_eq!(committed.planned_projection_generation, Some(3));
    assert!(committed.read_only);
    assert!(!committed.explicit_commit_required);
}

#[test]
fn unknown_fence_cursor_and_approval_fail_closed_without_auto_rerun() {
    let mut unknown = facts(8, 1);
    unknown[2] = fact(RepairTarget::Queue, 8, 1, true, true, true);
    let report =
        ReconcileReport::evaluate(&input(ReconcileCommand::CommitRepair, unknown)).unwrap();
    assert_eq!(report.status, ReconcileStatus::Unknown);
    assert!(report.planned_projection_generation.is_none());

    let mut missing_cursor = facts(8, 1);
    missing_cursor[0].projection_cursor = None;
    missing_cursor[0].fact_digest = missing_cursor[0].digest();
    assert_eq!(
        ReconcileReport::evaluate(&input(ReconcileCommand::Inspect, missing_cursor))
            .unwrap()
            .status,
        ReconcileStatus::Unknown
    );

    let mut blocked = facts(8, 1);
    blocked[3] = fact(RepairTarget::Lease, 10, 1, false, false, false);
    assert_eq!(
        ReconcileReport::evaluate(&input(ReconcileCommand::PlanRepair, blocked))
            .unwrap()
            .status,
        ReconcileStatus::Blocked
    );

    let mut missing_approval = input(ReconcileCommand::CommitRepair, facts(8, 1));
    missing_approval.approval_digest = None;
    missing_approval.input_digest = missing_approval.digest();
    assert_eq!(
        missing_approval.validate().unwrap_err(),
        "deployment_reconcile_approval_required"
    );
}

#[test]
fn reconcile_rejects_cross_target_cursor_digest_and_actor_path_tamper() {
    let mut cross_cursor = facts(8, 1);
    cross_cursor[1].source_cursor = 10;
    cross_cursor[1].fact_digest = cross_cursor[1].digest();
    assert_eq!(
        input(ReconcileCommand::Inspect, cross_cursor)
            .validate()
            .unwrap_err(),
        "deployment_reconcile_fact_binding_invalid"
    );

    let mut cross_digest = facts(8, 1);
    cross_digest[1].source_digest = E.to_owned();
    cross_digest[1].fact_digest = cross_digest[1].digest();
    assert_eq!(
        input(ReconcileCommand::Inspect, cross_digest)
            .validate()
            .unwrap_err(),
        "deployment_reconcile_source_digest_mismatch"
    );

    let mut forged = input(ReconcileCommand::Inspect, facts(9, 2));
    forged.actor_id = "/home/operator".to_owned();
    forged.input_digest = forged.digest();
    assert!(forged.validate().unwrap_err().contains("actor"));
}
