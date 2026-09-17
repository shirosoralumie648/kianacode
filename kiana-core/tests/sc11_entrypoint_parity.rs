use kiana_core::{EntrypointCommand, EntrypointDecision, EntrypointParityMatrix, ENTRYPOINT_ROUTE};
use kiana_domain::{CommandIntent, EntryPointKind, RequestContext};
use serde_json::json;

fn context() -> RequestContext {
    let mut context = RequestContext::local("sc11-session", "/repo");
    context.project_trusted = true;
    context
}

#[test]
fn all_entrypoint_labels_share_one_normalized_command_and_route() {
    let context = context();
    let intent = CommandIntent::new("context.snapshot", json!({"limit": 10}));
    let cli = EntrypointCommand::new(EntryPointKind::Cli, &context, &intent)
        .unwrap()
        .with_decision(EntrypointDecision::Allowed)
        .unwrap();
    let web = EntrypointCommand::new(EntryPointKind::Web, &context, &intent)
        .unwrap()
        .with_decision(EntrypointDecision::Allowed)
        .unwrap();
    let desktop = EntrypointCommand::new(EntryPointKind::Desktop, &context, &intent)
        .unwrap()
        .with_decision(EntrypointDecision::Allowed)
        .unwrap();
    assert_eq!(cli.command_digest, web.command_digest);
    assert_eq!(web.command_digest, desktop.command_digest);
    assert_eq!(cli.route, ENTRYPOINT_ROUTE);
    let matrix = EntrypointParityMatrix::new(vec![desktop, web, cli], 0).unwrap();
    assert_eq!(matrix.commands.len(), 3);
    assert!(matrix.validate().is_ok());
    assert_eq!(
        EntrypointParityMatrix::from_json(&matrix.to_json().unwrap()).unwrap(),
        matrix
    );
}

#[test]
fn deny_or_unknown_never_claims_handler_effect_and_entrypoint_mismatch_is_rejected() {
    let context = context();
    let intent = CommandIntent::new("sensitive.command", json!({"target":"release"}));
    let denied = EntrypointCommand::new(EntryPointKind::Cli, &context, &intent)
        .unwrap()
        .with_decision(EntrypointDecision::Denied)
        .unwrap();
    let mut forged = denied.clone();
    forged.entrypoint = EntryPointKind::Web;
    let matrix = EntrypointParityMatrix::new(vec![denied], 1);
    assert_eq!(
        matrix.unwrap_err(),
        "entrypoint_parity_denied_handler_effect"
    );
    let mut mismatch = EntrypointParityMatrix::new(vec![forged], 0).unwrap();
    mismatch.commands[0].command_digest =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    assert!(mismatch.validate().is_err());
}

#[test]
fn unknown_entrypoint_command_fields_and_alternate_route_fail_closed() {
    let context = context();
    let intent = CommandIntent::new("safe.read", json!({}));
    let command = EntrypointCommand::new(EntryPointKind::Scheduler, &context, &intent).unwrap();
    let mut value = command.to_json().unwrap();
    value["route"] = json!("direct-broker");
    assert!(EntrypointCommand::from_json(&value).is_err());
    value["route"] = json!(ENTRYPOINT_ROUTE);
    value["unexpected"] = json!(true);
    assert!(EntrypointCommand::from_json(&value).is_err());
}
