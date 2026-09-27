#[test]
fn billing_query_ingress_is_identity_checked_and_not_a_second_execution_path() {
    let daemon = include_str!("../src/lib.rs");
    for marker in [
        "RequestBody::BillingQuery",
        "billing_query_invalid",
        "billing_query_unauthenticated",
        "billing_query_projection_not_wired",
        "request_may_execute",
    ] {
        assert!(
            daemon.contains(marker),
            "BQ-23 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
    ] {
        assert!(
            !daemon.contains(forbidden),
            "BQ-23 daemon ingress widened authority: {forbidden}"
        );
    }
}
