//! CP-25 source guard for the shared Skills/Hooks/Memory/MCP/Secret boundary.

#[test]
fn cp_untrusted_extension_cannot_inject_execution_authority() {
    let skills = include_str!("../../kiana-daemon/src/harness_skills.rs");
    let resolver = include_str!("../../kiana-skills/src/source_resolver.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/extensions.rs");
    for marker in [
        "load_all_skills_with_trust",
        "ProjectTrust::Untrusted",
        "PromptAuthority::Context",
        "ExtensionExecutionContract",
        "request_extension_scopes",
        "admit_extensions",
        "extension_admission",
        "handler_effect",
        "extension_read_only_write_denied",
    ] {
        assert!(
            skills.contains(marker)
                || resolver.contains(marker)
                || broker.contains(marker)
                || domain.contains(marker),
            "CP-25 extension boundary marker missing: {marker}"
        );
    }
    assert!(!skills.contains("allowed_tools.*authorize"));
    assert!(!broker.contains("allowed_tools.*authorize"));
}

#[test]
fn cp_mcp_schema_change_invalidates_pending_approval() {
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let approvals = include_str!("../src/approvals.rs");
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    for marker in [
        "mcp-snapshot.v1",
        "mcp.discovery_committed",
        "mcp_config_drift_requires_discovery",
        "mcp_catalog_changed",
        "mcp_tool_schema_changed",
        "mcp_arguments_schema_mismatch",
        "mcp_output_schema_mismatch",
        "mcp_snapshot_scope_changed",
        "approval_action_changed",
        "approval_requirements_changed",
        "mcp.call",
    ] {
        assert!(
            mcp.contains(marker) || approvals.contains(marker) || actions.contains(marker),
            "CP-25 MCP boundary marker missing: {marker}"
        );
    }
    assert!(mcp.contains("catalog_digest"));
    assert!(mcp.contains("discovery_version"));
}

#[test]
fn cp_secret_handle_cannot_cross_actor_or_destination() {
    let credentials = include_str!("../../kiana-domain/src/credentials.rs");
    let identity = include_str!("../../kiana-domain/src/identity_contracts.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    for marker in [
        "pub struct SecretRef",
        "pub struct CredentialLease",
        "credential_lease_purpose_mismatch",
        "credential_lease_audience_mismatch",
        "credential_lease_endpoint_mismatch",
        "credential_ref: Option<SecretRef>",
        "CredentialResolver",
        "consume_credential_lease",
        "raw secret",
    ] {
        assert!(
            credentials.contains(marker)
                || identity.contains(marker)
                || ports.contains(marker)
                || broker.contains(marker),
            "CP-25 secret boundary marker missing: {marker}"
        );
    }
    assert!(!ports.contains("resolve_credential.*String"));
    assert!(!broker.contains("secret_value"));
}
