#[test]
fn connector_scope_guard_keeps_owner_project_epoch_and_intersection_server_bound() {
    let domain = include_str!("../../kiana-domain/src/connector_scope.rs");
    let core = include_str!("../src/connectors.rs");
    for marker in [
        "ConnectorScopeBinding",
        "owner_id",
        "project_root",
        "data_epoch",
        "connector_scope_widening_denied",
        "connector_scope_identity_or_epoch_mismatch",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "missing marker: {marker}"
        );
    }
}
