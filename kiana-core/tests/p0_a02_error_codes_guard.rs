#[test]
fn stable_error_mapping_is_shared_by_domain_protocol_and_entrypoints() {
    let domain = include_str!("../../kiana-domain/src/errors.rs");
    let capability = include_str!("../../kiana-domain/src/capabilities.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let cli = include_str!("../../kiana-entrypoints/src/command_dispatch.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    for marker in [
        "CapabilityErrorPolicy",
        "PathEscape",
        "ResultUnknown",
        "requires_reconciliation",
        "from_reason",
    ] {
        assert!(
            domain.contains(marker),
            "error contract marker missing: {marker}"
        );
    }
    assert!(capability.contains("failure_code"));
    assert!(protocol.contains("failure_policy"));
    assert!(cli.contains("status_name"));
    assert!(web.contains("result_unknown"));
}
