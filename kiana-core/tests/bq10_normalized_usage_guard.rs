#[test]
fn normalized_usage_adapter_is_a_bounded_observation_boundary() {
    let provider = include_str!("../../kiana-provider/src/usage_adapters.rs");
    let usage = include_str!("../../kiana-provider/src/usage.rs");
    for marker in [
        "PROVIDER_USAGE_ADAPTER_SCHEMA",
        "normalize_provider_usage",
        "requested_model_id.clone()",
        "provider_usage_total_inconsistent",
        "UsageConfidence::Partial",
        "BillingUnknownReason::ProviderUnreported",
        "audio_output_tokens: None",
    ] {
        assert!(provider.contains(marker), "BQ-10 marker missing: {marker}");
    }
    assert!(usage.contains("Missing provider usage remains `Unknown`"));
    for marker in [
        "cache_read_tokens: None",
        "reasoning_output_tokens: None",
        "audio_input_tokens: None",
        "audio_output_tokens: None",
        "served_model_id = reply.output.model_id.clone()",
    ] {
        assert!(
            usage.contains(marker),
            "legacy adapter marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStore",
        "ControlPlane",
        "CapabilityBroker",
        "ModelCallPermit",
        "std::process::Command",
        "reqwest::",
    ] {
        assert!(
            !provider.contains(forbidden),
            "usage adapter gained authority: {forbidden}"
        );
        assert!(
            !usage.contains(forbidden),
            "legacy usage adapter gained authority: {forbidden}"
        );
    }
}
