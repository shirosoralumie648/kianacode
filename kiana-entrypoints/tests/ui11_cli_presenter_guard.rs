#[test]
fn cli_presenter_is_a_pure_projection_and_keeps_authority_in_the_client_server_path() {
    let presenter = include_str!("../../kiana-client/src/cli_presenter.rs");
    let contract = include_str!("../../kiana-client/src/cli_contract.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");

    for marker in [
        "CliExitCode",
        "CliSignal",
        "present_cli_output",
        "stdout",
        "stderr",
        "cli_tty_output_requires_tty",
        "cli_pager_requires_tty",
        "kiana.cli-output.v1",
        "ResultUnknown",
        "BrokenPipe",
    ] {
        assert!(
            presenter.contains(marker) || contract.contains(marker),
            "UI-11 presenter marker missing: {marker}"
        );
    }
    assert!(client.contains("present_cli_output"));
    assert!(client.contains("KianaClient"));

    for forbidden in [
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "std::process::Command",
        "std::fs::",
        "reqwest::",
    ] {
        assert!(
            !presenter.contains(forbidden),
            "CLI presenter gained execution or transport authority: {forbidden}"
        );
    }
}

