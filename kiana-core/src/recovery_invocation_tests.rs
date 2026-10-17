//! CAP-24 行为夹具：重启后只依据事实重建 Invocation，并把重试限制在证据允许的范围内。
//!
//! 路线图卡片：`docs/roadmap/capability.md#step-cap-24`。三条拒绝夹具与卡片同名：
//! `restart_never_reexecutes_unknown_non_idempotent_call`、
//! `changed_input_or_descriptor_causes_replay_divergence`、
//! `redacted_snapshot_cannot_be_used_as_executable_payload`；另有完成结果复用、外来主体
//! 拒读/拒恢复、待批调用重建等成功与回归夹具。
//!
//! # 为什么这些夹具能证明行为
//!
//! 夹具直接驱动真实的 `ControlPlane::resume_run`、`checkpoint_run` 与
//! `rebuild_pending_invocation`，只把 Runner / ApprovalStore 换成记录调用次数的桩（Broker 用真实
//! `kiana_capability_broker::CapabilityBroker`，夹具场景不派发 handler）。
//! 进程重启用关闭并重开 `JsonlEventLog` 文件模拟；手工事件不带流元数据，因此存储层保留
//! 追加顺序（与 `dispatch_delivery_tests`、`er20_restart_projector` 相同的约定），而
//! `run.snapshot`、`run.resume_prepared` 与终态事件仍由生产代码按 CAS 的流版本写入。
//! 每个「拒绝」断言都同时证明：零 Runner 推进、零审批材料读取、零新事实落盘。
//!
//! # 这些夹具不证明什么
//!
//! 「进程重启」是同一个测试进程内关闭并重开 `JsonlEventLog` 文件，不是 daemon/BackendHost
//! 的真实进程级故障注入：事件落盘前/后、spawn 前/后、收到结果但未落盘等窗口由能力侧的
//! 崩溃窗口矩阵（CAP-26）负责。Runner 是桩：重启后仍持有 pending tool 队列的拒绝用与真实
//! `KianaHarness::resume` 相同的错误码复现，但没有驱动真实 harness 的 pending 队列。
//! 夹具也不证明跨外部服务的 exactly-once——本地 CAS 去重只覆盖本地日志。

use super::*;
use kiana_capability_broker::CapabilityBroker;
use kiana_domain::{
    capability_action_catalog_digest, capability_action_digest, ApprovalChallenge,
    ApprovalDecisionRecord, ApprovalState, AuthenticatedPrincipalRef, CapabilityExecutionState,
    ExecutionScope, InvocationResumeBinding, PendingApproval, ProjectIdentity, ScopeDimension,
    ScopeLimit, ScopeSet, SessionId, TurnId,
};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::path::PathBuf;

const SESSION: &str = "cap24-session";
const PROJECT_ROOT: &str = "/repo";

/// 固定摘要：只满足 sha256 形态，不认证任何真实项目、策略或内容。
fn sha_like(ch: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (ch as u32) & 0xffff).repeat(16)
    )
}

fn unique_path(label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("cap24 clock")
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-cap24-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
}

fn local_context() -> RequestContext {
    let mut context = RequestContext::local(SESSION, PROJECT_ROOT);
    context.project_trusted = true;
    context
}

fn event(sequence: u64, kind: &str, data: Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).expect("cap24 fixture event")
}

async fn read_facts(events: &Arc<dyn EventStorePort>) -> Vec<RuntimeEvent> {
    events.read_all().await.expect("read cap24 journal")
}

fn run_authorized(run_id: RunId, context: &RequestContext, sequence: u64) -> RuntimeEvent {
    event(
        sequence,
        "run.authorized",
        json!({
            "run_id": run_id,
            "session_id": context.session_id,
            "actor_id": context.actor_id,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "project_root": context.project_root,
            "project_trusted": context.project_trusted,
        }),
    )
}

/// 有外部副作用、目录标记为 `external_tool` 的调用：重启后不可自动重放。
fn mcp_call_request(run_id: RunId, turn_id: TurnId) -> CapabilityRequest {
    CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Network,
        "mcp.call",
        json!({
            "server": "cap24-fixture",
            "tool": "cap24.effectful",
            "call_id": "cap24-call",
            "run_id": run_id,
            "turn_id": turn_id,
        }),
    )
    .with_risk(RiskLevel::ExternalSideEffect)
}

fn run_capability_requested(
    run_id: RunId,
    request: &CapabilityRequest,
    sequence: u64,
) -> RuntimeEvent {
    event(
        sequence,
        "run.capability_requested",
        json!({
            "run_id": run_id,
            "capability_request_id": request.request_id,
            "capability": request.capability,
            "operation": request.operation,
            "arguments": request.arguments,
            "risk": request.risk,
            "execution_scope": request.execution_scope,
            "action_digest": capability_action_digest(request),
        }),
    )
}

fn capability_decision(
    run_id: RunId,
    request_id: RequestId,
    decision: &str,
    sequence: u64,
) -> RuntimeEvent {
    event(
        sequence,
        "capability.decision",
        json!({
            "run_id": run_id,
            "capability_request_id": request_id,
            "gate": {"decision": decision, "reason": "cap24-fixture"},
        }),
    )
}

fn execution_prepared(run_id: RunId, request: &CapabilityRequest, sequence: u64) -> RuntimeEvent {
    event(
        sequence,
        "execution.prepared",
        json!({
            "run_id": run_id,
            "capability_request_id": request.request_id,
            "attempt": 1,
            "effect_started": true,
        }),
    )
}

fn invocation_executing(run_id: RunId, request: &CapabilityRequest, sequence: u64) -> RuntimeEvent {
    event(
        sequence,
        "invocation.executing",
        json!({
            "run_id": run_id,
            "capability_request_id": request.request_id,
            "attempt": 1,
        }),
    )
}

/// Runner 桩：记录 checkpoint/restore/send 调用，只回答已声明的结果。
struct Cap24Runner {
    checkpoint_state: Value,
    resume_unknown: bool,
    checkpoint_calls: Mutex<Vec<RunId>>,
    restore_calls: Mutex<Vec<RunId>>,
    send_calls: Mutex<Vec<&'static str>>,
}

impl Cap24Runner {
    fn new(checkpoint_state: Value, resume_unknown: bool) -> Self {
        Self {
            checkpoint_state,
            resume_unknown,
            checkpoint_calls: Mutex::new(Vec::new()),
            restore_calls: Mutex::new(Vec::new()),
            send_calls: Mutex::new(Vec::new()),
        }
    }

    fn checkpoint_calls(&self) -> Vec<RunId> {
        self.checkpoint_calls.lock().unwrap().clone()
    }

    fn restore_calls(&self) -> Vec<RunId> {
        self.restore_calls.lock().unwrap().clone()
    }

    fn send_calls(&self) -> Vec<&'static str> {
        self.send_calls.lock().unwrap().clone()
    }
}

/// Harness 恢复时仍持有有序 pending tool 队列；真实 `KianaHarness::resume` 会以
/// `result_unknown:resume_pending_tool` 拒绝，夹具用同一错误码复现该事实。
fn pending_tool_state() -> Value {
    json!({
        "steps": 3,
        "pending_tools": [{"call_id": "cap24-call", "operation": "mcp.call"}],
    })
}

#[async_trait::async_trait]
impl RunnerPort for Cap24Runner {
    async fn checkpoint(&self, run_id: RunId) -> Result<Value, PortError> {
        self.checkpoint_calls.lock().unwrap().push(run_id);
        Ok(self.checkpoint_state.clone())
    }

    async fn restore(&self, run_id: RunId, _checkpoint: Value) -> Result<(), PortError> {
        self.restore_calls.lock().unwrap().push(run_id);
        Ok(())
    }

    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        let run_id = match command {
            RunnerCommand::Resume { run_id } => {
                self.send_calls.lock().unwrap().push("resume");
                run_id
            }
            _ => panic!("cap24 fixture runner only accepts resume commands"),
        };
        if self.resume_unknown {
            return Ok(vec![RunnerEvent::Failed {
                run_id,
                error: "result_unknown:resume_pending_tool".to_owned(),
            }]);
        }
        Ok(Vec::new())
    }
}

/// 审批存储桩：没有任何可执行材料；用于不涉及审批的恢复路径。
struct NoApprovals;

#[async_trait::async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable(
            "cap24_fixture_no_approval".to_owned(),
        ))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "cap24_fixture_no_approval".to_owned(),
        ))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "cap24_fixture_no_approval".to_owned(),
        ))
    }
}

/// 审批存储桩：返回预置的认证材料并记录读取次数，用来证明拒绝发生在读取之前/之后。
struct RecordingApprovals {
    decision: ApprovalDecisionRecord,
    material: PendingApproval,
    decision_reads: Mutex<usize>,
    material_reads: Mutex<usize>,
}

impl RecordingApprovals {
    fn new(decision: ApprovalDecisionRecord, material: PendingApproval) -> Self {
        Self {
            decision,
            material,
            decision_reads: Mutex::new(0),
            material_reads: Mutex::new(0),
        }
    }

    fn decision_reads(&self) -> usize {
        *self.decision_reads.lock().unwrap()
    }

    fn material_reads(&self) -> usize {
        *self.material_reads.lock().unwrap()
    }
}

#[async_trait::async_trait]
impl ApprovalStorePort for RecordingApprovals {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable(
            "cap24_fixture_stage_unsupported".to_owned(),
        ))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "cap24_fixture_activate_unsupported".to_owned(),
        ))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "cap24_fixture_consume_unsupported".to_owned(),
        ))
    }

    async fn read_decision(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<ApprovalDecisionRecord, PortError> {
        *self.decision_reads.lock().unwrap() += 1;
        Ok(self.decision.clone())
    }

    async fn pending_with_proof(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
        _request_hash: Option<&str>,
        _nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        *self.material_reads.lock().unwrap() += 1;
        Ok(self.material.clone())
    }
}

fn plane(
    events: Arc<dyn EventStorePort>,
    runner: Arc<Cap24Runner>,
    approvals: Arc<dyn ApprovalStorePort>,
) -> Arc<ControlPlane> {
    Arc::new(ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(CapabilityBroker::new()),
        approvals,
        runner,
    ))
}

fn approval_challenge(
    approval_id: ApprovalId,
    request_id: RequestId,
    request_hash: &str,
    expires_at_unix_ms: u64,
) -> ApprovalChallenge {
    ApprovalChallenge {
        schema: "kiana.approval.challenge.v1".to_owned(),
        approval_id,
        request_id,
        request_hash: request_hash.to_owned(),
        risk: RiskLevel::ReadOnly,
        expires_at_unix_ms,
        reason: "cap24 fixture approval".to_owned(),
        nonce: "cap24-nonce".to_owned(),
        policy_version: "cap24-policy-v1".to_owned(),
    }
}

/// 构造与 H14 同形的 ExecutionScope；只用于让 `validate_for_request` 走真实校验路径。
///
/// `action_digest` 始终基于去掉 scope 的请求副本计算，避免自引用摘要。
fn scope_for(request: &CapabilityRequest, run_id: RunId, turn_id: TurnId) -> ExecutionScope {
    let mut bare = request.clone();
    bare.execution_scope = None;
    let permission_scope = ScopeSet::new(
        ScopeDimension::Restricted(vec![request.operation.clone()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::Restricted(1),
        ScopeLimit::Restricted(1),
    )
    .expect("cap24 fixture scope set");
    let project = ProjectIdentity::new(PROJECT_ROOT, PROJECT_ROOT, None, None, sha_like('p'))
        .expect("cap24 fixture project identity");
    let mut scope = ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal: AuthenticatedPrincipalRef::local(),
        project,
        session_id: SessionId::new(SESSION),
        run_id: Some(run_id),
        turn_id: Some(turn_id),
        cell_id: None,
        grant_refs: Vec::new(),
        budget_lease_id: None,
        work_packet_id: None,
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope: permission_scope.clone(),
        read_roots: vec![PROJECT_ROOT.to_owned()],
        write_roots: Vec::new(),
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes: Vec::new(),
        server_scopes: Vec::new(),
        network_policy: Vec::new(),
        authority_epoch: 7,
        trust_revision: sha_like('t'),
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: capability_action_catalog_digest(),
        action_digest: capability_action_digest(&bare),
        permission_scope_digest: permission_scope.digest(),
        scope_digest: String::new(),
    };
    scope.scope_digest = scope.digest();
    scope
}

/// 一条等待人工决定的调用：请求事实、等待审批决策、带 resume_binding 的 approval.requested。
struct PendingFixture {
    request: CapabilityRequest,
    turn_id: TurnId,
    approval_id: ApprovalId,
    approval_event_request_id: RequestId,
    challenge: ApprovalChallenge,
    binding: InvocationResumeBinding,
    events: Vec<RuntimeEvent>,
}

fn pending_fixture(run_id: RunId, context: &RequestContext, suffix: &str) -> PendingFixture {
    let turn_id = TurnId::new();
    let mut request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({
            "command": format!("printf cap24-{suffix}"),
            "sandbox": "read-only",
            "call_id": format!("cap24-call-{suffix}"),
            "run_id": run_id,
            "turn_id": turn_id,
        }),
    )
    .with_risk(RiskLevel::ReadOnly);
    request.execution_scope = Some(scope_for(&request, run_id, turn_id));
    let approval_id = ApprovalId::new();
    let approval_event_request_id = RequestId::new();
    let request_hash = sha_like('h');
    let challenge = approval_challenge(
        approval_id,
        request.request_id,
        &request_hash,
        4_102_444_800_000,
    );
    let binding = InvocationResumeBinding::from_request(
        run_id,
        approval_event_request_id,
        &request,
        context,
        "read-only",
        sha_like('b'),
    )
    .expect("cap24 fixture resume binding");
    let events = vec![
        run_capability_requested(run_id, &request, 2),
        capability_decision(run_id, request.request_id, "awaiting_approval", 3),
        RuntimeEvent::new(
            approval_event_request_id,
            1,
            "approval.requested",
            json!({
                "run_id": run_id,
                "capability_request_id": request.request_id,
                "approval_id": approval_id,
                "action_digest": capability_action_digest(&request),
                "request_hash": request_hash,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
                "expires_at_unix_ms": challenge.expires_at_unix_ms,
                "resume_binding": binding,
            }),
        )
        .expect("cap24 fixture approval event"),
    ];
    PendingFixture {
        request,
        turn_id,
        approval_id,
        approval_event_request_id,
        challenge,
        binding,
        events,
    }
}

fn decision_record(pending: &PendingFixture) -> ApprovalDecisionRecord {
    ApprovalDecisionRecord {
        approval_id: pending.approval_id,
        state: ApprovalState::Active,
        challenge: pending.challenge.clone(),
        decision: None,
        decision_command_id: None,
        decided_by: None,
        dispatch_command_id: None,
        payload_available: true,
        version: 1,
        decision_digest: None,
        consumption_digest: None,
    }
}

fn pending_material(pending: &PendingFixture, request: CapabilityRequest) -> PendingApproval {
    PendingApproval {
        challenge: pending.challenge.clone(),
        request,
    }
}

/// 没有任何待批材料的记录桩：既记录读取次数，也保证误用时能被观察到。
fn empty_approvals() -> Arc<RecordingApprovals> {
    let approval_id = ApprovalId::new();
    let request_id = RequestId::new();
    let challenge = approval_challenge(approval_id, request_id, &sha_like('h'), 0);
    Arc::new(RecordingApprovals::new(
        ApprovalDecisionRecord {
            approval_id,
            state: ApprovalState::Active,
            challenge: challenge.clone(),
            decision: None,
            decision_command_id: None,
            decided_by: None,
            dispatch_command_id: None,
            payload_available: false,
            version: 1,
            decision_digest: None,
            consumption_digest: None,
        },
        PendingApproval {
            challenge,
            request: CapabilityRequest::new(
                request_id,
                CapabilityKind::Process,
                "shell.exec",
                json!({"command": "true"}),
            ),
        },
    ))
}

/// 【拒绝】有外部副作用且对账未完的调用，重启后必须停在 Unknown：不重放、不新增尝试。
#[tokio::test]
async fn restart_never_reexecutes_unknown_non_idempotent_call() {
    let path = unique_path("unknown-non-idempotent");
    let run_id = RunId::new();
    let context = local_context();
    let turn_id = TurnId::new();
    let request = mcp_call_request(run_id, turn_id);
    let mut snapshot_event_id = None;
    {
        let events: Arc<dyn EventStorePort> =
            Arc::new(JsonlEventLog::open(&path).expect("open cap24 durable journal"));
        for fact in [
            run_authorized(run_id, &context, 1),
            run_capability_requested(run_id, &request, 2),
            capability_decision(run_id, request.request_id, "allowed", 3),
            execution_prepared(run_id, &request, 4),
            invocation_executing(run_id, &request, 5),
        ] {
            events.append(fact).await.expect("append cap24 fact");
        }
        let runner = Arc::new(Cap24Runner::new(pending_tool_state(), false));
        let core = plane(events.clone(), runner.clone(), Arc::new(NoApprovals));
        let mut sequence = 1;
        core.checkpoint_run(&context, run_id, "read-only", &mut sequence)
            .await
            .expect("checkpoint paused run");
        assert_eq!(runner.checkpoint_calls(), vec![run_id]);
        assert!(runner.send_calls().is_empty());
        for fact in read_facts(&events).await {
            if fact.kind == "run.snapshot" {
                snapshot_event_id = Some(fact.event_id);
            }
        }
    }
    assert!(
        snapshot_event_id.is_some(),
        "checkpoint must persist a snapshot"
    );

    // 进程重启：只重开文件日志，不带任何进程内缓存。
    let events: Arc<dyn EventStorePort> =
        Arc::new(JsonlEventLog::open(&path).expect("reopen cap24 durable journal"));
    let runner = Arc::new(Cap24Runner::new(pending_tool_state(), true));
    let core = plane(events.clone(), runner.clone(), Arc::new(NoApprovals));
    let response = core
        .resume_run(context.clone(), Some(run_id))
        .await
        .expect("resume returns a response");
    assert_eq!(response.status, ExecutionStatus::ResultUnknown);
    assert_eq!(
        response.error.as_deref(),
        Some("result_unknown:resume_pending_tool")
    );

    // Runner 只被要求重新进入同一个 run；没有任何新的分派或执行事实。
    assert_eq!(runner.send_calls(), vec!["resume"]);
    assert_eq!(runner.restore_calls(), vec![run_id]);
    let facts = read_facts(&events).await;
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.kind == "execution.prepared")
            .count(),
        1
    );
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.kind == "invocation.executing")
            .count(),
        1
    );
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.kind == "run.resume_prepared")
            .count(),
        1
    );
    assert_eq!(
        facts
            .iter()
            .filter(|fact| fact.kind == "run.result_unknown")
            .count(),
        1
    );
    assert!(facts.iter().all(|fact| fact.kind != "run.completed"));
    let claim = facts
        .iter()
        .find(|fact| fact.kind == "run.resume_prepared")
        .expect("resume claim");
    assert_eq!(
        claim.data["snapshot_event_id"],
        json!(snapshot_event_id.expect("snapshot id"))
    );

    let projections = project_invocations(run_id, &facts).expect("invocation projection");
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].request_id, request.request_id);
    assert_eq!(projections[0].state, CapabilityExecutionState::Unknown);
    assert!(projections[0].result.is_none());

    // 对同一个已记录 ResultUnknown 的 run 再次恢复：不得二次安装 Runner。
    let response = core
        .resume_run(context, Some(run_id))
        .await
        .expect("second resume returns a response");
    assert_eq!(
        response.error.as_deref(),
        Some("result_unknown:run_resume_result_unknown")
    );
    assert_eq!(runner.send_calls(), vec!["resume"]);
    assert_eq!(runner.restore_calls(), vec![run_id]);
    assert_eq!(read_facts(&events).await.len(), facts.len());
    cleanup(&path);
}

/// 【拒绝】审批等待期间改动输入或执行描述符，重放必须分叉并零执行。
#[tokio::test]
async fn changed_input_or_descriptor_causes_replay_divergence() {
    let run_id = RunId::new();
    let context = local_context();
    let pending = pending_fixture(run_id, &context, "divergence");
    let mut events = vec![run_authorized(run_id, &context, 1)];
    events.extend(pending.events.clone());
    let projections = project_invocations(run_id, &events).expect("pending projection");
    assert_eq!(projections.len(), 1);
    assert_eq!(
        projections[0].state,
        CapabilityExecutionState::AwaitingApproval
    );
    assert_eq!(projections[0].approval_id, Some(pending.approval_id));

    // 变体 1：受保护材料里的命令被改写（scope 同步重算，先通过 scope 校验再撞上动作摘要）。
    let mut changed = pending.request.clone();
    changed.arguments["command"] = json!("printf changed");
    changed.execution_scope = Some(scope_for(&changed, run_id, pending.turn_id));
    let approvals = Arc::new(RecordingApprovals::new(
        decision_record(&pending),
        pending_material(&pending, changed),
    ));
    let runner = Arc::new(Cap24Runner::new(Value::Null, false));
    let store: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let core = plane(store.clone(), runner.clone(), approvals.clone());
    let error = core
        .rebuild_pending_invocation(&context, run_id, "read-only", &events, &projections, None)
        .await
        .expect_err("changed input must not rebuild a continuation");
    assert!(
        error.to_string().contains("approval_action_changed"),
        "unexpected divergence: {error}"
    );
    assert_eq!(approvals.decision_reads(), 1);
    // 材料被读两次：一次用于构造续体，一次是共享尾部的重新校验；读取次数本身也是证据。
    assert_eq!(approvals.material_reads(), 2);
    assert!(runner.send_calls().is_empty());
    assert!(runner.restore_calls().is_empty());
    assert!(read_facts(&store).await.is_empty());

    // 变体 2：材料一字未改，但恢复用的执行描述符（sandbox）与暂停时不同 → 绑定必须拒绝。
    let approvals = Arc::new(RecordingApprovals::new(
        decision_record(&pending),
        pending_material(&pending, pending.request.clone()),
    ));
    let runner = Arc::new(Cap24Runner::new(Value::Null, false));
    let core = plane(
        Arc::new(MemoryEventLog::new()),
        runner.clone(),
        approvals.clone(),
    );
    let error = core
        .rebuild_pending_invocation(
            &context,
            run_id,
            "workspace-write",
            &events,
            &projections,
            None,
        )
        .await
        .expect_err("changed descriptor must not rebuild a continuation");
    assert!(
        error
            .to_string()
            .contains("invocation_resume_binding_changed"),
        "unexpected divergence: {error}"
    );
    assert_eq!(approvals.decision_reads(), 1);
    // 材料被读两次：一次用于构造续体，一次是共享尾部的重新校验；读取次数本身也是证据。
    assert_eq!(approvals.material_reads(), 2);
    assert!(runner.send_calls().is_empty());
    assert!(runner.restore_calls().is_empty());
}

/// 【拒绝】被脱敏的快照不能被复活成可执行载荷：先看 `resumable` 标志，再谈解码。
#[tokio::test]
async fn redacted_snapshot_cannot_be_used_as_executable_payload() {
    let path = unique_path("redacted-snapshot");
    let run_id = RunId::new();
    let context = local_context();
    let secret_state = json!({
        "api_key": "cap24-secret",
        "messages": [{"content": "{\"api_key\":\"cap24-secret-nested\"}"}],
    });
    let mut snapshot_value = Value::Null;
    {
        let events: Arc<dyn EventStorePort> =
            Arc::new(JsonlEventLog::open(&path).expect("open cap24 durable journal"));
        events
            .append(run_authorized(run_id, &context, 1))
            .await
            .expect("append cap24 fact");
        let runner = Arc::new(Cap24Runner::new(secret_state, false));
        let core = plane(events.clone(), runner, Arc::new(NoApprovals));
        let mut sequence = 1;
        core.checkpoint_run(&context, run_id, "read-only", &mut sequence)
            .await
            .expect("checkpoint paused run");
        for fact in read_facts(&events).await {
            if fact.kind == "run.snapshot" {
                snapshot_value = fact.data["snapshot"].clone();
            }
        }
    }
    assert_eq!(snapshot_value["resumable"], json!(false));
    let durable = std::fs::read_to_string(&path).expect("read cap24 journal");
    assert!(
        !durable.contains("cap24-secret"),
        "redacted checkpoint leaked into the journal"
    );

    let events: Arc<dyn EventStorePort> =
        Arc::new(JsonlEventLog::open(&path).expect("reopen cap24 durable journal"));
    let approvals = empty_approvals();
    let runner = Arc::new(Cap24Runner::new(pending_tool_state(), true));
    let core = plane(events.clone(), runner.clone(), approvals.clone());
    let before = read_facts(&events).await;
    let response = core
        .resume_run(context.clone(), Some(run_id))
        .await
        .expect("resume returns a response");
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("run_snapshot_invalid"));
    assert_eq!(approvals.decision_reads(), 0);
    assert_eq!(approvals.material_reads(), 0);
    assert!(runner.send_calls().is_empty());
    assert!(runner.restore_calls().is_empty());
    assert_eq!(read_facts(&events).await.len(), before.len());

    // 伪造：老/坏写入方可能既忘记 `resumable=false`，又留下自洽摘要；即便这样，
    // 脱敏后的占位材料也不能被复活成可执行续体。
    let mut forged = snapshot_value;
    // 先单独取出摘要再改写字段：不要在同一条赋值里既读又写同一份 JSON，读起来也更清楚。
    let forged_runner_state_digest = json_digest(&forged["runner_state"]);
    forged["resumable"] = json!(true);
    forged["runner_state_digest"] = json!(forged_runner_state_digest);
    let forged_approval_id = ApprovalId::new();
    let forged_request_id = RequestId::new();
    let forged_context = forged["context"].clone();
    forged["pending_invocation"] = json!({
        "approval_id": forged_approval_id,
        "challenge": approval_challenge(forged_approval_id, forged_request_id, "[REDACTED]", 0),
        "request_id": forged_request_id,
        "event_request_id": forged_request_id,
        "event_sequence": 1,
        "run_id": run_id,
        "request": CapabilityRequest::new(
            forged_request_id,
            CapabilityKind::Process,
            "shell.exec",
            json!({"command": "[REDACTED]"}),
        ),
        "context": forged_context,
        "sandbox": "read-only",
        "resume_binding": Value::Null,
    });
    events
        .append(event(
            60,
            "run.snapshot",
            json!({"run_id": run_id, "snapshot": forged}),
        ))
        .await
        .expect("append forged snapshot");
    let before_forged = read_facts(&events).await;
    let response = core
        .resume_run(context, Some(run_id))
        .await
        .expect("resume returns a response");
    let forged_error = response.error.clone();
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert!(
        forged_error
            .as_deref()
            .unwrap_or_default()
            .contains("run_snapshot_pending_invocation_missing"),
        "unexpected forged-snapshot behavior: {forged_error:?}"
    );
    assert_eq!(approvals.decision_reads(), 0);
    assert_eq!(approvals.material_reads(), 0);
    assert!(runner.send_calls().is_empty());
    assert!(runner.restore_calls().is_empty());
    assert_eq!(read_facts(&events).await.len(), before_forged.len());
    cleanup(&path);
}

/// 【拒绝】外来主体不能在读取任何审批材料之前读到或恢复别人的 Run。
#[tokio::test]
async fn foreign_owner_cannot_read_or_resume_another_runs_invocation() {
    let path = unique_path("foreign-owner");
    let run_id = RunId::new();
    let context = local_context();
    let pending = pending_fixture(run_id, &context, "owner");
    {
        let events: Arc<dyn EventStorePort> =
            Arc::new(JsonlEventLog::open(&path).expect("open cap24 durable journal"));
        for fact in
            std::iter::once(run_authorized(run_id, &context, 1)).chain(pending.events.clone())
        {
            events.append(fact).await.expect("append cap24 fact");
        }
        let runner = Arc::new(Cap24Runner::new(pending_tool_state(), false));
        let core = plane(events.clone(), runner, Arc::new(NoApprovals));
        let mut sequence = 1;
        core.checkpoint_run(&context, run_id, "read-only", &mut sequence)
            .await
            .expect("checkpoint paused run");
    }
    let events: Arc<dyn EventStorePort> =
        Arc::new(JsonlEventLog::open(&path).expect("reopen cap24 durable journal"));
    let approvals = Arc::new(RecordingApprovals::new(
        decision_record(&pending),
        pending_material(&pending, pending.request.clone()),
    ));
    let runner = Arc::new(Cap24Runner::new(pending_tool_state(), true));
    let core = plane(events.clone(), runner.clone(), approvals.clone());
    let before = read_facts(&events).await;

    // 变体 1：同一 session 但换一个 actor。
    let mut intruder = context.clone();
    intruder.actor_id = Some("cap24-intruder".to_owned());
    let response = core
        .resume_run(intruder, Some(run_id))
        .await
        .expect("resume returns a response");
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("session_owner_mismatch"));

    // 变体 2：完全陌生的 session。
    let mut foreign_session = context.clone();
    foreign_session.session_id = SessionId::new("cap24-foreign-session");
    let response = core
        .resume_run(foreign_session, Some(run_id))
        .await
        .expect("resume returns a response");
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("session_not_found"));

    // 两条拒绝都发生在任何材料读取与 Runner 推进之前。
    assert_eq!(approvals.decision_reads(), 0);
    assert_eq!(approvals.material_reads(), 0);
    assert!(runner.send_calls().is_empty());
    assert!(runner.restore_calls().is_empty());
    assert_eq!(read_facts(&events).await.len(), before.len());
    cleanup(&path);
}

/// 【回归】完成结果只能从已提交事实复用；重放终态必须被拒绝且不再执行。
#[tokio::test]
async fn completed_result_is_reused_from_facts_and_terminal_replay_is_denied() {
    let path = unique_path("completed-result");
    let run_id = RunId::new();
    let context = local_context();
    let turn_id = TurnId::new();
    let request = mcp_call_request(run_id, turn_id);
    let output = json!({"stdout": "cap24-done", "exit_code": 0});
    let committed_result = json!({
        "request_id": request.request_id,
        "success": true,
        "output": output.clone(),
        "evidence_refs": [],
    });
    {
        let events: Arc<dyn EventStorePort> =
            Arc::new(JsonlEventLog::open(&path).expect("open cap24 durable journal"));
        for fact in [
            run_authorized(run_id, &context, 1),
            run_capability_requested(run_id, &request, 2),
            capability_decision(run_id, request.request_id, "allowed", 3),
            execution_prepared(run_id, &request, 4),
            event(
                5,
                "execution.result_committed",
                json!({
                    "run_id": run_id,
                    "capability_request_id": request.request_id,
                    "attempt": 1,
                    "effect_known": true,
                    "result": committed_result,
                }),
            ),
            event(
                6,
                "run.completed",
                json!({"run_id": run_id, "output": output.clone()}),
            ),
        ] {
            events.append(fact).await.expect("append cap24 fact");
        }
    }
    let events: Arc<dyn EventStorePort> =
        Arc::new(JsonlEventLog::open(&path).expect("reopen cap24 durable journal"));
    let runner = Arc::new(Cap24Runner::new(Value::Null, false));
    let core = plane(events.clone(), runner.clone(), Arc::new(NoApprovals));
    let facts = read_facts(&events).await;

    // 完成结果直接来自事件事实，不需要 Runner 或 Broker 再执行一次。
    let projections = project_invocations(run_id, &facts).expect("completed projection");
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].request_id, request.request_id);
    assert_eq!(projections[0].state, CapabilityExecutionState::Succeeded);
    let reused = projections[0]
        .result
        .clone()
        .expect("committed result reused");
    assert_eq!(reused["success"], json!(true));
    assert_eq!(reused["output"], output);

    let response = core
        .resume_run(context.clone(), Some(run_id))
        .await
        .expect("resume returns a response");
    assert_eq!(response.status, ExecutionStatus::ResultUnknown);
    assert_eq!(response.output["resume"], json!("denied"));
    assert_eq!(
        response.output["reason"],
        json!("run_resume_terminal_completed")
    );
    assert_eq!(
        response.error.as_deref(),
        Some("result_unknown:run_resume_terminal_completed")
    );

    // 没有第二次执行，也没有第二个终态。
    assert!(runner.send_calls().is_empty());
    assert!(runner.restore_calls().is_empty());
    let after = read_facts(&events).await;
    assert_eq!(
        after
            .iter()
            .filter(|fact| fact.kind == "execution.prepared")
            .count(),
        1
    );
    assert_eq!(
        after
            .iter()
            .filter(|fact| fact.kind == "execution.result_committed")
            .count(),
        1
    );
    assert_eq!(
        after
            .iter()
            .filter(|fact| fact.kind == "run.completed")
            .count(),
        1
    );
    assert_eq!(after.len(), facts.len());
    cleanup(&path);
}

/// 【成功/拒绝】待批调用完全从事实与受控材料重建；多个候选时必须 fail-closed。
#[tokio::test]
async fn pending_approval_rebuild_is_fact_bound_and_ambiguity_fails_closed() {
    let run_id = RunId::new();
    let context = local_context();
    let pending = pending_fixture(run_id, &context, "rebuild");
    let mut events = vec![run_authorized(run_id, &context, 1)];
    events.extend(pending.events.clone());
    let projections = project_invocations(run_id, &events).expect("pending projection");

    // 成功：续体完全来自已提交事件 + 受控审批材料，重建本身不执行、不写事实。
    let approvals = Arc::new(RecordingApprovals::new(
        decision_record(&pending),
        pending_material(&pending, pending.request.clone()),
    ));
    let runner = Arc::new(Cap24Runner::new(Value::Null, false));
    let store: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let core = plane(store.clone(), runner.clone(), approvals.clone());
    let rebuilt = core
        .rebuild_pending_invocation(&context, run_id, "read-only", &events, &projections, None)
        .await
        .expect("rebuild pending invocation")
        .expect("exactly one pending invocation");
    assert_eq!(rebuilt.approval_id, pending.approval_id);
    assert_eq!(rebuilt.request_id, pending.request.request_id);
    assert_eq!(rebuilt.event_request_id, pending.approval_event_request_id);
    assert_eq!(rebuilt.request, pending.request);
    assert_eq!(rebuilt.resume_binding.as_ref(), Some(&pending.binding));
    assert_eq!(rebuilt.event_sequence, 1);
    assert_eq!(approvals.decision_reads(), 1);
    // 材料被读两次：一次用于构造续体，一次是共享尾部的重新校验；读取次数本身也是证据。
    assert_eq!(approvals.material_reads(), 2);
    assert!(runner.send_calls().is_empty());
    assert!(runner.restore_calls().is_empty());
    assert!(read_facts(&store).await.is_empty());

    // 拒绝：两个待批候选时不能猜，且不得再读取任何审批材料。
    let second = pending_fixture(run_id, &context, "ambiguous");
    let mut ambiguous = events.clone();
    ambiguous.extend(second.events.clone());
    let projections = project_invocations(run_id, &ambiguous).expect("two pending projections");
    let pending_count = projections
        .iter()
        .filter(|projection| projection.state == CapabilityExecutionState::AwaitingApproval)
        .count();
    assert_eq!(pending_count, 2);
    let approvals = Arc::new(RecordingApprovals::new(
        decision_record(&pending),
        pending_material(&pending, pending.request.clone()),
    ));
    let runner = Arc::new(Cap24Runner::new(Value::Null, false));
    let core = plane(
        Arc::new(MemoryEventLog::new()),
        runner.clone(),
        approvals.clone(),
    );
    let error = core
        .rebuild_pending_invocation(
            &context,
            run_id,
            "read-only",
            &ambiguous,
            &projections,
            None,
        )
        .await
        .expect_err("ambiguous facts must fail closed");
    assert!(
        error
            .to_string()
            .contains("approval_continuation_ambiguous"),
        "unexpected ambiguity behavior: {error}"
    );
    assert_eq!(approvals.decision_reads(), 0);
    assert_eq!(approvals.material_reads(), 0);
    assert!(runner.send_calls().is_empty());
    assert!(runner.restore_calls().is_empty());
}
