#[test]
fn cp13_dispatch_requires_committed_opaque_permit_before_broker() {
    let domain = include_str!("../../kiana-domain/src/dispatch.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "DispatchPermit",
        "DISPATCH_PERMIT_VERSION",
        "permit_digest",
        "validate_for_request",
        "commit_confirmed",
        "execution.prepared",
        "invocation.dispatching",
        "execution_permit_required",
        "execution_permit_already_consumed",
        "cancelled:before_dispatch",
        "ExecutionPermitVerifierPort",
        "CapabilityBrokerPort",
    ] {
        assert!(
            domain.contains(marker)
                || dispatch.contains(marker)
                || capabilities.contains(marker)
                || ports.contains(marker),
            "CP-13 marker missing: {marker}"
        );
    }
    assert!(dispatch.contains("read_stream(\"execution_permit\""));
    assert!(dispatch.contains("commit_confirmed(self.events.as_ref(), batch)"));
    assert!(capabilities.contains("dispatch_authorized"));
    for forbidden in [
        "authorize_and_execute_from_permit",
        "broker.execute(request.request)",
    ] {
        assert!(
            !dispatch.contains(forbidden),
            "dispatch must not enable {forbidden}"
        );
    }
}
