//! 派发与结果投递的行为测试套。
//!
//! ER-13 behavioral fixtures 使用真实的原子日志和投递助手；没有 Handler 运行。
//!
//! # 测试覆盖范围
//!
//! 本模块测试派发层（dispatch）在以下场景的行为：
//!
//! **准备阶段**（事件流的构造与提交）
//! - `missing_or_append_only_execution_facts_never_claim_or_call_runner`：
//!   验证在准备不完整（缺少 prepare、缺少 result_committed）时，投递拒绝
//! - 各类参数篡改场景（run/turn/request/execution/invocation/attempt 等）
//!
//! **投递阶段**（结果的主张与验证）
//! - `delivery_before_commit_is_denied_then_claims_once_after_commit`：
//!   验证 committed fact 缺失/不匹配时以稳定码拒绝、读回一致后恰好一次 claim 且重试幂等
//! - `committed_source_receipt_is_claimed_before_exact_result_reaches_runner_once`：
//!   验证结果只向 Runner 报告一次，且会主张（claim）投递权
//! - `concurrent_claims_advance_runner_at_most_once`：
//!   验证并发的多个投递请求中，至多一个成功，其他因 CAS 冲突而失败
//! - `changed_result_duplicate_completion_and_old_turn_cannot_advance`：
//!   验证结果被篡改、或发生了重复/老 turn 时，投递拒绝
//!
//! **取消与竞态条件**
//! - `current_terminal_or_cancelling_run_never_claims_result`：
//!   验证 Run 已终止或已取消时，投递拒绝
//! - `cancellation_after_a_winning_delivery_does_not_reopen_its_claim`：
//!   验证投递赢得后，即使 Run 后来被取消，也不重新开放主张
//!
//! **收据与身份验证**
//! - `contradictory_committed_payloads_or_tampered_receipts_never_deliver`：
//!   验证收据被篡改时投递拒绝
//! - `delivery_uses_declared_typed_turn_and_rejects_missing_or_conflicting_identity`：
//!   验证 Turn 身份的验证与类型化语义
//!
//! **故障模式**
//! - `source_and_run_cas_races_or_unconfirmed_receipts_have_zero_delivery`：
//!   验证在 CAS 竞态或日志响应异常时，投递为 0
//! - `failed_callback_or_unknown_claim_ack_is_never_retried`：
//!   验证一旦 Runner 回调失败或日志返回 Unknown，就不再重试
//!
//! # 测试工具
//!
//! - **Fixture**：预构造一个完整的执行场景（Run、Permit、Result、Events）
//! - **FaultStore**：一个装饰器，在特定时刻注入故障（CAS 冲突、收据篡改等）
//! - **CountingRunner**：Mock Runner，记录接收到的所有回调，支持失败模式注入
//!
//! # fail-closed 验证
//!
//! 本模块的核心原则是：派发在任何不确定的地方都应该拒绝。
//! 每个测试都验证了「不应该投递」的情况下，确实没有投递。
use super::*;
use kiana_capability_broker::CapabilityBroker;
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityExecutionState, CapabilityKind, EventId,
    PendingApproval, TurnIdentity, TurnSemantics,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, RunnerPort};
use std::sync::atomic::{AtomicBool, Ordering};

/// Mock 审批存储，所有操作都返回不可用。
/// 这个 fixture 用于测试不需要审批的派发流程。
struct NoApprovals;

#[async_trait::async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _: &RequestContext,
        _: CapabilityRequest,
        _: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
    async fn activate(&self, _: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
    async fn consume(
        &self,
        _: &RequestContext,
        _: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
}

/// Mock Runner，用于测试。
///
/// 这个 Mock 的职责是：
/// - 记录每一次 Runner 被调用的事实（calls 向量）
/// - 验证派发层是否正确地主张了投递权（claims 已经在 events 中）
/// - 支持注入失败（fail = true 时，send() 会返回错误）
///
/// 通过控制 `fail` 标志，我们可以测试「Runner 回调失败」这个故障场景。
struct CountingRunner {
    /// 事件存储的引用，用于验证 result.deliver 命令是否已主张
    events: Arc<MemoryEventLog>,
    /// 记录的 (run_id, result) 对。每当 Runner 被调用一次，这里就增加一条
    calls: Mutex<Vec<(RunId, CapabilityResult)>>,
    /// 是否注入故障（true 时，send() 返回错误）
    fail: bool,
}

#[async_trait::async_trait]
impl RunnerPort for CountingRunner {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        // 派发层只会发送一种命令：CapabilityResult
        let RunnerCommand::CapabilityResult { run_id, result } = command else {
            panic!("delivery must only send its committed capability result")
        };

        // 验证派发层是否已经在事件日志中主张了这个投递权
        let claim_id = derived_request_id("result.deliver", &result.request_id.to_string());
        assert!(self.events.read_command(&claim_id).await?.is_some());
        self.calls.lock().unwrap().push((run_id, result));
        if self.fail {
            Err(PortError::Unavailable("fixture_callback_failed".to_owned()))
        } else {
            Ok(Vec::new())
        }
    }
}

fn plane(events: Arc<dyn EventStorePort>, runner: Arc<CountingRunner>) -> Arc<ControlPlane> {
    // 工厂函数：构造一个 ControlPlane（dispatch 层的入口）
    // 参数：事件存储和 Mock Runner
    Arc::new(ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        runner,
    ))
}

/// 构造一个 run.prompt 事件（回合开始事件）
/// 这是 Run 流中的标准事件，标记一个新的模型回合开始
fn scoped_prompt(run_id: RunId, version: u64, semantics: TurnSemantics) -> RuntimeEvent {
    let turn_id = TurnId::new();
    let turn =
        TurnIdentity::new("er13-fixture", run_id, turn_id, None, semantics, version).unwrap();
    RuntimeEvent::new(RequestId::new(), 1, "run.prompt",
        json!({"run_id":run_id,"turn_id":turn_id,"turn":turn,"session_id":"er13-fixture","text":"fixture"}),
    ).unwrap().with_stream_metadata("run", run_id.to_string(), version)
}

/// 测试 Fixture：一个完整的执行场景预构造。
///
/// 【包含的内容】
/// - 一个 Run（包含 prompt 事件）
/// - 一个有效的 DispatchPermit
/// - 一个能力请求和执行结果
/// - 关键的 ID（run_id、turn_id、permit.execution_id 等）
///
/// 【使用方式】
/// 1. 调用 Fixture::new() 或 with_turn_semantics() 创建
/// 2. 调用 prepare() 向事件日志提交执行.prepared 事件
/// 3. 构造 source() 事件（执行.result_committed）并提交
/// 4. 调用 core() 得到 ControlPlane，然后调用 deliver_capability_result()
/// 5. 调用 no_delivery() 验证「没有投递」的场景
struct Fixture {
    events: Arc<MemoryEventLog>,
    runner: Arc<CountingRunner>,
    run_id: RunId,
    turn_request_id: RequestId,
    request: CapabilityRequest,
    permit: DispatchPermit,
    result: CapabilityResult,
}

impl Fixture {
    /// 创建一个默认语义的 Fixture（Start）
    async fn new() -> Self {
        Self::with_turn_semantics(TurnSemantics::Start).await
    }

    /// 创建一个具有特定 Turn 语义的 Fixture
    ///
    /// 【Turn 语义】
    /// - Start：新的单轮对话
    /// - NewTurn：同一次运行中的新回合
    /// - LegacyContinue：兼容模式下的继续
    /// - Resume：从检查点恢复
    ///
    /// 这个方法预构造：
    /// - 一个新的 Run（添加初始 prompt 事件到事件流）
    /// - 一个新的 DispatchPermit（带有正确的 run_id、turn_id）
    /// - 一个能力请求和正常结果
    /// - 所有 ID 一致且有效
    async fn with_turn_semantics(semantics: TurnSemantics) -> Self {
        let events = Arc::new(MemoryEventLog::new());
        let runner = Arc::new(CountingRunner {
            events: events.clone(),
            calls: Mutex::new(Vec::new()),
            fail: false,
        });
        let run_id = RunId::new();
        let turn_request_id = RequestId::new();
        let turn_id = TurnId::new();
        let identity = TurnIdentity::new(
            "er13-fixture",
            run_id,
            turn_id,
            matches!(semantics, TurnSemantics::NewTurn).then(RunId::new),
            semantics,
            if matches!(semantics, TurnSemantics::LegacyContinue) {
                2
            } else {
                1
            },
        )
        .unwrap();
        events
            .append(
                RuntimeEvent::new(
                    turn_request_id,
                    1,
                    "run.prompt",
                    json!({"run_id":run_id,"turn_id":turn_id,"turn":identity,"session_id":"er13-fixture","text":"fixture"}),
                )
                .unwrap()
                .with_stream_metadata("run", run_id.to_string(), 1),
            )
            .await
            .expect("run prompt");
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "memory.search",
            json!({"call_id":"fixture-call","query":"fixture"}),
        );
        let mut permit = DispatchPermit {
            schema: DISPATCH_PERMIT_SCHEMA.to_owned(),
            version: DISPATCH_PERMIT_VERSION,
            execution_id: ExecutionId::new(),
            invocation_id: InvocationId::from_uuid(request.request_id.as_uuid()),
            request_id: request.request_id,
            run_id: Some(run_id),
            turn_id: Some(turn_id),
            decision_id: "fixture-authorization".to_owned(),
            approval_id: None,
            context: RequestContext::local("er13-fixture", "/fixture"),
            action_digest: kiana_domain::capability_action_digest(&request),
            project_identity: json!({"canonical_root":"/fixture"}),
            authority_versions: vec![AggregateVersion::new("run", run_id.to_string(), 1)],
            issued_at_unix_ms: 1_000,
            expires_at_unix_ms: 2_000,
            permit_digest: String::new(),
        };
        permit.permit_digest = permit.digest();
        permit.validate().expect("fixture permit");
        let result = kiana_domain::normalize_capability_result(
            request.request_id,
            CapabilityResult::success(request.request_id, json!({"answer":"committed"})),
        );
        Self {
            events,
            runner,
            run_id,
            turn_request_id,
            request,
            permit,
            result,
        }
    }

    fn core(&self) -> Arc<ControlPlane> {
        plane(self.events.clone(), self.runner.clone())
    }

    async fn prepare(&self, atomic: bool) {
        let command_id =
            derived_request_id("execution.prepare", &self.request.request_id.to_string());
        let invocation = typed_invocation_identity(
            Some(self.run_id),
            self.permit.turn_id,
            self.permit.invocation_id,
            self.permit.execution_id,
            &self.request,
            1,
        )
        .unwrap();
        let event = RuntimeEvent::new(
            command_id,
            1,
            "execution.prepared",
            json!({"run_id":self.run_id,"turn_id":self.permit.turn_id,
                "execution_id":self.permit.execution_id,"invocation_id":self.permit.invocation_id,
                "capability_request_id":self.request.request_id,"action_digest":self.permit.action_digest,
                "attempt":1,"permit":self.permit,"invocation":invocation}),
        )
        .unwrap()
        .with_stream_metadata("execution_permit", self.permit.execution_id.to_string(), 1);
        if atomic {
            assert!(!commit_confirmed(self.events.as_ref(), TransitionBatch {
                command_id,
                command_digest: json_digest(&json!({"context":self.permit.context,"run_id":self.run_id,"request":self.request})),
                expected_versions: vec![
                    AggregateVersion::new("execution_permit", self.permit.execution_id.to_string(), 0),
                    AggregateVersion::new("run", self.run_id.to_string(), 1),
                ],
                events: vec![event],
            }).await.expect("prepare commit"));
        } else {
            self.events
                .append(event)
                .await
                .expect("lookalike preparation");
        }
    }

    fn source(&self) -> RuntimeEvent {
        let receipt = CapabilityResultReceipt::from_result(
            &self.result,
            Some(self.permit.execution_id),
            Some(self.permit.invocation_id),
            1,
            true,
        )
        .unwrap();
        let invocation = typed_invocation_identity(
            Some(self.run_id),
            self.permit.turn_id,
            self.permit.invocation_id,
            self.permit.execution_id,
            &self.request,
            1,
        )
        .unwrap();
        RuntimeEvent::new(
            derived_request_id("execution.result", &self.permit.execution_id.to_string()),
            1,
            "execution.result_committed",
            committed_result_payload(
                &self.permit,
                &self.result,
                &receipt,
                invocation,
                false,
                true,
            ),
        )
        .unwrap()
        .with_stream_metadata("execution_permit", self.permit.execution_id.to_string(), 2)
    }

    async fn commit_source(&self, source: RuntimeEvent) {
        assert!(!commit_confirmed(
            self.events.as_ref(),
            TransitionBatch {
                command_id: derived_request_id(
                    "execution.result",
                    &self.permit.execution_id.to_string()
                ),
                command_digest: json_digest(&json!(self.result)),
                expected_versions: vec![AggregateVersion::new(
                    "execution_permit",
                    self.permit.execution_id.to_string(),
                    1,
                )],
                events: vec![source],
            }
        )
        .await
        .expect("result commit"));
    }

    async fn no_delivery(&self) {
        assert!(self.runner.calls.lock().unwrap().is_empty());
        assert!(self
            .events
            .read_stream("result_delivery", &self.request.request_id.to_string())
            .await
            .unwrap()
            .is_empty());
        assert!(self
            .events
            .read_command(&derived_request_id(
                "result.deliver",
                &self.request.request_id.to_string()
            ))
            .await
            .unwrap()
            .is_none());
    }
}

/// 【测试】缺少执行事实时，投递拒绝且不调用 Runner
///
/// 【场景】
/// 派发流程有三个关键事件：
/// 1. run.prompt（回合开始）
/// 2. execution.prepared（许可发行）
/// 3. execution.result_committed（结果提交）
///
/// 这个测试验证：在缺少 (2) 或缺少 (3)、或只用 append 而不用 CAS 提交时，
/// deliver_capability_result() 必须拒绝，不向 Runner 报告。
///
/// 【为什么这很重要】
/// 如果派发层只看「事件日志中是否存在」而不检查「是否通过 CAS 提交」，
/// 就会接受伪造的事件。这违反了 fail-closed 原则。
#[tokio::test]
async fn missing_or_append_only_execution_facts_never_claim_or_call_runner() {
    for (prepare, result) in [
        (None, false),
        (Some(true), false),
        (Some(false), true),
        (Some(true), true),
    ] {
        let fixture = Fixture::new().await;
        if let Some(atomic) = prepare {
            fixture.prepare(atomic).await;
        }
        if result {
            fixture
                .events
                .append(fixture.source())
                .await
                .expect("lookalike result");
        }
        assert!(fixture
            .core()
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .is_err());
        fixture.no_delivery().await;
    }
}

/// 【测试】delivery-before/after-commit 竞态：提交前拒绝，提交后恰好一次，重试幂等
///
/// 【时间线】
/// 1. `execution.prepared` 已提交，但 `execution.result_committed` 尚未提交：投递必须以
///    稳定码 `result_unknown:result_delivery_source_missing` 拒绝，且零 claim、零 Runner 调用。
/// 2. 提交后传入被改写的 result：以 `result_unknown:result_delivery_source_mismatch` 拒绝，
///    同样不写 claim、不调用 Runner。
/// 3. 提交 `execution.result_committed` 且传入结果一致：claim 恰好一次，绑定从 EventLog
///    读回的 committed fact 的 receipt digest 与事件身份，Runner 只收到一次同一结果。
/// 4. 重复投递同一结果：返回 `result_unknown:result_delivery_already_claimed`，不新增 claim，
///    也不再次推进 Runner。
///
/// 【为什么这很重要】
/// claim 只能发生在从 EventLog 读回 committed fact/receipt 之后；提交前抢先 claim 会在恢复
/// 路径留下没有结果事实的投递记录。两个方向的竞态都必须 fail-closed，且重放必须幂等。
#[tokio::test]
async fn delivery_before_commit_is_denied_then_claims_once_after_commit() {
    let fixture = Fixture::new().await;
    fixture.prepare(true).await;
    let core = fixture.core();

    // committed fact 缺失：在稳定码上拒绝，且不写 claim、不调用 Runner。
    assert_eq!(
        core.deliver_capability_result(fixture.run_id, fixture.result.clone()).await.unwrap_err(),
        dispatch_error("result_unknown:result_delivery_source_missing")
    );
    fixture.no_delivery().await;

    // 提交 committed fact；随后只有与它完全一致的 result 才允许 claim，claim 绑定其
    // receipt digest 与事件身份。
    let source = fixture.source();
    let committed_receipt_digest = source.data["result_receipt"]["receipt_digest"].clone();
    fixture.commit_source(source.clone()).await;

    // committed fact 与传入结果不匹配：同样在稳定码上拒绝，且不写 claim。
    let mut altered = fixture.result.clone();
    altered.output["answer"] = json!("uncommitted caller value");
    assert_eq!(
        core.deliver_capability_result(fixture.run_id, altered).await.unwrap_err(),
        dispatch_error("result_unknown:result_delivery_source_mismatch")
    );
    fixture.no_delivery().await;

    core.deliver_capability_result(fixture.run_id, fixture.result.clone())
        .await
        .expect("deliver after commit");
    assert_eq!(
        *fixture.runner.calls.lock().unwrap(),
        vec![(fixture.run_id, fixture.result.clone())]
    );
    let claims = fixture
        .events
        .read_stream("result_delivery", &fixture.request.request_id.to_string())
        .await
        .unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].causation_event_id, Some(source.event_id));
    assert_eq!(claims[0].data["receipt_digest"], committed_receipt_digest);
    assert_eq!(claims[0].data["result_digest"], json!(json_digest(&json!(fixture.result))));

    // 重试幂等：已有 claim 不新增，Runner 不再被推进。
    assert_eq!(
        core.deliver_capability_result(fixture.run_id, fixture.result.clone()).await.unwrap_err(),
        dispatch_error("result_unknown:result_delivery_already_claimed")
    );
    assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    assert_eq!(
        fixture
            .events
            .read_stream("result_delivery", &fixture.request.request_id.to_string())
            .await
            .unwrap()
            .len(),
        1
    );
}

/// 【测试】prepared 之后的 Broker 前失败必须写入唯一 Unknown 终态
///
/// 数据撤销、checkpoint 准备和取消检查都发生在 `execution.prepared` 提交之后。
/// 这个 fixture 直接调用共享收口 helper，验证它使用同一 execution permit stream 的 CAS
/// 版本，并且重复恢复不会追加第二条 terminal fact。
#[tokio::test]
async fn prepared_pre_broker_failure_is_closed_once_as_result_unknown() {
    for (reason, stop_requested) in [
        (
            "result_unknown:prepared_data_revocation_failed:fixture",
            false,
        ),
        ("result_unknown:prepared_checkpoint_failed:fixture", false),
        ("result_unknown:prepared_cancelled_before_broker", true),
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        let core = fixture.core();
        core.commit_prepared_result_unknown(
            &fixture.permit,
            &fixture.request,
            reason,
            stop_requested,
        )
        .await
        .expect("prepared terminal commit");

        let stream = fixture
            .events
            .read_stream("execution_permit", &fixture.permit.execution_id.to_string())
            .await
            .expect("execution stream");
        assert_eq!(
            stream
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            ["execution.prepared", "execution.result_committed"]
        );
        assert_eq!(stream[1].stream_version, Some(2));
        let receipt = CapabilityResultReceipt::from_json(&stream[1].data["result_receipt"])
            .expect("typed result receipt");
        assert!(!receipt.effect_known);
        assert!(receipt.fenced);
        assert_eq!(
            stream[1].data["stop_requested"],
            serde_json::json!(stop_requested)
        );
        if stop_requested {
            assert_eq!(
                stream[1].data["stop_state"],
                serde_json::json!("unconfirmed")
            );
            assert_eq!(stream[1].data["stop_confirmed"], serde_json::json!(false));
        }

        core.commit_prepared_result_unknown(
            &fixture.permit,
            &fixture.request,
            reason,
            stop_requested,
        )
        .await
        .expect("idempotent prepared terminal recovery");
        assert_eq!(
            fixture
                .events
                .read_stream("execution_permit", &fixture.permit.execution_id.to_string())
                .await
                .expect("execution stream after replay")
                .len(),
            2
        );
    }
}

/// 【测试】缺少 stream_version 的结果，投递拒绝
///
/// 【原因】
/// execution_permit 流中的每个事件都应该有 stream_version（CAS 提交的证据）。
/// 如果某个事件缺少这个标记，说明它是用 append() 伪造的，而不是通过
/// CAS 原子化地记录的。派发必须拒绝这样的「半可信」数据。
#[tokio::test]
async fn missing_committed_source_stream_version_is_denied_without_delivery() {
    let fixture = Fixture::new().await;
    fixture.prepare(true).await;
    fixture.commit_source(fixture.source()).await;
    assert_eq!(
        fault_core(&fixture, Fault::MissingSourceVersion)
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .unwrap_err(),
        dispatch_error("result_unknown:result_delivery_source_version_missing")
    );
    fixture.no_delivery().await;
}

/// 【测试】结果或收据被篡改时，投递拒绝
///
/// 【参数化】
/// 对以下多个篡改场景逐一验证：
/// - "run", "turn", "request", "execution", "invocation"：身份字段被改
/// - "attempt"：重试次数被改（会影响版本检查）
/// - "result"：执行结果被改（会影响摘要）
/// - "outcome"、"not_ready"、"effect"：执行状态标记被改
/// - "receipt_digest"：收据摘要被改
/// - "receipt_result"：收据对应的结果被改（不一致）
/// - "uncommitted_receipt"：收据的 committed 标记被改
/// - "invocation_identity"：调用身份被改
///
/// 【验证】
/// 每个篡改都应该导致投递拒绝，且不调用 Runner。
/// 这验证了派发层对每个字段都有检查。
#[tokio::test]
async fn contradictory_committed_payloads_or_tampered_receipts_never_deliver() {
    for drift in [
        "run",
        "turn",
        "request",
        "execution",
        "invocation",
        "attempt",
        "result",
        "outcome",
        "not_ready",
        "effect",
        "receipt_digest",
        "receipt_result",
        "uncommitted_receipt",
        "invocation_identity",
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        let mut source = fixture.source();
        match drift {
            "run" => source.data["run_id"] = json!(RunId::new()),
            "turn" => source.data["turn_id"] = json!(TurnId::new()),
            "request" => source.data["capability_request_id"] = json!(RequestId::new()),
            "execution" => source.data["execution_id"] = json!(ExecutionId::new()),
            "invocation" => source.data["invocation_id"] = json!(InvocationId::new()),
            "attempt" => source.data["attempt"] = json!(2),
            "result" => source.data["result"]["output"]["answer"] = json!("altered"),
            "outcome" => source.data["outcome_state"] = json!(CapabilityExecutionState::Unknown),
            "not_ready" => source.data["outcome_ready"] = json!(false),
            "effect" => source.data["effect_started"] = json!(false),
            "invocation_identity" => {
                let mut identity: kiana_domain::InvocationIdentity =
                    serde_json::from_value(source.data["invocation"].clone()).unwrap();
                identity.turn_id = TurnId::new();
                identity.identity_digest = identity.digest();
                source.data["invocation"] = json!(identity);
            }
            "receipt_digest" => {
                source.data["result_receipt"]["receipt_digest"] =
                    json!(json_digest(&json!("tampered")))
            }
            "receipt_result" => {
                let mut different = fixture.result.clone();
                different.output["answer"] = json!("altered");
                source.data["result_receipt"] = json!(CapabilityResultReceipt::from_result(
                    &different,
                    Some(fixture.permit.execution_id),
                    Some(fixture.permit.invocation_id),
                    1,
                    true,
                )
                .unwrap());
            }
            "uncommitted_receipt" => {
                let mut receipt =
                    CapabilityResultReceipt::from_json(&source.data["result_receipt"]).unwrap();
                receipt.committed = false;
                receipt.receipt_digest = receipt.digest();
                source.data["result_receipt"] = json!(receipt);
            }
            _ => unreachable!(),
        }
        fixture.commit_source(source).await;
        assert!(
            fixture
                .core()
                .deliver_capability_result(fixture.run_id, fixture.result.clone())
                .await
                .is_err(),
            "accepted {drift}"
        );
        fixture.no_delivery().await;
    }
}

/// 【测试】execution.result_committed 的停止元数据被篡改时，投递必须进入 ResultUnknown。
///
/// 这组篡改独立于一般结果/收据篡改 fixture：收据本身保持有效，只修改顶层的停止生命周期
/// 字段，确保 `committed_delivery_source` 不会把这些字段当作可独立信任的事实来源。
#[tokio::test]
async fn tampered_committed_stop_metadata_is_result_unknown_without_delivery() {
    for (field, value) in [
        ("stop_requested", json!(true)),
        ("stop_state", json!("confirmed")),
        ("stop_confirmed", json!(true)),
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        let mut source = fixture.source();
        source.data[field] = value;
        fixture.commit_source(source).await;

        assert_eq!(
            fixture
                .core()
                .deliver_capability_result(fixture.run_id, fixture.result.clone())
                .await
                .unwrap_err(),
            dispatch_error("result_unknown:result_delivery_stop_mismatch"),
            "accepted tampered {field}"
        );
        fixture.no_delivery().await;
    }
}

/// 【测试】结果被改、重复完成、老 turn 时，投递拒绝
///
/// 【场景分类】
/// - "result"：结果被改（output 字段被篡改）
/// - "duplicate"：result_committed 事件被复制（同一个流中出现两个）
/// - "old_turn"：Run 发生了 LegacyContinue（老风格继续），新 prompt 替换了旧的
/// - "wrong_run"：结果来自完全不同的 Run
///
/// 【为什么这些都应该失败】
/// - 结果被改：digest 不匹配，CAS 失败
/// - 重复完成：流中已有一个 result_committed，第二个表示重放或冲突
/// - 老 turn：Run 的拓扑已改变，原 permit 不再有效
/// - 错误的 Run：结果完全来自另一个 Run，身份不符
#[tokio::test]
async fn changed_result_duplicate_completion_and_old_turn_cannot_advance() {
    for drift in ["result", "duplicate", "old_turn", "wrong_run"] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        let source = fixture.source();
        fixture.commit_source(source.clone()).await;
        let mut result = fixture.result.clone();
        let mut run_id = fixture.run_id;
        match drift {
            "result" => result.output["answer"] = json!("uncommitted caller value"),
            "wrong_run" => {
                run_id = RunId::new();
                fixture
                    .events
                    .append(scoped_prompt(run_id, 1, TurnSemantics::Start))
                    .await
                    .unwrap();
            }
            "duplicate" => {
                let mut duplicate = source;
                duplicate.event_id = EventId::new();
                duplicate.sequence = 2;
                duplicate.stream_version = Some(3);
                fixture.events.append(duplicate).await.unwrap();
            }
            "old_turn" => {
                fixture
                    .events
                    .append(scoped_prompt(run_id, 2, TurnSemantics::LegacyContinue))
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            fixture
                .core()
                .deliver_capability_result(run_id, result)
                .await
                .is_err(),
            "accepted {drift}"
        );
        fixture.no_delivery().await;
    }
}

/// 【测试】Run 处于终止或取消状态时，投递拒绝
///
/// 【状态列表】
/// - run.cancelling：取消中（还未确认）
/// - run.cancelled：已取消
/// - run.result_unknown：结果未知（内部错误）
/// - run.failed：运行失败
/// - run.completed：运行完成
///
/// 【核心规则】
/// 任何这些状态都表示「这个 Run 已经不再活跃」。
/// 派发不应该尝试在一个非活跃的 Run 中投递结果。
/// 这确保了「一个已完成的 Run 不会再被修改」。
#[tokio::test]
async fn current_terminal_or_cancelling_run_never_claims_result() {
    for kind in [
        "run.cancelling",
        "run.cancelled",
        "run.result_unknown",
        "run.failed",
        "run.completed",
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        fixture
            .events
            .append(
                RuntimeEvent::new(
                    fixture.turn_request_id,
                    2,
                    kind,
                    json!({"run_id":fixture.run_id}),
                )
                .unwrap()
                .with_stream_metadata("run", fixture.run_id.to_string(), 2),
            )
            .await
            .unwrap();
        assert_eq!(
            fixture
                .core()
                .deliver_capability_result(fixture.run_id, fixture.result.clone())
                .await
                .unwrap_err(),
            dispatch_error("cancelled:result_delivery_run_inactive")
        );
        fixture.no_delivery().await;
    }
}

/// 【测试】投递使用声称的类型化 Turn，拒绝身份缺失或冲突
///
/// 【第一部分】身份验证
/// 对以下缺失或冲突的场景逐一验证拒绝：
/// - "missing_id"：turn_id 字段缺失
/// - "missing_identity"：turn 对象缺失
/// - "run"、"turn"、"session"、"digest"：身份字段不匹配或被篡改
///
/// 【第二部分】Turn 语义
/// 验证所有四种 Turn 语义（Start、NewTurn、LegacyContinue、Resume）
/// 都能通过投递。关键在于 permit 中声称的 turn_id 必须是当前 prompt
/// 事件中的 turn_id（而不是 request_id）。
///
/// 【为什么区分 Turn 语义】
/// - Start：第一个回合
/// - NewTurn：同 Run 内的新回合（可能来自不同 session）
/// - LegacyContinue：兼容模式，Run 拓扑改变但继续
/// - Resume：从检查点恢复
/// 派发必须通过 turn_id 正确地识别当前回合，而不能混淆。
#[tokio::test]
async fn delivery_uses_declared_typed_turn_and_rejects_missing_or_conflicting_identity() {
    for drift in [
        "missing_id",
        "missing_identity",
        "run",
        "turn",
        "session",
        "digest",
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        let mut prompt = fixture
            .events
            .read_stream("run", &fixture.run_id.to_string())
            .await
            .unwrap()[0]
            .clone();
        prompt.event_id = EventId::new();
        prompt.sequence = 2;
        prompt.stream_version = Some(2);
        let mut identity: TurnIdentity =
            serde_json::from_value(prompt.data["turn"].clone()).unwrap();
        match drift {
            "missing_id" => {
                prompt.data.as_object_mut().unwrap().remove("turn_id");
            }
            "missing_identity" => {
                prompt.data.as_object_mut().unwrap().remove("turn");
            }
            "run" => {
                identity.run_id = RunId::new();
                identity.identity_digest = identity.digest();
                prompt.data["turn"] = json!(identity);
            }
            "turn" => {
                identity.turn_id = TurnId::new();
                identity.identity_digest = identity.digest();
                prompt.data["turn"] = json!(identity);
            }
            "session" => prompt.data["session_id"] = json!("other-session"),
            "digest" => {
                prompt.data["turn"]["identity_digest"] = json!(json_digest(&json!("tampered")))
            }
            _ => unreachable!(),
        }
        fixture.events.append(prompt).await.unwrap();
        assert_eq!(
            fixture
                .core()
                .deliver_capability_result(fixture.run_id, fixture.result.clone())
                .await
                .unwrap_err(),
            dispatch_error("result_unknown:result_delivery_turn_invalid"),
            "accepted {drift}"
        );
        fixture.no_delivery().await;
    }
    for semantics in [
        TurnSemantics::Start,
        TurnSemantics::NewTurn,
        TurnSemantics::LegacyContinue,
        TurnSemantics::Resume,
    ] {
        let fixture = Fixture::with_turn_semantics(semantics).await;
        assert_ne!(
            fixture.permit.turn_id.unwrap().as_uuid(),
            fixture.turn_request_id.as_uuid()
        );
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        fixture
            .core()
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .expect("typed turn delivery");
        assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    }
}

/// 【文档】故障模式的枚举。
///
/// 用于 FaultStore，在特定时刻注入故障，模拟各种失败场景：
/// - Cancel、Terminal、NextTurn：Run 状态变化的竞态
/// - SourceAdvance：执行 permit 流被推进（并发）
/// - UnknownClaim、StoredUnknownClaim：日志返回 Unknown（无法确认）
/// - ReceiptEvent/Digest/Version/Cursor：CAS 回执的字段被破坏
/// - MissingSourceVersion：stream_version 被移除
#[derive(Clone, Copy)]
enum Fault {
    Cancel,
    Terminal,
    NextTurn,
    SourceAdvance,
    UnknownClaim,
    StoredUnknownClaim,
    ReceiptEvent,
    ReceiptDigest,
    ReceiptVersion,
    ReceiptCursor,
    MissingSourceVersion,
}

/// FaultStore：装饰器，在特定时刻向事件流注入故障。
///
/// 这个 Store 包装了真实的 MemoryEventLog，并在 commit_transition() 时，
/// 在第一次提交 result.delivery_claimed 时触发故障注入。
///
/// 【为什么需要这个】
/// 派发层在提交 result.delivery_claimed 之后才会调用 Runner。
/// 我们需要在提交之间注入竞态条件或其他故障，以验证派发层的鲁棒性。
///
/// 【使用方式】
/// 1. 选择一个 Fault 类型
/// 2. 创建 FaultStore(inner, fault, ...)
/// 3. 在第一次提交 result.delivery_claimed 时，注入故障
/// 4. 检查派发是否正确处理了这个故障
struct FaultStore {
    inner: Arc<MemoryEventLog>,
    fault: Fault,
    armed: AtomicBool,
    run_id: RunId,
    execution_id: ExecutionId,
}

#[async_trait::async_trait]
impl EventStorePort for FaultStore {
    fn supports_atomic_transitions(&self) -> bool {
        true
    }

    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }
    async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(id).await
    }
    async fn read_stream(&self, kind: &str, id: &str) -> Result<Vec<RuntimeEvent>, PortError> {
        let mut events = self.inner.read_stream(kind, id).await?;
        if matches!(self.fault, Fault::MissingSourceVersion)
            && kind == "execution_permit"
            && id == self.execution_id.to_string()
        {
            if let Some(event) = events
                .iter_mut()
                .find(|event| event.kind == "execution.result_committed")
            {
                event.stream_version = None;
            }
        }
        Ok(events)
    }

    async fn read_command(&self, id: &RequestId) -> Result<Option<CommandReceipt>, PortError> {
        let mut receipt = self.inner.read_command(id).await?;
        if *id == derived_request_id("execution.result", &self.execution_id.to_string()) {
            if let Some(receipt) = &mut receipt {
                match self.fault {
                    Fault::ReceiptEvent => receipt.event_ids = vec![EventId::new()],
                    Fault::ReceiptDigest => receipt.command_digest = "a".repeat(64),
                    Fault::ReceiptVersion => receipt.versions[0].version += 1,
                    Fault::ReceiptCursor => receipt.first_cursor = 0,
                    _ => {}
                }
            }
        }
        Ok(receipt)
    }

    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        if batch
            .events
            .iter()
            .any(|event| event.kind == "result.delivery_claimed")
            && !self.armed.swap(true, Ordering::SeqCst)
        {
            match self.fault {
                Fault::Cancel | Fault::Terminal | Fault::NextTurn => {
                    let event = match self.fault {
                        Fault::NextTurn => {
                            scoped_prompt(self.run_id, 2, TurnSemantics::LegacyContinue)
                        }
                        _ => RuntimeEvent::new(
                            RequestId::new(),
                            1,
                            if matches!(self.fault, Fault::Cancel) {
                                "run.cancelling"
                            } else {
                                "run.completed"
                            },
                            json!({"run_id":self.run_id}),
                        )
                        .unwrap()
                        .with_stream_metadata(
                            "run",
                            self.run_id.to_string(),
                            2,
                        ),
                    };
                    self.inner.append(event).await?;
                }
                Fault::SourceAdvance => {
                    self.inner
                        .append(
                            RuntimeEvent::new(
                                RequestId::new(),
                                1,
                                "diagnostic.source_changed",
                                json!({}),
                            )
                            .unwrap()
                            .with_stream_metadata(
                                "execution_permit",
                                self.execution_id.to_string(),
                                3,
                            ),
                        )
                        .await?;
                }
                Fault::UnknownClaim | Fault::StoredUnknownClaim => {
                    let command_id = batch.command_id;
                    if matches!(self.fault, Fault::StoredUnknownClaim) {
                        self.inner.commit_transition(batch).await?;
                    }
                    return Ok(CommitOutcome::Unknown {
                        command_id,
                        reason: "fixture_unconfirmed_claim".to_owned(),
                    });
                }
                _ => {}
            }
        }
        self.inner.commit_transition(batch).await
    }
}

fn fault_core(fixture: &Fixture, fault: Fault) -> Arc<ControlPlane> {
    plane(
        Arc::new(FaultStore {
            inner: fixture.events.clone(),
            fault,
            armed: AtomicBool::new(false),
            run_id: fixture.run_id,
            execution_id: fixture.permit.execution_id,
        }),
        fixture.runner.clone(),
    )
}

/// 【测试】CAS 竞态或日志异常时，投递为零（无 Runner 调用）
///
/// 【故障列表】
/// - Cancel、Terminal、NextTurn：Run 状态在派发期间变化
/// - SourceAdvance：execution_permit 流被推进（新事件）
/// - UnknownClaim：日志无法确认主张是否成功
/// - ReceiptEvent/Digest/Version/Cursor：CAS 回执被破坏
/// - MissingSourceVersion：stream_version 被移除
///
/// 【期望】
/// 所有这些故障都应该导致派发拒绝，且不调用 Runner。
/// 这验证了派发的「fail-closed」原则：
/// 不能确认的事情，就不做。
#[tokio::test]
async fn source_and_run_cas_races_or_unconfirmed_receipts_have_zero_delivery() {
    for fault in [
        Fault::Cancel,
        Fault::Terminal,
        Fault::NextTurn,
        Fault::SourceAdvance,
        Fault::UnknownClaim,
        Fault::ReceiptEvent,
        Fault::ReceiptDigest,
        Fault::ReceiptVersion,
        Fault::ReceiptCursor,
        Fault::MissingSourceVersion,
    ] {
        let fixture = Fixture::new().await;
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        assert!(fault_core(&fixture, fault)
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .is_err());
        fixture.no_delivery().await;
    }
}

/// 【测试】已提交的来源收据被主张，然后恰好一次传给 Runner
///
/// 【场景】
/// 测试成功和失败两种结果：
/// - 成功的执行：Runner 收到成功结果，effect_started = true
/// - 失败的执行：Runner 收到失败结果，effect_started = false（zero_effect）
///
/// 【验证项】
/// 1. 派发主张了投递权（result.delivery_claimed 事件）
/// 2. 主张的收据摘要与来源事件的收据摘要一致
/// 3. Runner 恰好被调用一次
/// 4. 第二次尝试投递同一个结果时失败（已主张过）
/// 5. 命令回执包含了所有权限版本
///
/// 【为什么这很重要】
/// 这验证了投递的幂等性和原子性：
/// - 幂等性：同一个结果投递多次，只有第一次成功
/// - 原子性：主张和 Runner 调用是原子化的（主张成功意味着 Runner 会被调用）
#[tokio::test]
async fn committed_source_receipt_is_claimed_before_exact_result_reaches_runner_once() {
    for failed in [false, true] {
        let mut fixture = Fixture::new().await;
        if failed {
            let mut result =
                CapabilityResult::failure(fixture.request.request_id, "capability_failed:fixture");
            result.output["not_executed"] = json!(true);
            fixture.result =
                kiana_domain::normalize_capability_result(fixture.request.request_id, result);
        } else {
            fixture.result.output["result_receipt"] = json!({"receipt_digest":"business-output"});
        }
        fixture.prepare(true).await;
        let source = fixture.source();
        assert_eq!(source.data["effect_started"], json!(!failed));
        assert_eq!(source.data["zero_effect"], json!(failed));
        assert_eq!(source.data["effect_known"], json!(true));
        fixture.commit_source(source.clone()).await;
        let core = fixture.core();
        core.deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .expect("deliver");
        assert_eq!(
            *fixture.runner.calls.lock().unwrap(),
            vec![(fixture.run_id, fixture.result.clone())]
        );
        let claims = fixture
            .events
            .read_stream("result_delivery", &fixture.request.request_id.to_string())
            .await
            .unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].causation_event_id, Some(source.event_id));
        assert_eq!(
            claims[0].data["receipt_digest"],
            source.data["result_receipt"]["receipt_digest"]
        );
        assert_ne!(
            claims[0].data["receipt_digest"],
            fixture.result.output["result_receipt"]["receipt_digest"]
        );
        assert_eq!(claims[0].data["turn_id"], source.data["turn_id"]);
        let receipt = fixture
            .events
            .read_command(&derived_request_id(
                "result.deliver",
                &fixture.request.request_id.to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert!(receipt.versions.contains(&AggregateVersion::new(
            "execution_permit",
            fixture.permit.execution_id.to_string(),
            2
        )));
        assert!(core
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .is_err());
        assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    }
}

/// 【测试】并发的多个投递请求中，至多一个成功
///
/// 【设置】
/// 创建 8 个并发任务，都尝试投递同一个结果。
///
/// 【期望】
/// - 恰好 1 个任务成功，其他 7 个因 CAS 冲突而失败
/// - Runner 恰好被调用 1 次
/// - result_delivery 流中恰好有 1 个主张事件
///
/// 【为什么这很重要】
/// 这验证了派发的 CAS 机制在高并发下的正确性。
/// 如果没有正确的 CAS，多个并发请求可能都认为自己成功了，
/// 导致 Runner 被多次调用（重复执行）。
#[tokio::test]
async fn concurrent_claims_advance_runner_at_most_once() {
    let fixture = Fixture::new().await;
    fixture.prepare(true).await;
    fixture.commit_source(fixture.source()).await;
    let core = fixture.core();
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let core = core.clone();
        let barrier = barrier.clone();
        let run_id = fixture.run_id;
        let result = fixture.result.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            core.deliver_capability_result(run_id, result).await
        }));
    }
    let mut successes = 0;
    for task in tasks {
        successes += usize::from(task.await.unwrap().is_ok());
    }
    assert_eq!(successes, 1);
    assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    assert_eq!(
        fixture
            .events
            .read_stream("result_delivery", &fixture.request.request_id.to_string())
            .await
            .unwrap()
            .len(),
        1
    );
}

/// 【测试】Runner 回调失败或日志返回 Unknown 时，不重试
///
/// 【两种故障】
/// 1. callback_failure = true：Runner.send() 返回错误（模拟 Runner 不可用）
/// 2. callback_failure = false：日志返回 Unknown（无法确认是否成功）
///
/// 【验证】
/// 第一次投递返回错误（result_unknown:...）
/// 第二次投递返回「已主张」错误（不重试）
///
/// 这验证了派发的「一次性」语义：
/// - 如果一次调用失败且无法确认，后续调用不会重试
/// - 这避免了无限重试或重复执行
///
/// 【设计哲学】
/// 一旦派发把结果交给 Runner，就不再管 Runner 的生死。
/// 如果 Runner 失败了，应该由上层（如 Harness）来处理重试。
/// 派发的职责是「至多一次」而不是「至少一次」。
#[tokio::test]
async fn failed_callback_or_unknown_claim_ack_is_never_retried() {
    for callback_failure in [false, true] {
        let mut fixture = Fixture::new().await;
        fixture.prepare(true).await;
        fixture.commit_source(fixture.source()).await;
        let core = if callback_failure {
            fixture.runner = Arc::new(CountingRunner {
                events: fixture.events.clone(),
                calls: Mutex::new(Vec::new()),
                fail: true,
            });
            fixture.core()
        } else {
            fault_core(&fixture, Fault::StoredUnknownClaim)
        };
        let first = core
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .unwrap_err();
        assert!(first.to_string().contains("result_unknown:"));
        let replay = core
            .deliver_capability_result(fixture.run_id, fixture.result.clone())
            .await
            .unwrap_err();
        assert!(replay
            .to_string()
            .contains("result_delivery_already_claimed"));
        assert_eq!(
            fixture.runner.calls.lock().unwrap().len(),
            usize::from(callback_failure)
        );
        assert_eq!(
            fixture
                .events
                .read_stream("result_delivery", &fixture.request.request_id.to_string())
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

/// 【测试】赢得投递后，即使 Run 后来被取消，也不重新开放主张
///
/// 【时间线】
/// 1. prepare() 和 commit_source() 成功
/// 2. 第一次 deliver_capability_result() 成功（赢得）
/// 3. 添加 run.cancelling 事件（Run 被取消）
/// 4. 第二次 deliver_capability_result() 失败（已主张过）
///
/// 【验证】
/// - Runner 恰好被调用 1 次（第一次）
/// - result_delivery 流中恰好有 1 个主张事件
/// - 第二次投递因「已主张」而失败，而不是因「Run 取消」
///
/// 【原则】
/// 投递权一旦赢得，就不再因后续 Run 状态变化而失效。
/// 这确保了「已完成的投递是永久性的」。
#[tokio::test]
async fn cancellation_after_a_winning_delivery_does_not_reopen_its_claim() {
    let fixture = Fixture::new().await;
    fixture.prepare(true).await;
    fixture.commit_source(fixture.source()).await;
    let core = fixture.core();
    core.deliver_capability_result(fixture.run_id, fixture.result.clone())
        .await
        .expect("claim wins before cancellation");
    fixture
        .events
        .append(
            RuntimeEvent::new(
                RequestId::new(),
                1,
                "run.cancelling",
                json!({"run_id":fixture.run_id}),
            )
            .unwrap()
            .with_stream_metadata("run", fixture.run_id.to_string(), 2),
        )
        .await
        .unwrap();
    assert!(core
        .deliver_capability_result(fixture.run_id, fixture.result.clone())
        .await
        .is_err());
    assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    assert_eq!(
        fixture
            .events
            .read_stream("result_delivery", &fixture.request.request_id.to_string())
            .await
            .unwrap()
            .len(),
        1
    );
}
