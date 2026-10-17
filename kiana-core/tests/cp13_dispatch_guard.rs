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

#[test]
fn cp13_linearization_points_and_crash_classes_are_pinned() {
    let domain = include_str!("../../kiana-domain/src/dispatch.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    // 崩溃分类合同：仅由已提交事实推导 handler 是否可能已开始，且未知/损坏前缀 fail-closed。
    for marker in [
        "pub enum DispatchCrashClass",
        "NotPrepared",
        "PreparedUndispatched",
        "DispatchUnconfirmed",
        "EffectPossiblyStarted",
        "ResultCommitted",
        "Corrupt",
        "pub fn classify_dispatch_stream(",
        "pub fn dispatch_reservation_digest(",
        "handler_may_have_started",
        "requires_unknown_terminal",
    ] {
        assert!(
            domain.contains(marker),
            "CP-13 crash-classification marker missing: {marker}"
        );
    }
    // prepared 事实必须与本条事务内的预算/资源预留快照绑定；消费方重算摘要，未绑定则拒绝。
    assert!(dispatch.contains("reservation_digest"));
    assert!(dispatch.contains("dispatch_reservation_digest"));
    assert!(dispatch.contains("execution_permit_reservation_unbound"));
    assert!(dispatch.contains("pub async fn classify_dispatch_execution("));
    assert!(contracts.contains("\"reservation_digest\""));
    // 消费 CAS 必须同时写 dispatching 与 executing；prepared 之后不得单独写 executing。
    assert!(dispatch.contains("events: vec![dispatching, executing]"));
    assert!(!dispatch.contains("commit_invocation_executing"));
    // 未绑定预留的拒绝必须发生在消费 CAS 之前（源码顺序），不能带病进入 handler。
    let consume = dispatch
        .find("execution_permit_reservation_unbound")
        .expect("reservation binding check must exist");
    let mint = dispatch
        .find("let command_id = derived_request_id(\"permit.consume\", id);")
        .expect("permit.consume command must exist");
    assert!(
        consume < mint,
        "reservation binding must be verified before the permit-consume CAS"
    );
    for forbidden in ["optimistic_no_handler", "assume_not_dispatched"] {
        assert!(
            !domain.contains(forbidden) && !dispatch.contains(forbidden),
            "crash classification must not enable {forbidden}"
        );
    }
}
