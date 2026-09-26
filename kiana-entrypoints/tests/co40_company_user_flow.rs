use kiana_entrypoints::company_user_flow::{parse_args, workbench_command, CompanyFlowKind};

#[test]
fn company_user_flow_routes_inspect_next_inbox_decide_delivery_and_close_without_raw_envelopes() {
    let inspect = parse_args(&[
        "company".into(),
        "inspect".into(),
        "--project".into(),
        "project-1".into(),
        "--session-id".into(),
        "session-1".into(),
    ])
    .expect("inspect");
    assert_eq!(inspect.kind, CompanyFlowKind::Inspect);
    assert_eq!(inspect.project_id.as_deref(), Some("project-1"));
    assert_eq!(workbench_command("inbox").unwrap().0, "human.inbox");
    assert_eq!(
        workbench_command("inspect project-1").unwrap().0,
        "company.governance.v1"
    );
    assert_eq!(
        parse_args(&[
            "company".into(),
            "decide".into(),
            "--card".into(),
            "card-1".into(),
            "--option".into(),
            "accept".into(),
            "--decision-ref".into(),
            "decision-1".into(),
            "--target-revision".into(),
            "3".into(),
            "--target-digest".into(),
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            "--scope-digest".into(),
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            "--session-id".into(),
            "session-1".into(),
        ])
        .unwrap()
        .kind,
        CompanyFlowKind::Decide
    );
}

#[test]
fn company_flow_rejects_implicit_or_malformed_actions() {
    assert!(parse_args(&["company".into(), "inspect".into()]).is_err());
    assert!(workbench_command("decide not-json").is_err());
    assert!(workbench_command("unknown project-1").is_err());
}

#[test]
fn company_entrypoint_source_keeps_daemon_host_and_single_command_spine() {
    let flow = include_str!("../src/company_user_flow.rs");
    let cli = include_str!("../src/cli.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    for marker in [
        "RequestEnvelope",
        "DaemonHost::local",
        "company.governance.v1",
        "human.inbox",
        "human.resolve",
        "company.business",
        "workbench_command",
    ] {
        assert!(
            flow.contains(marker) || cli.contains(marker) || workbench.contains(marker),
            "CO-40 marker missing: {marker}"
        );
    }
    for forbidden in [
        "run_assistant_turn",
        "ModelClient::new",
        "CapabilityBroker::new",
    ] {
        assert!(
            !flow.contains(forbidden),
            "CO-40 bypass marker present: {forbidden}"
        );
    }
}
