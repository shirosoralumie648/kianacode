#[test]
fn fake_provider_is_an_offline_model_port_fixture() {
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let provider_start = runtime
        .find("pub enum FakeProviderScenario")
        .expect("fake provider source start missing");
    let provider_end = runtime
        .find("pub const EVAL_DENY_BROKER_SCHEMA")
        .expect("fake provider source end missing");
    assert!(provider_start < provider_end);
    let provider = &runtime[provider_start..provider_end];
    for marker in [
        "FakeProviderScenario",
        "FakeProviderAdapter",
        "complete_streaming",
        "ModelDelta::Text",
        "ModelDelta::ToolArguments",
        "chunk.is_empty()",
        "MAX_STREAM_TEXT_BYTES",
        "try_fold",
        "checked_add",
        "AtomicUsize",
    ] {
        assert!(
            provider.contains(marker),
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
            !provider.contains(forbidden),
            "forbidden provider boundary: {forbidden}"
        );
    }
}
