#[test]
fn external_resource_is_untrusted_scoped_quota_bound_and_revocable() {
    let domain = include_str!("../../kiana-domain/src/external_resource.rs");
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let connectors = include_str!("../../kiana-daemon/src/connectors.rs");
    let context = include_str!("../../kiana-query/src/context_inputs.rs");

    for marker in [
        "ExternalResourceKind",
        "EXTERNAL_RESOURCE_REQUEST_SCHEMA",
        "EXTERNAL_RESOURCE_SNAPSHOT_SCHEMA",
        "processing_grant_digest",
        "quota_digest",
        "base_scope_digest",
        "requested_scope_digest",
        "allow_memory_write",
        "allow_scope_widening",
        "EvidenceStatus::Attributed",
        "revoked_at_ms",
        "readable_at",
        "data_epoch",
        "untrusted_data",
        "connector_binding_revoked",
        "mcp_project_untrusted",
        "SourceRef",
    ] {
        assert!(
            domain.contains(marker)
                || mcp.contains(marker)
                || connectors.contains(marker)
                || context.contains(marker),
            "CM-35 source marker missing: {marker}"
        );
    }

    assert!(domain.contains("requested_scope_digest != self.base_scope_digest"));
    assert!(domain.contains("self.revoked_at_ms.is_none()"));
    assert!(!domain.contains("CapabilityBroker"));
}
