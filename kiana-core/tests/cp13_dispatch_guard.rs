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

#[test]
fn cp13_prepare_consumes_approval_and_mints_permit_in_one_transaction() {
    let dispatch = include_str!("../src/dispatch.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let broker_registry = include_str!("../../kiana-capability-broker/tests/cap01_registry.rs");
    // Step 1/2：一次派发的准备事务必须同时包含许可事实与（如有）一次性审批消费事实。
    // `prepare_consumption` 只准备事件，真正的 Approved→Consumed 由同一条 prepared CAS 提交；
    // 因此 approval.consumed 事件必须被 push 进与 permit 同一个 `events` 批次。
    let prepare = compact_code(&item_source(
        dispatch,
        "pub(crate) async fn dispatch_authorized(",
    ));
    assert!(prepare.contains("self.approvals.prepare_consumption("));
    assert!(prepare.contains("events.push(prepared.event);"));
    assert!(prepare.contains("authority_versions.push(prepared.expected_version);"));
    // 审批消费事件必须在构造许可/提交批次之前进入同一事务，不能出现“先提交许可、再消费审批”
    // 的双写窗口；两者与同一条 TransitionBatch 的先后顺序由源码索引固定。
    let approval_event = prepare
        .find("events.push(prepared.event);")
        .expect("approval consumption event must join the prepared batch");
    let permit_mint = prepare
        .find("letmutpermit=DispatchPermit{")
        .expect("permit must be minted inside the same function");
    let prepared_batch = prepare
        .find("letbatch=TransitionBatch{")
        .expect("the prepared fact must be committed as one transition batch");
    let prepared_commit = prepare
        .find("commit_confirmed(self.events.as_ref(),batch)")
        .expect("the prepared batch must be committed");
    assert!(
        approval_event < permit_mint
            && permit_mint < prepared_batch
            && prepared_batch < prepared_commit,
        "approval consumption, permit minting and the prepared commit must share one batch"
    );
    // 直接命令没有 Harness run：invocation 身份必须是 null，不能伪造一个无 run 的调用身份。
    assert!(dispatch.contains("let Some(run_id) = run_id else {"));
    assert!(dispatch.contains("return Ok(None);"));
    // Step 3：真实 handler 之前先做 catalog/扩展/路由（未注册/错误版本 handler 不回退 shell），
    // 再消费 permit。拒绝必须发生在消费 CAS 之前，否则会浪费一次性许可并把未执行误报为已派发。
    let unregistered = broker
        .find("capability_unregistered")
        .expect("broker must reject unregistered operations");
    let consume = broker
        .find("verify_and_consume")
        .expect("broker must consume the permit");
    assert!(
        unregistered < consume,
        "broker must reject before consuming the one-shot permit"
    );
    // 未注册/错误版本 handler 的“不回退 shell”行为由既有 CAP-01 夹具证明，CP-13 只交叉引用。
    for marker in [
        "unregistered_operation_does_not_fallback_to_shell",
        "descriptor_binding_version_mismatch_never_dispatches",
    ] {
        assert!(
            broker_registry.contains(marker),
            "CAP-01 no-fallback fixture missing: {marker}"
        );
    }
}
