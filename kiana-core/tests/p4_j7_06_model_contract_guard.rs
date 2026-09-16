#[test]
fn provider_neutral_model_contract_stays_below_provider_and_runner_implementations() {
    let domain = include_str!("../../kiana-domain/src/model.rs");
    let ports = include_str!("../../kiana-ports/src/model.rs");
    let runner = include_str!("../../kiana-runner/src/model.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let request = include_str!("../../kiana-provider/src/request.rs");
    let response = include_str!("../../kiana-provider/src/response.rs");

    for marker in [
        "ModelContent",
        "PreparedModelCall",
        "ModelFinish",
        "ModelError",
        "ModelOutcome",
        "model_content_legacy_conflict",
    ] {
        assert!(
            domain.contains(marker),
            "model contract marker missing: {marker}"
        );
    }
    assert!(ports.contains("trait ModelClient"));
    assert!(runner.contains("pub use kiana_ports::ModelClient"));
    for marker in [
        "prepare_call",
        "complete_admitted",
        "provider_requires_model_admission",
    ] {
        assert!(
            provider.contains(marker),
            "provider port boundary missing: {marker}"
        );
    }
    assert!(request.contains("ModelContent"));
    assert!(response.contains("ModelError"));
    assert!(!ports.contains("reqwest"));
}
