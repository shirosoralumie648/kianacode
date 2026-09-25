use std::fs;

#[test]
fn product_chain_contract_binds_effects_and_four_surface_receipts() {
    let chain = fs::read_to_string("../kiana-domain/src/provider_product_chain.rs")
        .expect("product chain source");
    for marker in [
        "PROVIDER_PRODUCT_CHAIN_SCHEMA",
        "ProviderProductChainScenario",
        "CodingRoundTrip",
        "CancelBetweenModelFinishAndDispatch",
        "SlowSubscriber",
        "RunContinueResume",
        "BudgetDenied",
        "CapabilityDispatch",
        "ToolResult",
        "RuntimeEvent",
        "Receipt",
        "ProviderProductSurface::ALL",
        "provider_product_chain_surface_drift",
        "provider_product_chain_cancel_fence_invalid",
        "provider_product_chain_slow_subscriber_terminal_lost",
        "provider_product_chain_matrix_coverage_missing",
    ] {
        assert!(chain.contains(marker), "P4-J7-30 marker missing: {marker}");
    }
    for forbidden in [
        "reqwest::Client",
        "TcpStream",
        "TcpListener",
        "tokio::spawn",
        "CapabilityBrokerPort",
        "EventStorePort",
        "ModelBudgetPort",
        "std::process::Command",
    ] {
        assert!(
            !chain.contains(forbidden),
            "product-chain contract gained effect authority: {forbidden}"
        );
    }
}

#[test]
fn product_chain_keeps_the_single_daemon_controlplane_harness_spine() {
    let daemon = fs::read_to_string("../kiana-daemon/src/lib.rs").expect("daemon source");
    let runner = fs::read_to_string("../kiana-runner/src/harness.rs").expect("harness source");
    let provider = fs::read_to_string("../kiana-provider/src/lib.rs").expect("provider source");
    let cli = fs::read_to_string("../kiana-entrypoints/src/cli.rs").expect("cli source");
    let workbench =
        fs::read_to_string("../kiana-entrypoints/src/workbench_chat.rs").expect("workbench");
    let web = fs::read_to_string("../kiana-entrypoints/src/web.rs").expect("web source");
    let desktop = fs::read_to_string("../contrib/desktop/main.js").expect("desktop source");
    for marker in ["DaemonHost", "ControlPlane"] {
        assert!(
            daemon.contains(marker),
            "daemon spine marker missing: {marker}"
        );
    }
    assert!(runner.contains("KianaHarness"));
    assert!(provider.contains("ProviderGateway"));
    for (surface, source) in [
        ("cli", cli),
        ("workbench", workbench),
        ("web", web),
        ("desktop", desktop),
    ] {
        assert!(!source.is_empty(), "surface source is empty: {surface}");
    }
    assert!(desktop.contains("startHarness"));
    assert!(!chain_has_second_loop());
}

fn chain_has_second_loop() -> bool {
    let chain = fs::read_to_string("../kiana-domain/src/provider_product_chain.rs")
        .expect("product chain source");
    [
        "tokio::spawn",
        "ProviderGateway::new",
        "CapabilityBroker::new",
    ]
    .iter()
    .any(|marker| chain.contains(marker))
}

#[test]
fn surface_projection_uses_existing_read_only_parity_comparator() {
    let parity =
        fs::read_to_string("../kiana-client/src/surface_parity.rs").expect("surface parity source");
    for marker in [
        "compare_surface_traces",
        "REQUIRED_SURFACES",
        "ParitySurface::Desktop",
        "receipt_digest",
        "SensitiveFields",
    ] {
        assert!(
            parity.contains(marker),
            "surface parity marker missing: {marker}"
        );
    }
}
