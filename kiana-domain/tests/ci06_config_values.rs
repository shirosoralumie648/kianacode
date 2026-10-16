use kiana_domain::contains_forbidden_provider_config_control;

#[test]
fn provider_model_config_labels_reject_line_and_nul_controls() {
    for value in ["provider\0id", "model\rname", "model\nname"] {
        assert!(
            contains_forbidden_provider_config_control(value),
            "control character must be rejected: {value:?}"
        );
    }
    for value in ["ollama", "fixture-model", "gpt-4.1-mini"] {
        assert!(!contains_forbidden_provider_config_control(value));
    }
}
