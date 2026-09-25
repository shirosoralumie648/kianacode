use std::fs;

#[test]
fn extension_golden_trace_binds_deny_allow_hook_broker_receipt_and_surfaces() {
    let golden = fs::read_to_string("../kiana-domain/src/extension_golden.rs")
        .expect("extension golden source");
    for marker in [
        "EXTENSION_GOLDEN_TRACE_SCHEMA",
        "ExtensionGoldenScenario",
        "UntrustedProject",
        "SkillCollision",
        "PathEscape",
        "BudgetExceeded",
        "HookBlock",
        "HookAsk",
        "HookUpdate",
        "HookTimeout",
        "HookCancelled",
        "SignedInstall",
        "DependencyCycle",
        "McpDenied",
        "AllowedCapability",
        "capability_request_digest",
        "broker_result_digest",
        "receipt_digest",
        "extension_golden_denied_effect_leak",
        "extension_golden_surface_snapshot_drift",
        "extension_golden_matrix_coverage_missing",
    ] {
        assert!(golden.contains(marker), "EXT-29 marker missing: {marker}");
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::spawn",
        "reqwest::Client",
        "CapabilityBroker::new",
        "EventStorePort",
        "ProviderGateway",
        "remove_file",
    ] {
        assert!(
            !golden.contains(forbidden),
            "extension golden contract gained effect authority: {forbidden}"
        );
    }
}

#[test]
fn extension_golden_reuses_existing_trust_hook_command_and_snapshot_contracts() {
    let contracts = fs::read_to_string("../kiana-domain/src/extension_contracts.rs")
        .expect("extension contracts");
    let hooks =
        fs::read_to_string("../kiana-domain/src/hook_lifecycle.rs").expect("hook lifecycle");
    let commands = fs::read_to_string("../kiana-domain/src/extension_commands.rs")
        .expect("extension commands");
    let visibility = fs::read_to_string("../kiana-domain/src/extension_visibility.rs")
        .expect("extension visibility");
    for (name, source, markers) in [
        (
            "contracts",
            contracts,
            vec!["ExtensionSnapshot", "HookDecision", "PluginLifecycle"],
        ),
        (
            "hooks",
            hooks,
            vec![
                "HookLifecyclePlan",
                "HookLifecycleResult",
                "revalidate_on_change",
            ],
        ),
        (
            "commands",
            commands,
            vec!["ExtensionCommandReceipt", "ControlPlane", "idempotency_key"],
        ),
        (
            "visibility",
            visibility,
            vec![
                "ExtensionVisibilitySnapshot",
                "source_snapshot_digest",
                "Revoked",
            ],
        ),
    ] {
        for marker in markers {
            assert!(source.contains(marker), "{name} marker missing: {marker}");
        }
    }
}

#[test]
fn extension_golden_fixture_has_no_live_or_broker_effect() {
    let fixture = fs::read_to_string("../kiana-domain/tests/fixtures/ext29-extension-golden.json")
        .expect("extension golden fixture");
    assert!(fixture.contains("\"mode\": \"offline_no_effects\""));
    assert!(fixture.contains("\"broker_invocations\": 0"));
    assert!(!fixture.contains("shell_command") && !fixture.contains("http://"));
}
