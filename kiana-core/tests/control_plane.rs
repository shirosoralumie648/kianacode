use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_domain::{
    ApprovalChallenge, ApprovalDecision, ApprovalId, AuthorizedCapabilityRequest, CapabilityKind,
    CapabilityRequest, CapabilityResult, CommandIntent, ExecutionStatus, PendingApproval,
    RequestContext, RequestId, RuntimeEvent, APPROVAL_CHALLENGE_SCHEMA,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, CapabilityBrokerPort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

struct SuccessfulBroker;

#[async_trait]
impl CapabilityBrokerPort for SuccessfulBroker {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        Ok(CapabilityResult::success(
            request.request.request_id,
            json!({ "changed": true }),
        ))
    }
}

#[derive(Default)]
struct TestApprovalStore {
    pending: Mutex<Option<(PendingApproval, bool, bool)>>,
}

#[async_trait]
impl ApprovalStorePort for TestApprovalStore {
    async fn stage(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        let challenge = ApprovalChallenge {
            schema: APPROVAL_CHALLENGE_SCHEMA.to_owned(),
            approval_id: ApprovalId::new(),
            request_id: context.request_id,
            request_hash: "a".repeat(64),
            expires_at_unix_ms: u64::MAX,
            reason: reason.to_owned(),
        };
        *self.pending.lock().await = Some((
            PendingApproval {
                challenge: challenge.clone(),
                request,
            },
            false,
            false,
        ));
        Ok(challenge)
    }

    async fn activate(&self, approval_id: ApprovalId) -> Result<(), PortError> {
        let mut pending = self.pending.lock().await;
        let Some((pending, active, consumed)) = pending.as_mut() else {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        };
        if pending.challenge.approval_id != approval_id || *consumed {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        }
        *active = true;
        Ok(())
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        let mut pending = self.pending.lock().await;
        let Some((pending, active, consumed)) = pending.as_mut() else {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        };
        if pending.challenge.approval_id != approval_id {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        }
        if !*active {
            return Err(PortError::Failed("approval_not_active".to_owned()));
        }
        if *consumed {
            return Err(PortError::Conflict("approval_already_consumed".to_owned()));
        }
        *consumed = true;
        Ok(pending.clone())
    }
}

#[derive(Default)]
struct FailResultEventStore {
    inner: MemoryEventLog,
}

#[async_trait]
impl EventStorePort for FailResultEventStore {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        if event.sequence == 3 {
            return Err(PortError::Failed("result_event_write_failed".to_owned()));
        }
        self.inner.append(event).await
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(request_id).await
    }
}

struct CoreHarness {
    core: ControlPlane,
    events: Arc<MemoryEventLog>,
}

impl CoreHarness {
    fn new() -> Self {
        let events = Arc::new(MemoryEventLog::new());
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events.clone(),
            Arc::new(CapabilityBroker::new()),
            Arc::new(TestApprovalStore::default()),
            Arc::new(UnavailableRunner),
        );
        Self { core, events }
    }
}

fn trusted_context() -> RequestContext {
    let mut context = RequestContext::local("session-1", "/repo");
    context.project_trusted = true;
    context
}

#[tokio::test]
async fn architecture_command_records_accepted_and_completed_events() {
    let harness = CoreHarness::new();
    let context = trusted_context();
    let request_id = context.request_id;
    let response = harness
        .core
        .handle_command(
            context,
            CommandIntent::new("system.architecture", Value::Null),
        )
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.output["control_plane"], "kiana-core");
    let events = harness.events.read_request(&request_id).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        ["request.accepted", "command.completed"]
    );
}

#[tokio::test]
async fn routed_context_query_records_one_monotonic_capability_event_sequence() {
    let events = Arc::new(MemoryEventLog::new());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(SuccessfulBroker),
        Arc::new(TestApprovalStore::default()),
        Arc::new(UnavailableRunner),
    );
    let context = trusted_context();
    let request_id = context.request_id;
    let response = core
        .handle_command(
            context,
            CommandIntent::new(
                "context.query.v1",
                json!({
                    "operation": "repo_map",
                    "output": "json",
                    "options": { "max_tokens": 1000 },
                }),
            ),
        )
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed);
    let events = events.read_request(&request_id).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| (event.sequence, event.kind.as_str()))
            .collect::<Vec<_>>(),
        [
            (1, "request.accepted"),
            (2, "capability.decision"),
            (3, "capability.completed"),
        ]
    );
}

#[tokio::test]
async fn untrusted_and_unknown_commands_fail_closed_with_events() {
    let harness = CoreHarness::new();
    let untrusted = RequestContext::local("session-1", "/repo");
    let untrusted_id = untrusted.request_id;
    let response = harness
        .core
        .handle_command(
            untrusted,
            CommandIntent::new("system.architecture", Value::Null),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(
        harness.events.read_request(&untrusted_id).await.unwrap()[1].kind,
        "command.rejected"
    );

    let context = trusted_context();
    let response = harness
        .core
        .handle_command(context, CommandIntent::new("unknown.command", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("command_unregistered"));
}

#[tokio::test]
async fn capability_policy_and_broker_failures_preserve_event_order() {
    let harness = CoreHarness::new();
    let context = trusted_context();
    let request = CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Query,
        "search",
        json!({ "query": "architecture" }),
    );
    let response = harness
        .core
        .authorize_and_execute(&context, request)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed);
    let events = harness
        .events
        .read_request(&context.request_id)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        [
            "request.accepted",
            "capability.decision",
            "capability.failed"
        ]
    );
}

#[tokio::test]
async fn write_capability_waits_for_approval_without_calling_broker() {
    let harness = CoreHarness::new();
    let context = trusted_context();
    let request = CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Filesystem,
        "write",
        Value::Null,
    )
    .with_risk(kiana_domain::RiskLevel::LocalWrite);
    let response = harness
        .core
        .authorize_and_execute(&context, request)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::AwaitingApproval);
    let challenge: ApprovalChallenge =
        serde_json::from_value(response.output["approval"].clone()).unwrap();
    assert_eq!(challenge.request_id, context.request_id);
    assert_eq!(challenge.request_hash.len(), 64);
    let events = harness
        .events
        .read_request(&context.request_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[1].kind, "capability.decision");
    assert_eq!(events[2].kind, "approval.requested");
}

#[tokio::test]
async fn approval_resumes_the_stored_request_once_with_monotonic_events() {
    let events = Arc::new(MemoryEventLog::new());
    let approvals = Arc::new(TestApprovalStore::default());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(SuccessfulBroker),
        approvals,
        Arc::new(UnavailableRunner),
    );
    let context = trusted_context();
    let request = CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Filesystem,
        "write",
        json!({ "path": "approved.txt" }),
    )
    .with_risk(kiana_domain::RiskLevel::LocalWrite);
    let awaiting = core.authorize_and_execute(&context, request).await.unwrap();
    let challenge: ApprovalChallenge =
        serde_json::from_value(awaiting.output["approval"].clone()).unwrap();

    let mut decision_context = context.clone();
    decision_context.request_id = RequestId::new();
    let completed = core
        .decide_approval(
            &decision_context,
            challenge.approval_id,
            ApprovalDecision::Approve,
        )
        .await
        .unwrap();
    assert_eq!(completed.status, ExecutionStatus::Completed);
    assert_eq!(completed.request_id, context.request_id);
    let events = events.read_request(&context.request_id).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| (event.sequence, event.kind.as_str()))
            .collect::<Vec<_>>(),
        [
            (1, "request.accepted"),
            (2, "capability.decision"),
            (3, "approval.requested"),
            (4, "approval.approved"),
            (5, "capability.completed"),
        ]
    );

    let replay = core
        .decide_approval(
            &decision_context,
            challenge.approval_id,
            ApprovalDecision::Approve,
        )
        .await
        .unwrap();
    assert_eq!(replay.status, ExecutionStatus::Blocked);
    assert!(replay
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("approval_already_consumed"));
}

#[tokio::test]
async fn completed_side_effect_with_missing_result_event_is_result_unknown() {
    let events = Arc::new(FailResultEventStore::default());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(SuccessfulBroker),
        Arc::new(TestApprovalStore::default()),
        Arc::new(UnavailableRunner),
    );
    let context = trusted_context();
    let request = CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Query,
        "search",
        Value::Null,
    );
    let response = core.authorize_and_execute(&context, request).await.unwrap();
    assert_eq!(response.status, ExecutionStatus::ResultUnknown);
    assert_eq!(
        response.error.as_deref(),
        Some("result_event_persistence_failed")
    );
}
