//! CP-13 执行许可与派发线性化点的行为夹具。
//!
//! 这些夹具把「授权字符串」「取消」「重放」「预算/资源预留绑定」四条边界钉在真实
//! `JournalPermitVerifier` 与 `CapabilityBroker` 上：只有许可被一次性消费并提交
//! `invocation.dispatching`/`invocation.executing` 之后，handler 才可能被调用。任何
//! 伪造授权串、派发前取消、重复消费或未绑定预留的 prepared 事实都必须在 handler 之前拒绝。
//!
//! 夹具只使用内存 EventStore 与计数 handler；它们证明的是**提交边界**上的调用次数，
//! 不证明外部副作用、跨进程恢复或 across-provider exactly-once。
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_core::JournalPermitVerifier;
use kiana_domain::{
    AggregateVersion, AuthenticatedPrincipalRef, AuthorizedCapabilityRequest, CapabilityKind,
    CapabilityRequest, CapabilityResult, DispatchCrashClass, DispatchPermit, ExecutionId,
    ExecutionScope, InvocationId, InvocationIdentity, ProjectIdentity, RequestContext, RequestId,
    RunId, RuntimeEvent, ScopeDimension, ScopeLimit, ScopeSet, TurnId, DISPATCH_PERMIT_SCHEMA,
    DISPATCH_PERMIT_VERSION,
};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{CapabilityBrokerPort, EventStorePort, ExecutionPermitVerifierPort, PortError};
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
        json!({"query":"permit","call_id":"cp13-call"}),
    )
}

fn broker_scope(
    context: &RequestContext,
    request: &CapabilityRequest,
    run_id: RunId,
    turn_id: TurnId,
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
    let trust_revision = kiana_domain::json_digest(&json!("cp13-server-trust"));
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
        run_id: Some(run_id),
        turn_id: Some(turn_id),
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

/// 在 EventStore 中写入一条已提交 `execution.prepared` 事实，并返回可消费的授权请求。
///
/// `cell_reservation` 与 `reservation_digest` 由调用方提供，用于覆盖“预留已绑定/未绑定”两条路径；
/// 其他字段严格复刻 Core 写出的准备事实形状，使 verifier 的逐字段校验通过。
async fn prepared_permit(
    events: Arc<dyn EventStorePort>,
    cell_reservation: Option<Value>,
    tamper_reservation_digest: bool,
) -> (
    Arc<JournalPermitVerifier>,
    Arc<dyn EventStorePort>,
    AuthorizedCapabilityRequest,
    DispatchPermit,
) {
    let verifier = Arc::new(JournalPermitVerifier::new(events.clone()));
    let mut context = RequestContext::local("session-cp13", ".");
    context.project_trusted = true;
    let mut request = request(&context);
    kiana_domain::normalize_capability_action(&mut request).unwrap();
    let execution_id = ExecutionId::new();
    let invocation_id = InvocationId::from_uuid(request.request_id.as_uuid());
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    request.execution_scope = Some(broker_scope(&context, &request, run_id, turn_id));
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
        schema: DISPATCH_PERMIT_SCHEMA.to_owned(),
        version: DISPATCH_PERMIT_VERSION,
        execution_id,
        invocation_id,
        request_id: request.request_id,
        run_id: Some(run_id),
        turn_id: Some(turn_id),
        decision_id: "policy:cp13".to_owned(),
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
    let invocation = InvocationIdentity::new(
        run_id,
        turn_id,
        invocation_id,
        execution_id,
        request.arguments["call_id"].as_str().map(str::to_owned),
        1,
    )
    .unwrap();
    let reservation_value = cell_reservation.unwrap_or(Value::Null);
    let mut reservation_digest = kiana_domain::dispatch_reservation_digest(
        &reservation_value,
        &permit.authority_versions,
    );
    if tamper_reservation_digest {
        reservation_digest = format!("sha256:{}", "0".repeat(64));
    }
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
                    "attempt":1,
                    "permit":permit,
                    "cell_reservation":reservation_value,
                    "reservation_digest":reservation_digest,
                    "invocation":invocation,
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
        Err(PortError::Failed("cp13_unexpected_handler".to_owned()))
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

#[tokio::test]
async fn cp_forged_authorization_string_is_not_a_permit() {
    // 伪造的授权串不是许可：既不是 `permit:<execution_id>` 形状，也没有对应的 prepared 事实。
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_permit(events, None, false).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    for authorization_id in ["policy:cp13", "approval:cp13", "opaque", "permit:"] {
        let forged =
            AuthorizedCapabilityRequest::new(authorization_id, authorized.request.clone()).unwrap();
        assert!(
            matches!(broker.execute(forged).await, Err(PortError::Failed(reason))
                if reason == "execution_permit_required"),
            "forged authorization {authorization_id} must never act as a permit"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    // 一个未知但形状合法的 execution id 同样没有 prepared 事实可消费。
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
    // 拒绝路径不能写任何消费事实：流仍只有 prepared。
    assert_eq!(
        stream_kinds(events.as_ref(), &permit).await,
        ["execution.prepared"]
    );
    let class = kiana_domain::classify_dispatch_stream(
        &events
            .read_stream("execution_permit", &permit.execution_id.to_string())
            .await
            .unwrap(),
    );
    assert_eq!(class, DispatchCrashClass::PreparedUndispatched);
    assert!(!class.handler_may_have_started());

    // 许可只能从已提交事实解码，不能由一个 JSON 字符串冒充。
    assert!(DispatchPermit::from_json(&json!(format!("permit:{}", permit.execution_id))).is_err());
    assert!(DispatchPermit::from_json(&Value::Null).is_err());
}

#[tokio::test]
async fn cp_cancel_before_dispatch_commit_starts_no_handler() {
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_permit(events, None, false).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    let (_keep, cancelled) = tokio::sync::watch::channel(true);
    assert!(matches!(
        broker.execute_cancellable(authorized, cancelled).await,
        Err(PortError::Failed(reason)) if reason == "cancelled:before_broker"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // 取消发生在 permit 消费 CAS 之前，因此不会有 dispatching/executing，也没有消费回执。
    assert_eq!(
        stream_kinds(events.as_ref(), &permit).await,
        ["execution.prepared"]
    );
    let consume =
        kiana_domain::derived_request_id("permit.consume", &permit.execution_id.to_string());
    assert!(events.read_command(&consume).await.unwrap().is_none());
    let class = kiana_domain::classify_dispatch_stream(
        &events
            .read_stream("execution_permit", &permit.execution_id.to_string())
            .await
            .unwrap(),
    );
    assert_eq!(class, DispatchCrashClass::PreparedUndispatched);
    assert!(!class.handler_may_have_started());
    // 孤儿许可仍需要一条 Unknown 终结事实，不能被当成已成功派发。
    assert!(class.requires_unknown_terminal());
}

#[tokio::test]
async fn cp_replayed_permit_never_dispatches_twice() {
    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) = prepared_permit(events, None, false).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    let result = broker.execute(authorized.clone()).await.unwrap();
    assert!(result.success);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let after_first = events
        .read_stream("execution_permit", &permit.execution_id.to_string())
        .await
        .unwrap();
    assert_eq!(
        after_first
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        [
            "execution.prepared",
            "invocation.dispatching",
            "invocation.executing"
        ]
    );
    assert_eq!(
        kiana_domain::classify_dispatch_stream(&after_first),
        DispatchCrashClass::EffectPossiblyStarted
    );

    // 同一许可重放：消费 CAS 已提交，必须拒绝且不改变流、不再调用 handler。
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
}

#[tokio::test]
async fn cp_prepared_reservation_that_is_not_bound_to_the_transaction_is_rejected() {
    // 预留快照与许可 read-set 绑定正确时，消费可以在提交 dispatching/executing 后进入 handler。
    let bound: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let reservation = json!({"budget":{"lease_id":"budget-1"},"cell":{"cell_id":"cell-1"}});
    let (verifier, events, authorized, permit) =
        prepared_permit(bound, Some(reservation.clone()), false).await;
    verifier.verify_and_consume(&authorized).await.unwrap();
    assert_eq!(
        stream_kinds(events.as_ref(), &permit).await,
        [
            "execution.prepared",
            "invocation.dispatching",
            "invocation.executing"
        ]
    );

    // 同一份预留快照但绑定摘要被改写：verifier 必须在写 dispatching/executing 之前拒绝，
    // handler 调用次数保持为零，流仍只有 prepared。
    let tampered: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let (verifier, events, authorized, permit) =
        prepared_permit(tampered, Some(reservation), true).await;
    let (broker, calls) = broker_with_counter(verifier, events.clone());
    assert!(matches!(
        broker.execute(authorized).await,
        Err(PortError::Failed(reason)) if reason == "execution_permit_reservation_unbound"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        stream_kinds(events.as_ref(), &permit).await,
        ["execution.prepared"]
    );
}
