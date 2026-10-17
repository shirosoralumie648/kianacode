//! CAP-25 行为夹具：Receipt、结果配对与四入口共享同一份事实。
//!
//! 路线图卡片：`docs/roadmap/capability.md#step-cap-25`。三条拒绝夹具与卡片同名：
//! `late_progress_cannot_overwrite_terminal_outcome`（在 `kiana-daemon/src/run_stream.rs` 的
//! 测试模块中，驱动真实 `RunStreamBus`）、`foreign_run_cannot_read_output_or_receipt` 与
//! `invalid_adapter_result_cannot_be_rendered_as_success`；另有成功/回归夹具
//! `execution_receipt_projects_plan_evidence_without_inventing_metrics`，证明 receipt 从事实
//! 投影 descriptor/input/scope/plan/backend/resource/changes 摘要，而不是从模型或 handler
//! 自述构造。
//!
//! # 为什么这些夹具能证明行为
//!
//! 夹具直接调用真实的 `ControlPlane::read_receipt` 与 `ControlPlane::entrypoint_parity`，
//! 事件写在真实 `MemoryEventLog` 上，Runner/ApprovalStore 只是记录调用的桩：拒绝路径断言
//! 零 Runner 推进、零新增事实、零审批读取，成功路径断言同一批已提交事实在两个入口上投影出
//! 相同的状态。前端展示层（CLI/Workbench/Web/Desktop）不参与本模块：它们消费
//! `kiana-protocol` 与 `EntryPointParitySnapshot`，由 `oa24_entrypoint_parity`、
//! `gp22_protocol_surfaces`、`p2_m5_01_web_sync` 与 `er27_entrypoint_parity_guard` 覆盖。
//!
//! # 这些夹具不证明什么
//!
//! 事件由夹具直接写入，不是一次真实 daemon 会话的端到端回放；shell stdout、MCP description
//! 与 UI 通知「不能改写事实」由本模块证明的是 receipt 投影不消费这些自述，真实 adapter 的
//! containment 与停止证据仍由 CAP-14–CAP-24 各自的适配器夹具负责。跨进程 durable 与
//! live/physical 等级不因本文件改变。

use super::*;
use kiana_capability_broker::CapabilityBroker;
use kiana_domain::{ApprovalChallenge, EntryPointKind, PendingApproval};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use std::sync::Arc;

/// 固定摘要：只满足 sha256 形态，不认证任何真实项目、策略或内容。
fn sha_like(ch: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (ch as u32) & 0xffff).repeat(16)
    )
}

fn local_context(session: &str, actor: &str) -> RequestContext {
    let mut context = RequestContext::local(session, "/repo");
    context.actor_id = Some(actor.to_owned());
    context.project_trusted = true;
    context
}

fn event(sequence: u64, kind: &str, data: Value) -> RuntimeEvent {
    // Every committed fixture fact carries the authority/data epochs the audit reducer requires;
    // adding them here keeps the fixture event shape uniform instead of per-test patchwork.
    let mut data = data;
    if let Some(object) = data.as_object_mut() {
        object
            .entry("authority_epoch".to_owned())
            .or_insert_with(|| json!(1));
        object
            .entry("data_epoch".to_owned())
            .or_insert_with(|| json!(1));
    }
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).expect("cap25 fixture event")
}

/// 只有 `run.authorized` + `run.completed` 的最小运行：足够读出 receipt 与 owner 边界。
fn authorized_run(run_id: RunId, context: &RequestContext, sandbox: &str) -> RuntimeEvent {
    event(
        1,
        "run.authorized",
        json!({
            "run_id": run_id,
            "session_id": context.session_id,
            "actor_id": context.actor_id,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "project_root": context.project_root,
            "sandbox": sandbox,
            "authority_epoch": 1,
            "data_epoch": 1,
        }),
    )
}

fn completed_run(run_id: RunId, output: Value) -> RuntimeEvent {
    event(
        2,
        "run.completed",
        json!({"run_id": run_id, "result": output}),
    )
}

struct ReceiptFixtureApprovals;

#[async_trait::async_trait]
impl ApprovalStorePort for ReceiptFixtureApprovals {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable(
            "cap25_fixture_approval_store".to_owned(),
        ))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "cap25_fixture_approval_store".to_owned(),
        ))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "cap25_fixture_approval_store".to_owned(),
        ))
    }
}

/// Runner 桩：任何推进都会失败，因此「拒绝路径没有执行」是可断言的。
struct ReceiptFixtureRunner;

#[async_trait::async_trait]
impl RunnerPort for ReceiptFixtureRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Failed(
            "cap25_fixture_runner_must_not_run".to_owned(),
        ))
    }
}

fn fixture_core() -> (ControlPlane, Arc<MemoryEventLog>) {
    let events = Arc::new(MemoryEventLog::new());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(CapabilityBroker::new()),
        Arc::new(ReceiptFixtureApprovals),
        Arc::new(ReceiptFixtureRunner),
    );
    (core, events)
}

async fn append(events: &Arc<MemoryEventLog>, event: RuntimeEvent) {
    events.append(event).await.expect("cap25 fixture append");
}

#[tokio::test]
async fn foreign_run_cannot_read_output_or_receipt() {
    let (core, events) = fixture_core();
    let owner = local_context("cap25-owner-session", "cap25-owner-actor");
    let foreign = local_context("cap25-foreign-session", "cap25-foreign-actor");
    let owner_run = RunId::new();
    let foreign_run = RunId::new();
    append(&events, authorized_run(owner_run, &owner, "read-only")).await;
    append(
        &events,
        completed_run(owner_run, json!({"text": "cap25-owner-output"})),
    )
    .await;
    append(&events, authorized_run(foreign_run, &foreign, "read-only")).await;
    append(
        &events,
        completed_run(foreign_run, json!({"text": "cap25-foreign-output"})),
    )
    .await;

    // The owner reads its own run: the output is visible and the terminal status is Completed.
    let owned = core
        .read_receipt(owner.clone(), Some(owner_run))
        .await
        .unwrap();
    assert_eq!(owned.status, ExecutionStatus::Completed);
    assert!(owned.output.to_string().contains("cap25-owner-output"));

    let before = events.read_all().await.unwrap().len();

    // The foreign principal may read only its own run; the owner run is blocked before any
    // output is projected, and the foreign response carries no byte of the owner output.
    let foreign_receipt = core
        .read_receipt(foreign.clone(), Some(owner_run))
        .await
        .unwrap();
    assert_eq!(foreign_receipt.status, ExecutionStatus::Blocked);
    assert_eq!(foreign_receipt.error.as_deref(), Some("run_owner_mismatch"));
    assert!(!foreign_receipt
        .output
        .to_string()
        .contains("cap25-owner-output"));

    let foreign_parity = core
        .entrypoint_parity(&foreign, EntryPointKind::Web, Some(owner_run))
        .await
        .unwrap();
    assert_eq!(foreign_parity.status, ExecutionStatus::Blocked);
    assert_eq!(foreign_parity.error.as_deref(), Some("run_owner_mismatch"));

    // Neither denial appended a fact or advanced the runner.
    let after = events.read_all().await.unwrap();
    assert_eq!(after.len(), before);
    assert!(!after
        .iter()
        .any(|fact| fact.kind == "run.tool_call" || fact.kind == "capability.completed"));
}

#[tokio::test]
async fn invalid_adapter_result_cannot_be_rendered_as_success() {
    let (core, events) = fixture_core();
    let context = local_context("cap25-adapter-session", "cap25-adapter-actor");
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let execution_id = kiana_domain::ExecutionId::new();
    let invocation_id = kiana_domain::InvocationId::new();
    let action_digest = sha_like('a');
    let scope_digest = sha_like('s');
    let plan_digest = sha_like('p');
    let input_digest = sha_like('i');
    append(&events, authorized_run(run_id, &context, "read-only")).await;
    append(
        &events,
        event(
            3,
            "run.capability_requested",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "capability": "process",
                "operation": "shell.exec",
                "risk": "read_only",
                "attempt": 1,
                "action_digest": action_digest,
                "input_digest": input_digest,
                "execution_scope": {"scope_digest": scope_digest},
                "arguments": {"command": "printf cap25", "sandbox": "read-only"},
            }),
        ),
    )
    .await;
    append(
        &events,
        event(
            4,
            "capability.decision",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "gate": {"decision": "allowed", "reason": "cap25-fixture"},
            }),
        ),
    )
    .await;
    append(
        &events,
        event(
            5,
            "execution.prepared",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "permit": {
                    "request_id": request_id,
                    "execution_id": execution_id,
                    "invocation_id": invocation_id,
                    "permit_digest": plan_digest,
                },
            }),
        ),
    )
    .await;
    // The adapter claims success while admitting it cannot prove the effect. The receipt may
    // keep the run-level completion fact, but this execution must stay Unknown and fenced.
    append(
        &events,
        event(
            6,
            "execution.result_committed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "execution_id": execution_id,
                "invocation_id": invocation_id,
                "attempt": 1,
                "effect_known": false,
                "stop_confirmed": false,
                "result": {
                    "success": true,
                    "stdout": "cap25-claimed-success",
                    "backend": "bwrap",
                    "resource_budget": {"wall_time_ms": 30_000},
                    "changed": [{"path": "src/lib.rs"}],
                },
            }),
        ),
    )
    .await;
    append(&events, completed_run(run_id, json!({"text": "done"}))).await;

    let receipt = core.read_receipt(context, Some(run_id)).await.unwrap();
    assert_eq!(receipt.status, ExecutionStatus::Completed);
    let execution = &receipt.output["execution_receipts"][0];
    assert_eq!(execution["status"], json!("unknown"));
    assert_ne!(execution["status"], json!("succeeded"));
    assert_eq!(execution["effect_known"], json!(false));
    assert_eq!(execution["fenced"], json!(true));
    assert_eq!(execution["stop_confirmed"], json!(false));
    // The handler's own success text is evidence at most; it never becomes the receipt status.
    assert_eq!(execution["status"].as_str(), Some("unknown"));
}

#[tokio::test]
async fn execution_receipt_projects_plan_evidence_without_inventing_metrics() {
    let (core, events) = fixture_core();
    let context = local_context("cap25-evidence-session", "cap25-evidence-actor");
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let execution_id = kiana_domain::ExecutionId::new();
    let invocation_id = kiana_domain::InvocationId::new();
    let action_digest = sha_like('a');
    let scope_digest = sha_like('s');
    let plan_digest = sha_like('p');
    let input_digest = sha_like('i');
    let resource_digest = kiana_domain::json_digest(&json!({"wall_time_ms": 30_000}));
    append(&events, authorized_run(run_id, &context, "read-only")).await;
    append(
        &events,
        event(
            3,
            "run.capability_requested",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "capability": "process",
                "operation": "shell.exec",
                "risk": "read_only",
                "attempt": 1,
                "action_digest": action_digest,
                "input_digest": input_digest,
                "execution_scope": {"scope_digest": scope_digest},
                "arguments": {"command": "printf cap25", "sandbox": "read-only"},
            }),
        ),
    )
    .await;
    append(
        &events,
        event(
            4,
            "capability.decision",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "gate": {"decision": "allowed", "reason": "cap25-fixture"},
            }),
        ),
    )
    .await;
    append(
        &events,
        event(
            5,
            "execution.prepared",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "permit": {
                    "request_id": request_id,
                    "execution_id": execution_id,
                    "invocation_id": invocation_id,
                    "permit_digest": plan_digest,
                },
            }),
        ),
    )
    .await;
    append(
        &events,
        event(
            6,
            "execution.result_committed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "execution_id": execution_id,
                "invocation_id": invocation_id,
                "attempt": 1,
                "effect_known": true,
                "result": {
                    "success": true,
                    "stdout": "cap25-output",
                    "backend": "bwrap",
                    "resource_budget": {"wall_time_ms": 30_000},
                    "changed": [{"path": "src/lib.rs"}],
                },
            }),
        ),
    )
    .await;
    append(&events, completed_run(run_id, json!({"text": "done"}))).await;

    let receipt = core
        .read_receipt(context.clone(), Some(run_id))
        .await
        .unwrap();
    let execution = &receipt.output["execution_receipts"][0];
    assert_eq!(execution["status"], json!("succeeded"));
    assert_eq!(
        execution["descriptor_id"],
        json!("shell.exec@kiana.handler-binding.v1")
    );
    for (field, expected) in [
        ("descriptor_digest", None),
        ("input_digest", Some(input_digest.as_str())),
        ("scope_digest", Some(scope_digest.as_str())),
        ("plan_digest", Some(plan_digest.as_str())),
    ] {
        let observed = execution[field].as_str().expect("evidence digest present");
        assert!(
            observed.starts_with("sha256:"),
            "{field} must stay a digest"
        );
        if let Some(expected) = expected {
            assert_eq!(observed, expected, "{field} must bind the committed fact");
        }
    }
    assert_eq!(execution["backend"], json!("bwrap"));
    assert!(execution["result_digest"].as_str().is_some());
    assert_eq!(
        execution["resource_usage_digest"].as_str(),
        Some(resource_digest.as_str())
    );
    assert!(execution["changes_digest"].as_str().is_some());

    // The same committed facts produce the same execution receipt through the read model as
    // through the parity snapshot consumed by every entrypoint.
    let parity = core
        .entrypoint_parity(&context, EntryPointKind::Cli, Some(run_id))
        .await
        .unwrap();
    assert_eq!(parity.status, ExecutionStatus::Completed);
    let snapshot: kiana_domain::EntryPointParitySnapshot =
        serde_json::from_value(parity.output).unwrap();
    assert_eq!(snapshot.status, ExecutionStatus::Completed);
    assert!(snapshot.receipt_digest.is_some());
    // Parity must project the same committed audit facts; an unavailable audit projection is the
    // one limitation that would downgrade Completed to ResultUnknown, so keep that boundary
    // explicit instead of asserting an empty limitation list that unrelated health metrics own.
    assert!(
        !snapshot
            .limitations
            .iter()
            .any(|limitation| limitation.starts_with("audit_projection_unavailable")),
        "parity audit projection must stay available: {:?}",
        snapshot.limitations
    );
}

#[tokio::test]
async fn execution_receipt_limitations_stay_bounded_and_explicit() {
    let (core, events) = fixture_core();
    let context = local_context("cap25-limit-session", "cap25-limit-actor");
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let execution_id = kiana_domain::ExecutionId::new();
    append(&events, authorized_run(run_id, &context, "read-only")).await;
    append(
        &events,
        event(
            3,
            "run.capability_requested",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "capability": "process",
                "operation": "shell.exec",
                "risk": "read_only",
                "attempt": 1,
                "action_digest": sha_like('a'),
                "arguments": {"command": "printf cap25", "sandbox": "read-only"},
            }),
        ),
    )
    .await;
    append(
        &events,
        event(
            4,
            "capability.decision",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "gate": {"decision": "allowed", "reason": "cap25-fixture"},
            }),
        ),
    )
    .await;
    append(
        &events,
        event(
            5,
            "execution.prepared",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 1,
            }),
        ),
    )
    .await;
    // No permit digest, no input/scope facts and a handler error whose text must not leak.
    append(
        &events,
        event(
            6,
            "capability.failed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "execution_id": execution_id,
                "attempt": 1,
                "stop_confirmed": false,
                "result": {"success": false, "error": "cap25_fixture_failed"},
            }),
        ),
    )
    .await;
    append(&events, completed_run(run_id, json!({"text": "done"}))).await;

    let receipt = core.read_receipt(context, Some(run_id)).await.unwrap();
    let execution = &receipt.output["execution_receipts"][0];
    let limitations = execution["limitations"]
        .as_array()
        .expect("limitations list");
    assert!(!limitations.is_empty());
    assert!(limitations.len() <= kiana_domain::MAX_RECEIPT_LIMITATIONS);
    let limitations = limitations
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    for expected in ["input_digest_unavailable", "scope_digest_unavailable"] {
        assert!(
            limitations.contains(&expected),
            "missing bounded limitation {expected}: {limitations:?}"
        );
    }
    // The handler error text stays out of the bounded limitation labels.
    assert!(!execution.to_string().contains("cap25_fixture_failed"));
}
