#[test]
fn dep06_operation_lease_is_a_pure_cas_contract() {
    let domain = include_str!("../../kiana-domain/src/operation_lease.rs");
    let core = include_str!("../src/deployment_lease.rs");
    for marker in [
        "OperationLease",
        "OperationLeaseCas",
        "OperationLeaseState",
        "heartbeat",
        "fence_token",
        "authority_epoch",
        "data_epoch",
        "operation_lease_cas_conflict",
        "operation_lease_expired_reclaim_required",
        "validate_operation_lease_cas",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-06 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "dispatch_capability",
        "acquire_os_lock",
        "flock(",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-06 lease path crossed an effect boundary: {forbidden}"
        );
    }
}
