//! DEP-25 接线测试：一次「外部 effect 结果未知时，哪种动作可被记录」的判定，经由**命令入口**发起。
//!
//! deny-first。这条判定决定的是一个**已经发出去的请求算什么**——重发、放弃还是补偿，
//! 每一个都会改变真实世界的后果。所以入口必须先挡住 cell 内部的 worker：让它参与裁定
//! 自己发出去的请求算什么，等于让它为自己的行为定性。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, CellId, CommandIntent, EffectObservation,
    EffectObservationState, ExecutionId, InvocationId, PendingApproval, RequestContext,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::sync::Arc;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

/// 判定路径不碰审批；三个方法都返回 Unavailable，也就是万一被碰到就拒绝。
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

fn operator() -> RequestContext {
    let mut context = RequestContext::local("dep25-session", "/repo");
    context.project_trusted = true;
    context
}

fn observation(state: EffectObservationState, evidence: Vec<String>) -> EffectObservation {
    EffectObservation::new(
        ExecutionId::new(),
        InvocationId::new(),
        1,
        digest('a'),
        digest('b'),
        digest('c'),
        Some("receipt-1".to_owned()),
        Some("pending".to_owned()),
        1_700_000_000_000,
        Some(digest('d')),
        evidence,
        state,
    )
    .expect("observation")
}

fn payload(resolution: &str, state: EffectObservationState, evidence: Vec<String>) -> Value {
    json!({
        "observation": observation(state, evidence),
        "resolution": resolution,
        "approval_ref": "approval-1",
        "authority_epoch": 4,
        "fence_token": "fence-1",
        "idempotency_key": "idem-1",
    })
}

async fn check(context: &RequestContext, arguments: Value) -> String {
    let response = core()
        .handle_command(
            context.clone(),
            CommandIntent::new("effect.reconcile", arguments),
        )
        .await
        .expect("response");
    let value = serde_json::to_value(&response).expect("serialized response");
    value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_else(|| "completed")
        .to_owned()
}

#[tokio::test]
async fn a_cell_worker_cannot_decide_what_its_own_request_counts_as() {
    let mut context = operator();
    context.cell_id = Some(CellId::new());
    assert_eq!(
        check(
            &context,
            payload(
                "compensated",
                EffectObservationState::Unknown,
                vec![digest('f')]
            )
        )
        .await,
        "effect_reconcile_operator_required"
    );
}

#[tokio::test]
async fn an_anonymous_caller_cannot_reach_the_reconciliation() {
    let mut context = operator();
    context.actor_id = None;
    assert_eq!(
        check(
            &context,
            payload(
                "compensated",
                EffectObservationState::Unknown,
                vec![digest('f')]
            )
        )
        .await,
        "effect_reconcile_operator_required"
    );
}

#[tokio::test]
async fn a_missing_or_malformed_request_is_refused_before_any_decision() {
    for arguments in [
        Value::Null,
        json!("not an object"),
        json!({}),
        json!({"resolution": "abandoned"}),
    ] {
        assert_eq!(
            check(&operator(), arguments.clone()).await,
            "effect_reconcile_payload_required",
            "payload {arguments} should be refused as missing"
        );
    }
    // 缺 resolution 与写错它是两件事：前者要补字段，后者要改字段。
    let mut missing = payload("abandoned", EffectObservationState::NoEffect, vec![]);
    missing
        .as_object_mut()
        .expect("object")
        .remove("resolution");
    assert_eq!(
        check(&operator(), missing).await,
        "effect_reconcile_payload_required"
    );
    assert_eq!(
        check(
            &operator(),
            payload("teleport", EffectObservationState::NoEffect, vec![])
        )
        .await,
        "effect_reconcile_resolution_unknown"
    );
    let mut broken = payload("abandoned", EffectObservationState::NoEffect, vec![]);
    broken
        .as_object_mut()
        .expect("object")
        .insert("observation".to_owned(), json!({"schema": "nope"}));
    assert_eq!(
        check(&operator(), broken).await,
        "effect_reconcile_payload_invalid"
    );
}

#[tokio::test]
async fn an_unknown_outcome_is_still_refused_when_the_retry_comes_through_a_command() {
    // 这条路由最要紧的断言：`Unknown` 不允许用重试解决，不管发命令的是谁。
    assert_eq!(
        check(
            &operator(),
            payload(
                "retry_without_effect",
                EffectObservationState::Unknown,
                vec![digest('f')]
            )
        )
        .await,
        "effect_reconcile_unknown_auto_retry"
    );
}

#[tokio::test]
async fn an_admissible_resolution_is_recorded_and_the_route_stays_read_only() {
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
            operator(),
            CommandIntent::new(
                "effect.reconcile",
                payload(
                    "compensated",
                    EffectObservationState::Unknown,
                    vec![digest('f')],
                ),
            ),
        )
        .await
        .expect("response");
    let value = serde_json::to_value(&response).expect("serialized");
    assert_eq!(
        value.get("status").and_then(Value::as_str),
        Some("completed"),
        "an admissible resolution must be recorded: {value}"
    );
    // 判定只回答「能不能这样记」，它不记：事件流仍然是空的。
    let stream = events.read_stream("effect.reconcile", "dep25").await;
    assert!(
        stream.expect("readable stream").is_empty(),
        "a reconciliation decision must not append an event"
    );
}
