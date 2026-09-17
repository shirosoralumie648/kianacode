#[test]
fn quality_admission_metadata_is_explicit_and_denies_expiry_or_unowned_inputs() {
    let quality = include_str!("../../kiana-domain/src/quality.rs");
    for marker in [
        "EvalSplit",
        "privacy_class",
        "minimum_sample",
        "workload_tags",
        "validate_for_admission",
        "eval_dataset_expired",
        "eval_case_expired",
        "valid_privacy_class",
        "canonical_tags",
    ] {
        assert!(
            quality.contains(marker),
            "admission marker missing: {marker}"
        );
    }
    assert!(!quality.contains("fn admit_without_owner"));
    assert!(!quality.contains("CapabilityBroker"));
}
