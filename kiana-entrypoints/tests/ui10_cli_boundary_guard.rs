#[test]
fn cli_contract_is_a_client_boundary_and_has_no_execution_authority() {
    let contract = include_str!("../../kiana-client/src/cli_contract.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let cli = include_str!("../src/cli.rs");

    for marker in [
        "CliCommand",
        "CliInvocation",
        "CliOutput",
        "CLI_COMMAND_SCHEMA",
        "cli_session_required",
        "cli_interactive_requires_tty",
        "cli_tty_output_requires_tty",
        "cli_implicit_retry_forbidden",
        "contains_secret_key",
        "run",
        "run.status",
        "run.events",
        "approval.approve",
        "approval.deny",
        "run.cancel",
        "run.resume",
        "run.receipt",
        "audit.export",
        "session.query",
    ] {
        assert!(contract.contains(marker), "UI-10 contract marker missing: {marker}");
    }
    assert!(client.contains("KianaClient"));
    assert!(cli.contains("KianaClient"));

    for forbidden in [
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "std::process::Command",
    ] {
        assert!(
            !contract.contains(forbidden),
            "CLI contract gained execution authority: {forbidden}"
        );
    }
}
