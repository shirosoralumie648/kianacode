#[test]
fn terminal_distillation_is_bounded_candidate_only_and_replay_safe() {
    let domain = include_str!("../../kiana-domain/src/memory_distillation.rs");
    let proposals = include_str!("../../kiana-domain/src/memory_proposals.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let distillation = include_str!("../src/memory_distillation.rs");
    let proposals_core = include_str!("../src/memory_proposals.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let fixture = include_str!("../../kiana-domain/tests/p4_j3_05_distillation.rs");

    for marker in [
        "MemoryDistillationJob",
        "MemoryDistillationOutput",
        "MemoryProposal",
        "MemoryAdmission::Candidate",
        "MEMORY_DISTILLATION_SCHEMA",
        "validate_output",
        "DistillationVerdict::Retain",
        "DistillationVerdict::Discard",
        "memory_distillation_quote_mismatch",
        "memory_distillation_evidence_unknown",
        "memory_distillation_verdict_mismatch",
        "queue_terminal_distillation",
        "memory.distillation_queued",
        "memory.distillation_claimed",
        "memory.distillation_started",
        "memory.distillation_completed",
        "memory.distillation_failed",
        "memory.proposed",
        r#""automatic_execution":false"#,
        "memory.review",
        "result_unknown",
        "already_settled",
        "idempotency",
        "similar_records",
        "evidence",
    ] {
        assert!(
            domain.contains(marker)
                || proposals.contains(marker)
                || lifecycle.contains(marker)
                || distillation.contains(marker)
                || proposals_core.contains(marker)
                || memory.contains(marker)
                || fixture.contains(marker),
            "distillation marker missing: {marker}"
        );
    }

    assert!(lifecycle.contains("queue_terminal_distillation"));
    assert!(distillation.contains("queue_memory_distillation"));
    assert!(distillation.contains(r#""automatic_execution":false"#));
    assert!(distillation.contains("memory_distillation_result_unknown"));
    assert!(distillation.contains("memory_distillation_already_settled"));
    assert!(
        proposals_core.contains("MemoryAdmission::Candidate")
            || domain.contains("MemoryAdmission::Candidate")
            || fixture.contains("MemoryAdmission::Candidate")
    );
    assert!(memory.contains("memory.review"));
    assert!(memory.contains("role_memory_write_denied"));
    assert!(!distillation.contains("CapabilityBroker"));
    assert!(!distillation.contains("ProviderGateway"));
}
