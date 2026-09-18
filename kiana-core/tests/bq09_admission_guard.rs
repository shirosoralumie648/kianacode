#[test]
fn admission_estimator_has_final_wire_upper_bounds_and_no_exact_unknown_price() {
    let admission = include_str!("../../kiana-domain/src/billing_admission.rs");
    for marker in [
        "AdmissionEstimateInput",
        "AdmissionEstimate",
        "TokenEstimateBasis",
        "wire_request_digest",
        "max_output_tokens",
        "retry_allowance",
        "tool_calls_upper",
        "effects_upper",
        "storage_bytes_upper",
        "cost_hard_limit_requires_known_price",
        "rate_card_version",
    ] {
        assert!(
            admission.contains(marker),
            "admission marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest",
        "tokio",
        "CapabilityBroker",
        "SystemTime",
        "FinancialBudget",
    ] {
        assert!(
            !admission.contains(forbidden),
            "admission boundary widened: {forbidden}"
        );
    }
}
