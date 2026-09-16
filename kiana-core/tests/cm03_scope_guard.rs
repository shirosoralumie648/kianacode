#[test]
fn memory_scope_is_server_derived_before_handler_dispatch() {
    let core = include_str!("../src/capabilities.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let domain = include_str!("../../kiana-domain/src/context_scope.rs");
    for marker in [
        "RoleSpec::lookup",
        "memory_scope_collection_required",
        "memory_scope_read_denied",
        "memory_scope_write_denied",
        "memory_scopes",
    ] {
        assert!(
            core.contains(marker),
            "missing core memory scope marker {marker}"
        );
    }
    assert!(daemon.contains("server_memory_scope"));
    assert!(daemon.contains("request.request.execution_scope"));
    assert!(daemon.contains("DomainMemoryScope::from_execution_scope"));
    assert!(domain.contains("pub fn intersect"));
    assert!(domain.contains("memory_scope_intersection_empty"));
    assert!(!daemon.contains("principal_id"));
}
