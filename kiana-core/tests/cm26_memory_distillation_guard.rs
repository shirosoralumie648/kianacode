#[test]
fn distillation_uses_one_source_bound_candidate_path() {
    let domain = include_str!("../../kiana-domain/src/memory_distillation.rs");
    let core = include_str!("../src/memory_distillation.rs");
    let proposals = include_str!("../src/memory_proposals.rs");
    let collaboration = include_str!("../src/collaboration.rs");

    for marker in [
        "MemoryDistillationSource",
        "MemoryDistillationSourceStatus",
        "MEMORY_DISTILLATION_SOURCE_SCHEMA",
        "source_for_event",
        "source_status",
        "source.validate_for_job",
        "validate_output_with_source",
        "memory_distillation_source_unknown",
        "memory_distillation_source_untrusted",
        "memory_distillation_source_binding_invalid",
        "MEMORY_DISTILL_SESSION_PREFIX",
        "memory.distillation_queued",
        "memory.distillation_claimed",
        "memory.distillation_started",
        "memory.distillation_completed",
        "memory.distillation_failed",
        "MemoryAdmission::Candidate",
        "memory_proposal_source_required",
        "published",
        "verified",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || proposals.contains(marker)
                || collaboration.contains(marker),
            "CM-26 source marker missing: {marker}"
        );
    }

    assert!(core.contains("source_status != MemoryDistillationSourceStatus::Confirmed"));
    assert!(core.contains("source.kind != \"symposium.closed\""));
    assert!(core.contains("source.data[\"published\"] != true"));
    assert!(core.contains("source_for_event(context, run_id, source, kind, events)"));
    assert!(!core.contains("CapabilityBroker"));
    assert!(!core.contains("ModelClient"));
}
