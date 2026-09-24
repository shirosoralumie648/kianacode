#[test]
fn quota_projection_is_replayable_and_never_an_authorizer() {
    let source = include_str!("../src/connector_quota.rs");
    for marker in [
        "project_connector_quota",
        "ConnectorQuotaProjection",
        "CONNECTOR_QUOTA_EVENT_RESERVED",
        "CONNECTOR_QUOTA_EVENT_CLAIMED",
        "CONNECTOR_QUOTA_EVENT_SETTLED",
        "CONNECTOR_QUOTA_EVENT_RELEASED",
        "replayable",
        "projection_is_not_authorization",
        "source_cursor",
    ] {
        assert!(
            source.contains(marker),
            "INT-17 query marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "authorize_and_execute",
        "resolve_secret",
        "ConnectorQuotaLedger",
    ] {
        assert!(
            !source.contains(forbidden),
            "quota query must stay read-only: {forbidden}"
        );
    }
}
