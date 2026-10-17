//! SC-12 待处理调用、执行许可、CAS 与幂等边界的拒绝优先夹具。
//!
//! 这张卡把「一条已经授权的动作在真正进入 handler 前的线性化点」钉在真实
//! `JournalPermitVerifier` 与 `CapabilityBroker` 上：没有 admission 的派发、被改写或
//! 序号/围栏不匹配的准备事实、重复 command 和并发消费都必须在 handler 之前 fail closed；
//! 第一条成功消费则必须连同 `invocation.dispatching`/`invocation.executing` 一次性提交，
//! 之后的重放只能去重到原来那条 command receipt。
//!
//! 夹具只使用内存 EventStore 与计数 handler，证明的是**进程内提交边界**上的调用次数与
//! 命令回执去重，不证明跨进程 durable permit、外部副作用或跨 provider exactly-once。
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_core::JournalPermitVerifier;
use kiana_domain::{
    AggregateVersion, ApprovalId, AuthenticatedPrincipalRef, AuthorizedCapabilityRequest,
    CapabilityKind, CapabilityRequest, CapabilityResult, DispatchPermit, ExecutionId,
    ExecutionScope, InvocationId, ProjectIdentity, RequestContext, RequestId, RunId, RuntimeEvent,
    ScopeDimension, ScopeLimit, ScopeSet, TurnId,
};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{CapabilityBrokerPort, EventStorePort, PortError};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn request(context: &RequestContext) -> CapabilityRequest {
    CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Query,
        "context.search",
        json!({"query":"permit","call_id":"sc12-call"}),
    )
}

fn broker_scope(
    context: &RequestContext,
    request: &CapabilityRequest,
    run_id: Option<RunId>,
    turn_id: Option<TurnId>,
) -> ExecutionScope {
    let permission_scope = ScopeSet::new(
        ScopeDimension::Restricted(vec![request.operation.clone()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::NotApplicable,
        ScopeLimit::NotApplicable,
    )
    .unwrap();
    let trust_revision = kiana_domain::json_digest(&json!("sc12-server-trust"));
    let project_identity = kiana_core::project_root_identity(&context.project_root).unwrap();
    let canonical_root = project_identity["canonical_root"].as_str().unwrap();
    let project = ProjectIdentity::new(
        context.project_root.as_str(),
        canonical_root,
        None,
        None,
        trust_revision.as_str(),
    )
    .unwrap();
    let mut scope = ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal: AuthenticatedPrincipalRef::local(),
        project,
        session_id: context.session_id.clone(),
        run_id,
        turn_id,
        cell_id: None,
        grant_refs: Vec::new(),
        budget_lease_id: None,
        work_packet_id: None,
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope_digest: permission_scope.digest(),
        permission_scope,
        read_roots: vec![canonical_root.to_owned()],
        write_roots: Vec::new(),
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes: Vec::new(),
        server_scopes: Vec::new(),
        network_policy: Vec::new(),
        authority_epoch: 1,
        trust_revision,
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: kiana_domain::capability_action_catalog_digest(),
        action_digest: kiana_domain::capability_action_digest(request),
        scope_digest: String::new(),
    };
    scope.scope_digest = scope.digest();
    scope.validate_for_request(request).unwrap();
    scope
}

/// 准备事实被改写的位置；用于覆盖 digest/sequence 两类不匹配的拒绝路径。
#[derive(Clone, Copy, Debug)]
enum Tamper {
    /// 未改写：准备事实与许可自洽。
    None,
    /// 许可自带摘要被改写但未重算：解码许可时即应失败。
    PermitDigest,
    /// 准备事实的 attempt 序号被改写：字段不再与许可一致。
    EventAttempt,
}

/// 在 EventStore 中写入一条已提交 `execution.prepared` 事实，并返回可消费的授权请求。
///
/// `tamper` 决定准备事实是否在提交前被改写；其余字段严格复刻 Core 写出的准备事实形状，
/// 使 verifier 的逐字段校验通过。直接命令没有 Harness run，因此 invocation 身份为 null。
async fn prepared_permit(
    events: Arc<dyn EventStorePort>,
    tamper: Tamper,
) -> (
    Arc<JournalPermitVerifier>,
    Arc<dyn EventStorePort>,
    AuthorizedCapabilityRequest,
    DispatchPermit,
) {
    let verifier = Arc::new(JournalPermitVerifier::new(events.clone()));
    let mut context = RequestContext::local("session-sc12", ".");
    context.project_trusted = true;
    let mut request = request(&context);
    kiana_domain::normalize_capability_action(&mut request).unwrap();
    let execution_id = ExecutionId::new();
    let invocation_id = InvocationId::from_uuid(request.request_id.as_uuid());
    request.execution_scope = Some(broker_scope(&context, &request, None, None));
    let project_identity = kiana_core::project_root_identity(&context.project_root).unwrap();
    events
        .append(
            RuntimeEvent::new(
                RequestId::new(),
                1,
                "authority.snapshot",
                json!({"project_trusted":true}),
            )
            .unwrap()
            .with_stream_metadata("authority", "authority-key", 1),
        )
        .await
        .unwrap();
    let mut permit = DispatchPermit {
        schema: kiana_domain::DISPATCH_PERMIT_SCHEMA.to_owned(),
        version: kiana_domain::DISPATCH_PERMIT_VERSION,
        execution_id,
        invocation_id,
        request_id: request.request_id,
        run_id: None,
        turn_id: None,
        decision_id: "policy:sc12".to_owned(),
        approval_id: None,
        context,
        action_digest: kiana_domain::capability_action_digest(&request),
        project_identity,
        authority_versions: vec![AggregateVersion::new("authority", "authority-key", 1)],
        issued_at_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        permit_digest: String::new(),
    };
    permit.permit_digest = permit.digest();
    if matches!(tamper, Tamper::PermitDigest) {
        permit.permit_digest = format!("sha256:{}", "0".repeat(64));
    }
    let attempt = if matches!(tamper, Tamper::EventAttempt) {
        2
    } else {
        1
    };
    let reservation_digest =
        kiana_domain::dispatch_reservation_digest(&Value::Null, &permit.authority_versions);
    let prepared_command =
        kiana_domain::derived_request_id("execution.prepare", &request.request_id.to_string());
    events
        .append(
            RuntimeEvent::new(
                prepared_command,
                1,
                "execution.prepared",
                json!({
                    "run_id":permit.run_id,
                    "turn_id":permit.turn_id,
                    "invocation_id":permit.invocation_id,
                    "execution_id":permit.execution_id,
                    "capability_request_id":permit.request_id,
                    "action_digest":permit.action_digest,
                    "attempt":attempt,
                    "permit":permit,
                    "cell_reservation":Value::Null,
                    "reservation_digest":reservation_digest,
                    "invocation":Value::Null,
                }),
            )
            .unwrap()
            .with_stream_metadata("execution_permit", execution_id.to_string(), 1)
            .with_identity_links(
                Some(prepared_command),
                Some(request.request_id),
                None,
                None,
            ),
        )
        .await
        .unwrap();
    let authorized =
        AuthorizedCapabilityRequest::new(format!("permit:{execution_id}"), request).unwrap();
    (verifier, events, authorized, permit)
}

struct CountingQueryHandler {
    events: Arc<dyn EventStorePort>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl CapabilityHandler for CountingQueryHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let id = request.authorization_id.strip_prefix("permit:").unwrap();
        let records = self.events.read_stream("execution_permit", id).await?;
        assert_eq!(
            records
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            [
                "execution.prepared",
                "invocation.dispatching",
                "invocation.executing"
            ],
            "handler may only run after the dispatch consumption CAS committed both facts"
        );
        Ok(CapabilityResult::success(
            request.request.request_id,
            json!({"hits":[]}),
        ))
    }
}

struct UnusedHandler {
    binding_version: &'static str,
}

#[async_trait]
impl CapabilityHandler for UnusedHandler {
    fn binding_version(&self) -> &'static str {
        self.binding_version
    }

    async fn execute(&self, _: AuthorizedCapabilityRequest) -> Result<CapabilityResult, PortError> {
        Err(PortError::Failed("sc12_unexpected_handler".to_owned()))
    }
}

fn broker_with_counter(
    verifier: Arc<JournalPermitVerifier>,
    events: Arc<dyn EventStorePort>,
) -> (CapabilityBroker, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let query: Arc<dyn CapabilityHandler> = Arc::new(CountingQueryHandler {
        events,
        calls: calls.clone(),
    });
    let mut broker = CapabilityBroker::new();
    for operation in kiana_domain::ACTION_OPERATIONS {
        let descriptor = kiana_domain::capability_action_descriptor(operation).unwrap();
        let handler: Arc<dyn CapabilityHandler> = if *operation == "context.search" {
            query.clone()
        } else {
            Arc::new(UnusedHandler {
                binding_version: descriptor.binding_version,
            })
        };
        broker
            .register_static(descriptor.capability, *operation, handler)
            .unwrap();
    }
    broker.validate_catalog_bindings().unwrap();
    broker.set_permit_verifier(verifier);
    (broker, calls)
}

async fn stream_kinds(events: &dyn EventStorePort, permit: &DispatchPermit) -> Vec<String> {
    events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap()
        .iter()
        .map(|event| event.kind.clone())
        .collect()
}

fn consume_command(permit: &DispatchPermit) -> RequestId {
    kiana_domain::derived_request_id("permit.consume", &permit.execution_id.to_string())
}

#[tokio::test]
async fn sc12_unadmitted_dispatch_never_reaches_a_handler() {
    // 一条真实存在的准备事实并不等于可派发：伪造授权串或未知 execution id 都没有 admission。
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_permit(events, Tamper::None).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    for authorization_id in ["policy:sc12", "approval:sc12", "opaque", "permit:"] {
        let body = authorized.request.clone();
        let forged = AuthorizedCapabilityRequest::new(authorization_id, body).unwrap();
        assert!(
            matches!(broker.execute(forged).await, Err(PortError::Failed(reason))
                if reason == "execution_permit_required"),
            "forged authorization {authorization_id} must never act as a permit"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    let unknown = AuthorizedCapabilityRequest::new(
        format!("permit:{}", ExecutionId::new()),
        authorized.request.clone(),
    )
    .unwrap();
    assert!(matches!(
        broker.execute(unknown).await,
        Err(PortError::Failed(reason)) if reason == "execution_permit_unavailable"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // 拒绝路径不能写任何消费事实，也不能留下 command receipt。
    assert_eq!(
        stream_kinds(events.as_ref(), &permit).await,
        ["execution.prepared"]
    );
    let consume = consume_command(&permit);
    assert!(events.read_command(&consume).await.unwrap().is_none());
}

#[tokio::test]
async fn sc12_permit_digest_and_sequence_tamper_are_rejected_before_effect() {
    // digest 不匹配（许可自带摘要被改写）与 sequence 不匹配（准备事实 attempt 漂移）
    // 都必须在消费 CAS 之前拒绝，handler 调用次数保持为零。
    for tamper in [Tamper::PermitDigest, Tamper::EventAttempt] {
        let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
        let (verifier, events, authorized, permit) = prepared_permit(events, tamper).await;
        let (broker, calls) = broker_with_counter(verifier, events.clone());
        assert!(
            matches!(broker.execute(authorized).await, Err(PortError::Failed(reason))
                if reason == "execution_permit_invalid"),
            "tampered prepared fact must be rejected: {tamper:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            stream_kinds(events.as_ref(), &permit).await,
            ["execution.prepared"]
        );
        let consume = consume_command(&permit);
        assert!(events.read_command(&consume).await.unwrap().is_none());
    }
}

#[tokio::test]
async fn sc12_authority_fence_mismatch_rejects_before_effect() {
    // 许可快照了 authority read-set 版本；准备后该聚合被推进（撤销/配置变更），
    // 旧许可必须在消费前以 old_epoch_permit_rejected 拒绝，且不产生任何消费事实。
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_permit(events, Tamper::None).await;
    events
        .append(
            RuntimeEvent::new(
                RequestId::new(),
                2,
                "authority.snapshot",
                json!({"project_trusted":true}),
            )
            .unwrap()
            .with_stream_metadata("authority", "authority-key", 2),
        )
        .await
        .unwrap();
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    assert!(matches!(
        broker.execute(authorized).await,
        Err(PortError::Failed(reason)) if reason == "old_epoch_permit_rejected"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        stream_kinds(events.as_ref(), &permit).await,
        ["execution.prepared"]
    );
    let consume = consume_command(&permit);
    assert!(events.read_command(&consume).await.unwrap().is_none());
}

#[tokio::test]
async fn sc12_committed_permit_consumes_once_and_dedups_the_original_receipt() {
    // 成功路径：一次性消费把 dispatching 与 executing 作为同一条 command 提交；
    // 之后重复 command 必须去重到原来那条 receipt，而不是产生第二次效果或新回执。
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_permit(events, Tamper::None).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    let result = broker.execute(authorized.clone()).await.unwrap();
    assert!(result.success);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        stream_kinds(events.as_ref(), &permit).await,
        [
            "execution.prepared",
            "invocation.dispatching",
            "invocation.executing"
        ]
    );
    let consume = consume_command(&permit);
    let original = events
        .read_command(&consume)
        .await
        .unwrap()
        .expect("the consumption CAS must leave one command receipt");
    assert_eq!(original.command_id, consume);
    assert_eq!(original.event_ids.len(), 2);
    let after_first = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap();

    // 重复 command：同一许可重放必须被拒绝，流与 handler 调用都不再变化。
    assert!(matches!(
        broker.execute(authorized).await,
        Err(PortError::Failed(reason)) if reason == "execution_permit_already_consumed"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        events
            .read_stream("execution_permit", &permit.execution_id.to_string())
            .await
            .unwrap(),
        after_first
    );
    // dedup 原 receipt：重试返回同一条 command receipt，而不是新提交。
    let replay = events
        .read_command(&consume)
        .await
        .unwrap()
        .expect("the original receipt must persist");
    assert_eq!(replay, original);
    assert_eq!(replay.commit_id, original.commit_id);
}

#[tokio::test]
async fn sc12_concurrent_consume_yields_exactly_one_effect() {
    // 同一许可被两个并发请求同时消费：CAS 只允许一个赢家提交 dispatching/executing，
    // 另一个必须失败，handler 只运行一次。
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_permit(events, Tamper::None).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    let (first, second) = tokio::join!(
        broker.execute(authorized.clone()),
        broker.execute(authorized.clone())
    );
    let outcomes = [first, second];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(outcomes.iter().filter(|result| result.is_err()).count(), 1);
    assert!(outcomes.iter().any(|result| {
        matches!(result, Err(PortError::Failed(reason)) if reason == "execution_permit_already_consumed")
            || matches!(result, Err(PortError::Conflict(reason)) if reason == "control_transition_conflict")
    }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        stream_kinds(events.as_ref(), &permit).await,
        [
            "execution.prepared",
            "invocation.dispatching",
            "invocation.executing"
        ]
    );
    let receipt = events
        .read_command(&consume_command(&permit))
        .await
        .unwrap()
        .expect("the winning consume must commit exactly one receipt");
    assert_eq!(receipt.event_ids.len(), 2);
}

#[test]
fn sc12_pending_invocation_rebinds_and_fences_resume_identity() {
    // PendingInvocation 只是等待授权的持久化形式；审批暂停期间任何身份/参数/sandbox 漂移
    // 都必须在 Core 重新派发之前被绑定校验拒绝。
    let mut ctx = RequestContext::local("session-sc12", ".");
    ctx.project_trusted = true;
    let mut request = request(&ctx);
    kiana_domain::normalize_capability_action(&mut request).unwrap();
    request.execution_scope = Some(broker_scope(&ctx, &request, None, None));
    let run = RunId::new();
    let cursor = RequestId::new();
    // Runner 队列摘要必须是 sha256 形状；这里用固定摘要模拟“已绑定的有序批次”。
    let pending_batch_digest = format!("sha256:{}", "b".repeat(64));
    let binding = kiana_domain::InvocationResumeBinding::from_request(
        run,
        cursor,
        &request,
        &ctx,
        "a",
        pending_batch_digest,
    )
    .expect("resume binding");
    assert!(binding.validate().is_ok());
    assert!(binding
        .validate_against(run, cursor, &request, &ctx, "a")
        .is_ok());
    // sandbox 漂移：执行隔离配置变化后不能复用原绑定。
    let sandbox_drift = binding.validate_against(run, cursor, &request, &ctx, "b");
    assert_eq!(
        sandbox_drift.unwrap_err(),
        "invocation_resume_binding_changed"
    );
    // session 漂移：其他会话不能消费该待处理调用。
    let mut other = ctx.clone();
    other.session_id = kiana_domain::SessionId::new("session-other");
    let session_drift = binding.validate_against(run, cursor, &request, &other, "a");
    assert_eq!(
        session_drift.unwrap_err(),
        "invocation_resume_binding_changed"
    );
    // 参数漂移：批准后替换参数不能复用同一绑定。
    let mut changed = request.clone();
    changed.arguments["query"] = json!("different");
    let arg_drift = binding.validate_against(run, cursor, &changed, &ctx, "a");
    assert_eq!(arg_drift.unwrap_err(), "invocation_resume_binding_changed");
    // 摘要被改写但未重算：内部一致性校验必须发现漂移。
    let mut tampered = binding.clone();
    tampered.parameter_digest = format!("sha256:{}", "0".repeat(64));
    let tampered_error = tampered.validate().unwrap_err();
    assert_eq!(tampered_error, "invocation_resume_binding_digest_mismatch");
    // 显式 resume 只允许改事件流游标，不改变 invocation/request 身份。
    let mut rebound = binding.clone();
    let new_cursor = RequestId::new();
    rebound.rebind_event_request(new_cursor).unwrap();
    assert_eq!(rebound.request_id, request.request_id);
    assert_eq!(rebound.event_request_id, new_cursor);
    assert!(rebound.validate().is_ok());

    // PendingInvocation 往返保留事件序号与服务端续接绑定。
    let challenge: kiana_domain::ApprovalChallenge = serde_json::from_value(json!({
        "schema": kiana_domain::APPROVAL_CHALLENGE_SCHEMA,
        "approval_id": ApprovalId::new(),
        "request_id": request.request_id,
        "request_hash": "sha256:binding",
        "expires_at_unix_ms": 1,
        "reason": "approval_required",
        "nonce": "nonce",
        "policy_version": "kiana.policy.v1",
    }))
    .unwrap();
    let pending = kiana_domain::PendingInvocation {
        approval_id: challenge.approval_id,
        challenge,
        request_id: request.request_id,
        event_request_id: cursor,
        event_sequence: 7,
        run_id: run,
        request: request.clone(),
        context: ctx.clone(),
        sandbox: "a".to_owned(),
        resume_binding: Some(binding.clone()),
    };
    let restored: kiana_domain::PendingInvocation =
        serde_json::from_value(serde_json::to_value(&pending).unwrap()).unwrap();
    assert_eq!(restored, pending);
    assert_eq!(restored.event_sequence, 7);
    assert_eq!(restored.resume_binding, Some(binding));
    // 旧 checkpoint 可以缺省 resume_binding，但恢复仍要重新验证受保护材料。
    let mut legacy = serde_json::to_value(&pending).unwrap();
    legacy.as_object_mut().unwrap().remove("resume_binding");
    let legacy: kiana_domain::PendingInvocation = serde_json::from_value(legacy).unwrap();
    assert!(legacy.resume_binding.is_none());
}
