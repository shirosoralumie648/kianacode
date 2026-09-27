#[test]
fn dep04_ops_protocol_has_no_execution_authority() {
    let protocol = include_str!("../../kiana-protocol/src/ops.rs");
    for marker in [
        "OpsCommand",
        "ops.status",
        "OpsQuery",
        "OpsEvent",
        "OpsError",
        "OpsUnknownEnvelope",
        "OpsEnvelope",
        "idempotency_key",
        "authority_epoch",
        "ops_scope_mismatch",
        "ops_idempotency_conflict",
    ] {
        assert!(protocol.contains(marker), "DEP-04 marker missing: {marker}");
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "consume_approval",
        "dispatch_capability",
    ] {
        assert!(
            !protocol.contains(forbidden),
            "DEP-04 protocol crossed an execution boundary: {forbidden}"
        );
    }
}
