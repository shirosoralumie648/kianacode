use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_domain::{
    ApprovalChallenge, ApprovalDecision, ApprovalId, AuthorizedCapabilityRequest, CapabilityKind,
    CapabilityRequest, CapabilityResult, CommandIntent, ExecutionStatus, PendingApproval,
    PermissionProfile, RequestContext, RequestId, RoleSpec, RunId, RuntimeEvent, WorkPacket,
    APPROVAL_CHALLENGE_SCHEMA,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, CapabilityBrokerPort, EventStorePort, PortError, RunnerPort};
use kiana_runner::{KianaHarness, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, Notify};

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

struct CountingBroker {
    calls: Mutex<usize>,
}

#[async_trait]
impl CapabilityBrokerPort for CountingBroker {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        *self.calls.lock().await += 1;
        Ok(CapabilityResult::success(
            request.request.request_id,
            json!({ "stdout": "listed" }),
        ))
    }
}

fn scripted_runner(outputs: Value) -> Arc<KianaHarness> {
    Arc::new(KianaHarness::new(Arc::new(
        ScriptedModel::from_json(&outputs).unwrap(),
    )))
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
            json!({
                "changed": true,
                "api_key": "broker-secret",
                "secret_ref": "vault://capability",
            }),
        ))
    }
}

struct MismatchedResultBroker;

#[async_trait]
impl CapabilityBrokerPort for MismatchedResultBroker {
    async fn execute(
        &self,
        _request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        Ok(CapabilityResult::success(
            RequestId::new(),
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
            nonce: String::new(),
            policy_version: String::new(),
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

    async fn invalidate(
        &self,
        _context: &RequestContext,
        approval_id: ApprovalId,
        _reason: &str,
    ) -> Result<(), PortError> {
        let mut pending = self.pending.lock().await;
        let Some((pending, _active, consumed)) = pending.as_mut() else {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        };
        if pending.challenge.approval_id != approval_id {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        }
        if *consumed {
            return Err(PortError::Conflict("approval_already_consumed".to_owned()));
        }
        *consumed = true;
        Ok(())
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

    async fn append_expected(
        &self,
        event: RuntimeEvent,
        _expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        self.append(event).await
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(request_id).await
    }
}

#[derive(Default)]
struct FailHarnessResultEventStore {
    inner: MemoryEventLog,
}

#[async_trait]
impl EventStorePort for FailHarnessResultEventStore {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        if event.kind == "capability.completed" {
            return Err(PortError::Failed(
                "harness_result_event_write_failed".to_owned(),
            ));
        }
        self.inner.append(event).await
    }

    async fn append_expected(
        &self,
        event: RuntimeEvent,
        _expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        self.append(event).await
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
        Self::with_runner(Arc::new(UnavailableRunner))
    }

    fn with_runner(runner: Arc<dyn RunnerPort>) -> Self {
        let events = Arc::new(MemoryEventLog::new());
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events.clone(),
            Arc::new(CapabilityBroker::new()),
            Arc::new(TestApprovalStore::default()),
            runner,
        );
        Self { core, events }
    }

    fn with_runner_and_broker(
        runner: Arc<dyn RunnerPort>,
        broker: Arc<dyn CapabilityBrokerPort>,
    ) -> Self {
        let events = Arc::new(MemoryEventLog::new());
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events.clone(),
            broker,
            Arc::new(TestApprovalStore::default()),
            runner,
        );
        Self { core, events }
    }
}

fn trusted_context() -> RequestContext {
    let mut context = RequestContext::local("session-1", "/repo");
    context.project_trusted = true;
    context
}

fn temp_project() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-core-{stamp}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn trusted_context_in(root: &PathBuf, session: &str) -> RequestContext {
    let mut context = RequestContext::local(session, root.to_string_lossy());
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
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
    assert_eq!(response.output["harness"], "kiana-harness");
    assert_eq!(response.output["capability_mode"], "brokered");
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
async fn incomplete_cell_capability_scope_is_rejected_before_broker() {
    let broker = Arc::new(CountingBroker {
        calls: Mutex::new(0),
    });
    let harness = CoreHarness::with_runner_and_broker(
        Arc::new(UnavailableRunner),
        broker.clone(),
    );
    let context = trusted_context();
    let mut request = CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Query,
        "search",
        json!({ "query": "architecture" }),
    );
    request.capability_grant_id = Some(kiana_domain::CapabilityGrantId::new());
    request.budget_lease_id = Some(kiana_domain::BudgetLeaseId::new());

    let error = harness
        .core
        .authorize_and_execute(&context, request)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "port_failed:cell_capability_scope_incomplete");
    assert_eq!(*broker.calls.lock().await, 0);
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
    let completed_event = events
        .iter()
        .find(|event| event.kind == "capability.completed")
        .unwrap();
    assert_eq!(completed_event.data["api_key"], "[REDACTED]");
    assert_eq!(completed_event.data["secret_ref"], "vault://capability");

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

#[tokio::test]
async fn receipt_replays_result_unknown_without_claiming_success() {
    let events = Arc::new(MemoryEventLog::new());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(CapabilityBroker::new()),
        Arc::new(TestApprovalStore::default()),
        Arc::new(UnavailableRunner),
    );
    let context = trusted_context();
    let run_id = RunId::new();
    events
        .append(
            RuntimeEvent::new(
                context.request_id,
                1,
                "run.authorized",
                json!({
                    "run_id": run_id,
                    "session_id": context.session_id,
                    "actor_id": context.actor_id,
                    "project_root": context.project_root,
                    "role_id": "builder",
                    "sandbox": "workspace-write"
                }),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    events
        .append(
            RuntimeEvent::new(
                context.request_id,
                2,
                "run.result_unknown",
                json!({ "run_id": run_id, "error": "result_unknown:provider_timeout" }),
            )
            .unwrap(),
        )
        .await
        .unwrap();

    let receipt = core.read_receipt(context, Some(run_id)).await.unwrap();
    assert_eq!(receipt.status, ExecutionStatus::ResultUnknown, "{receipt:?}");
    assert_eq!(
        receipt.error.as_deref(),
        Some("result_unknown:provider_timeout")
    );
}

#[tokio::test]
async fn persisted_run_approval_without_pending_invocation_fails_closed() {
    let events = Arc::new(MemoryEventLog::new());
    let broker = Arc::new(CountingBroker {
        calls: Mutex::new(0),
    });
    let approvals = Arc::new(TestApprovalStore::default());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        broker.clone(),
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
    let persisted_run_id = RunId::new();
    events
        .append(
            RuntimeEvent::new(
                RequestId::new(),
                1,
                "approval.requested",
                json!({
                    "approval_id": challenge.approval_id,
                    "run_id": persisted_run_id
                }),
            )
            .unwrap(),
        )
        .await
        .unwrap();

    let approved = core
        .decide_approval(&context, challenge.approval_id, ApprovalDecision::Approve)
        .await
        .unwrap();
    assert_eq!(approved.status, ExecutionStatus::Blocked, "{approved:?}");
    assert_eq!(
        approved.error.as_deref(),
        Some("approval_continuation_unavailable")
    );
    assert_eq!(*broker.calls.lock().await, 0);
}

#[tokio::test]
async fn mismatched_direct_capability_result_is_unknown() {
    let events = Arc::new(MemoryEventLog::new());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(MismatchedResultBroker),
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
        Some("capability_result_mismatch")
    );
    let events = events.read_request(&context.request_id).await.unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == "capability.result_unknown"));
    assert!(!events
        .iter()
        .any(|event| event.kind == "capability.completed"));
}

#[tokio::test]
async fn mismatched_harness_capability_result_is_unknown() {
    let events = Arc::new(MemoryEventLog::new());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(MismatchedResultBroker),
        Arc::new(TestApprovalStore::default()),
        scripted_runner(json!([
            {
                "text": "calling",
                "tool_calls": [{
                    "id": "c1",
                    "name": "shell",
                    "arguments": { "command": ["printf", "ok"] }
                }]
            },
            { "text": "done" }
        ])),
    );
    let context = trusted_context();
    let request_id = context.request_id;
    let response = core
        .start_run(context, "inspect".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(
        response.status,
        ExecutionStatus::ResultUnknown,
        "{response:?}"
    );
    assert_eq!(
        response.error.as_deref(),
        Some("result_unknown:capability_result_mismatch")
    );
    let events = events.read_request(&request_id).await.unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == "run.result_unknown"));
    assert!(!events.iter().any(|event| event.kind == "run.completed"));
}

#[tokio::test]
async fn harness_effect_without_result_event_is_result_unknown() {
    let events = Arc::new(FailHarnessResultEventStore::default());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(SuccessfulBroker),
        Arc::new(TestApprovalStore::default()),
        scripted_runner(json!([
            {
                "text": "reading",
                "tool_calls": [{
                    "id": "c1",
                    "name": "shell",
                    "arguments": { "command": ["printf", "ok"] }
                }]
            },
            { "text": "done" }
        ])),
    );
    let context = trusted_context();
    let request_id = context.request_id;
    let response = core
        .start_run(context, "inspect the project".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(
        response.status,
        ExecutionStatus::ResultUnknown,
        "{response:?}"
    );
    assert!(response
        .error
        .as_deref()
        .unwrap_or_default()
        .starts_with("result_unknown:"));
    let events = events.read_request(&request_id).await.unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == "run.result_unknown"));
    assert!(!events.iter().any(|event| event.kind == "run.completed"));
}

#[tokio::test]
async fn start_run_brokers_harness_tools() {
    let broker = Arc::new(CountingBroker {
        calls: Mutex::new(0),
    });
    let harness = CoreHarness::with_runner_and_broker(
        scripted_runner(json!([
            {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "architecture mapped"}
        ])),
        broker.clone(),
    );
    let context = trusted_context();
    let request_id = context.request_id;
    let response = harness
        .core
        .start_run(context, "map the architecture".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.output["schema"], "kiana.run-result.v1");
    assert_eq!(response.output["harness"], "kiana-harness");
    assert_eq!(response.output["role_id"], "builder");
    assert_eq!(response.output["department_id"], "executing");
    assert_eq!(
        response.output["output"]["schema"],
        "kiana.harness-result.v1"
    );
    assert_eq!(*broker.calls.lock().await, 1);
    let events = harness.events.read_request(&request_id).await.unwrap();
    let kinds = events
        .iter()
        .map(|event| event.kind.clone())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"run.authorized".to_owned()));
    assert!(kinds.contains(&"run.capability_requested".to_owned()));
    assert!(kinds.contains(&"capability.completed".to_owned()));
    assert!(kinds.contains(&"run.completed".to_owned()));
    assert!(!kinds.iter().any(|kind| kind == "run.capability_observed"));
    let requested = events
        .iter()
        .find(|event| event.kind == "run.capability_requested")
        .unwrap();
    let completed = events
        .iter()
        .find(|event| event.kind == "capability.completed")
        .unwrap();
    assert_eq!(completed.data["run_id"], response.output["run_id"]);
    assert_eq!(completed.data["session_id"], "session-1");
    assert_eq!(completed.data["capability"], "process");
    assert_eq!(completed.data["operation"], "shell.exec");
    assert_eq!(
        completed.data["capability_request_id"],
        requested.data["request_id"]
    );
    let run_id = response.output["run_id"].as_str().unwrap();
    let stream_versions = events
        .iter()
        .map(|event| {
            assert_eq!(event.aggregate_type.as_deref(), Some("run"));
            assert_eq!(event.aggregate_id.as_deref(), Some(run_id));
            event.stream_version.unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stream_versions,
        (1..=stream_versions.len() as u64).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn untrusted_read_only_run_is_allowed_but_workspace_write_is_blocked() {
    let harness =
        CoreHarness::with_runner(scripted_runner(json!([{"text": "architecture mapped"}])));
    let untrusted = RequestContext::local("session-1", "/repo");
    let allowed = harness
        .core
        .start_run(untrusted, "map the architecture".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(allowed.status, ExecutionStatus::Completed);

    let mut write_context = RequestContext::local("session-2", "/repo");
    write_context.permission_profile = PermissionProfile::Balanced;
    let blocked = harness
        .core
        .start_run(
            write_context,
            "edit the architecture".to_owned(),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status, ExecutionStatus::Blocked);
    assert_eq!(
        blocked.error.as_deref(),
        Some("workspace_write_requires_trusted_non_safe_profile")
    );
}

#[tokio::test]
async fn start_run_brokers_apply_patch_when_trusted_workspace_write() {
    let broker = Arc::new(CountingBroker {
        calls: Mutex::new(0),
    });
    let harness = CoreHarness::with_runner_and_broker(
        scripted_runner(json!([
            {
                "text": "writing",
                "tool_calls": [{
                    "id": "c1",
                    "name": "apply_patch",
                    "arguments": {
                        "patch": "*** Begin Patch\n*** Add File: GOLDEN_PATH.txt\n+hello\n*** End Patch\n"
                    }
                }]
            },
            {"text": "created GOLDEN_PATH.txt"}
        ])),
        broker.clone(),
    );
    let mut context = trusted_context();
    context.permission_profile = PermissionProfile::Balanced;
    let response = harness
        .core
        .start_run(
            context,
            "create a file named GOLDEN_PATH.txt containing hello".to_owned(),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.output["schema"], "kiana.run-result.v1");
    assert_eq!(response.output["harness"], "kiana-harness");
    assert_eq!(response.output["sandbox"], "workspace-write");
    assert_eq!(response.output["role_id"], "builder");
    assert_eq!(response.output["department_id"], "executing");
    assert_eq!(*broker.calls.lock().await, 1);
}

#[tokio::test]
async fn spawn_from_packet_starts_a_fresh_builder_session() {
    let harness = CoreHarness::with_runner(scripted_runner(json!([
        {"text": "planned"},
        {"text": "spawned from packet"}
    ])));
    let mut planner = trusted_context();
    planner.session_id = kiana_domain::SessionId::new("planner-1");
    planner.permission_profile = PermissionProfile::Balanced;
    let planned = harness
        .core
        .start_run(
            planner,
            "PLANNER_SECRET_TOKEN write a packet".to_owned(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(planned.status, ExecutionStatus::Completed);

    let mut worker = trusted_context();
    worker.session_id = kiana_domain::SessionId::new("builder-1");
    worker.permission_profile = PermissionProfile::Balanced;
    worker.assign_role(&kiana_domain::RoleSpec::pm());
    let packet = WorkPacket::builder_task("wp-1", "create GOLDEN_PATH.txt containing hello");
    let spawned = harness
        .core
        .spawn_from_packet(worker, packet, Some("workspace-write".to_owned()))
        .await
        .unwrap();
    assert_eq!(spawned.status, ExecutionStatus::Completed, "{spawned:?}");
    assert_eq!(spawned.output["session_id"], "builder-1");
    assert_ne!(spawned.output["session_id"], planned.output["session_id"]);
    assert_eq!(spawned.output["role_id"], "builder");
    assert_eq!(spawned.output["department_id"], "executing");
    assert_eq!(spawned.output["work_packet_id"], "wp-1");
    assert_eq!(spawned.output["packet_status"], "succeeded");
    let all_events = harness.events.read_all().await.unwrap();
    let packet_events = all_events
        .iter()
        .filter(|event| event.aggregate_type.as_deref() == Some("work_packet"))
        .collect::<Vec<_>>();
    assert_eq!(
        packet_events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        [
            "packet.approved",
            "packet.assigned",
            "packet.accepted",
            "packet.succeeded"
        ]
    );
    assert_eq!(
        packet_events
            .iter()
            .map(|event| event.stream_version.unwrap())
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    let run_id = spawned.output["run_id"].as_str().unwrap();
    let run_events = all_events
        .iter()
        .filter(|event| {
            event.aggregate_type.as_deref() == Some("run")
                && event.aggregate_id.as_deref() == Some(run_id)
        })
        .collect::<Vec<_>>();
    assert!(!run_events.is_empty());
    assert_eq!(
        run_events
            .iter()
            .map(|event| event.stream_version.unwrap())
            .collect::<Vec<_>>(),
        (1..=run_events.len() as u64).collect::<Vec<_>>()
    );
    assert_eq!(spawned.output["input"], "work_packet");
    assert_eq!(spawned.output["output"]["text"], "spawned from packet");
}

#[tokio::test]
async fn spawn_reuses_of_a_live_session_fail_closed() {
    let harness = CoreHarness::with_runner(scripted_runner(json!([
        {"text": "first"},
        {"text": "should not spawn"}
    ])));
    let mut context = trusted_context();
    context.permission_profile = PermissionProfile::Balanced;
    let started = harness
        .core
        .start_run(context.clone(), "hello".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(started.status, ExecutionStatus::Completed);
    let mut spawn_context = context;
    spawn_context.request_id = RequestId::new();
    let spawned = harness
        .core
        .spawn_from_packet(
            spawn_context,
            WorkPacket::builder_task("wp-1", "create GOLDEN_PATH.txt"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(spawned.status, ExecutionStatus::Blocked, "{spawned:?}");
    assert_eq!(spawned.error.as_deref(), Some("spawn_session_not_fresh"));
}

struct HoldingRunner {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl RunnerPort for HoldingRunner {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        match command {
            RunnerCommand::Start { run_id, .. } => {
                self.entered.notify_one();
                self.release.notified().await;
                Ok(vec![
                    RunnerEvent::Started { run_id },
                    RunnerEvent::Completed {
                        run_id,
                        output: json!({ "text": "held" }),
                    },
                ])
            }
            other => Ok(vec![RunnerEvent::Failed {
                run_id: other.run_id(),
                error: "unexpected_runner_command".to_owned(),
            }]),
        }
    }
}

#[tokio::test]
async fn overlapping_live_spawns_fail_closed_on_path_locks() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let harness = CoreHarness::with_runner(Arc::new(HoldingRunner {
        entered: entered.clone(),
        release: release.clone(),
    }));
    let mut first = trusted_context();
    first.session_id = kiana_domain::SessionId::new("builder-a");
    first.permission_profile = PermissionProfile::Balanced;
    let first_packet =
        WorkPacket::builder_task("wp-a", "create ALPHA.txt").with_path_allow(["ALPHA.txt"]);
    let first_core = Arc::new(harness.core);
    let entered_wait = entered.notified();
    let first_task = {
        let core = first_core.clone();
        tokio::spawn(async move {
            core.spawn_from_packet(first, first_packet, Some("workspace-write".to_owned()))
                .await
        })
    };
    entered_wait.await;

    let mut second = trusted_context();
    second.session_id = kiana_domain::SessionId::new("builder-b");
    second.permission_profile = PermissionProfile::Balanced;
    let overlapping =
        WorkPacket::builder_task("wp-b", "also ALPHA.txt").with_path_allow(["ALPHA.txt"]);
    let blocked = first_core
        .spawn_from_packet(second, overlapping, Some("workspace-write".to_owned()))
        .await
        .unwrap();
    assert_eq!(blocked.status, ExecutionStatus::Blocked, "{blocked:?}");
    assert_eq!(blocked.error.as_deref(), Some("path_lock_conflict"));

    release.notify_one();
    let first_done = first_task.await.unwrap().unwrap();
    assert_eq!(
        first_done.status,
        ExecutionStatus::Completed,
        "{first_done:?}"
    );
}

#[tokio::test]
async fn independent_control_planes_share_builder_path_locks() {
    let root = temp_project();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let first = CoreHarness::with_runner(Arc::new(HoldingRunner {
        entered: entered.clone(),
        release: release.clone(),
    }));
    let second = CoreHarness::with_runner(Arc::new(UnavailableRunner));
    let first_core = Arc::new(first.core);
    let second_core = Arc::new(second.core);

    let first_context = trusted_context_in(&root, "builder-a");
    let first_packet = WorkPacket::builder_task("wp-a", "create ALPHA.txt")
        .with_path_allow(["ALPHA.txt"]);
    let entered_wait = entered.notified();
    let first_task = {
        let core = first_core.clone();
        tokio::spawn(async move {
            core.spawn_from_packet(
                first_context,
                first_packet,
                Some("workspace-write".to_owned()),
            )
            .await
        })
    };
    entered_wait.await;

    let second_context = trusted_context_in(&root, "builder-b");
    let second_packet = WorkPacket::builder_task("wp-b", "also ALPHA.txt")
        .with_path_allow(["ALPHA.txt"]);
    let blocked = second_core
        .spawn_from_packet(
            second_context,
            second_packet,
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status, ExecutionStatus::Blocked, "{blocked:?}");
    assert_eq!(blocked.error.as_deref(), Some("path_lock_conflict"));

    release.notify_one();
    let completed = first_task.await.unwrap().unwrap();
    assert_eq!(completed.status, ExecutionStatus::Completed, "{completed:?}");
}

struct CancellableRunner {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl RunnerPort for CancellableRunner {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        match command {
            RunnerCommand::Start { run_id, .. } => {
                self.entered.notify_one();
                self.release.notified().await;
                Ok(vec![
                    RunnerEvent::Started { run_id },
                    RunnerEvent::Failed {
                        run_id,
                        error: "cancelled:user".to_owned(),
                    },
                ])
            }
            RunnerCommand::Cancel { run_id, .. } => {
                self.release.notify_one();
                Ok(vec![RunnerEvent::Failed {
                    run_id,
                    error: "cancelled:user".to_owned(),
                }])
            }
            other => Ok(vec![RunnerEvent::Failed {
                run_id: other.run_id(),
                error: "unexpected_runner_command".to_owned(),
            }]),
        }
    }
}

#[tokio::test]
async fn cancelled_packet_transitions_cell_and_packet_to_cancelled() {
    let root = temp_project();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let harness = CoreHarness::with_runner(Arc::new(CancellableRunner {
        entered: entered.clone(),
        release: release.clone(),
    }));
    let events = harness.events.clone();
    let core = Arc::new(harness.core);
    let spawn_context = trusted_context_in(&root, "builder-cancel");
    let packet = WorkPacket::builder_task("wp-cancel", "cancel this packet")
        .with_path_allow(["ALPHA.txt"]);
    let entered_wait = entered.notified();
    let spawn_task = {
        let core = core.clone();
        tokio::spawn(async move {
            core.spawn_from_packet(spawn_context, packet, Some("workspace-write".to_owned()))
                .await
        })
    };
    entered_wait.await;

    let mut cancel_context = trusted_context_in(&root, "builder-cancel");
    cancel_context.request_id = RequestId::new();
    let cancelled = core
        .cancel_run(cancel_context, None, "user".to_owned())
        .await
        .unwrap();
    assert_eq!(cancelled.status, ExecutionStatus::Cancelled, "{cancelled:?}");

    let spawned = spawn_task.await.unwrap().unwrap();
    assert_eq!(spawned.status, ExecutionStatus::Cancelled, "{spawned:?}");
    let all_events = events.read_all().await.unwrap();
    let kinds: Vec<&str> = all_events.iter().map(|event| event.kind.as_str()).collect();
    assert!(kinds.contains(&"cell.cancel_requested"), "{kinds:?}");
    assert!(kinds.contains(&"cell.cancelled"), "{kinds:?}");
    assert!(kinds.contains(&"cell.retired"), "{kinds:?}");
    assert!(kinds.contains(&"packet.cancelled"), "{kinds:?}");
}

#[tokio::test]
async fn failed_cell_reservation_releases_builder_path_lock() {
    let root = temp_project();
    let harness = CoreHarness::with_runner(scripted_runner(json!([{"text": "second packet"}])));
    let first_core = Arc::new(harness.core);
    let mut expired = trusted_context_in(&root, "builder-expired");
    expired.permission_profile = PermissionProfile::Balanced;
    let mut expired_packet = WorkPacket::builder_task("wp-expired", "expired packet")
        .with_path_allow(["ALPHA.txt"]);
    expired_packet.deadline_unix_ms = Some(1);
    let rejected = first_core
        .spawn_from_packet(expired, expired_packet, Some("workspace-write".to_owned()))
        .await
        .unwrap();
    assert_eq!(rejected.status, ExecutionStatus::Blocked, "{rejected:?}");
    assert_eq!(rejected.error.as_deref(), Some("port_failed:spawn_deadline_expired"));

    let retry = first_core
        .spawn_from_packet(
            trusted_context_in(&root, "builder-retry"),
            WorkPacket::builder_task("wp-retry", "retry packet").with_path_allow(["ALPHA.txt"]),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(retry.status, ExecutionStatus::Completed, "{retry:?}");
}

#[tokio::test]
async fn spawn_packet_path_allow_fails_closed_outside_the_packet() {
    let broker = Arc::new(CountingBroker {
        calls: Mutex::new(0),
    });
    let harness = CoreHarness::with_runner_and_broker(
        scripted_runner(json!([
            {
                "text": "writing",
                "tool_calls": [{
                    "id": "c1",
                    "name": "apply_patch",
                    "arguments": {
                        "patch": "*** Begin Patch\n*** Add File: GOLDEN_PATH.txt\n+hello\n*** End Patch\n"
                    }
                }]
            },
            {"text": "created GOLDEN_PATH.txt"}
        ])),
        broker.clone(),
    );
    let mut worker = trusted_context();
    worker.session_id = kiana_domain::SessionId::new("builder-a");
    worker.permission_profile = PermissionProfile::Balanced;
    let denied = harness
        .core
        .spawn_from_packet(
            worker,
            WorkPacket::builder_task("wp-a", "create GOLDEN_PATH.txt containing hello")
                .with_path_allow(["ALPHA.txt"]),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(denied.status, ExecutionStatus::Failed, "{denied:?}");
    assert_eq!(denied.error.as_deref(), Some("packet_path_denied"));
    assert_eq!(*broker.calls.lock().await, 0);
}

#[tokio::test]
async fn sequential_empty_packets_still_spawn() {
    let harness = CoreHarness::with_runner(scripted_runner(json!([
        {"text": "one"},
        {"text": "two"}
    ])));
    let mut first = trusted_context();
    first.session_id = kiana_domain::SessionId::new("builder-a");
    first.permission_profile = PermissionProfile::Balanced;
    let first_done = harness
        .core
        .spawn_from_packet(
            first,
            WorkPacket::builder_task("wp-a", "first packet"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        first_done.status,
        ExecutionStatus::Completed,
        "{first_done:?}"
    );

    let mut second = trusted_context();
    second.session_id = kiana_domain::SessionId::new("builder-b");
    second.permission_profile = PermissionProfile::Balanced;
    let second_done = harness
        .core
        .spawn_from_packet(
            second,
            WorkPacket::builder_task("wp-b", "second packet"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        second_done.status,
        ExecutionStatus::Completed,
        "{second_done:?}"
    );
}

#[tokio::test]
async fn unavailable_harness_fails_without_completed_run() {
    let harness = CoreHarness::new();
    let context = trusted_context();
    let request_id = context.request_id;
    let response = harness
        .core
        .start_run(context, "hello".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Failed);
    assert_eq!(
        response.error.as_deref(),
        Some("port_unavailable:runner_unavailable")
    );
    let kinds = harness
        .events
        .read_request(&request_id)
        .await
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"run.failed".to_owned()));
    assert!(!kinds.iter().any(|kind| kind == "run.completed"));
}

#[tokio::test]
async fn continue_run_reuses_the_same_run_id() {
    let harness = CoreHarness::with_runner(scripted_runner(json!([
        {"text": "first turn"},
        {"text": "continued"}
    ])));
    let context = trusted_context();
    let started = harness
        .core
        .start_run(context.clone(), "hello".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(started.status, ExecutionStatus::Completed);
    let run_id = started.output["run_id"].clone();
    let continued = harness
        .core
        .continue_run(trusted_context(), "keep going".to_owned(), None, None)
        .await
        .unwrap();
    assert_eq!(
        continued.status,
        ExecutionStatus::Completed,
        "{continued:?}"
    );
    assert_eq!(continued.output["run_id"], run_id);
    assert_eq!(continued.output["output"]["text"], "continued");
    assert_eq!(continued.output["session_id"], "session-1");
}

#[tokio::test]
async fn continue_unknown_session_fails_closed() {
    let harness = CoreHarness::with_runner(scripted_runner(json!([{"text": "should not run"}])));
    let continued = harness
        .core
        .continue_run(trusted_context(), "keep going".to_owned(), None, None)
        .await
        .unwrap();
    assert_eq!(continued.status, ExecutionStatus::Blocked);
    assert_eq!(continued.error.as_deref(), Some("session_not_found"));
}

#[tokio::test]
async fn cancel_unknown_run_fails_closed() {
    let harness = CoreHarness::with_runner(scripted_runner(json!([{"text": "should not run"}])));
    let cancelled = harness
        .core
        .cancel_run(trusted_context(), None, "user".to_owned())
        .await
        .unwrap();
    assert_eq!(cancelled.status, ExecutionStatus::Blocked);
    assert_eq!(cancelled.error.as_deref(), Some("session_not_found"));
}

#[tokio::test]
async fn cancel_after_awaiting_approval_does_not_resume_the_pending_invocation() {
    let broker = Arc::new(CountingBroker {
        calls: Mutex::new(0),
    });
    let harness = CoreHarness::with_runner_and_broker(
        scripted_runner(json!([
            {
                "text": "writing",
                "tool_calls": [{
                    "id": "c1",
                    "name": "apply_patch",
                    "arguments": {
                        "patch": r#"*** Begin Patch
*** Add File: GOLDEN_PATH.txt
+hello
*** End Patch
"#
                    }
                }]
            },
            {"text": "created GOLDEN_PATH.txt"}
        ])),
        broker.clone(),
    );
    let awaiting = harness
        .core
        .start_run(
            trusted_context(),
            "create GOLDEN_PATH.txt containing hello".to_owned(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        awaiting.status,
        ExecutionStatus::AwaitingApproval,
        "{awaiting:?}"
    );
    assert_eq!(awaiting.error.as_deref(), Some("approval_required"));
    let challenge: ApprovalChallenge =
        serde_json::from_value(awaiting.output["approval"].clone()).unwrap();
    assert_eq!(*broker.calls.lock().await, 0);

    let cancelled = harness
        .core
        .cancel_run(trusted_context(), None, "user".to_owned())
        .await
        .unwrap();
    assert_eq!(cancelled.status, ExecutionStatus::Cancelled, "{cancelled:?}");
    assert!(
        cancelled
            .error
            .as_deref()
            .unwrap_or_default()
            .starts_with("cancelled:"),
        "{cancelled:?}"
    );

    let resumed = harness
        .core
        .decide_approval(
            &trusted_context(),
            challenge.approval_id,
            ApprovalDecision::Approve,
        )
        .await
        .unwrap();
    assert_eq!(resumed.status, ExecutionStatus::Blocked, "{resumed:?}");
    assert!(
        resumed
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("approval_already_consumed"),
        "{resumed:?}"
    );
    assert_eq!(*broker.calls.lock().await, 0);
}

fn trusted_write_pm(root: &std::path::Path) -> RequestContext {
    trusted_write_role(root, &kiana_domain::RoleSpec::pm())
}

fn trusted_write_role(root: &std::path::Path, role: &RoleSpec) -> RequestContext {
    let mut context = RequestContext::local("chair-1", root.to_string_lossy());
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context.assign_role(role);
    context
}

#[tokio::test]
async fn anti_meeting_writes_artifacts_without_runner() {
    let harness = CoreHarness::new();
    let root = temp_project();
    let convened = harness
        .core
        .convene_symposium(
            trusted_write_pm(&root),
            "create GOLDEN_PATH.txt containing hello".to_owned(),
            true,
            Some(4),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Completed, "{convened:?}");
    assert_eq!(convened.output["schema"], "kiana.symposium-result.v1");
    assert_eq!(convened.output["builder_present"], false);
    assert_eq!(convened.output["skipped_meeting"], true);
    assert_eq!(convened.output["packet"]["assignee_role"], "builder");
    assert_eq!(convened.output["decision"]["skipped_meeting"], true);
    let decision = std::fs::read_to_string(root.join("plan/DECISION.json")).unwrap();
    let packet = std::fs::read_to_string(root.join("packet/TASK.json")).unwrap();
    assert!(decision.contains("kiana.decision-record.v1"), "{decision}");
    assert!(packet.contains("kiana.work-packet.v1"), "{packet}");
    assert!(
        packet.contains("create GOLDEN_PATH.txt containing hello"),
        "{packet}"
    );
}

#[tokio::test]
async fn convene_two_rounds_uses_private_speaker_sessions() {
    let harness = CoreHarness::with_runner(scripted_runner(json!([
        {"text": "choose the vertical slice"},
        {"text": "agree with one slice"},
        {"text": "keep the slice"},
        {"text": "still agree"}
    ])));
    let root = temp_project();
    let context = trusted_write_pm(&root);
    let request_id = context.request_id.to_string();
    let convened = harness
        .core
        .convene_symposium(
            context,
            "one vertical slice vs two packets".to_owned(),
            false,
            Some(2),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Completed, "{convened:?}");
    assert_eq!(convened.output["builder_present"], false);
    assert_eq!(convened.output["skipped_meeting"], false);
    let sessions = convened.output["speaker_sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "{sessions:?}");
    assert_eq!(sessions[0]["role_id"], "pm");
    assert_eq!(sessions[1]["role_id"], "architect");
    assert_eq!(sessions[0]["session_id"], format!("{request_id}-pm"));
    assert_eq!(sessions[1]["session_id"], format!("{request_id}-architect"));
    assert_ne!(sessions[0]["session_id"], sessions[1]["session_id"]);
    assert_ne!(sessions[0]["session_id"], "chair-1");
    let claims = convened.output["blackboard"]["claims"].as_array().unwrap();
    assert!(
        claims
            .iter()
            .any(|claim| claim["speaker"] == "pm" && claim["text"] == "choose the vertical slice"),
        "{claims:?}"
    );
    assert!(
        claims.iter().any(|claim| claim["speaker"] == "architect"
            && claim["text"] == "agree with one slice"),
        "{claims:?}"
    );
    assert!(root.join("plan/DECISION.json").exists());
    assert!(root.join("packet/TASK.json").exists());
}

#[tokio::test]
async fn symposium_architect_chair_fails_closed() {
    let harness = CoreHarness::new();
    let root = temp_project();
    let convened = harness
        .core
        .convene_symposium(
            trusted_write_role(&root, &RoleSpec::architect()),
            "create GOLDEN_PATH.txt containing hello".to_owned(),
            true,
            Some(4),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Blocked, "{convened:?}");
    assert_eq!(
        convened.error.as_deref(),
        Some("symposium_chair_must_be_pm")
    );
    assert!(!root.join("plan/DECISION.json").exists());
}

#[tokio::test]
async fn symposium_builder_chair_writes_executing_decision() {
    let harness = CoreHarness::new();
    let root = temp_project();
    let convened = harness
        .core
        .convene_symposium(
            trusted_write_role(&root, &RoleSpec::builder()),
            "create GOLDEN_PATH.txt containing hello".to_owned(),
            true,
            Some(4),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Completed, "{convened:?}");
    assert_eq!(convened.output["department_id"], "executing");
    assert_eq!(convened.output["chair"], "builder");
    assert_eq!(convened.output["builder_present"], true);
    assert_eq!(convened.output["packet"], json!(null));
    assert!(root.join("receipt/DECISION.json").exists());
    assert!(!root.join("plan/DECISION.json").exists());
    assert!(!root.join("packet/TASK.json").exists());
}

#[tokio::test]
async fn symposium_empty_goal_fails_closed() {
    let harness = CoreHarness::new();
    let root = temp_project();
    let convened = harness
        .core
        .convene_symposium(
            trusted_write_pm(&root),
            "   ".to_owned(),
            true,
            Some(4),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Blocked, "{convened:?}");
    assert_eq!(convened.error.as_deref(), Some("symposium_goal_required"));
}

#[tokio::test]
async fn symposium_max_rounds_zero_fails_closed() {
    let harness = CoreHarness::new();
    let root = temp_project();
    let convened = harness
        .core
        .convene_symposium(
            trusted_write_pm(&root),
            "create GOLDEN_PATH.txt containing hello".to_owned(),
            true,
            Some(0),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(convened.status, ExecutionStatus::Blocked, "{convened:?}");
    assert_eq!(
        convened.error.as_deref(),
        Some("symposium_max_rounds_invalid")
    );
}

#[tokio::test]
async fn each_department_anti_meeting_writes_its_own_artifact() {
    let cases = [
        (
            RoleSpec::sponsor(),
            "initiating",
            "charter/DECISION.json",
            false,
            false,
        ),
        (
            RoleSpec::pm(),
            "planning",
            "plan/DECISION.json",
            true,
            false,
        ),
        (
            RoleSpec::builder(),
            "executing",
            "receipt/DECISION.json",
            false,
            true,
        ),
        (
            RoleSpec::reviewer(),
            "monitoring",
            "gate/DECISION.json",
            false,
            false,
        ),
        (
            RoleSpec::closer(),
            "closing",
            "lessons/DECISION.json",
            false,
            false,
        ),
    ];
    for (role, department, decision_path, emits_packet, builder_present) in cases {
        let harness = CoreHarness::new();
        let root = temp_project();
        let convened = harness
            .core
            .convene_symposium(
                trusted_write_role(&root, &role),
                "decide the next slice".to_owned(),
                true,
                Some(4),
                Some("workspace-write".to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(
            convened.status,
            ExecutionStatus::Completed,
            "{department} {convened:?}"
        );
        assert_eq!(convened.output["department_id"], department);
        assert_eq!(convened.output["builder_present"], builder_present);
        assert_eq!(convened.output["skipped_meeting"], true);
        assert_eq!(convened.output["decision_path"], decision_path);
        assert!(root.join(decision_path).exists(), "{department}");
        assert_eq!(root.join("packet/TASK.json").exists(), emits_packet);
        if department != "planning" {
            assert!(!root.join("plan/DECISION.json").exists(), "{department}");
            assert_eq!(convened.output["packet"], json!(null));
        } else {
            assert_eq!(convened.output["packet"]["assignee_role"], "builder");
        }
    }
}

#[tokio::test]
async fn review_author_run_uses_a_fresh_reviewer_session() {
    let root = temp_project();
    let harness = CoreHarness::with_runner(scripted_runner(json!([
        {"text": "created GOLDEN_PATH.txt"}
    ])));
    let builder = trusted_context_in(&root, "builder-1");
    let built = harness
        .core
        .start_run(builder, "create GOLDEN_PATH.txt".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(built.status, ExecutionStatus::Completed, "{built:?}");
    assert_eq!(built.output["role_id"], "builder");

    let mut reviewer = trusted_context_in(&root, "reviewer-1");
    reviewer.request_id = RequestId::new();
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = harness
        .core
        .review_author_run(reviewer, "builder-1".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Completed, "{reviewed:?}");
    assert_eq!(reviewed.output["schema"], "kiana.review-result.v1");
    assert_eq!(reviewed.output["role_id"], "reviewer");
    assert_eq!(reviewed.output["department_id"], "monitoring");
    assert_eq!(reviewed.output["session_id"], "reviewer-1");
    assert_eq!(reviewed.output["author_session_id"], "builder-1");
    assert_ne!(reviewed.output["session_id"], built.output["session_id"]);
    assert_eq!(reviewed.output["input"], "review");
    let packet = fs::read_to_string(root.join("gate").join("REVIEW.json")).unwrap();
    assert!(packet.contains("kiana.review-packet.v1"), "{packet}");
    assert!(packet.contains("builder-1"), "{packet}");
    assert!(packet.contains("reviewer-1"), "{packet}");
}

#[tokio::test]
async fn review_reuses_of_author_session_fail_closed() {
    let root = temp_project();
    let harness = CoreHarness::with_runner(scripted_runner(json!([{"text": "done"}])));
    let builder = trusted_context_in(&root, "builder-1");
    let built = harness
        .core
        .start_run(builder, "hello".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(built.status, ExecutionStatus::Completed);

    let mut reviewer = trusted_context_in(&root, "builder-1");
    reviewer.request_id = RequestId::new();
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = harness
        .core
        .review_author_run(reviewer, "builder-1".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Blocked, "{reviewed:?}");
    assert_eq!(
        reviewed.error.as_deref(),
        Some("review_author_session_denied")
    );
    assert!(!root.join("gate").join("REVIEW.json").exists());
}

#[tokio::test]
async fn review_builder_chair_fails_closed() {
    let root = temp_project();
    let harness = CoreHarness::with_runner(scripted_runner(json!([{"text": "done"}])));
    let builder = trusted_context_in(&root, "builder-1");
    let built = harness
        .core
        .start_run(builder, "hello".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(built.status, ExecutionStatus::Completed);

    let mut reviewer = trusted_context_in(&root, "reviewer-1");
    reviewer.request_id = RequestId::new();
    reviewer.assign_role(&RoleSpec::builder());
    let reviewed = harness
        .core
        .review_author_run(reviewer, "builder-1".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Blocked, "{reviewed:?}");
    assert_eq!(
        reviewed.error.as_deref(),
        Some("review_role_must_be_reviewer")
    );
}

#[tokio::test]
async fn review_author_from_another_project_fails_closed() {
    let author_root = temp_project();
    let reviewer_root = temp_project();
    let harness = CoreHarness::with_runner(scripted_runner(json!([{"text": "done"}])));

    let built = harness
        .core
        .start_run(
            trusted_context_in(&author_root, "builder-1"),
            "hello".to_owned(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(built.status, ExecutionStatus::Completed);

    let mut reviewer = trusted_context_in(&reviewer_root, "reviewer-1");
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = harness
        .core
        .review_author_run(reviewer, "builder-1".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Blocked, "{reviewed:?}");
    assert_eq!(reviewed.error.as_deref(), Some("review_author_not_found"));
    assert!(!reviewer_root.join("gate").join("REVIEW.json").exists());
}

#[tokio::test]
async fn review_author_from_another_actor_fails_closed() {
    let root = temp_project();
    let harness = CoreHarness::with_runner(scripted_runner(json!([{"text": "done"}])));

    let mut author = trusted_context_in(&root, "builder-1");
    author.actor_id = Some("author".to_owned());
    let built = harness
        .core
        .start_run(author, "hello".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(built.status, ExecutionStatus::Completed);

    let mut reviewer = trusted_context_in(&root, "reviewer-1");
    reviewer.actor_id = Some("different-actor".to_owned());
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = harness
        .core
        .review_author_run(reviewer, "builder-1".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Blocked, "{reviewed:?}");
    assert_eq!(reviewed.error.as_deref(), Some("review_author_not_found"));
}

#[tokio::test]
async fn review_explicit_run_id_must_match_author_session() {
    let root = temp_project();
    let harness = CoreHarness::with_runner(scripted_runner(json!([{"text": "done"}])));
    let built = harness
        .core
        .start_run(
            trusted_context_in(&root, "builder-1"),
            "hello".to_owned(),
            None,
        )
        .await
        .unwrap();
    let run_id: RunId = serde_json::from_value(built.output["run_id"].clone()).unwrap();

    let mut reviewer = trusted_context_in(&root, "reviewer-1");
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = harness
        .core
        .review_author_run(reviewer, "not-the-author".to_owned(), Some(run_id))
        .await
        .unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Blocked, "{reviewed:?}");
    assert_eq!(reviewed.error.as_deref(), Some("review_author_not_found"));
}
#[tokio::test]
async fn review_unknown_author_fails_closed() {
    let root = temp_project();
    let harness = CoreHarness::new();
    let mut reviewer = trusted_context_in(&root, "reviewer-1");
    reviewer.assign_role(&RoleSpec::reviewer());
    let reviewed = harness
        .core
        .review_author_run(reviewer, "missing-author".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(reviewed.status, ExecutionStatus::Blocked, "{reviewed:?}");
    assert_eq!(reviewed.error.as_deref(), Some("review_author_not_found"));
}
