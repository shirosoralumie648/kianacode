use kiana_domain::{
    CompanyCommand, CompanyCommandReceipt, CompanyReceiptStatus, DispatchIntent,
    DispatchIntentStatus, RequestContext,
};

fn context() -> RequestContext {
    let mut context = RequestContext::local("co07", "/tmp/kiana-co07");
    context.project_trusted = true;
    context.permission_profile = kiana_domain::PermissionProfile::Balanced;
    context
}

#[test]
fn company_receipt_is_stable_for_retries_and_binds_dispatch_intent() {
    let command = CompanyCommand::StartRun {
        project_id: "project-1".to_owned(),
        packet_id: "packet-1".to_owned(),
        sandbox: Some("workspace-write".to_owned()),
    };
    let context = context();
    let aggregate = "local-user\n/tmp/kiana-co07";
    let command_id = CompanyCommandReceipt::command_id(aggregate, "logical-1");
    let intent = DispatchIntent::new(
        command_id,
        "company.start_run",
        CompanyCommandReceipt::payload_digest(&command),
        CompanyCommandReceipt::authority_digest(&context),
        100,
    )
    .unwrap();
    assert_eq!(intent.status, DispatchIntentStatus::Prepared);
    let receipt = CompanyCommandReceipt::new(
        aggregate,
        "logical-1",
        &command,
        &context,
        4,
        Some(5),
        Some(kiana_domain::EventId::new()),
        CompanyReceiptStatus::Committed,
        Some(intent),
    )
    .unwrap();
    receipt.validate().unwrap();
    assert_eq!(receipt.command_id, command_id);
    assert_eq!(
        receipt.command_id,
        CompanyCommandReceipt::command_id(aggregate, "logical-1")
    );
    assert_ne!(
        receipt.command_id,
        CompanyCommandReceipt::command_id(aggregate, "logical-2")
    );

    let mut changed = command;
    if let CompanyCommand::StartRun { sandbox, .. } = &mut changed {
        *sandbox = Some("read-only".to_owned());
    }
    assert_ne!(
        receipt.payload_digest,
        CompanyCommandReceipt::payload_digest(&changed)
    );
}

#[test]
fn unknown_company_receipt_cannot_claim_a_commit() {
    let receipt = CompanyCommandReceipt::new(
        "aggregate",
        "logical-unknown",
        &CompanyCommand::RecordRunStarted {
            packet_id: "packet-1".to_owned(),
        },
        &context(),
        2,
        None,
        None,
        CompanyReceiptStatus::ResultUnknown,
        None,
    )
    .unwrap();
    receipt.validate().unwrap();
}
