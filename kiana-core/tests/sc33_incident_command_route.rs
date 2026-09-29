//! SC-33 接线测试：一次「这个动作对这份事件是否可被记录」的判定，经由**命令入口**发起。
//!
//! deny-first。这组测试存在的理由是接线本身：模块内部的规则已经被 `sc33_security_incident.rs`
//! 覆盖，而这里覆盖的是**入口必须先问的两件事**——你是谁、这个项目能不能信。
//! 一个拒绝原因写错，模块内部的规则再好也没用，因为调用方看不到它。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{
    ControlPlane, SecurityIncident, SecurityIncidentAction, SecurityIncidentActionRequest,
    SecurityIncidentClass, SecurityIncidentEvidence, SecurityIncidentSeverity,
    SecurityIncidentState,
};
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, CommandIntent, PendingApproval,
    RequestContext,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::sync::Arc;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// 判定路径不碰能力，所以 runner 只需要「不存在」——真被调用到就会失败，而不是静默成功。
struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

/// 判定路径同样不碰审批。这三个方法全部返回 Unavailable，也就是**万一被碰到就 fail-closed**，
/// 而不是「没配置就放行」——一个空的审批存储必须是拒绝，而不是通过。
struct UnavailableApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnavailableApprovalStore {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }
}

fn core() -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        Arc::new(MemoryEventLog::new()),
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnavailableApprovalStore),
        Arc::new(UnavailableRunner),
    )
}

fn trusted() -> RequestContext {
    let mut context = RequestContext::local("sc33-session", "/repo");
    context.project_trusted = true;
    context
}

fn evidence() -> SecurityIncidentEvidence {
    SecurityIncidentEvidence::new("evidence-1", DIGEST, 1_000).expect("evidence")
}

fn incident() -> SecurityIncident {
    SecurityIncident::open(
        "incident-1",
        SecurityIncidentClass::UnknownOutcome,
        SecurityIncidentSeverity::High,
        "owner-1",
        2_000,
        1_000,
        None,
        vec![evidence()],
    )
    .expect("incident")
}

fn state() -> SecurityIncidentState {
    SecurityIncidentState::opened("incident-1").expect("state")
}

fn action() -> SecurityIncidentActionRequest {
    SecurityIncidentActionRequest::new(
        "incident-1",
        SecurityIncidentAction::Contain,
        "responder-1",
        1_100,
        vec![evidence()],
        None,
    )
    .expect("action")
}

fn payload() -> Value {
    json!({
        "incident": incident(),
        "state": state(),
        "action": action(),
    })
}

async fn evaluate(context: &RequestContext, arguments: Value) -> String {
    let response = core()
        .handle_command(
            context.clone(),
            CommandIntent::new("security.incident.evaluate", arguments),
        )
        .await
        .expect("response");
    // 成功时返回 completed，失败时返回 blocked；两者都带一个可断言的原因字段。
    let value = serde_json::to_value(&response).expect("serialized response");
    // `CoreResponse::blocked(request_id, reason)` 把原因放进 **`error`** 字段
    // （见 kiana-domain::CoreResponse），并不存在顶层 `reason` 字段。此前这里读 `reason`，
    // 于是**每一次拒绝断言都拿到默认值 `"completed"`**——包括本该 fail-closed 的
    // 「载荷为 null / 形状不完整必须被拒」这类用例，看起来像是「空载荷被放行了」。
    // 先读 `error`，再回退到 `reason`（万一某个响应把原因放在 output 的 reason 上），
    // 最后才落到 "completed"。
    value
        .get("error")
        .or_else(|| value.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| "completed")
        .to_owned()
}

#[tokio::test]
async fn an_anonymous_caller_cannot_ask_whether_an_action_is_admissible() {
    // 判定要指名 owner 与 reviewer；匿名调用等于让任何人替别人签署一次响应。
    let mut context = trusted();
    context.actor_id = None;
    assert_eq!(
        evaluate(&context, payload()).await,
        "security_incident_actor_required"
    );
}

#[tokio::test]
async fn an_untrusted_project_cannot_drive_a_security_response() {
    // 项目本地的配置能够注入指令，所以未信任项目不该驱动一次安全响应。
    let mut context = trusted();
    context.project_trusted = false;
    assert_eq!(
        evaluate(&context, payload()).await,
        "security_incident_project_untrusted"
    );
}

#[tokio::test]
async fn a_malformed_or_incomplete_payload_is_refused_before_any_decision() {
    for arguments in [
        Value::Null,
        json!("not an object"),
        json!({}),
        json!({"incident": incident()}),
    ] {
        assert_eq!(
            evaluate(&trusted(), arguments.clone()).await,
            "security_incident_payload_required",
            "payload {arguments} should be refused as missing"
        );
    }
    // 字段齐全但内容坏掉，是另一条拒绝：调用方需要知道是「没给」还是「给坏了」。
    let broken = json!({"incident": {"schema": "nope"}, "state": state(), "action": action()});
    assert_eq!(
        evaluate(&trusted(), broken).await,
        "security_incident_payload_invalid"
    );
}

#[tokio::test]
async fn an_inadmissible_action_is_refused_with_the_modules_own_reason() {
    // 跳过 reconcile 直接 close——这正是卡片要防的「把 unknown 悄悄变成成功」。
    let closing = SecurityIncidentActionRequest::new(
        "incident-1",
        SecurityIncidentAction::Close,
        "responder-1",
        1_100,
        vec![evidence()],
        None,
    )
    .expect("close action");
    let payload = json!({
        "incident": incident(),
        "state": state(),
        "action": closing,
    });
    let reason = evaluate(&trusted(), payload).await;
    assert_eq!(reason, "security_incident_reconcile_required");
}

#[tokio::test]
async fn a_well_formed_action_is_admitted_and_the_route_stays_read_only() {
    // 成功路径：返回 completed，并且判定本身不写任何事件。
    let events = Arc::new(MemoryEventLog::new());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnavailableApprovalStore),
        Arc::new(UnavailableRunner),
    );
    let response = core
        .handle_command(
            trusted(),
            CommandIntent::new("security.incident.evaluate", payload()),
        )
        .await
        .expect("response");
    let value = serde_json::to_value(&response).expect("serialized");
    assert_eq!(
        value.get("status").and_then(Value::as_str),
        Some("completed"),
        "an admissible action must be admitted: {value}"
    );
    // 判定不产生副作用：事件流仍然是空的。这条断言是接线的核心不变式——
    // 一次只读判定绝不能顺手写一条事实。
    let stream = events
        .read_stream("security.incident.evaluate", "incident-1")
        .await;
    assert!(
        stream.expect("readable stream").is_empty(),
        "a read-only decision must not append an event"
    );
}
