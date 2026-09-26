#[test]
fn er31_fault_matrix_reuses_event_receipt_recovery_boundaries_without_effects() {
    let domain = include_str!("../../kiana-domain/src/er31_fault_matrix.rs");
    let core = include_str!("../src/er31_fault_matrix.rs");
    let old_fault = include_str!("../../kiana-domain/src/fault.rs");
    let old_core_fault = include_str!("../src/fault_injection.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let receipt = include_str!("../src/receipts.rs");
    for marker in [
        "Er31FaultMatrix",
        "Er31FaultCase",
        "Er31FaultPoint",
        "PartialWrite",
        "PatchRename",
        "StopReap",
        "ResultDelivery",
        "ProjectionCheckpoint",
        "ArtifactPublish",
        "Cleanup",
        "unknown_queryable",
        "resources_fenced",
        "receipt_limitations",
        "FaultMatrix",
        "TransitionBatch",
        "validate_er31_fault_matrix",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || old_fault.contains(marker)
                || old_core_fault.contains(marker)
                || journal.contains(marker)
                || receipt.contains(marker),
            "ER-31 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::process::Command",
        "tokio::process::Command",
        "CapabilityBroker::new",
        "ModelClient::new",
        "kill -9",
        "append_event",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "ER-31 effect/fake crash marker present: {forbidden}"
        );
    }
}
