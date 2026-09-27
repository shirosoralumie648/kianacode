use kiana_domain::*;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn facts(cursor: u64, status: OpsAxisStatus) -> Vec<OpsDiagnosticFact> {
    OpsDiagnosticAxis::ALL
        .into_iter()
        .map(|axis| {
            let axis_status = if axis == OpsDiagnosticAxis::Health {
                status
            } else {
                OpsAxisStatus::Ready
            };
            OpsDiagnosticFact::new(
                axis,
                axis_status,
                cursor,
                (axis_status == OpsAxisStatus::Ready).then(|| D.to_owned()),
                if axis_status == OpsAxisStatus::Ready {
                    "ok"
                } else {
                    "health_unknown"
                },
                if axis_status == OpsAxisStatus::Ready {
                    "none"
                } else {
                    "inspect the redacted health evidence"
                },
            )
            .unwrap()
        })
        .collect()
}

fn input(status: OpsAxisStatus) -> OpsDiagnosticsInput {
    OpsDiagnosticsInput::new(
        OpsDiagnosticsMode::Doctor,
        9,
        100,
        facts(9, status),
        vec!["source facts are adapter supplied".to_owned()],
    )
    .unwrap()
}

#[test]
fn diagnostics_schema_and_ready_outputs_are_stable_and_redacted() {
    for schema in [
        OPS_DIAGNOSTIC_FACT_SCHEMA,
        OPS_DIAGNOSTICS_INPUT_SCHEMA,
        OPS_DIAGNOSTICS_REPORT_SCHEMA,
    ] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    let input = input(OpsAxisStatus::Ready);
    let report = OpsDiagnosticsReport::evaluate(&input).unwrap();
    assert_eq!(report.status, OpsDiagnosticsStatus::Ready);
    assert!(report.healthy);
    assert_eq!(report.exit_code, 0);
    assert_eq!(
        report.reproduction.render().unwrap(),
        "kiana ops doctor --json"
    );
    assert!(report
        .render_json()
        .unwrap()
        .contains(OPS_DIAGNOSTICS_REPORT_SCHEMA));
    let human = report.render_human().unwrap();
    assert!(human.contains("status: ready"));
    assert!(human.contains("reproduction: kiana ops doctor --json"));
    assert!(!human.contains("/home/") && !human.contains("api_key="));
}

#[test]
fn unknown_and_missing_evidence_never_become_healthy() {
    let unknown = OpsDiagnosticsReport::evaluate(&input(OpsAxisStatus::Unknown)).unwrap();
    assert_eq!(unknown.status, OpsDiagnosticsStatus::Unknown);
    assert!(!unknown.healthy);
    assert_eq!(unknown.exit_code, 30);

    let mut missing_facts = facts(9, OpsAxisStatus::Ready);
    missing_facts[0] = OpsDiagnosticFact::new(
        OpsDiagnosticAxis::Config,
        OpsAxisStatus::Missing,
        9,
        None,
        "config_evidence_missing",
        "load a trusted redacted config snapshot",
    )
    .unwrap();
    let missing = OpsDiagnosticsInput::new(
        OpsDiagnosticsMode::Preflight,
        9,
        100,
        missing_facts,
        Vec::new(),
    )
    .unwrap();
    let report = OpsDiagnosticsReport::evaluate(&missing).unwrap();
    assert_eq!(report.status, OpsDiagnosticsStatus::Blocked);
    assert!(!report.healthy);
    assert_eq!(report.exit_code, 20);
}

#[test]
fn diagnostics_reject_secret_paths_tamper_and_wrong_mode() {
    assert!(OpsDiagnosticFact::new(
        OpsDiagnosticAxis::Health,
        OpsAxisStatus::Blocked,
        9,
        Some(D.to_owned()),
        "provider failed at /home/operator",
        "use token=raw",
    )
    .is_err());

    let input = input(OpsAxisStatus::Ready);
    let mut report = OpsDiagnosticsReport::evaluate(&input).unwrap();
    report.healthy = false;
    assert_eq!(
        report.validate_against(&input).unwrap_err(),
        "ops_diagnostics_report_binding_invalid"
    );
}
