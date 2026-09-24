use kiana_client::{
    CliCommand, CliCommandId, CliInvocation, CliOutput, CliOutputMode,
};
use kiana_protocol::{ExecutionStatus, RequestId};
use serde_json::json;

#[test]
fn canonical_commands_and_legacy_aliases_have_stable_wire_names() {
    assert_eq!(CliCommand::ALL.len(), 10);
    assert_eq!(CliCommand::parse("approval").unwrap(), CliCommand::Approve);
    assert_eq!(CliCommand::parse("events").unwrap(), CliCommand::Events);
    assert_eq!(CliCommand::parse("sessions").unwrap(), CliCommand::Session);
    assert_eq!(CliCommand::Status.wire_name(), "run.status");
    assert_eq!(CliCommand::Export.wire_name(), "audit.export");
    assert_eq!(CliCommand::parse("unknown").unwrap_err(), "cli_command_unknown");
}

fn make_invocation(command: CliCommand) -> CliInvocation {
    CliInvocation::new(
        command,
        RequestId::new(),
        "/workspace",
        Some("session-1".to_owned()),
        CliOutputMode::Json,
        false,
        false,
        false,
        json!({"prompt": "show status"}),
    )
}

#[test]
fn invocation_rejects_missing_scope_interaction_and_implicit_mutation_retry() {
    let mut missing_workspace = make_invocation(CliCommand::Run);
    missing_workspace.workspace.clear();
    assert_eq!(
        missing_workspace.validate().unwrap_err(),
        "cli_workspace_required"
    );

    let mut missing_session = make_invocation(CliCommand::Status);
    missing_session.session_id = None;
    assert_eq!(
        missing_session.validate().unwrap_err(),
        "cli_session_required"
    );

    let mut no_tty = make_invocation(CliCommand::Run);
    no_tty.interactive = true;
    assert_eq!(
        no_tty.validate().unwrap_err(),
        "cli_interactive_requires_tty"
    );

    let mut tty_output = make_invocation(CliCommand::Run);
    tty_output.output_mode = CliOutputMode::Tty;
    assert_eq!(
        tty_output.validate().unwrap_err(),
        "cli_tty_output_requires_tty"
    );

    let mut retry = make_invocation(CliCommand::Cancel);
    retry.implicit_retry = true;
    assert_eq!(
        retry.validate().unwrap_err(),
        "cli_implicit_retry_forbidden"
    );
}

#[test]
fn invocation_and_output_reject_secret_fields_and_json_ansi() {
    let mut invocation = make_invocation(CliCommand::Run);
    invocation.arguments = json!({"api_key": "do-not-send"});
    assert_eq!(
        invocation.validate().unwrap_err(),
        "cli_arguments_sensitive_or_oversized"
    );

    let command_id = CliCommandId::new(CliCommand::Status, RequestId::new());
    let output = CliOutput::new(
        command_id.clone(),
        CliOutputMode::Json,
        ExecutionStatus::Completed,
        json!({"authorization": "Bearer secret"}),
        None,
    );
    assert_eq!(
        output.validate().unwrap_err(),
        "cli_output_sensitive_or_oversized"
    );

    let ansi = CliOutput::new(
        command_id,
        CliOutputMode::Json,
        ExecutionStatus::Completed,
        json!({"text": "\u{1b}[31mred\u{1b}[0m"}),
        None,
    );
    assert_eq!(ansi.validate().unwrap_err(), "cli_json_ansi_forbidden");
}
