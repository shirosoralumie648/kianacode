//! CP-15 acceptance fixtures: cancel vs approval/dispatch race and queued-invocation terminals.
//!
//! These fixtures drive the single production execution spine
//! (`ControlPlane::start_run` → approval gate → `cancel_run`/`decide_approval`) with a scripted
//! Runner, the real `MemoryEventLog` and an in-memory approval store. They assert only facts
//! committed to the event ledger: a cancellation request never fabricates a stop, and dispatch
//! winning the race is never rewritten as a "not executed" result.

use async_trait::async_trait;
use kiana_core::{ControlPlane, RunOutcome};
use kiana_domain::{
    AggregateVersion, ApprovalChallenge, ApprovalDecision, ApprovalDecisionRecord, ApprovalId,
    ApprovalState, AuthorizedCapabilityRequest, CapabilityRequest, CapabilityResult,
    ExecutionStatus, PendingApproval, PermissionProfile, PolicyDecision,
    PreparedApprovalConsumption, RequestContext, RequestId, RunId, RuntimeEvent,
    APPROVAL_CHALLENGE_SCHEMA,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::{GateDecision, GateEngine};
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, CapabilityBrokerPort, EventStorePort, PortError};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const TERMINAL_KINDS: [&str; 4] = [
    "run.completed",
    "run.failed",
    "run.cancelled",
    "run.result_unknown",
];

/// Every committed terminal run fact, in stream order.
fn terminal_kinds(events: &[RuntimeEvent]) -> Vec<&str> {
    events
        .iter()
        .map(|event| event.kind.as_str())
        .filter(|kind| TERMINAL_KINDS.contains(kind))
        .collect()
}

/// Invocation ids that received a committed `not_executed` terminal tool result.
fn not_executed_call_ids(events: &[RuntimeEvent]) -> Vec<String> {
    let mut ids = events
        .iter()
        .filter(|event| {
            event.kind == "run.tool_result"
                && event.data.get("not_executed") == Some(&json!(true))
        })
        .filter_map(|event| event.data.get("call_id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn patch_call(call_id: &str, file: &str) -> Value {
    json!({
        "id": call_id,
        "name": "apply_patch",
        "arguments": {
            "patch": format!("*** Begin Patch\n*** Add File: {file}\n+hello\n*** End Patch\n"),
        },
    })
}

fn single_patch_model(file: &str) -> Value {
    json!([
        {
            "text": "writing",
            "tool_calls": [patch_call("c1", file)],
        },
        { "text": format!("created {file}") },
    ])
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

struct ApprovalGateEngine;

impl GateEngine for ApprovalGateEngine {
    fn evaluate(&self, _policy: &PolicyDecision) -> GateDecision {
        GateDecision::AwaitingApproval {
            reason: "test_approval_required".to_owned(),
        }
    }
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

// Install facts through the production authority/session APIs before testing downstream behavior.
// This keeps the fixture's existing role and grants, including the denied memory-write boundary.
async fn bind_test_runtime_authority(core: &ControlPlane, context: &RequestContext) {
    core.synchronize_authority(context, "control-plane-runtime-fixture.v1")
        .await
        .unwrap();
    core.bind_session_assignment(context).await.unwrap();
}

#[derive(Default)]
struct TestApprovalStore {
    pending: Mutex<Option<(PendingApproval, bool, bool)>>,
    decision: Mutex<Option<(ApprovalDecision, RequestId)>>,
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
            risk: request.risk,
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
        *self.decision.lock().await = None;
        Ok(challenge)
    }

    async fn prepare_activation(
        &self,
        approval_id: ApprovalId,
        command_id: RequestId,
    ) -> Result<PreparedApprovalConsumption, PortError> {
        let mut pending = self.pending.lock().await;
        let Some((pending, active, consumed)) = pending.as_mut() else {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        };
        if pending.challenge.approval_id != approval_id || *consumed || *active {
            return Err(PortError::Failed("approval_not_staged".to_owned()));
        }
        *active = true;
        let event = RuntimeEvent::new(
            command_id,
            1,
            "approval.activated",
            json!({
                "schema": "kiana.approval.v1",
                "approval_id": approval_id,
                "previous_state": ApprovalState::Staged,
                "state": ApprovalState::Active,
                "request_hash": pending.challenge.request_hash,
                "at_unix_ms": 1,
                "activation_command_id": command_id,
            }),
        )
        .map_err(|error| PortError::Failed(error.to_string()))?
        .with_stream_metadata("approval", approval_id.to_string(), 1);
        Ok(PreparedApprovalConsumption {
            expected_version: AggregateVersion::new("approval", approval_id.to_string(), 0),
            authority_versions: Vec::new(),
            event,
            pending: pending.clone(),
        })
    }

    async fn read_decision(
        &self,
        _context: &RequestContext,
        approval_id: ApprovalId,
    ) -> Result<ApprovalDecisionRecord, PortError> {
        let pending = self.pending.lock().await;
        let Some((pending, active, consumed)) = pending.as_ref() else {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        };
        if pending.challenge.approval_id != approval_id || !*active {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        }
        let decisions = self.decision.lock().await;
        let recorded = decisions.as_ref();
        Ok(ApprovalDecisionRecord {
            approval_id,
            state: if *consumed {
                ApprovalState::Consumed
            } else if recorded.is_some_and(|(decision, _)| *decision == ApprovalDecision::Approve) {
                ApprovalState::Approved
            } else if recorded.is_some() {
                ApprovalState::Denied
            } else {
                ApprovalState::Active
            },
            challenge: pending.challenge.clone(),
            decision: recorded.map(|(decision, _)| decision.clone()),
            decision_command_id: recorded.map(|(_, command_id)| *command_id),
            decided_by: recorded.map(|_| "test-approver".to_owned()),
            dispatch_command_id: None,
            payload_available: true,
            version: if *consumed {
                3
            } else if recorded.is_some() {
                2
            } else {
                1
            },
            decision_digest: None,
            consumption_digest: None,
        })
    }

    async fn decide_with_proof_and_version(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<&str>,
        nonce: Option<&str>,
        expected_version: Option<u64>,
    ) -> Result<PendingApproval, PortError> {
        if expected_version.is_some() {
            return Err(PortError::Unavailable(
                "approval_expected_version_unsupported".to_owned(),
            ));
        }
        self.validate_with_proof(context, approval_id, request_hash, nonce)
            .await?;
        let pending = self
            .pending
            .lock()
            .await
            .as_ref()
            .map(|(pending, _, _)| pending.clone())
            .ok_or_else(|| PortError::Failed("approval_not_found".to_owned()))?;
        *self.decision.lock().await = Some((decision, context.request_id));
        Ok(pending)
    }

    async fn prepare_consumption(
        &self,
        _context: &RequestContext,
        approval_id: ApprovalId,
        dispatch_command_id: RequestId,
    ) -> Result<PreparedApprovalConsumption, PortError> {
        let mut pending = self.pending.lock().await;
        let Some((pending, active, consumed)) = pending.as_mut() else {
            return Err(PortError::Failed("approval_not_found".to_owned()));
        };
        let decision = self.decision.lock().await;
        if pending.challenge.approval_id != approval_id || !*active || *consumed {
            return Err(PortError::Failed("approval_not_approved".to_owned()));
        }
        if !decision.is_some_and(|(value, _)| value == ApprovalDecision::Approve) {
            return Err(PortError::Failed("approval_not_approved".to_owned()));
        }
        *consumed = true;
        let decision_command_id = decision
            .as_ref()
            .map(|(_, command_id)| *command_id)
            .ok_or_else(|| PortError::Failed("approval_decision_missing".to_owned()))?;
        let event = RuntimeEvent::new(
            dispatch_command_id,
            2,
            "approval.consumed",
            json!({
                "schema": "kiana.approval.v1",
                "approval_id": approval_id,
                "previous_state": ApprovalState::Approved,
                "state": ApprovalState::Consumed,
                "request_hash": pending.challenge.request_hash,
                "at_unix_ms": 2,
                "dispatch_command_id": dispatch_command_id,
                "decision_command_id": decision_command_id,
                "decided_by": "test-approver",
                "consumption_fact": {"schema": "test.approval-consumption.v1"},
            }),
        )
        .map_err(|error| PortError::Failed(error.to_string()))?
        .with_stream_metadata("approval", approval_id.to_string(), 2);
        Ok(PreparedApprovalConsumption {
            expected_version: AggregateVersion::new("approval", approval_id.to_string(), 1),
            authority_versions: Vec::new(),
            event,
            pending: pending.clone(),
        })
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

    async fn validate_with_proof(
        &self,
        _context: &RequestContext,
        approval_id: ApprovalId,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<(), PortError> {
        let pending = self.pending.lock().await;
        let Some((pending, active, consumed)) = pending.as_ref() else {
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
        if request_hash.is_some() != nonce.is_some() {
            return Err(PortError::Failed("approval_proof_incomplete".to_owned()));
        }
        if let Some(request_hash) = request_hash {
            if request_hash != pending.challenge.request_hash {
                return Err(PortError::Failed(
                    "approval_request_hash_mismatch".to_owned(),
                ));
            }
        }
        if let Some(nonce) = nonce {
            if nonce != pending.challenge.nonce {
                return Err(PortError::Failed("approval_nonce_mismatch".to_owned()));
            }
        }
        Ok(())
    }

    async fn pending_with_proof(
        &self,
        _context: &RequestContext,
        approval_id: ApprovalId,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        self.validate_with_proof(_context, approval_id, request_hash, nonce)
            .await?;
        self.pending
            .lock()
            .await
            .as_ref()
            .map(|(pending, _, _)| pending.clone())
            .ok_or_else(|| PortError::Failed("approval_not_found".to_owned()))
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

#[tokio::test]
async fn cp_cancel_approval_dispatch_race_has_one_legal_outcome() {
    // Interleaving A — cancel commits the durable intent before the approval is consumed:
    // there is no new dispatch, the pending invocation is terminal `not_executed`, and the
    // run has exactly one terminal fact (`run.cancelled`).
    {
        let root = temp_project();
        let broker = Arc::new(CountingBroker {
            calls: Mutex::new(0),
        });
        let events = Arc::new(MemoryEventLog::new());
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(ApprovalGateEngine),
            events.clone(),
            broker.clone(),
            Arc::new(TestApprovalStore::default()),
            scripted_runner(single_patch_model("RACE_CANCEL_FIRST.txt")),
        );
        let mut context = trusted_context_in(&root, "cp15-cancel-first");
        bind_test_runtime_authority(&core, &context).await;

        let awaiting = core
            .start_run(context.clone(), "race cancel first".to_owned(), None)
            .await
            .unwrap();
        assert_eq!(
            awaiting.status,
            ExecutionStatus::AwaitingApproval,
            "{awaiting:?}"
        );
        let challenge: ApprovalChallenge =
            serde_json::from_value(awaiting.output["approval"].clone()).unwrap();
        let run_id = RunId::parse_str(awaiting.output["run_id"].as_str().unwrap()).unwrap();

        let mut cancel_context = context.clone();
        cancel_context.request_id = RequestId::new();
        let cancelled = core
            .cancel_run(cancel_context, Some(run_id), "user".to_owned())
            .await
            .unwrap();
        assert_eq!(cancelled.status, ExecutionStatus::Cancelled, "{cancelled:?}");

        // A late approval cannot resurrect the invocation once cancel won the read-set.
        let mut decision_context = context.clone();
        decision_context.request_id = RequestId::new();
        let late_decision = core
            .decide_approval(
                &decision_context,
                challenge.approval_id,
                ApprovalDecision::Approve,
            )
            .await
            .unwrap();
        assert_eq!(
            late_decision.status,
            ExecutionStatus::Blocked,
            "{late_decision:?}"
        );

        assert_eq!(*broker.calls.lock().await, 0);
        let stream = events
            .read_stream("run", &run_id.to_string())
            .await
            .unwrap();
        assert!(stream.iter().any(|event| event.kind == "run.cancelling"));
        assert_eq!(terminal_kinds(&stream), vec!["run.cancelled"]);
        assert_eq!(not_executed_call_ids(&stream), vec!["c1"]);
        let _ = fs::remove_dir_all(root);
    }

    // Interleaving B — the approval/dispatch commits first: the invocation really executed
    // (`broker.calls == 1`), the run reaches exactly one non-cancel terminal, and a later
    // cancel degrades to an idempotent terminal read without opening a stop flow.
    {
        let root = temp_project();
        let broker = Arc::new(CountingBroker {
            calls: Mutex::new(0),
        });
        let events = Arc::new(MemoryEventLog::new());
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(ApprovalGateEngine),
            events.clone(),
            broker.clone(),
            Arc::new(TestApprovalStore::default()),
            scripted_runner(single_patch_model("RACE_DISPATCH_FIRST.txt")),
        );
        let mut context = trusted_context_in(&root, "cp15-dispatch-first");
        bind_test_runtime_authority(&core, &context).await;

        let awaiting = core
            .start_run(context.clone(), "race dispatch first".to_owned(), None)
            .await
            .unwrap();
        assert_eq!(
            awaiting.status,
            ExecutionStatus::AwaitingApproval,
            "{awaiting:?}"
        );
        let challenge: ApprovalChallenge =
            serde_json::from_value(awaiting.output["approval"].clone()).unwrap();
        let run_id = RunId::parse_str(awaiting.output["run_id"].as_str().unwrap()).unwrap();

        let mut decision_context = context.clone();
        decision_context.request_id = RequestId::new();
        let dispatched = core
            .decide_approval(
                &decision_context,
                challenge.approval_id,
                ApprovalDecision::Approve,
            )
            .await
            .unwrap();
        assert_ne!(
            dispatched.status,
            ExecutionStatus::AwaitingApproval,
            "{dispatched:?}"
        );
        assert_eq!(*broker.calls.lock().await, 1);
        assert_eq!(
            core.run_state(run_id).await.unwrap().outcome,
            Some(RunOutcome::Completed)
        );

        let before_cancel = events
            .read_stream("run", &run_id.to_string())
            .await
            .unwrap();
        assert!(!before_cancel
            .iter()
            .any(|event| event.kind == "run.cancelling"));
        assert_eq!(terminal_kinds(&before_cancel), vec!["run.completed"]);
        // The dispatched invocation is never rewritten as "not executed".
        assert!(not_executed_call_ids(&before_cancel).is_empty());

        let mut cancel_context = context.clone();
        cancel_context.request_id = RequestId::new();
        let already_terminal = core
            .cancel_run(cancel_context, Some(run_id), "user".to_owned())
            .await
            .unwrap();
        assert_eq!(
            already_terminal.status,
            ExecutionStatus::Completed,
            "{already_terminal:?}"
        );
        assert_eq!(already_terminal.output["already_terminal"], json!(true));
        assert_eq!(*broker.calls.lock().await, 1);
        assert_eq!(
            events
                .read_stream("run", &run_id.to_string())
                .await
                .unwrap(),
            before_cancel
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[tokio::test]
async fn cp_every_queued_invocation_gets_a_terminal_result() {
    // One model step queues three writes. The first is handed to the approval gate; the other
    // two stay queued in the Runner. Cancelling must close all three with a committed
    // `not_executed` terminal result and exactly one run terminal fact.
    let root = temp_project();
    let broker = Arc::new(CountingBroker {
        calls: Mutex::new(0),
    });
    let events = Arc::new(MemoryEventLog::new());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(ApprovalGateEngine),
        events.clone(),
        broker.clone(),
        Arc::new(TestApprovalStore::default()),
        scripted_runner(json!([
            {
                "text": "batch",
                "tool_calls": [
                    patch_call("c1", "QUEUED_A.txt"),
                    patch_call("c2", "QUEUED_B.txt"),
                    patch_call("c3", "QUEUED_C.txt"),
                ],
            },
            { "text": "batch aborted" },
        ])),
    );
    let mut context = trusted_context_in(&root, "cp15-queued");
    bind_test_runtime_authority(&core, &context).await;

    let awaiting = core
        .start_run(context.clone(), "queue three writes".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(
        awaiting.status,
        ExecutionStatus::AwaitingApproval,
        "{awaiting:?}"
    );
    let run_id = RunId::parse_str(awaiting.output["run_id"].as_str().unwrap()).unwrap();

    let mut cancel_context = context.clone();
    cancel_context.request_id = RequestId::new();
    let cancelled = core
        .cancel_run(cancel_context, Some(run_id), "user".to_owned())
        .await
        .unwrap();
    assert_eq!(cancelled.status, ExecutionStatus::Cancelled, "{cancelled:?}");
    assert_eq!(*broker.calls.lock().await, 0);

    let stream = events
        .read_stream("run", &run_id.to_string())
        .await
        .unwrap();
    assert!(stream.iter().any(|event| event.kind == "run.cancelling"));
    assert_eq!(not_executed_call_ids(&stream), vec!["c1", "c2", "c3"]);
    assert_eq!(terminal_kinds(&stream), vec!["run.cancelled"]);
    assert!(!stream
        .iter()
        .any(|event| event.kind == "capability.completed"));
    let _ = fs::remove_dir_all(root);
}
