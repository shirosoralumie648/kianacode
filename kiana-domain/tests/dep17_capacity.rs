use kiana_domain::*;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const E: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fact(
    resource: CapacityResource,
    used: u64,
    requested: u64,
    queue_depth: u64,
    bounded: bool,
    preserved: bool,
    unknown: bool,
    deadline: Option<u64>,
    elapsed: Option<u64>,
) -> CapacityFact {
    CapacityFact::new(
        resource,
        9,
        3,
        4,
        2,
        100,
        used,
        requested,
        10,
        queue_depth,
        bounded,
        preserved,
        unknown,
        deadline,
        elapsed,
        D,
        E,
    )
    .unwrap()
}

fn facts() -> Vec<CapacityFact> {
    CapacityResource::ALL
        .into_iter()
        .map(|resource| {
            if resource == CapacityResource::Shutdown {
                fact(resource, 10, 5, 0, true, true, false, Some(100), Some(10))
            } else {
                fact(resource, 10, 5, 0, true, true, false, None, None)
            }
        })
        .collect()
}

fn input(facts: Vec<CapacityFact>) -> CapacityInput {
    CapacityInput::new(OperationId::new(), 9, 3, 4, 2, D, E, facts).unwrap()
}

#[test]
fn capacity_schema_and_ready_report_are_stable() {
    for schema in [
        DEPLOYMENT_CAPACITY_FACT_SCHEMA,
        DEPLOYMENT_CAPACITY_INPUT_SCHEMA,
        DEPLOYMENT_CAPACITY_REPORT_SCHEMA,
    ] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    let report = CapacityReport::evaluate(&input(facts())).unwrap();
    assert_eq!(report.status, CapacityReportStatus::Ready);
    assert!(report.allow_new_work);
    assert!(report.allow_existing_work);
    assert!(report.facts_preserved);
    assert!(report
        .render_json()
        .unwrap()
        .contains(DEPLOYMENT_CAPACITY_REPORT_SCHEMA));
    assert!(report.render_human().unwrap().contains("status: ready"));
}

#[test]
fn backpressure_blocks_new_work_but_preserves_existing_work() {
    let mut observed = facts();
    observed[3] = fact(
        CapacityResource::Log,
        10,
        5,
        1,
        true,
        true,
        false,
        None,
        None,
    );
    let report = CapacityReport::evaluate(&input(observed)).unwrap();
    assert_eq!(report.status, CapacityReportStatus::Backpressure);
    assert!(!report.allow_new_work);
    assert!(report.allow_existing_work);
    assert_eq!(report.diagnostics[3].stable_code(), "capacity_backpressure");

    let mut saturated = facts();
    saturated[3] = fact(
        CapacityResource::Log,
        10,
        5,
        10,
        true,
        true,
        false,
        None,
        None,
    );
    let report = CapacityReport::evaluate(&input(saturated)).unwrap();
    assert_eq!(report.status, CapacityReportStatus::Blocked);
    assert_eq!(report.diagnostics[3].stable_code(), "capacity_exceeded");
}

#[test]
fn over_limit_unbounded_and_shutdown_deadline_are_structured_rejections() {
    let mut over = facts();
    over[0] = fact(
        CapacityResource::Append,
        99,
        2,
        0,
        true,
        true,
        false,
        None,
        None,
    );
    let report = CapacityReport::evaluate(&input(over)).unwrap();
    assert_eq!(report.status, CapacityReportStatus::Blocked);
    assert_eq!(report.diagnostics[0].stable_code(), "capacity_exceeded");

    let mut unbounded = facts();
    unbounded[1] = fact(
        CapacityResource::Artifact,
        10,
        5,
        0,
        false,
        true,
        false,
        None,
        None,
    );
    assert_eq!(
        CapacityReport::evaluate(&input(unbounded))
            .unwrap()
            .diagnostics[1]
            .stable_code(),
        "capacity_unbounded"
    );

    let mut shutdown = facts();
    shutdown[6] = fact(
        CapacityResource::Shutdown,
        10,
        5,
        0,
        true,
        true,
        false,
        Some(10),
        Some(10),
    );
    let report = CapacityReport::evaluate(&input(shutdown)).unwrap();
    assert_eq!(report.status, CapacityReportStatus::Blocked);
    assert_eq!(
        report.diagnostics[6].stable_code(),
        "shutdown_deadline_exceeded"
    );
}

#[test]
fn unknown_capacity_and_missing_shutdown_deadline_never_admit() {
    let mut unknown = facts();
    unknown[2] = fact(
        CapacityResource::Operation,
        10,
        5,
        0,
        true,
        true,
        true,
        None,
        None,
    );
    let report = CapacityReport::evaluate(&input(unknown)).unwrap();
    assert_eq!(report.status, CapacityReportStatus::Unknown);
    assert!(!report.allow_new_work);
    assert!(!report.allow_existing_work);

    let mut missing = facts();
    missing[6] = fact(
        CapacityResource::Shutdown,
        10,
        5,
        0,
        true,
        true,
        false,
        None,
        None,
    );
    let report = CapacityReport::evaluate(&input(missing)).unwrap();
    assert_eq!(report.status, CapacityReportStatus::Unknown);
    assert_eq!(
        report.diagnostics[6].stable_code(),
        "capacity_evidence_unknown"
    );
}

#[test]
fn capacity_rejects_overflow_cross_binding_and_digest_tamper() {
    let mut overflow = facts();
    overflow[0] = fact(
        CapacityResource::Append,
        u64::MAX,
        1,
        0,
        true,
        true,
        false,
        None,
        None,
    );
    assert_eq!(
        CapacityReport::evaluate(&input(overflow))
            .unwrap()
            .diagnostics[0]
            .stable_code(),
        "capacity_exceeded"
    );

    let mut drift = facts();
    drift[1].source_cursor = 10;
    drift[1].fact_digest = drift[1].digest();
    assert_eq!(
        input(drift).validate().unwrap_err(),
        "deployment_capacity_fact_binding_invalid"
    );

    let mut tampered = input(facts());
    tampered.facts[0].queue_depth = 3;
    tampered.input_digest = tampered.digest();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "deployment_capacity_fact_digest_mismatch"
    );
}
