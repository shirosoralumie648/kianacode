//! CAP-34 source guard for the evidence matrix and non-skip semantics.

#[test]
fn conformance_matrix_keeps_unsupported_rows_visible_and_fenced() {
    let source = include_str!("../../kiana-domain/src/capability_conformance.rs");
    let baseline = include_str!("../../docs/roadmap/cap34-conformance-baseline.md");
    for marker in [
        "ConformanceBackend",
        "Linux",
        "Macos",
        "Windows",
        "Container",
        "ConformanceProfile",
        "ConformanceTool",
        "ConformanceScenario",
        "ConformanceCaseStatus",
        "Verified",
        "NotApplicable",
        "NotImplemented",
        "Blocked",
        "catalog_epoch",
        "credential_epoch",
        "data_epoch",
        "effect_fence_verified",
        "resume_fence_verified",
        "backend_switch_cannot_expand_existing_grant",
        "extension_transport_and_resume_cannot_bypass_effect_fences",
        "conformance_matrix_duplicate_case",
    ] {
        assert!(
            source.contains(marker),
            "CAP-34 source marker missing: {marker}"
        );
    }
    for marker in [
        "support matrix",
        "NotApplicable",
        "NotImplemented",
        "skip",
        "grant",
        "catalog",
        "credential",
        "data epoch",
        "effect fence",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "CAP-34 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
    assert!(!source.contains("tokio::"));
}
