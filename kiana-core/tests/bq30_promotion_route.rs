//! BQ-30 接线测试：一次「这个主张能不能被叫得比它的证据更强」的判定，经由**命令入口**发起。
//!
//! deny-first。这里覆盖的是入口必须自己守住的那条边界：一个没人能调用的 promotion gate 不是门，
//! 而是一个函数；而一旦能被调用，它就必须先拒绝「cell 内部的 worker 自行提权」这件事。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{
    evaluate_promotion, ClaimedLevel, CommandInvocation, EnvironmentFact, EvidenceFeatureStatus,
    EvidenceManifest, EvidenceProofLevel, FixtureKind, FixtureRef,
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

const RECEIPT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

/// 判定路径不碰审批。三个方法都返回 Unavailable，也就是**万一被碰到就拒绝**——
/// 一个空的审批存储必须是拒绝，不是通过。
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

use kiana_core::ControlPlane;

fn operator() -> RequestContext {
    let mut context = RequestContext::local("bq30-session", "/repo");
    context.project_trusted = true;
    context
}

fn evidence(proof_level: EvidenceProofLevel) -> EvidenceManifest {
    EvidenceManifest::new(
        "bq30-evidence",
        "0be643aa",
        "recorder",
        "reviewer",
        CommandInvocation::new(
            vec!["cargo".to_owned(), "test".to_owned()],
            "/repo",
            Some(0),
        ),
        vec![EnvironmentFact::new("CI", "github-actions")],
        vec![FixtureRef::new(
            "kiana-core/tests/fixtures/one.json",
            FixtureKind::Fixture,
            RECEIPT,
        )],
        "",
        "row moved",
        EvidenceFeatureStatus::Partial,
        proof_level,
        vec!["no external system was exercised".to_owned()],
        None,
    )
    .expect("manifest")
}

fn payload(claimed_level: &str, proof_level: EvidenceProofLevel) -> Value {
    json!({
        "claim_id": "bq30-claim",
        "claimed_level": claimed_level,
        "evidence": evidence(proof_level),
        "restart_replay_ref": "replay-1",
    })
}

async fn check(context: &RequestContext, arguments: Value) -> String {
    let response = core()
        .handle_command(
            context.clone(),
            CommandIntent::new("promotion.check", arguments),
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
async fn a_cell_worker_cannot_decide_whether_its_own_claim_is_promoted() {
    // 这是整个接线最要紧的一条：promotion 决定的是「对外声称什么」。让 cell 内部的 worker
    // 自行把一条主张升到 opt_in_live，等于给它开了一个自我提权的口子。
    let mut context = operator();
    context.cell_id = Some(kiana_domain::CellId::new());
    assert_eq!(
        check(
            &context,
            payload("offline_durable", EvidenceProofLevel::Durable)
        )
        .await,
        "promotion_operator_required"
    );
}

#[tokio::test]
async fn an_anonymous_caller_cannot_reach_the_promotion_gate() {
    let mut context = operator();
    context.actor_id = None;
    assert_eq!(
        check(
            &context,
            payload("offline_durable", EvidenceProofLevel::Durable)
        )
        .await,
        "promotion_operator_required"
    );
}

#[tokio::test]
async fn a_missing_or_malformed_request_is_refused_before_any_decision() {
    for arguments in [Value::Null, json!("not an object"), json!({})] {
        assert_eq!(
            check(&operator(), arguments.clone()).await,
            "promotion_payload_required",
            "payload {arguments} should be refused as missing"
        );
    }
    // 缺 claimed_level 与写错它是两件事：前者要补字段，后者要改字段。
    let mut missing_level = payload("offline_durable", EvidenceProofLevel::Durable);
    missing_level
        .as_object_mut()
        .expect("object")
        .remove("claimed_level");
    assert_eq!(
        check(&operator(), missing_level).await,
        "promotion_payload_required"
    );
    assert_eq!(
        check(
            &operator(),
            payload("platinum", EvidenceProofLevel::Durable)
        )
        .await,
        "promotion_level_unknown"
    );
    let mut broken_evidence = payload("offline_durable", EvidenceProofLevel::Durable);
    broken_evidence
        .as_object_mut()
        .expect("object")
        .insert("evidence".to_owned(), json!({"schema": "nope"}));
    assert_eq!(
        check(&operator(), broken_evidence).await,
        "promotion_payload_invalid"
    );
}

#[tokio::test]
async fn a_claim_the_evidence_cannot_reach_is_refused_with_the_gates_own_reason() {
    // 类型级证据不能被叫成 billing live——这正是卡片点名的第一种失败。
    assert_eq!(
        check(
            &operator(),
            payload("opt_in_live", EvidenceProofLevel::Source)
        )
        .await,
        "promotion_above_evidence_ceiling"
    );
    // 无 provider receipt 不能宣称 measured。
    assert_eq!(
        check(
            &operator(),
            payload("opt_in_live", EvidenceProofLevel::Live)
        )
        .await,
        "promotion_live_requires_provider_receipt"
    );
}

#[tokio::test]
async fn a_claim_the_evidence_reaches_is_promoted_and_the_route_stays_read_only() {
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
                "promotion.check",
                payload("offline_durable", EvidenceProofLevel::Durable),
            ),
        )
        .await
        .expect("response");
    let value = serde_json::to_value(&response).expect("serialized");
    assert_eq!(
        value.get("status").and_then(Value::as_str),
        Some("completed"),
        "a reachable claim must be promoted: {value}"
    );
    // 判定不产生副作用：事件流仍然是空的。一次只读判定顺手写一条事实，正是这条路线要避免的。
    let stream = events.read_stream("promotion.check", "bq30-claim").await;
    assert!(
        stream.expect("readable stream").is_empty(),
        "a promotion decision must not append an event"
    );
}
