#[test]
fn fake_provider_is_an_offline_model_port_fixture() {
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    for marker in [
        "FakeProviderScenario",
        "FakeProviderAdapter",
        "complete_streaming",
        "ModelDelta::Text",
        "ModelDelta::ToolArguments",
        "AtomicUsize",
    ] {
        assert!(
            runtime.contains(marker),
            "fake provider marker missing: {marker}"
        );
    }
    assert!(daemon.contains("pub mod eval_runtime"));
    for forbidden in [
        "reqwest",
        "KianaHarness",
        "CapabilityBroker",
        "tokio::spawn",
        "std::env::var",
        "KIANA_API_KEY",
        "OPENAI_API_KEY",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "forbidden provider boundary: {forbidden}"
        );
    }
}
