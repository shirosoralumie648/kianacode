#[test]
fn eval_cli_mapping_is_versioned_and_uses_the_shared_command_router() {
    let eval = include_str!("../../kiana-commands/src/eval.rs");
    for marker in [
        "EVAL_CLI_COMMAND_SCHEMA",
        "kiana.eval-cli.v1",
        "EVAL_CLI_COMMAND_VERSION",
        "(\"run\", \"eval.run\")",
        "(\"capture\", \"eval.capture\")",
        "(\"compare\", \"eval.compare\")",
        "(\"explain\", \"eval.explain\")",
        "(\"list\", \"eval.list\")",
        "CommandRoute::ControlPlane",
        "parse_cli_route",
    ] {
        assert!(eval.contains(marker), "eval CLI marker missing: {marker}");
    }
    assert!(
        eval.contains("run_suite"),
        "legacy compatibility adapter disappeared"
    );
    assert!(
        eval.contains("EvalCliRoute::LegacyRun"),
        "legacy route must remain explicit"
    );
    for forbidden in [
        "kiana_provider::",
        "kiana-provider",
        "CapabilityBroker",
        "tokio::spawn",
        "KianaHarness",
    ] {
        assert!(
            !eval.contains(forbidden),
            "eval CLI parser must not own provider/runner/effect execution: {forbidden}"
        );
    }
}

#[test]
fn quality_protocol_registry_contains_the_extended_eval_wire_names() {
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let events = include_str!("../../kiana-domain/src/event_contracts.rs");
    for marker in [
        "eval.run",
        "eval.capture",
        "eval.compare",
        "eval.explain",
        "eval.list",
    ] {
        assert!(
            protocol.contains(marker),
            "protocol marker missing: {marker}"
        );
        assert!(
            events.contains(marker),
            "event registry marker missing: {marker}"
        );
    }
}
