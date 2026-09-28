//! SC-40 接线测试：一次「故障期间系统是否仍在界内」的判定，经由**命令入口**发起。
//!
//! deny-first。覆盖的是入口必须自己守住的那条边界：这份判定的结论会被用来解释
//! 「当时有没有越界」，所以 cell 内部的 worker 不该参与裁定系统是否守住了边界。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{
    Bq26FaultCase, Bq26FaultClass, CapacityBudget, CapacityFaultSample, ControlPlane,
};
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, CellId, CommandIntent, PendingApproval,
    RequestContext,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::sync::Arc;

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
    let mut context = RequestContext::local("sc40-session", "/repo");
    context.project_trusted = true;
    context
}

fn budget() -> CapacityBudget {
    CapacityBudget::new(5_000, 64, 1_048_576, true)
}

fn sample() -> CapacityFaultSample {
    CapacityFaultSample::new(
        Bq26FaultCase::CasRace,
        Bq26FaultClass::Concurrency,
        1_200,
        8,
        4_096,
        true,
        0,
        0,
        true,
        0,
        false,
    )
}

fn payload() -> Value {
    json!({ "sample": sample(), "budget": budget() })
}

async fn check(context: &RequestContext, arguments: Value) -> String {
    let response = core()
        .handle_command(
            context.clone(),
            CommandIntent::new("capacity.envelope", arguments),
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
async fn a_cell_worker_cannot_adjudicate_whether_the_system_stayed_bounded() {
    let mut context = operator();
    context.cell_id = Some(CellId::new());
    assert_eq!(
        check(&context, payload()).await,
        "capacity_envelope_operator_required"
    );
}

#[tokio::test]
async fn an_anonymous_caller_cannot_reach_the_envelope() {
    let mut context = operator();
    context.actor_id = None;
    assert_eq!(
        check(&context, payload()).await,
        "capacity_envelope_operator_required"
    );
}

#[tokio::test]
async fn a_missing_or_malformed_request_is_refused_before_any_decision() {
    for arguments in [Value::Null, json!("not an object"), json!({}), json!({"sample": {}})] {
        assert_eq!(
            check(&operator(), arguments.clone()).await,
            "capacity_envelope_payload_required",
            "payload {arguments} should be refused as missing"
        );
    }
    let broken = json!({"sample": {"case": "nope"}, "budget": budget()});
    assert_eq!(
        check(&operator(), broken).await,
        "capacity_envelope_payload_invalid"
    );
}

#[tokio::test]
async fn a_sample_outside_the_envelope_is_refused_with_the_modules_own_reason() {
    // 时钟回拨最先判：它一旦成立，后面每个数字都不再能说明任何事。
    let mut rolled_back = sample();
    rolled_back.clock_rollback_detected = true;
    let refused = check(
        &operator(),
        json!({"sample": rolled_back, "budget": budget()}),
    )
    .await;
    assert_eq!(refused, "capacity_fault_clock_rollback");

    // 无界队列。
    let mut unbounded = sample();
    unbounded.observed_queue_depth = 65;
    let refused = check(
        &operator(),
        json!({"sample": unbounded, "budget": budget()}),
    )
    .await;
    assert_eq!(refused, "capacity_fault_queue_unbounded");
}

#[tokio::test]
async fn a_sample_inside_the_envelope_is_admitted_and_the_route_stays_read_only() {
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
        .handle_command(operator(), CommandIntent::new("capacity.envelope", payload()))
        .await
        .expect("response");
    let value = serde_json::to_value(&response).expect("serialized");
    assert_eq!(
        value.get("status").and_then(Value::as_str),
        Some("completed"),
        "a sample inside every bound must be admitted: {value}"
    );
    // 判定不产生副作用：事件流仍然是空的。
    let stream = events.read_stream("capacity.envelope", "sc40").await;
    assert!(
        stream.expect("readable stream").is_empty(),
        "a capacity decision must not append an event"
    );
}
