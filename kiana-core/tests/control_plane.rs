use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest, CapabilityResult,
    CommandIntent, ExecutionStatus, RequestContext, RequestId, RuntimeEvent,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{CapabilityBrokerPort, EventStorePort, PortError, RunnerPort};
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
    let events = harness
        .events
        .read_request(&context.request_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].kind, "capability.decision");
}

#[tokio::test]
async fn completed_side_effect_with_missing_result_event_is_result_unknown() {
    let events = Arc::new(FailResultEventStore::default());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(SuccessfulBroker),
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
