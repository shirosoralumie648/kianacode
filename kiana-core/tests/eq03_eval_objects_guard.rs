#[test]
fn quality_eval_objects_are_strict_and_only_value_contracts() {
    let quality = include_str!("../../kiana-domain/src/quality.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "pub struct EvalDataset",
        "pub struct EvalSuite",
        "pub struct EvalCase",
        "pub struct GoldenTrace",
        "EVAL_DATASET_SCHEMA",
        "EVAL_SUITE_OBJECT_SCHEMA",
        "EVAL_CASE_OBJECT_SCHEMA",
        "GOLDEN_TRACE_SCHEMA",
        "provenance",
        "target_versions",
        "normalized_events",
        "deny_unknown_fields",
        "golden_trace_digest_mismatch",
    ] {
        assert!(
            quality.contains(marker),
            "quality object marker missing: {marker}"
        );
    }
    for marker in [
        "kiana.eval-dataset.v1",
        "kiana.eval-suite.v1",
        "kiana.eval-case.v1",
        "kiana.golden-trace.v1",
    ] {
        assert!(
            contracts.contains(marker),
            "quality schema marker missing: {marker}"
        );
    }
    assert!(protocol.contains("EvalDataset"));
    assert!(protocol.contains("GoldenTrace"));
    assert!(!quality.contains("tokio::"));
    assert!(!quality.contains("CapabilityBroker"));
}
