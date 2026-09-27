use kiana_domain::*;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn route() -> AlertRoute {
    AlertRoute::new("route:incident", 3, "human-inbox", true).unwrap()
}

fn runbook(phase: IncidentPhase) -> RunbookRef {
    RunbookRef::new(
        "runbook:deployment",
        D,
        phase,
        "follow the reviewed phase checklist",
    )
    .unwrap()
}

fn evidence(
    operation_id: OperationId,
    phase: IncidentPhase,
    cursor: u64,
    unknown: bool,
    observed_at_ms: u64,
    deadline_ms: u64,
) -> RunbookEvidence {
    let reference = runbook(phase);
    let alert = route();
    RunbookEvidence::new(
        "incident:one",
        operation_id,
        D,
        phase,
        "operator-1",
        cursor,
        3,
        4,
        reference.ref_digest,
        alert.route_digest,
        observed_at_ms,
        deadline_ms,
        unknown,
        if unknown {
            "evidence_unknown"
        } else {
            "observed"
        },
    )
    .unwrap()
}

fn input(
    operation_id: OperationId,
    phase: IncidentPhase,
    cursor: u64,
    history: Vec<RunbookEvidence>,
    unknown: bool,
    observed_at_ms: u64,
    deadline_ms: u64,
) -> IncidentInput {
    IncidentInput::new(
        "incident:one",
        operation_id,
        D,
        "operator-1",
        phase,
        cursor,
        3,
        4,
        runbook(phase),
        route(),
        history,
        evidence(
            operation_id,
            phase,
            cursor,
            unknown,
            observed_at_ms,
            deadline_ms,
        ),
    )
    .unwrap()
}

#[test]
fn incident_schema_and_ordered_phase_are_replayable() {
    for schema in [
        DEPLOYMENT_INCIDENT_SCHEMA,
        DEPLOYMENT_RUNBOOK_REF_SCHEMA,
        DEPLOYMENT_ALERT_ROUTE_SCHEMA,
        DEPLOYMENT_RUNBOOK_EVIDENCE_SCHEMA,
        DEPLOYMENT_INCIDENT_REPORT_SCHEMA,
    ] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    let operation = OperationId::new();
    let observed = input(
        operation,
        IncidentPhase::Observed,
        1,
        Vec::new(),
        false,
        10,
        100,
    );
    assert_eq!(
        IncidentReport::evaluate(&observed).unwrap().status,
        IncidentDecisionStatus::Accepted
    );

    let observed_evidence = observed.evidence.clone();
    let triaged = input(
        operation,
        IncidentPhase::Triaged,
        2,
        vec![observed_evidence],
        false,
        20,
        100,
    );
    let triaged_report = IncidentReport::evaluate(&triaged).unwrap();
    assert_eq!(triaged_report.phase, IncidentPhase::Triaged);
    assert_eq!(triaged_report.status, IncidentDecisionStatus::Accepted);

    let replay = input(
        operation,
        IncidentPhase::Observed,
        1,
        vec![observed.evidence.clone()],
        false,
        10,
        100,
    );
    assert_eq!(
        IncidentReport::evaluate(&replay).unwrap().status,
        IncidentDecisionStatus::Replayed
    );
    assert!(IncidentReport::evaluate(&replay)
        .unwrap()
        .render_human()
        .unwrap()
        .contains("replayed"));
}

#[test]
fn incident_rejects_skip_close_unknown_and_expired_deadline() {
    let operation = OperationId::new();
    let skipped = input(
        operation,
        IncidentPhase::Contained,
        1,
        Vec::new(),
        false,
        10,
        100,
    );
    assert_eq!(
        skipped.validate().unwrap_err(),
        "deployment_incident_phase_order_invalid"
    );

    let observed = input(
        operation,
        IncidentPhase::Observed,
        1,
        Vec::new(),
        false,
        10,
        100,
    );
    let triaged = input(
        operation,
        IncidentPhase::Triaged,
        2,
        vec![observed.evidence.clone()],
        false,
        20,
        100,
    );
    let contained = input(
        operation,
        IncidentPhase::Contained,
        3,
        vec![observed.evidence.clone(), triaged.evidence.clone()],
        false,
        30,
        100,
    );
    let recovering = input(
        operation,
        IncidentPhase::Recovering,
        4,
        vec![observed.evidence, triaged.evidence, contained.evidence],
        false,
        40,
        100,
    );
    let close = input(
        operation,
        IncidentPhase::Closed,
        5,
        vec![
            recovering.history[0].clone(),
            recovering.history[1].clone(),
            recovering.history[2].clone(),
            recovering.evidence.clone(),
        ],
        false,
        50,
        100,
    );
    assert_eq!(
        close.validate().unwrap_err(),
        "deployment_incident_phase_order_invalid"
    );

    let unknown = input(
        operation,
        IncidentPhase::Triaged,
        2,
        vec![recovering.history[0].clone()],
        true,
        20,
        100,
    );
    assert_eq!(
        IncidentReport::evaluate(&unknown).unwrap().status,
        IncidentDecisionStatus::Unknown
    );

    let expired = input(
        operation,
        IncidentPhase::Observed,
        1,
        Vec::new(),
        false,
        100,
        100,
    );
    assert_eq!(
        IncidentReport::evaluate(&expired).unwrap().status,
        IncidentDecisionStatus::Unknown
    );
}

#[test]
fn incident_rejects_route_authority_expansion_secret_and_digest_tamper() {
    let operation = OperationId::new();
    let mut route = route();
    route.expands_authority = true;
    route.route_digest = route.digest();
    assert_eq!(
        IncidentInput::new(
            "incident:one",
            operation,
            D,
            "operator-1",
            IncidentPhase::Observed,
            1,
            3,
            4,
            runbook(IncidentPhase::Observed),
            route,
            Vec::new(),
            evidence(operation, IncidentPhase::Observed, 1, false, 10, 100),
        )
        .unwrap_err(),
        "deployment_alert_route_header_invalid"
    );

    assert!(RunbookRef::new("/home/operator", D, IncidentPhase::Observed, "safe").is_err());

    let mut valid = input(
        operation,
        IncidentPhase::Observed,
        1,
        Vec::new(),
        false,
        10,
        100,
    );
    valid.evidence.actor_id = "operator-2".to_owned();
    valid.evidence.evidence_digest = valid.evidence.digest();
    valid.input_digest = valid.digest();
    assert_eq!(
        valid.validate().unwrap_err(),
        "deployment_incident_evidence_binding_invalid"
    );
}
