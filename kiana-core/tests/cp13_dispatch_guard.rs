#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::{compact_code, item_source, production_source};

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
        "dispatch_permit_invocation_request_mismatch",
        "InvocationId::from_uuid(self.request_id.as_uuid())",
        "commit_confirmed",
        "execution.prepared",
        "invocation.dispatching",
        "invocation.executing",
        "events: vec![dispatching, executing]",
        "result_unknown:invocation_already_prepared",
        "prepare_data_revocation",
        "prepare_checkpoint_restore",
        "run_dispatch_after_terminal_or_cancel",
        "cancelled:before_broker",
        "result_unknown:{}",
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
    assert!(!dispatch.contains("commit_invocation_executing"));
    assert!(dispatch.contains("records[0].stream_version != Some(1)"));
    assert!(dispatch.contains("execution_permit_invocation_mismatch"));
    assert!(dispatch.contains("events: vec![dispatching, executing]"));
    assert!(capabilities.contains("dispatch_authorized"));
    let prepare = compact_code(&item_source(
        dispatch,
        "pub(crate) async fn dispatch_authorized(",
    ));
    assert!(prepare.contains("self.prepare_data_revocation(&authorized.request).await"));
    assert!(prepare.contains("self.prepare_checkpoint_restore(&authorized.request).await"));
    assert!(
        prepare.contains("commit_prepared_result_unknown"),
        "prepared failures must close the execution permit with a result_unknown terminal fact"
    );
    for reason in [
        "result_unknown:prepared_data_revocation_failed:",
        "result_unknown:prepared_checkpoint_failed:",
        "result_unknown:prepared_cancelled_before_broker",
    ] {
        assert!(
            prepare.contains(reason),
            "post-prepare exit must carry {reason}"
        );
    }
    assert_eq!(prepare.matches("commit_prepared_result_unknown").count(), 3);
    assert!(prepare.contains("result_unknown:invocation_already_prepared"));
    assert!(prepare.contains("cancelled:before_broker"));
    assert!(
        prepare.contains("self.capabilities.execute_cancellable(authorized,cancellation.clone())")
    );
    let authority = compact_code(&item_source(
        dispatch,
        "async fn dispatch_authority_versions(",
    ));
    assert!(authority.contains("run_dispatch_after_terminal_or_cancel"));
    assert!(authority.contains("events[turn..].iter().any(|event|"));
    for forbidden in [
        "authorize_and_execute_from_permit",
        "broker.execute(request.request)",
    ] {
        assert!(
            !production_source(dispatch).contains(forbidden),
            "dispatch must not enable {forbidden}"
        );
    }
}
