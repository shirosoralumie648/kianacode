#[test]
fn five_department_decisions_use_the_candidate_memory_path() {
    let roles = include_str!("../../kiana-domain/src/roles.rs");
    let symposiums = include_str!("../../kiana-domain/src/symposiums.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let proposals = include_str!("../src/memory_proposals.rs");
    let distillation = include_str!("../src/memory_distillation.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let fixture = include_str!("../../kiana-domain/tests/p4_e03_department_memory.rs");

    for marker in [
        "DepartmentSpec::catalog",
        "can_convene",
        "Symposium::department",
        "pub fn close(",
        "DecisionRecord",
        "symposium.closed",
        "derive_memory_proposal",
        "queue_memory_distillation",
        "MemoryDistillationJob",
        "kind == \"decision\"",
        "MemoryProposal",
        "MemoryAdmission::Candidate",
        "memory.proposed",
        r#""automatic_execution":false"#,
        "memory.review",
        "allows_knowledge",
        "department:",
        "evidence",
        "similar_records",
    ] {
        assert!(
            roles.contains(marker)
                || symposiums.contains(marker)
                || collaboration.contains(marker)
                || proposals.contains(marker)
                || distillation.contains(marker)
                || memory.contains(marker)
                || fixture.contains(marker),
            "department memory marker missing: {marker}"
        );
    }

    for marker in [
        "DEPARTMENT_INITIATING",
        "DEPARTMENT_PLANNING",
        "DEPARTMENT_EXECUTING",
        "DEPARTMENT_MONITORING",
        "DEPARTMENT_CLOSING",
    ] {
        assert!(roles.contains(marker));
    }
    assert!(collaboration.contains("self.derive_memory_proposal("));
    assert!(collaboration.contains("\"decision\""));
    assert!(distillation.contains(r#""automatic_execution":false"#));
    assert!(distillation.contains("MemoryAdmission::Candidate") || proposals.contains("Candidate"));
    assert!(memory.contains("memory.review"));
    assert!(memory.contains("role_memory_write_denied"));
    assert!(!collaboration.contains("CapabilityBroker"));
}
