#[test]
fn model_event_boundary_has_lifecycle_redaction_and_commit_guards() {
    let model_event = include_str!("../../kiana-domain/src/model_event.rs");
    let registry = include_str!("../../kiana-domain/src/event_contracts.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "MODEL_EVENT_SCHEMA",
        "ModelFactCommitment",
        "MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_EFFECT",
        "ProviderTraceMetadata",
        "ProviderDeltaLedger",
        "StreamingRedactor",
        "into_runtime_event",
        "model.prepared",
        "model.denied",
        "model.attempt_started",
        "model.retry_scheduled",
        "model.finished",
        "model.usage_correction",
        "model_usage_correction_digest_mismatch",
        "pub fn digest(&self) -> String",
        "provider_request_id_status",
        "provider_response_id_status",
    ] {
        assert!(
            model_event.contains(marker) || registry.contains(marker) || harness.contains(marker),
            "P4-J7-26 marker missing: {marker}"
        );
    }
    for forbidden in [
        "Authorization:",
        "Bearer ",
        "reqwest::Client",
        "tokio::spawn",
        "std::net::TcpStream",
    ] {
        assert!(
            !model_event.contains(forbidden),
            "model fact boundary contains provider effect or secret marker: {forbidden}"
        );
    }
}
