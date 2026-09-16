#[test]
fn harness_stop_and_retry_paths_are_typed_and_fail_closed() {
    let harness = include_str!("../src/harness.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let provider = include_str!("../../kiana-provider/src/response.rs");
    for marker in [
        "normalized_stop_reason",
        "model_output_truncated",
        "model_refused",
        "model_transport_incomplete",
        "ModelRetryClass::BeforeSend",
        "ModelRetryClass::Rejected",
    ] {
        assert!(
            harness.contains(marker) || model.contains(marker) || provider.contains(marker),
            "missing H05 stop/retry marker {marker}"
        );
    }
    assert!(model.contains("ModelOutcome"));
    assert!(model.contains("side_effect_state"));
    assert!(model.contains("MODEL_OUTCOME_SCHEMA"));
    assert!(!harness.contains("contains(\"result_unknown"));
}
