#[test]
fn governance_projection_rechecks_memory_epoch_contract() {
    let core = include_str!("../src/data_governance.rs");
    let domain = include_str!("../../kiana-domain/src/memory_projection.rs");
    assert!(core.contains("MemoryProjectionFence::validate_epoch"));
    assert!(domain.contains("memory_projection_data_epoch_mismatch"));
    assert!(domain.contains("project_scope_denied"));
    assert!(domain.contains("source_revoked_or_retention_expired"));
}
