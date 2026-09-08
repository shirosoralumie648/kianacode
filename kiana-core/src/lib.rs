//! The single command and capability control plane for Kiana.

mod cell_registry;

use cell_registry::MemoryCellRegistry;
use kiana_domain::{
    builder_lock_paths, path_locks_conflict, ApprovalDecision, ApprovalId,
    AuthorizedCapabilityRequest, BudgetLease, CapabilityGrant, CapabilityGrantId, CapabilityKind,
    CapabilityRequest, CapabilityResult, CellId, CellLifecycle, CellSpec, ClosingReceipt,
    CommandIntent, CoreResponse, DecisionRecord, ExecutionStatus, GateDecision, MergeReceipt,
    PendingInvocation, PermissionProfile, PolicyDecision, RequestContext, RequestId, ReviewPacket,
    RiskLevel, RoleSpec, RunId, RuntimeEvent, SpawnPlan, SpawnPlanId, SpawnPlanStatus,
    SupervisionLease, SupervisionLeaseId, Symposium, SymposiumClaim, WorkFingerprint, WorkPacket,
    CAPABILITY_GRANT_SCHEMA, CELL_SCHEMA, DEPARTMENT_EXECUTING, DEPARTMENT_PLANNING,
    MEMORY_SEARCH_SCHEMA, REVIEW_PACKET_PATH, REVIEW_RESULT_SCHEMA, ROLE_BUILDER, ROLE_CLOSER,
    ROLE_REVIEWER, SPAWN_PLAN_SCHEMA, SUPERVISION_LEASE_SCHEMA, SYMPOSIUM_RESULT_SCHEMA,
    WORK_PACKET_PATH,
};
use kiana_gates::GateEngine;
use kiana_policy::{capability_risk_violation, PolicyEngine};
use kiana_ports::{
    AllowAllPreToolHooks, ApprovalStorePort, CapabilityBrokerPort, CapabilityLease,
    CapabilityOutcome, EventStorePort, PortError, PreToolHookDecision, PreToolHookPort, RunnerPort,
    SpawnReservationRequest,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "linux")]
use std::ffi::{CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

struct PathLockLease {
    _file: File,
}

struct BuilderPathLockGuard<'a> {
    control_plane: &'a ControlPlane,
    project_root: String,
    session_id: String,
}

impl Drop for BuilderPathLockGuard<'_> {
    fn drop(&mut self) {
        self.control_plane
            .release_builder_path_locks(&self.project_root, &self.session_id);
    }
}

pub const LEGACY_EDGES_REMAINING: usize = 9;
pub const HARNESS_ID: &str = "kiana-harness";
pub const RUN_RESULT_SCHEMA: &str = "kiana.run-result.v1";
pub const COMPACT_SCHEMA: &str = "kiana.compact.v1";
const CONTEXT_QUERY_COMMAND: &str = "context.query.v1";
const CONTEXT_REPO_MAP_OPERATION: &str = "context.repo_map";
const CONTEXT_INDEX_OPERATION: &str = "context.index.read";
const CONTEXT_INDEX_CACHE_OPERATION: &str = "context.index.cache.write";
const CONTEXT_ARTIFACTS_OPERATION: &str = "context.artifacts.read";
const CONTEXT_ARTIFACTS_CACHE_OPERATION: &str = "context.artifacts.cache.write";
const CONTEXT_ARTIFACT_STORE_OPERATION: &str = "context.artifact_store.read";
const CONTEXT_ARTIFACT_STORE_CACHE_OPERATION: &str = "context.artifact_store.cache.write";
const CONTEXT_ARTIFACT_INGEST_OPERATION: &str = "context.artifact_ingest.write";
const CONTEXT_ARTIFACT_GRAPH_OPERATION: &str = "context.artifact_graph.read";
const CONTEXT_ARTIFACT_READINESS_OPERATION: &str = "context.artifact_readiness.read";
const CONTEXT_SEARCH_OPERATION: &str = "context.search";
const CONTEXT_VECTOR_SEARCH_OPERATION: &str = "context.vector_search";
const CONTEXT_PACK_OPERATION: &str = "context.pack";
const MAX_CONTEXT_LIMIT: u64 = 1_000;
const MAX_CONTEXT_BYTES_PER_FILE: u64 = 16 * 1024 * 1024;
const MAX_CONTEXT_SNIPPET_LINES: u64 = 1_000;

#[derive(Clone, Debug)]
struct SessionBinding {
    run_id: RunId,
    actor_id: Option<String>,
    project_root: String,
    role_id: String,
    department_id: String,
}

#[derive(Clone, Copy)]
struct PersistedApprovalCursor {
    run_id: RunId,
    event_request_id: RequestId,
    event_sequence: u64,
    continuation_recorded: bool,
}

pub struct ControlPlane {
    policy: Arc<dyn PolicyEngine>,
    gates: Arc<dyn GateEngine>,
    events: Arc<dyn EventStorePort>,
    capabilities: Arc<dyn CapabilityBrokerPort>,
    approvals: Arc<dyn ApprovalStorePort>,
    runner: Arc<dyn RunnerPort>,
    pre_tool_hooks: Arc<dyn PreToolHookPort>,
    cell_registry: Arc<dyn kiana_ports::CellRegistryPort>,
    sessions: Mutex<HashMap<String, SessionBinding>>,
    pending_invocations: Mutex<HashMap<ApprovalId, PendingInvocation>>,
    cancellations: Mutex<HashMap<RunId, watch::Sender<bool>>>,
    path_locks: Mutex<HashMap<String, String>>,
    durable_path_locks: Mutex<HashMap<String, Vec<PathLockLease>>>,
}

impl ControlPlane {
    pub fn new(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
    ) -> Self {
        Self::with_pre_tool_hooks(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            Arc::new(AllowAllPreToolHooks),
        )
    }

    pub fn with_pre_tool_hooks(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
        pre_tool_hooks: Arc<dyn PreToolHookPort>,
    ) -> Self {
        Self::with_pre_tool_hooks_and_cell_registry(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            pre_tool_hooks,
            Arc::new(MemoryCellRegistry::new()),
        )
    }

    pub fn with_pre_tool_hooks_and_cell_registry(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
        pre_tool_hooks: Arc<dyn PreToolHookPort>,
        cell_registry: Arc<dyn kiana_ports::CellRegistryPort>,
    ) -> Self {
        Self {
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            pre_tool_hooks,
            cell_registry,
            sessions: Mutex::new(HashMap::new()),
            pending_invocations: Mutex::new(HashMap::new()),
            cancellations: Mutex::new(HashMap::new()),
            path_locks: Mutex::new(HashMap::new()),
            durable_path_locks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn handle_command(
        &self,
        context: RequestContext,
        intent: CommandIntent,
    ) -> Result<CoreResponse, CoreError> {
        if intent.name == CONTEXT_QUERY_COMMAND {
            return self.handle_context_query(context, intent.arguments).await;
        }
        let request_id = context.request_id;
        self.append_event(
            request_id,
            1,
            "request.accepted",
            json!({
                "command": &intent.name,
            }),
        )
        .await?;

        if !context.project_trusted {
            let reason = "project_untrusted";
            self.append_event(
                request_id,
                2,
                "command.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        if intent.name != "system.architecture" {
            let reason = "command_unregistered";
            self.append_event(
                request_id,
                2,
                "command.rejected",
                json!({ "reason": reason, "command": &intent.name }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        let output = json!({
            "schema": "kiana.architecture-status.v1",
            "control_plane": "kiana-core",
            "composition_root": "kiana-daemon",
            "runner": "kiana-runner",
            "harness": HARNESS_ID,
            "capability_mode": "brokered",
            "legacy_prompt_loop": false,
            "legacy_edges_remaining": LEGACY_EDGES_REMAINING,
        });
        self.append_event(request_id, 2, "command.completed", output.clone())
            .await?;
        Ok(CoreResponse::completed(request_id, output))
    }

    async fn handle_context_query(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let normalized = match normalize_context_query_arguments(&context, &arguments) {
            Ok(normalized) => normalized,
            Err(error) => {
                let reason = error.reason();
                self.append_event(
                    request_id,
                    1,
                    "request.accepted",
                    json!({ "command": CONTEXT_QUERY_COMMAND }),
                )
                .await?;
                self.append_event(
                    request_id,
                    2,
                    "command.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        let request = CapabilityRequest::new(
            request_id,
            CapabilityKind::Query,
            normalized.operation,
            normalized.arguments,
        )
        .with_risk(normalized.risk);
        self.authorize_and_execute(&context, request).await
    }

    pub async fn authorize_and_execute(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = request.request_id;
        if request_id != context.request_id {
            return Ok(CoreResponse::blocked(
                request_id,
                "request_context_mismatch",
            ));
        }
        self.append_event(
            request_id,
            1,
            "request.accepted",
            json!({
                "capability": &request.capability,
                "operation": &request.operation,
                "risk": request.risk,
            }),
        )
        .await?;

        let policy = self.evaluate_policy(context, &request);
        let gate = self.evaluate_gate(&request, &policy);
        self.append_event(
            request_id,
            2,
            "capability.decision",
            json!({ "policy": &policy, "gate": &gate }),
        )
        .await?;

        let authorization_id = match gate {
            GateDecision::Allowed { authorization_id } => authorization_id,
            GateDecision::AwaitingApproval { reason } => {
                let challenge = self.approvals.stage(context, request, &reason).await?;
                self.append_event(
                    request_id,
                    3,
                    "approval.requested",
                    json!({
                        "approval_id": challenge.approval_id,
                        "request_hash": &challenge.request_hash,
                        "session_id": context.session_id,
                        "actor_id": context.actor_id,
                        "expires_at_unix_ms": challenge.expires_at_unix_ms,
                    }),
                )
                .await?;
                self.approvals.activate(challenge.approval_id).await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::AwaitingApproval,
                    output: json!({ "approval": challenge }),
                    error: Some(reason),
                });
            }
            GateDecision::Denied { reason } => {
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Denied,
                    output: Value::Null,
                    error: Some(reason),
                });
            }
        };

        self.execute_authorized_request(request, authorization_id, 3)
            .await
    }

    pub async fn decide_approval(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<CoreResponse, CoreError> {
        self.decide_approval_with_proof(context, approval_id, decision, None, None)
            .await
    }

    pub async fn decide_approval_with_proof(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<CoreResponse, CoreError> {
        let persisted_approval = self.approval_cursor(approval_id).await?;
        let has_live_invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&approval_id);
        if decision == ApprovalDecision::Approve
            && persisted_approval.is_some()
            && !has_live_invocation
        {
            // A restarted host can authenticate and preserve the approval while
            // it lacks the Runner continuation needed to execute it. Do not
            // consume the durable approval before that continuation exists.
            if let Err(error) = self
                .approvals
                .validate_with_proof(context, approval_id, request_hash, nonce)
                .await
            {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
            let persisted_approval = persisted_approval.expect("checked above");
            if !persisted_approval.continuation_recorded {
                self.append_event(
                    persisted_approval.event_request_id,
                    persisted_approval.event_sequence.saturating_add(1),
                    "approval.continuation_unavailable",
                    json!({
                        "approval_id": approval_id,
                        "run_id": persisted_approval.run_id,
                        "error": "approval_continuation_unavailable",
                    }),
                )
                .await?;
            }
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_continuation_unavailable",
            ));
        }
        let pending = match self
            .approvals
            .consume_with_proof(context, approval_id, request_hash, nonce)
            .await
        {
            Ok(pending) => pending,
            Err(error) => {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
        };
        let invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&approval_id);
        let request_id = pending.request.request_id;
        let event_request_id = invocation
            .as_ref()
            .map(|pending| pending.event_request_id)
            .unwrap_or(request_id);
        let event_sequence = invocation
            .as_ref()
            .map(|pending| pending.event_sequence)
            .unwrap_or(4);
        let event_kind = match decision {
            ApprovalDecision::Approve => "approval.approved",
            ApprovalDecision::Deny => "approval.denied",
        };
        let mut approval_event = json!({
            "approval_id": approval_id,
            "request_hash": pending.challenge.request_hash,
            "session_id": context.session_id,
            "actor_id": context.actor_id,
        });
        if let Some(invocation) = &invocation {
            approval_event["run_id"] = json!(invocation.run_id);
        }
        self.append_event(event_request_id, event_sequence, event_kind, approval_event)
            .await?;

        if let Some(invocation) = invocation {
            return self
                .resume_approved_invocation(context, request_id, approval_id, decision, invocation)
                .await;
        }

        if decision == ApprovalDecision::Deny {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Denied,
                output: json!({ "approval_id": approval_id }),
                error: Some("approval_denied".to_owned()),
            });
        }
        self.execute_authorized_request(pending.request, format!("approval:{approval_id}"), 5)
            .await
    }

    async fn resume_approved_invocation(
        &self,
        _decision_context: &RequestContext,
        request_id: RequestId,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        invocation: PendingInvocation,
    ) -> Result<CoreResponse, CoreError> {
        if decision == ApprovalDecision::Deny {
            let _ = self
                .runner
                .send(RunnerCommand::CapabilityResult {
                    run_id: invocation.run_id,
                    result: CapabilityResult::failure(invocation.request_id, "approval_denied"),
                })
                .await;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Denied,
                output: json!({ "approval_id": approval_id, "run_id": invocation.run_id }),
                error: Some("approval_denied".to_owned()),
            });
        }

        if let Some(reason) = capability_risk_violation(&invocation.request) {
            self.append_event(
                invocation.event_request_id,
                invocation.event_sequence + 1,
                "run.capability_blocked",
                json!({
                    "run_id": invocation.run_id,
                    "reason": reason,
                }),
            )
            .await?;
            let _ = self
                .runner
                .send(RunnerCommand::CapabilityResult {
                    run_id: invocation.run_id,
                    result: CapabilityResult::failure(
                        invocation.request_id,
                        format!("capability_blocked:{reason}"),
                    ),
                })
                .await;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Blocked,
                output: run_identity(&invocation.context, invocation.run_id, &invocation.sandbox),
                error: Some(reason.to_owned()),
            });
        }

        let mut request = invocation.request.clone();
        if let Some(object) = request.arguments.as_object_mut() {
            object.insert("sandbox".to_owned(), json!(invocation.sandbox));
        }
        if let Err(error) = self
            .bind_cell_scope(&invocation.context, &mut request)
            .await
        {
            let reason = error.to_string();
            let _ = self
                .runner
                .send(RunnerCommand::CapabilityResult {
                    run_id: invocation.run_id,
                    result: CapabilityResult::failure(invocation.request_id, reason.clone()),
                })
                .await;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Blocked,
                output: run_identity(&invocation.context, invocation.run_id, &invocation.sandbox),
                error: Some(reason),
            });
        }
        let cell_lease = self.begin_cell_capability_from_request(&request).await?;
        let result = match self
            .capabilities
            .execute(AuthorizedCapabilityRequest::new(
                format!("approval:{approval_id}"),
                request.clone(),
            )?)
            .await
        {
            Ok(result) => result,
            Err(error) => CapabilityResult::failure(
                invocation.request_id,
                redact_event_text(&error.to_string()),
            ),
        };
        let result = redact_capability_result(result);
        let outcome = if result.success {
            CapabilityOutcome::Succeeded
        } else {
            CapabilityOutcome::Failed
        };
        if let Some(lease) = cell_lease {
            self.finish_cell_capability(lease, outcome).await?;
        }
        if result.request_id != invocation.request_id {
            let reason = "capability_result_mismatch";
            self.append_event(
                invocation.event_request_id,
                invocation.event_sequence + 1,
                "capability.result_unknown",
                json!({
                    "run_id": invocation.run_id,
                    "capability_request_id": invocation.request_id,
                    "result_request_id": result.request_id,
                    "error": reason,
                }),
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: run_identity(&invocation.context, invocation.run_id, &invocation.sandbox),
                error: Some(reason.to_owned()),
            });
        }
        self.append_event(
            invocation.event_request_id,
            invocation.event_sequence + 1,
            if result.success {
                "capability.completed"
            } else {
                "capability.failed"
            },
            capability_event_payload(
                &result.output,
                &request,
                &invocation.context,
                invocation.run_id,
            ),
        )
        .await?;
        let events = match self
            .runner
            .send(RunnerCommand::CapabilityResult {
                run_id: invocation.run_id,
                result,
            })
            .await
        {
            Ok(events) => events,
            Err(error) => {
                let error = redact_event_text(&error.to_string());
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(
                        &invocation.context,
                        invocation.run_id,
                        &invocation.sandbox,
                    ),
                    error: Some(error),
                });
            }
        };
        let mut sequence = invocation.event_sequence + 2;
        self.drive_run(
            &invocation.context,
            invocation.run_id,
            &invocation.sandbox,
            events,
            &mut sequence,
        )
        .await
    }

    async fn execute_authorized_request(
        &self,
        request: CapabilityRequest,
        authorization_id: String,
        result_sequence: u64,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = request.request_id;
        if let Some(reason) = capability_risk_violation(&request) {
            self.append_event(
                request_id,
                result_sequence,
                "capability.blocked",
                json!({ "error": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        let cell_lease = self.begin_cell_capability_from_request(&request).await?;
        let authorized = AuthorizedCapabilityRequest::new(authorization_id, request.clone())?;
        match self.capabilities.execute(authorized).await {
            Ok(result) => {
                let result = redact_capability_result(result);
                let outcome = if result.request_id != request_id {
                    CapabilityOutcome::Unknown
                } else if result.success {
                    CapabilityOutcome::Succeeded
                } else {
                    CapabilityOutcome::Failed
                };
                if let Some(lease) = cell_lease {
                    self.finish_cell_capability(lease, outcome).await?;
                }
                if result.request_id != request_id {
                    self.append_event(
                        request_id,
                        result_sequence,
                        "capability.result_unknown",
                        json!({
                            "capability_request_id": request_id,
                            "result_request_id": result.request_id,
                            "error": "capability_result_mismatch",
                        }),
                    )
                    .await?;
                    return Ok(CoreResponse {
                        request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: Value::Null,
                        error: Some("capability_result_mismatch".to_owned()),
                    });
                }
                let success = result.success;
                let status = if success {
                    ExecutionStatus::Completed
                } else {
                    ExecutionStatus::Failed
                };
                let output = result.output;
                let error = (!success).then(|| {
                    output
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("capability_failed")
                        .to_owned()
                });
                if self
                    .append_event(
                        request_id,
                        result_sequence,
                        if success {
                            "capability.completed"
                        } else {
                            "capability.failed"
                        },
                        direct_capability_event_payload(&output, &request),
                    )
                    .await
                    .is_err()
                {
                    return Ok(CoreResponse {
                        request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: Value::Null,
                        error: Some("result_event_persistence_failed".to_owned()),
                    });
                }
                Ok(CoreResponse {
                    request_id,
                    status,
                    output,
                    error,
                })
            }
            Err(error) => {
                if let Some(lease) = cell_lease {
                    self.finish_cell_capability(lease, CapabilityOutcome::Unknown)
                        .await?;
                }
                let reason = redact_event_text(&error.to_string());
                self.append_event(
                    request_id,
                    result_sequence,
                    "capability.failed",
                    direct_capability_event_payload(
                        &json!({ "error": redact_event_text(&reason) }),
                        &request,
                    ),
                )
                .await?;
                Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: Value::Null,
                    error: Some(reason),
                })
            }
        }
    }

    /// Apply server-owned operation invariants before consulting the replaceable policy engine.
    ///
    /// Policy implementations may vary by deployment, but no policy is allowed to turn a
    /// malformed MCP request into an executable low-risk capability.
    fn evaluate_policy(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> PolicyDecision {
        if let Some(reason) = capability_risk_violation(request) {
            return PolicyDecision::Deny {
                reason: reason.to_owned(),
            };
        }
        self.policy.evaluate(context, request)
    }

    /// Keep server-owned operation invariants authoritative even if a deployment supplies a
    /// custom Gate implementation that would otherwise widen a policy decision.
    fn evaluate_gate(&self, request: &CapabilityRequest, policy: &PolicyDecision) -> GateDecision {
        if let Some(reason) = capability_risk_violation(request) {
            return GateDecision::Denied {
                reason: reason.to_owned(),
            };
        }
        self.gates.evaluate(policy)
    }

    pub async fn spawn_from_packet(
        &self,
        mut context: RequestContext,
        mut packet: WorkPacket,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        if let Err(reason) = packet.validate() {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({ "reason": reason, "command": "run.spawn" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        if self.session_known(context.session_id.as_str()) {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({
                    "reason": "spawn_session_not_fresh",
                    "command": "run.spawn",
                    "session_id": context.session_id,
                }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "spawn_session_not_fresh"));
        }
        if !matches!(
            packet.status,
            kiana_domain::WorkPacketStatus::Draft
                | kiana_domain::WorkPacketStatus::Approved
                | kiana_domain::WorkPacketStatus::Assigned
        ) {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({
                    "reason": "packet_status_not_spawnable",
                    "command": "run.spawn",
                    "packet_id": &packet.id,
                    "status": packet.status,
                }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "packet_status_not_spawnable",
            ));
        }
        context.assign_role(&RoleSpec::builder());
        context.work_packet_id = Some(packet.id.clone());
        context.path_allow = packet.path_allow.clone();
        let session_id = context.session_id.as_str().to_owned();
        let project_root = context.project_root.clone();
        if let Err(reason) =
            self.acquire_builder_path_locks(&project_root, &session_id, &context.path_allow)
        {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({ "reason": reason, "command": "run.spawn", "session_id": session_id }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        let _path_lock_guard = BuilderPathLockGuard {
            control_plane: self,
            project_root: project_root.clone(),
            session_id: session_id.clone(),
        };
        let run_id = RunId::new();
        let admission = match self.reserve_packet_cell(&context, &packet, run_id).await {
            Ok(admission) => admission,
            Err(error) => {
                let error = spawn_error_reason(&error);
                self.append_event(
                    request_id,
                    1,
                    "run.rejected",
                    json!({
                        "reason": &error,
                        "command": "run.spawn",
                        "packet_id": &packet.id,
                    }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, error));
            }
        };
        let mut context = context;
        context.cell_id = Some(admission.cell.cell_id);
        packet.owner_cell_id = Some(admission.cell.cell_id);
        packet.budget_lease_id = Some(admission.budget.lease_id);
        packet.acceptor_id = context.actor_id.clone();
        let mut packet_sequence = 1u64;
        self.record_event(
            request_id,
            &mut packet_sequence,
            "cell.validated",
            cell_event_payload(&admission, "validated"),
        )
        .await?;
        let admission = match self
            .cell_registry
            .commit_spawn(admission.plan.plan_id)
            .await
        {
            Ok(admission) => admission,
            Err(error) => {
                let _ = self
                    .cell_registry
                    .abort_spawn(admission.plan.plan_id, "spawn_commit_failed")
                    .await;
                return Ok(CoreResponse::blocked(request_id, error.to_string()));
            }
        };
        self.record_event(
            request_id,
            &mut packet_sequence,
            "spawn.committed",
            spawn_event_payload(&admission),
        )
        .await?;
        let admission = match self
            .cell_registry
            .transition_cell(
                admission.cell.cell_id,
                CellLifecycle::Ready,
                CellLifecycle::Running,
            )
            .await
        {
            Ok(cell) => {
                let mut admission = admission;
                admission.cell = cell;
                admission
            }
            Err(error) => {
                let _ = self
                    .cell_registry
                    .abort_spawn(admission.plan.plan_id, "cell_start_failed")
                    .await;
                return Ok(CoreResponse::blocked(request_id, error.to_string()));
            }
        };
        self.record_event(
            request_id,
            &mut packet_sequence,
            "cell.started",
            cell_event_payload(&admission, "running"),
        )
        .await?;
        if packet.status == kiana_domain::WorkPacketStatus::Draft {
            packet.transition_status(kiana_domain::WorkPacketStatus::Approved)?;
            self.record_event(
                request_id,
                &mut packet_sequence,
                "packet.approved",
                json!({
                    "packet_id": &packet.id,
                    "status": packet.status,
                    "actor_id": context.actor_id,
                }),
            )
            .await?;
        }
        if packet.status == kiana_domain::WorkPacketStatus::Approved {
            packet.transition_status(kiana_domain::WorkPacketStatus::Assigned)?;
            self.record_event(
                request_id,
                &mut packet_sequence,
                "packet.assigned",
                json!({
                    "packet_id": &packet.id,
                    "status": packet.status,
                    "assignee_role": &packet.assignee_role,
                }),
            )
            .await?;
        }
        if packet.status == kiana_domain::WorkPacketStatus::Assigned {
            packet.transition_status(kiana_domain::WorkPacketStatus::Running)?;
            self.record_event(
                request_id,
                &mut packet_sequence,
                "packet.accepted",
                json!({
                    "packet_id": &packet.id,
                    "status": packet.status,
                    "acceptor_id": context.actor_id,
                }),
            )
            .await?;
        }
        let response = self
            .start_run_with_id(
                context.clone(),
                packet.as_prompt(),
                Vec::new(),
                sandbox,
                Some(run_id),
            )
            .await;
        let mut response = match response {
            Ok(response) => response,
            Err(error) => {
                let _ = self
                    .cell_registry
                    .abort_spawn(admission.plan.plan_id, "runner_start_failed")
                    .await;
                return Err(error);
            }
        };
        let cell_lifecycle = match response.status {
            ExecutionStatus::Completed => {
                Some((CellLifecycle::Running, CellLifecycle::ReadyToMerge))
            }
            ExecutionStatus::AwaitingApproval => {
                Some((CellLifecycle::Running, CellLifecycle::WaitingInput))
            }
            ExecutionStatus::Blocked | ExecutionStatus::Denied | ExecutionStatus::Failed => {
                Some((CellLifecycle::Running, CellLifecycle::Failed))
            }
            ExecutionStatus::Cancelled => {
                Some((CellLifecycle::Running, CellLifecycle::CancelRequested))
            }
            ExecutionStatus::ResultUnknown => {
                Some((CellLifecycle::Running, CellLifecycle::Quarantined))
            }
            ExecutionStatus::Accepted | ExecutionStatus::Running => None,
        };
        if let Some((expected, next)) = cell_lifecycle {
            let lifecycle = self
                .cell_registry
                .transition_cell(admission.cell.cell_id, expected, next)
                .await;
            match lifecycle {
                Ok(lifecycle) => {
                    let mut lifecycle_admission = admission.clone();
                    lifecycle_admission.cell = lifecycle.clone();
                    self.record_event(
                        request_id,
                        &mut packet_sequence,
                        cell_event_kind(lifecycle.lifecycle),
                        cell_event_payload(&lifecycle_admission, lifecycle.lifecycle.as_str()),
                    )
                    .await?;
                    let lifecycle = if response.status == ExecutionStatus::Cancelled {
                        let cancelled = self
                            .cell_registry
                            .transition_cell(
                                admission.cell.cell_id,
                                CellLifecycle::CancelRequested,
                                CellLifecycle::Cancelled,
                            )
                            .await?;
                        let mut cancelled_admission = admission.clone();
                        cancelled_admission.cell = cancelled.clone();
                        self.record_event(
                            request_id,
                            &mut packet_sequence,
                            "cell.cancelled",
                            cell_event_payload(&cancelled_admission, cancelled.lifecycle.as_str()),
                        )
                        .await?;
                        cancelled
                    } else {
                        lifecycle
                    };
                    if matches!(
                        lifecycle.lifecycle,
                        CellLifecycle::ReadyToMerge
                            | CellLifecycle::Failed
                            | CellLifecycle::Quarantined
                            | CellLifecycle::Cancelled
                    ) {
                        let retirement = self
                            .cell_registry
                            .retire_cell(admission.cell.cell_id, "packet_run_terminal")
                            .await;
                        match retirement {
                            Ok(retirement) => {
                                self.record_event(
                                    request_id,
                                    &mut packet_sequence,
                                    "cell.retired",
                                    retirement_event_payload(&retirement),
                                )
                                .await?;
                            }
                            Err(error) => {
                                return Ok(CoreResponse {
                                    request_id,
                                    status: ExecutionStatus::ResultUnknown,
                                    output: run_identity(&context, run_id, "read-only"),
                                    error: Some(format!("cell_retirement_failed:{error}")),
                                });
                            }
                        }
                    }
                }
                Err(error) => {
                    return Ok(CoreResponse {
                        request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: run_identity(&context, run_id, "read-only"),
                        error: Some(format!("cell_transition_failed:{error}")),
                    });
                }
            }
        }
        let next_status = match response.status {
            ExecutionStatus::Completed => kiana_domain::WorkPacketStatus::Succeeded,
            ExecutionStatus::AwaitingApproval => kiana_domain::WorkPacketStatus::AwaitingApproval,
            ExecutionStatus::Blocked => kiana_domain::WorkPacketStatus::Blocked,
            ExecutionStatus::Cancelled => kiana_domain::WorkPacketStatus::Cancelled,
            ExecutionStatus::Denied | ExecutionStatus::Failed | ExecutionStatus::ResultUnknown => {
                kiana_domain::WorkPacketStatus::Failed
            }
            ExecutionStatus::Accepted | ExecutionStatus::Running => packet.status,
        };
        if next_status != packet.status {
            packet.transition_status(next_status)?;
            let event_kind = match next_status {
                kiana_domain::WorkPacketStatus::Succeeded => "packet.succeeded",
                kiana_domain::WorkPacketStatus::AwaitingApproval => "packet.awaiting_approval",
                kiana_domain::WorkPacketStatus::Blocked => "packet.blocked",
                kiana_domain::WorkPacketStatus::Cancelled => "packet.cancelled",
                _ => "packet.failed",
            };
            self.record_event(
                request_id,
                &mut packet_sequence,
                event_kind,
                json!({
                    "packet_id": &packet.id,
                    "status": packet.status,
                    "run_id": response.output.get("run_id"),
                }),
            )
            .await?;
        }
        if let Some(output) = response.output.as_object_mut() {
            output.insert("packet_status".to_owned(), json!(packet.status));
        }
        Ok(response)
    }

    async fn reserve_packet_cell(
        &self,
        context: &RequestContext,
        packet: &WorkPacket,
        run_id: RunId,
    ) -> Result<kiana_ports::SpawnReservation, CoreError> {
        let template = self
            .cell_registry
            .resolve_template(ROLE_BUILDER, cell_registry::TEMPLATE_VERSION)
            .await?;
        let now = unix_ms();
        let deadline = packet
            .deadline_unix_ms
            .unwrap_or_else(|| now.saturating_add(template.ttl_seconds.saturating_mul(1_000)));
        if deadline <= now {
            return Err(CoreError::Port(PortError::Failed(
                "spawn_deadline_expired".to_owned(),
            )));
        }
        let grant_paths = if packet.path_allow.is_empty() {
            vec![".".to_owned()]
        } else {
            packet.path_allow.clone()
        };
        let budget = BudgetLease::new(
            template.estimated_cost.max(1),
            template.estimated_cost.max(1).saturating_mul(4096),
            template.ttl_seconds.saturating_mul(1_000),
            1,
            1,
        );
        let supervision = SupervisionLease {
            schema: SUPERVISION_LEASE_SCHEMA.to_owned(),
            lease_id: SupervisionLeaseId::new(),
            heartbeat_interval_seconds: template.heartbeat_interval_seconds,
            stall_threshold_seconds: template.ttl_seconds,
            retry_limit: 0,
            retries_used: 0,
        };
        let grant = CapabilityGrant {
            schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
            grant_id: CapabilityGrantId::new(),
            capability: CapabilityKind::Other("coding".to_owned()),
            operation: "builder.packet".to_owned(),
            resources: vec!["workspace".to_owned()],
            paths: grant_paths,
            expires_at_unix_ms: deadline,
            approval_id: None,
            delegation_allowed: false,
        };
        let plan = SpawnPlan {
            schema: SPAWN_PLAN_SCHEMA.to_owned(),
            plan_id: SpawnPlanId::new(),
            parent_cell_id: None,
            reason_code: "packet_spawn".to_owned(),
            candidate_templates: vec![template.template_id],
            count: 1,
            partition: packet.id.clone(),
            input_refs: packet.inputs.clone(),
            output_contract: RUN_RESULT_SCHEMA.to_owned(),
            requested_capabilities: template.default_capabilities.clone(),
            budget_reservation: 1,
            deadline_unix_ms: deadline,
            rollback_policy: "abort".to_owned(),
            idempotency_key: format!("packet:{}:{}", context.project_root, packet.id),
            expected_utility: 0,
            status: SpawnPlanStatus::Proposed,
        };
        let cell = CellSpec {
            schema: CELL_SCHEMA.to_owned(),
            cell_id: CellId::new(),
            parent_cell_id: None,
            root_run_id: run_id,
            template_id: template.template_id,
            template_version: template.version.clone(),
            role_id: ROLE_BUILDER.to_owned(),
            objective: packet.goal.clone(),
            input_refs: packet.inputs.clone(),
            output_contract: plan.output_contract.clone(),
            partition_key: plan.partition.clone(),
            owned_paths: packet.path_allow.clone(),
            work_packet_id: Some(packet.id.clone()),
            owner_actor_id: context.actor_id.clone(),
            capability_grant_id: grant.grant_id,
            budget_lease_id: budget.lease_id,
            supervision_lease_id: supervision.lease_id,
            depth: 0,
            spawn_quota: template.max_children,
            lifecycle: CellLifecycle::Proposed,
        };
        let fingerprint = WorkFingerprint::from_parts(
            &packet.goal,
            &packet.inputs,
            &plan.partition,
            &plan.output_contract,
            "kiana.policy.v1",
        )
        .map_err(|reason| CoreError::Port(PortError::Failed(reason.to_owned())))?;
        Ok(self
            .cell_registry
            .reserve_spawn(SpawnReservationRequest {
                plan,
                cell,
                template,
                budget,
                grant,
                supervision,
                fingerprint,
                owned_paths: packet.path_allow.clone(),
            })
            .await?)
    }

    pub async fn review_author_run(
        &self,
        mut context: RequestContext,
        author_session_id: String,
        author_run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({ "command": "run.review" }),
        )
        .await?;

        let author_session_id = author_session_id.trim().to_owned();
        if author_session_id.is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "review_author_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "review_author_required"));
        }

        let Some(reviewer) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if reviewer.role_id != ROLE_REVIEWER {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "review_role_must_be_reviewer" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "review_role_must_be_reviewer",
            ));
        }
        context.assign_role(&RoleSpec::reviewer());

        if context.session_id.as_str() == author_session_id {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({
                    "reason": "review_author_session_denied",
                    "author_session_id": author_session_id,
                    "reviewer_session_id": context.session_id,
                }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "review_author_session_denied",
            ));
        }
        if self.session_known(context.session_id.as_str()) {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "review_session_not_fresh" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "review_session_not_fresh",
            ));
        }

        let events = self
            .events_for_author(&context, &author_session_id, author_run_id)
            .await?;
        if events.is_empty() || !events.iter().any(|event| event.kind == "run.completed") {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "review_author_not_found" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "review_author_not_found"));
        }

        let author_role = events
            .iter()
            .rev()
            .find_map(|event| event.data.get("role_id").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned();
        if author_role != ROLE_BUILDER {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({
                    "reason": "review_author_must_be_builder",
                    "author_role_id": author_role,
                }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "review_author_must_be_builder",
            ));
        }

        let author_run_id = author_run_id
            .or_else(|| {
                events.iter().find_map(|event| {
                    (event.kind == "run.authorized")
                        .then(|| event.data.get("run_id"))
                        .flatten()
                        .and_then(Value::as_str)
                        .and_then(RunId::parse_str)
                })
            })
            .ok_or_else(|| {
                CoreError::Port(PortError::Failed("review_author_run_missing".to_owned()))
            })?;
        let files = files_changed_from_events(&events);
        let verdict = if files.is_empty() {
            "needs_change"
        } else {
            "pass"
        };
        let summary = if files.is_empty() {
            "author produced no files_changed"
        } else {
            "author files reviewed against builder receipt"
        };
        let packet = ReviewPacket::closed(
            format!("rv-{}", context.session_id),
            author_session_id.clone(),
            ROLE_BUILDER,
            context.session_id.to_string(),
            verdict,
            summary,
            files.clone(),
        );
        if let Err(reason) = packet.validate() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        if let Err(reason) = write_review_artifact(&context.project_root, &packet) {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        let reviewer_session_id = kiana_domain::SessionId::new(context.session_id.as_str());
        let merge_receipt = if verdict == "pass" {
            let receipt = MergeReceipt {
                schema: kiana_domain::MERGE_RECEIPT_SCHEMA.to_owned(),
                receipt_id: kiana_domain::ReceiptId::new(),
                author_run_id,
                author_session_id: kiana_domain::SessionId::new(author_session_id.clone()),
                reviewer_session_id: reviewer_session_id.clone(),
                reviewer_verdict: verdict.to_owned(),
                files: files.clone(),
                accepted: true,
                provenance: vec![REVIEW_PACKET_PATH.to_owned(), "run.completed".to_owned()],
            };
            receipt
                .validate()
                .map_err(|reason| CoreError::Port(PortError::Failed(reason.to_owned())))?;
            write_merge_artifact(&context.project_root, &receipt)
                .map_err(|reason| CoreError::Port(PortError::Failed(reason.to_owned())))?;
            Some(receipt)
        } else {
            None
        };

        let run_id = RunId::parse_str(context.session_id.as_str()).unwrap_or_else(RunId::new);
        self.remember_session(&context, run_id);
        self.record_event(
            request_id,
            &mut sequence,
            "review.closed",
            json!({
                "author_session_id": author_session_id,
                "reviewer_session_id": context.session_id,
                "verdict": verdict,
                "builder_present": false,
                "merge_receipt_id": merge_receipt.as_ref().map(|receipt| receipt.receipt_id),
            }),
        )
        .await?;

        Ok(CoreResponse::completed(
            request_id,
            json!({
                "schema": REVIEW_RESULT_SCHEMA,
                "harness": HARNESS_ID,
                "run_id": run_id,
                "session_id": context.session_id,
                "role_id": ROLE_REVIEWER,
                "department_id": reviewer.department_id,
                "author_session_id": author_session_id,
                "author_role_id": ROLE_BUILDER,
                "input": "review",
                "verdict": verdict,
                "files_reviewed": files,
                "review_path": REVIEW_PACKET_PATH,
                "merge_path": merge_receipt.as_ref().map(|_| kiana_domain::MERGE_RECEIPT_PATH),
                "packet": packet,
                "merge_receipt": merge_receipt,
            }),
        ))
    }

    pub async fn close_author_run(
        &self,
        mut context: RequestContext,
        author_session_id: String,
        author_run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({ "command": "run.close" }),
        )
        .await?;

        let author_session_id = author_session_id.trim().to_owned();
        if author_session_id.is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_author_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "close_author_required"));
        }
        let Some(closer) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if closer.role_id != ROLE_CLOSER {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_role_must_be_closer" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_role_must_be_closer",
            ));
        }
        context.assign_role(&RoleSpec::closer());
        if context.session_id.as_str() == author_session_id {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_author_session_denied" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_author_session_denied",
            ));
        }
        if self.session_known(context.session_id.as_str()) {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_session_not_fresh" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "close_session_not_fresh"));
        }

        let author_events = self
            .events_for_author(&context, &author_session_id, author_run_id)
            .await?;
        let Some(author_run_id) = author_run_id.or_else(|| {
            author_events.iter().find_map(|event| {
                (event.kind == "run.authorized")
                    .then(|| event.data.get("run_id"))
                    .flatten()
                    .and_then(Value::as_str)
                    .and_then(RunId::parse_str)
            })
        }) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_author_not_found" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "close_author_not_found"));
        };
        if author_events.is_empty()
            || !author_events
                .iter()
                .any(|event| event.kind == "run.completed")
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_author_not_completed" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_author_not_completed",
            ));
        }
        let review = match read_review_artifact(&context.project_root) {
            Ok(review) => review,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        if review.author_session_id != author_session_id {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_review_author_mismatch" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_review_author_mismatch",
            ));
        }
        if review.author_role_id != ROLE_BUILDER || review.verdict != "pass" {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_review_not_accepted" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_review_not_accepted",
            ));
        }
        let merge = match read_merge_artifact(&context.project_root) {
            Ok(merge) => merge,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        if merge.author_run_id != author_run_id
            || merge.author_session_id.as_str() != author_session_id
            || merge.reviewer_session_id.as_str() != review.reviewer_session_id
            || merge.files != review.files_reviewed
            || !merge.accepted
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_merge_receipt_mismatch" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_merge_receipt_mismatch",
            ));
        }
        let reviewer_session_id = kiana_domain::SessionId::new(review.reviewer_session_id.clone());
        let receipt = ClosingReceipt {
            schema: kiana_domain::CLOSING_RECEIPT_SCHEMA.to_owned(),
            receipt_id: kiana_domain::ReceiptId::new(),
            project_id: None,
            author_run_id,
            author_session_id: kiana_domain::SessionId::new(author_session_id.clone()),
            reviewer_session_id: reviewer_session_id.clone(),
            closer_session_id: context.session_id.clone(),
            review_id: review.id.clone(),
            verdict: review.verdict.clone(),
            accepted: true,
            files_verified: review.files_reviewed.clone(),
            exceptions: Vec::new(),
            lessons_path: "lessons/LEARNED.md".to_owned(),
        };
        if let Err(reason) = receipt.validate() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        if let Err(reason) = write_closing_artifact(&context.project_root, &receipt) {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        self.remember_session(&context, RunId::new());
        self.record_event(
            request_id,
            &mut sequence,
            "closing.completed",
            json!({
                "receipt_id": receipt.receipt_id,
                "author_run_id": author_run_id,
                "author_session_id": author_session_id,
                "reviewer_session_id": reviewer_session_id,
                "closer_session_id": context.session_id,
                "review_id": review.id,
                "files_verified": receipt.files_verified,
            }),
        )
        .await?;
        Ok(CoreResponse::completed(
            request_id,
            serde_json::to_value(receipt).map_err(|error| {
                CoreError::Port(PortError::Failed(format!(
                    "close_receipt_serialize:{error}"
                )))
            })?,
        ))
    }

    pub async fn convene_symposium(
        &self,
        mut context: RequestContext,
        goal: String,
        anti_meeting: bool,
        max_rounds: Option<u32>,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.symposium",
                "anti_meeting": anti_meeting,
            }),
        )
        .await?;

        let goal = goal.trim().to_owned();
        if goal.is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "symposium_goal_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "symposium_goal_required"));
        }

        let Some(chair) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if !chair.can_convene {
            let reason = if chair.department_id == DEPARTMENT_PLANNING {
                "symposium_chair_must_be_pm"
            } else {
                "symposium_chair_cannot_convene"
            };
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        context.assign_role(&chair);

        let max_rounds = match Symposium::validate_max_rounds(
            max_rounds.unwrap_or(Symposium::DEFAULT_MAX_ROUNDS),
        ) {
            Ok(value) => value,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        if chair.department_id == DEPARTMENT_PLANNING {
            match authorized_harness_sandbox(&context, sandbox.as_deref()) {
                Ok("workspace-write") => {}
                Ok(_) => {
                    self.record_event(
                        request_id,
                        &mut sequence,
                        "run.rejected",
                        json!({ "reason": "symposium_requires_workspace_write" }),
                    )
                    .await?;
                    return Ok(CoreResponse::blocked(
                        request_id,
                        "symposium_requires_workspace_write",
                    ));
                }
                Err(reason) => {
                    self.record_event(
                        request_id,
                        &mut sequence,
                        "run.rejected",
                        json!({ "reason": reason }),
                    )
                    .await?;
                    return Ok(CoreResponse::blocked(request_id, reason));
                }
            }
        } else if !context.project_trusted
            || matches!(context.permission_profile, PermissionProfile::Safe)
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "workspace_write_requires_trusted_non_safe_profile" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "workspace_write_requires_trusted_non_safe_profile",
            ));
        }

        let mut meeting = match Symposium::department(
            &chair.department_id,
            request_id.to_string(),
            goal,
            max_rounds,
        ) {
            Ok(meeting) => meeting,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        if let Err(reason) = meeting.validate() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        let mut speaker_sessions = Vec::new();
        if !anti_meeting {
            let attendees = meeting.attendees.clone();
            for round in 0..max_rounds {
                for role_id in &attendees {
                    let role = RoleSpec::lookup(role_id).unwrap_or_else(RoleSpec::pm);
                    let session_id = meeting.speaker_session_id(role_id);
                    let mut speaker = context.clone();
                    speaker.request_id = RequestId::new();
                    speaker.session_id = kiana_domain::SessionId::new(session_id.clone());
                    speaker.assign_role(&role);
                    speaker.permission_profile = PermissionProfile::Safe;
                    let prompt = meeting.speaker_prompt(role_id);
                    let turn = if round == 0 {
                        self.start_run(speaker, prompt, None).await?
                    } else {
                        self.continue_run(speaker, prompt, None, None).await?
                    };
                    if turn.status != ExecutionStatus::Completed {
                        let mut failed = turn;
                        failed.request_id = request_id;
                        return Ok(failed);
                    }
                    if let Some(text) = turn
                        .output
                        .get("output")
                        .and_then(|output| output.get("text"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        meeting
                            .blackboard
                            .claims
                            .push(SymposiumClaim::new(role_id, text));
                    }
                    if round == 0 {
                        speaker_sessions.push(json!({
                            "role_id": role_id,
                            "session_id": session_id,
                        }));
                    }
                }
            }
        }

        let (decision, packet) = match meeting.close(anti_meeting) {
            Ok(closed) => closed,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        if let Err(reason) =
            write_symposium_artifacts(&context.project_root, &meeting, &decision, packet.as_ref())
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        self.record_event(
            request_id,
            &mut sequence,
            "symposium.closed",
            json!({
                "symposium_id": meeting.id,
                "department_id": meeting.department_id,
                "skipped_meeting": anti_meeting,
                "builder_present": meeting.builder_present(),
                "decision_id": decision.id,
                "work_packet_id": packet.as_ref().map(|packet| packet.id.clone()),
            }),
        )
        .await?;

        Ok(CoreResponse::completed(
            request_id,
            json!({
                "schema": SYMPOSIUM_RESULT_SCHEMA,
                "harness": HARNESS_ID,
                "symposium_id": meeting.id,
                "department_id": meeting.department_id,
                "chair": meeting.chair,
                "attendees": meeting.attendees,
                "status": meeting.status,
                "builder_present": meeting.builder_present(),
                "skipped_meeting": anti_meeting,
                "decision": decision,
                "packet": packet,
                "decision_path": meeting.decision_path(),
                "packet_path": packet.as_ref().map(|_| WORK_PACKET_PATH),
                "speaker_sessions": speaker_sessions,
                "blackboard": meeting.blackboard,
            }),
        ))
    }

    pub async fn start_run(
        &self,
        context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        self.start_run_with_id(context, prompt, Vec::new(), sandbox, None)
            .await
    }

    pub async fn start_run_with_history(
        &self,
        context: RequestContext,
        prompt: String,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        self.start_run_with_id(context, prompt, history, sandbox, None)
            .await
    }

    async fn start_run_with_id(
        &self,
        context: RequestContext,
        prompt: String,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
        requested_run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let run_id = requested_run_id.unwrap_or_else(|| {
            RunId::parse_str(context.session_id.as_str()).unwrap_or_else(RunId::new)
        });
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.start",
                "run_id": run_id,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
                "project_root": context.project_root,
                "role_id": context.role_id,
                "harness": HARNESS_ID,
            }),
        )
        .await?;

        if prompt.trim().is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "prompt_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "prompt_required"));
        }

        let Some(role) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if !context.department_id.trim().is_empty() && context.department_id != role.department_id {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_department_mismatch" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "role_department_mismatch",
            ));
        }

        let sandbox = match authorized_harness_sandbox(&context, sandbox.as_deref()) {
            Ok(sandbox) => sandbox,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        self.record_event(
            request_id,
            &mut sequence,
            "run.authorized",
            json!({
                "run_id": run_id,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
                "project_root": context.project_root,
                "role_id": context.role_id,
                "department_id": context.department_id,
                "harness": HARNESS_ID,
                "sandbox": sandbox,
                "capability_mode": "brokered",
            }),
        )
        .await?;

        // Session index and cancel watch must exist before the first model step
        // so an in-flight cancel can resolve session-1 and abort shell.exec.
        self.remember_session(&context, run_id);
        let _cancel_rx = self.watch_cancel(run_id);

        let pending_events = match self
            .runner
            .send(RunnerCommand::start_in_with_history(
                run_id,
                prompt,
                history,
                context.project_root.clone(),
                sandbox.to_owned(),
                String::new(),
                context.project_trusted,
            ))
            .await
        {
            Ok(events) => events,
            Err(error) => {
                self.forget_session(context.session_id.as_str(), run_id);
                self.clear_cancel(run_id);
                let reason = redact_event_text(&error.to_string());
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.failed",
                    json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, sandbox),
                    error: Some(reason),
                });
            }
        };

        self.drive_run(&context, run_id, sandbox, pending_events, &mut sequence)
            .await
    }

    pub async fn continue_run(
        &self,
        context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
        run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        let run_id = match self.resolve_run_id(&context, run_id) {
            Ok(run_id) => run_id,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "request.accepted",
                    json!({ "command": "run.continue" }),
                )
                .await?;
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.continue",
                "run_id": run_id,
                "harness": HARNESS_ID,
            }),
        )
        .await?;

        if prompt.trim().is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "prompt_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "prompt_required"));
        }

        let sandbox = match authorized_harness_sandbox(&context, sandbox.as_deref()) {
            Ok(sandbox) => sandbox,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        let pending_events = match self
            .runner
            .send(RunnerCommand::continue_run(run_id, prompt))
            .await
        {
            Ok(events) => events,
            Err(error) => {
                let reason = redact_event_text(&error.to_string());
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.failed",
                    json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, sandbox),
                    error: Some(reason),
                });
            }
        };

        self.drive_run(&context, run_id, sandbox, pending_events, &mut sequence)
            .await
    }

    pub async fn cancel_run(
        &self,
        context: RequestContext,
        run_id: Option<RunId>,
        reason: String,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        let reason = if reason.trim().is_empty() {
            "user".to_owned()
        } else {
            reason
        };
        // Cancellation reasons cross the runner and direct-response boundaries, so sanitize
        // them once at ingress instead of relying only on the event-log redaction boundary.
        let reason = redact_event_text(&reason);
        let run_id = match self.resolve_run_id(&context, run_id) {
            Ok(run_id) => run_id,
            Err(code) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "request.accepted",
                    json!({ "command": "run.cancel" }),
                )
                .await?;
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": code }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, code));
            }
        };

        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.cancel",
                "run_id": run_id,
                "reason": &reason,
            }),
        )
        .await?;

        let pending_approvals: Vec<ApprovalId> = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(|(approval_id, pending)| (pending.run_id == run_id).then_some(*approval_id))
            .collect();
        for approval_id in pending_approvals {
            if let Err(error) = self
                .approvals
                .invalidate(&context, approval_id, &reason)
                .await
            {
                let error = redact_event_text(&error.to_string());
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.failed",
                    json!({
                        "run_id": run_id,
                        "error": &error,
                        "reason": "approval_invalidation_failed",
                        "approval_id": approval_id,
                    }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, "read-only"),
                    error: Some(format!("approval_invalidation_failed:{error}")),
                });
            }
            self.pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&approval_id);
        }

        self.signal_cancel(run_id);
        let events = match self
            .runner
            .send(RunnerCommand::Cancel {
                run_id,
                reason: reason.clone(),
            })
            .await
        {
            Ok(events) => events,
            Err(error) => {
                let error = redact_event_text(&error.to_string());
                let error = format!("result_unknown:{error}");
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.result_unknown",
                    json!({ "run_id": run_id, "error": &error }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::ResultUnknown,
                    output: run_identity(&context, run_id, "read-only"),
                    error: Some(error),
                });
            }
        };

        let response_run_ids_match = events.iter().all(|event| event.run_id() == run_id);
        let matching_failure_errors = events
            .iter()
            .filter_map(|event| match event {
                RunnerEvent::Failed {
                    run_id: event_run_id,
                    error,
                } if *event_run_id == run_id => Some(error.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let matching_non_cancel_error = matching_failure_errors
            .iter()
            .copied()
            .find(|error| !error.starts_with("cancelled:"));
        let has_matching_completion = events.iter().any(|event| {
            matches!(
                event,
                RunnerEvent::Completed {
                    run_id: event_run_id,
                    ..
                } if *event_run_id == run_id
            )
        });
        let cancelled = response_run_ids_match
            && matching_failure_errors
                .iter()
                .any(|error| error.starts_with("cancelled:"))
            && matching_non_cancel_error.is_none()
            && !has_matching_completion;
        if cancelled {
            let cancelled = matching_failure_errors
                .iter()
                .copied()
                .find(|error| error.starts_with("cancelled:"))
                .map(redact_event_text)
                .expect("cancelled response was checked above");
            self.forget_session(context.session_id.as_str(), run_id);
            self.record_event(
                request_id,
                &mut sequence,
                "run.cancelled",
                json!({ "run_id": run_id, "error": &cancelled }),
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Cancelled,
                output: run_identity(&context, run_id, "read-only"),
                error: Some(cancelled),
            });
        }

        let runner_error = if !response_run_ids_match {
            "cancel_response_run_id_mismatch".to_owned()
        } else if has_matching_completion {
            "cancel_confirmation_inconsistent".to_owned()
        } else if let Some(error) = matching_non_cancel_error {
            redact_event_text(error)
        } else {
            "cancel_confirmation_missing".to_owned()
        };
        let error = format!("result_unknown:{runner_error}");
        self.record_event(
            request_id,
            &mut sequence,
            "run.result_unknown",
            json!({ "run_id": run_id, "error": &error }),
        )
        .await?;
        Ok(CoreResponse {
            request_id,
            status: ExecutionStatus::ResultUnknown,
            output: run_identity(&context, run_id, "read-only"),
            error: Some(error),
        })
    }

    async fn drive_run(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        mut pending_events: Vec<RunnerEvent>,
        sequence: &mut u64,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let cancel_rx = self.watch_cancel(run_id);
        let mut output = Value::Null;
        let mut failed = None;
        let mut completed = false;
        while !pending_events.is_empty() {
            let event = pending_events.remove(0);
            if event.run_id() != run_id {
                failed = Some("result_unknown:runner_event_run_id_mismatch".to_owned());
                break;
            }
            match event {
                RunnerEvent::Started { run_id } => {
                    self.record_event(
                        request_id,
                        sequence,
                        "run.started",
                        json!({ "run_id": run_id }),
                    )
                    .await?;
                }
                RunnerEvent::Delta { run_id, text } => {
                    self.record_event(
                        request_id,
                        sequence,
                        "run.delta",
                        json!({ "run_id": run_id, "text": text }),
                    )
                    .await?;
                }
                RunnerEvent::CapabilityRequested { run_id, request } => {
                    match self
                        .broker_harness_capability(
                            context, request_id, run_id, sandbox, sequence, request, &cancel_rx,
                        )
                        .await?
                    {
                        Ok(Some(events)) => pending_events.extend(events),
                        Ok(None) => {
                            let pending = self
                                .pending_invocations
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .values()
                                .find(|pending| pending.run_id == run_id)
                                .cloned();
                            let Some(pending) = pending else {
                                failed = Some("pending_invocation_missing".to_owned());
                                break;
                            };
                            self.clear_cancel(run_id);
                            return Ok(CoreResponse {
                                request_id,
                                status: ExecutionStatus::AwaitingApproval,
                                output: json!({
                                    "approval": pending.challenge,
                                    "capability": pending.request,
                                    "run_id": run_id
                                }),
                                error: Some("approval_required".to_owned()),
                            });
                        }
                        Err(reason) => {
                            failed = Some(reason);
                            break;
                        }
                    }
                }
                RunnerEvent::Completed {
                    run_id,
                    output: mut harness_output,
                } => {
                    if let Some(object) = harness_output.as_object_mut() {
                        object.insert("run_id".to_owned(), json!(run_id));
                    }
                    output = redact_event_value(&harness_output);
                    completed = true;
                    self.record_event(request_id, sequence, "run.completed", output.clone())
                        .await?;
                }
                RunnerEvent::Failed { run_id, error } => {
                    let error = redact_event_text(&error);
                    failed = Some(error.clone());
                    self.record_event(
                        request_id,
                        sequence,
                        "run.failed",
                        json!({ "run_id": run_id, "error": error }),
                    )
                    .await?;
                }
                RunnerEvent::Compacted {
                    run_id,
                    tokens_before,
                    tokens_after,
                    summary_present,
                } => {
                    self.record_event(
                        request_id,
                        sequence,
                        "run.compacted",
                        json!({
                            "schema": COMPACT_SCHEMA,
                            "run_id": run_id,
                            "tokens_before": tokens_before,
                            "tokens_after": tokens_after,
                            "summary_present": summary_present,
                        }),
                    )
                    .await?;
                }
            }
            if completed || failed.is_some() {
                break;
            }
        }
        self.clear_cancel(run_id);

        if let Some(error) = failed {
            if error == "run_not_found" {
                self.forget_session(context.session_id.as_str(), run_id);
            }
            let result_unknown = error.strip_prefix("result_unknown:").is_some();
            if result_unknown {
                let _ = self
                    .record_event(
                        request_id,
                        sequence,
                        "run.result_unknown",
                        json!({ "run_id": run_id, "error": redact_event_text(&error) }),
                    )
                    .await;
            }
            return Ok(CoreResponse {
                request_id,
                status: if error == "run_not_found" {
                    ExecutionStatus::Blocked
                } else if result_unknown {
                    ExecutionStatus::ResultUnknown
                } else if error.starts_with("cancelled:") {
                    ExecutionStatus::Cancelled
                } else {
                    ExecutionStatus::Failed
                },
                output: run_identity(context, run_id, sandbox),
                error: Some(error),
            });
        }
        if !completed {
            let reason = "run_result_missing";
            self.record_event(
                request_id,
                sequence,
                "run.result_unknown",
                json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: run_identity(context, run_id, sandbox),
                error: Some(reason.to_owned()),
            });
        }

        self.remember_session(&context, run_id);
        let receipt = self
            .run_receipt_from_store(context, run_id, sandbox, output)
            .await?;
        self.record_event(request_id, sequence, "run.receipt", receipt.clone())
            .await?;
        Ok(CoreResponse::completed(request_id, receipt))
    }

    pub async fn read_receipt(
        &self,
        context: RequestContext,
        run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let run_id = match run_id {
            Some(run_id) => {
                if let Some(binding) = self.session_binding(context.session_id.as_str()) {
                    if !Self::same_session_principal(&binding, &context) || binding.run_id != run_id
                    {
                        return Ok(CoreResponse::blocked(request_id, "run_owner_mismatch"));
                    }
                }
                run_id
            }
            None => match self.resolve_run_id(&context, None) {
                Ok(run_id) => run_id,
                Err(reason) => return Ok(CoreResponse::blocked(request_id, reason)),
            },
        };
        let events = self.events_for_persisted_run(run_id).await?;
        if events.is_empty() {
            return Ok(CoreResponse::blocked(request_id, "receipt_not_found"));
        }
        if receipt_owner_mismatch(&events, &context) {
            return Ok(CoreResponse::blocked(request_id, "run_owner_mismatch"));
        }
        let sandbox = events
            .iter()
            .rev()
            .find_map(|event| event.data.get("sandbox").and_then(Value::as_str))
            .unwrap_or("read-only");
        let terminal_error = |kind: &str, fallback: &str| {
            events
                .iter()
                .rev()
                .find(|event| event.kind == kind)
                .and_then(|event| event.data.get("error").and_then(Value::as_str))
                .map(redact_event_text)
                .unwrap_or_else(|| fallback.to_owned())
        };
        let has_completed = events.iter().any(|event| event.kind == "run.completed");
        let has_failed = events.iter().any(|event| event.kind == "run.failed");
        let has_cancelled = events.iter().any(|event| event.kind == "run.cancelled");
        let has_result_unknown = events
            .iter()
            .any(|event| event.kind == "run.result_unknown");
        let terminal_count = has_completed as usize
            + has_failed as usize
            + has_cancelled as usize
            + has_result_unknown as usize;

        // A replay may not invent success from an incomplete or contradictory event stream.
        // There is no reconciliation authority here, so either condition remains unknown.
        if has_result_unknown || terminal_count > 1 {
            let error = if has_result_unknown {
                terminal_error("run.result_unknown", "result_unknown")
            } else {
                "run_terminal_conflict".to_owned()
            };
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some(error),
            });
        }
        if has_cancelled {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Cancelled,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some(terminal_error("run.cancelled", "run_cancelled")),
            });
        }
        if has_failed {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Failed,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some(terminal_error("run.failed", "run_failed")),
            });
        }
        if !has_completed {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some("run_result_missing".to_owned()),
            });
        }
        let output = events
            .iter()
            .rev()
            .find(|event| event.kind == "run.completed")
            .map(|event| event.data.clone())
            .unwrap_or(Value::Null);
        Ok(CoreResponse::completed(
            request_id,
            receipt_from_events(&context, run_id, sandbox, output, &events),
        ))
    }

    async fn run_receipt_from_store(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        output: Value,
    ) -> Result<Value, CoreError> {
        let events = self.events_for_current_run(context, run_id).await?;
        Ok(receipt_from_events(
            context, run_id, sandbox, output, &events,
        ))
    }

    async fn approval_cursor(
        &self,
        approval_id: ApprovalId,
    ) -> Result<Option<PersistedApprovalCursor>, CoreError> {
        let Some(events) = self.read_all_events().await? else {
            // A legacy adapter may intentionally expose only read_request. That is a
            // capability limitation, not evidence that a persisted Run association is absent.
            return Ok(None);
        };
        let approval_id = approval_id.to_string();
        Ok(events.iter().rev().find_map(|event| {
            if event.kind != "approval.requested"
                || event.data.get("approval_id").and_then(Value::as_str)
                    != Some(approval_id.as_str())
            {
                return None;
            }
            let run_id = event
                .data
                .get("run_id")
                .and_then(Value::as_str)
                .and_then(RunId::parse_str)?;
            Some(PersistedApprovalCursor {
                run_id,
                event_request_id: event.request_id,
                event_sequence: event.sequence,
                continuation_recorded: events.iter().any(|candidate| {
                    candidate.kind == "approval.continuation_unavailable"
                        && candidate.data.get("approval_id").and_then(Value::as_str)
                            == Some(approval_id.as_str())
                }),
            })
        }))
    }

    async fn events_for_persisted_run(
        &self,
        run_id: RunId,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        match self.read_all_events().await? {
            Some(all) => Ok(filter_run_events(&all, run_id)),
            None => {
                let events = self.events.read_stream("run", &run_id.to_string()).await?;
                Ok(filter_run_events(&events, run_id))
            }
        }
    }

    /// Build an immediate receipt for the command that just produced this run.
    /// This is the sole request-scoped compatibility path: a later public receipt
    /// has no trustworthy request-to-run association to use as a fallback.
    async fn events_for_current_run(
        &self,
        context: &RequestContext,
        run_id: RunId,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        match self.read_all_events().await? {
            Some(all) => Ok(filter_run_events(&all, run_id)),
            None => {
                let events = self.events.read_request(&context.request_id).await?;
                Ok(filter_run_events(&events, run_id))
            }
        }
    }

    async fn events_for_author(
        &self,
        reviewer: &RequestContext,
        author_session_id: &str,
        author_run_id: Option<RunId>,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        let run_id = author_run_id
            .or_else(|| RunId::parse_str(author_session_id))
            .or_else(|| self.session_run_id(author_session_id));
        let events = match self.read_all_events().await? {
            Some(all) => match run_id {
                Some(run_id) => filter_run_events(&all, run_id),
                None => unique_authorized_run_id(&all, reviewer, author_session_id)
                    .map(|run_id| filter_run_events(&all, run_id))
                    .unwrap_or_default(),
            },
            None => {
                let run_id = run_id.ok_or_else(|| {
                    CoreError::Port(PortError::Failed(
                        "event_store_read_all_unsupported".to_owned(),
                    ))
                })?;
                let events = self.events.read_stream("run", &run_id.to_string()).await?;
                filter_run_events(&events, run_id)
            }
        };
        let identity = events.iter().find(|event| event.kind == "run.authorized");
        if let Some(identity) = identity {
            let session_matches = identity
                .data
                .get("session_id")
                .and_then(Value::as_str)
                .is_none_or(|session| session == author_session_id);
            let project_matches = identity
                .data
                .get("project_root")
                .and_then(Value::as_str)
                .is_none_or(|project| {
                    Self::canonical_project_root(project)
                        == Self::canonical_project_root(&reviewer.project_root)
                });
            let actor_matches = match (
                identity.data.get("actor_id").and_then(Value::as_str),
                reviewer.actor_id.as_deref(),
            ) {
                (Some(actor), Some(expected)) => actor == expected,
                _ => true,
            };
            if !session_matches || !project_matches || !actor_matches {
                return Ok(Vec::new());
            }
        }
        Ok(events)
    }

    /// `None` is reserved for the explicit legacy capability limit documented on
    /// `EventStorePort::read_all`; every actual read failure must reach the caller.
    async fn read_all_events(&self) -> Result<Option<Vec<RuntimeEvent>>, CoreError> {
        match self.events.read_all().await {
            Ok(events) => Ok(Some(events)),
            Err(PortError::Failed(reason)) if reason == "event_store_read_all_unsupported" => {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn bind_cell_scope(
        &self,
        context: &RequestContext,
        request: &mut CapabilityRequest,
    ) -> Result<(), CoreError> {
        let Some(cell_id) = context.cell_id else {
            if request.capability_grant_id.is_some() || request.budget_lease_id.is_some() {
                return Err(CoreError::Port(PortError::Failed(
                    "cell_capability_scope_incomplete".to_owned(),
                )));
            }
            return Ok(());
        };
        let reservation = self
            .cell_registry
            .reservation_for_cell(cell_id)
            .await?
            .ok_or_else(|| CoreError::Port(PortError::Failed("cell_not_found".to_owned())))?;
        request.cell_id = Some(cell_id);
        request.capability_grant_id = Some(reservation.grant.grant_id);
        request.budget_lease_id = Some(reservation.budget.lease_id);
        Ok(())
    }

    async fn begin_cell_capability_from_request(
        &self,
        request: &CapabilityRequest,
    ) -> Result<Option<CapabilityLease>, CoreError> {
        let Some(cell_id) = request.cell_id else {
            if request.capability_grant_id.is_some() || request.budget_lease_id.is_some() {
                return Err(CoreError::Port(PortError::Failed(
                    "cell_capability_scope_incomplete".to_owned(),
                )));
            }
            return Ok(None);
        };
        let grant_id = request.capability_grant_id.ok_or_else(|| {
            CoreError::Port(PortError::Failed(
                "cell_capability_scope_incomplete".to_owned(),
            ))
        })?;
        let budget_id = request.budget_lease_id.ok_or_else(|| {
            CoreError::Port(PortError::Failed(
                "cell_capability_scope_incomplete".to_owned(),
            ))
        })?;
        Ok(Some(
            self.cell_registry
                .begin_capability(cell_id, grant_id, budget_id, request)
                .await?,
        ))
    }

    async fn finish_cell_capability(
        &self,
        lease: CapabilityLease,
        outcome: CapabilityOutcome,
    ) -> Result<(), CoreError> {
        self.cell_registry
            .finish_capability(lease, outcome)
            .await
            .map_err(CoreError::from)
    }

    async fn broker_harness_capability(
        &self,
        context: &RequestContext,
        request_id: kiana_domain::RequestId,
        run_id: RunId,
        sandbox: &str,
        sequence: &mut u64,
        mut request: CapabilityRequest,
        cancel_rx: &watch::Receiver<bool>,
    ) -> Result<Result<Option<Vec<RunnerEvent>>, String>, CoreError> {
        stamp_request_identity(&mut request, context);
        if let Err(error) = self.bind_cell_scope(context, &mut request).await {
            let reason = error.to_string();
            self.record_event(
                request_id,
                sequence,
                "run.capability_blocked",
                json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
            )
            .await?;
            return Ok(Err(reason));
        }
        self.record_event(
            request_id,
            sequence,
            "run.capability_requested",
            json!({
                "run_id": run_id,
                "request_id": request.request_id,
                "capability": request.capability,
                "operation": request.operation,
                "risk": request.risk,
                "cell_id": request.cell_id,
                "capability_grant_id": request.capability_grant_id,
                "budget_lease_id": request.budget_lease_id,
                "arguments": redact_event_value(&request.arguments),
            }),
        )
        .await?;

        if *cancel_rx.borrow() {
            self.record_event(
                request_id,
                sequence,
                "run.cancelled",
                json!({ "run_id": run_id, "error": "cancelled:user" }),
            )
            .await?;
            return Ok(Err("cancelled:user".to_owned()));
        }

        let policy = self.evaluate_policy(context, &request);
        let gate = self.evaluate_gate(&request, &policy);
        self.record_event(
            request_id,
            sequence,
            "capability.decision",
            json!({ "run_id": run_id, "policy": &policy, "gate": &gate }),
        )
        .await?;

        let result = match gate {
            GateDecision::Allowed { authorization_id } => {
                let hook_decision = self.pre_tool_hooks.decide(context, &request).await;
                let hook_error = match hook_decision {
                    Ok(PreToolHookDecision::Allow) => None,
                    Ok(PreToolHookDecision::Block(reason)) => {
                        Some(format!("hook_blocked:{reason}"))
                    }
                    Ok(PreToolHookDecision::Ask { reason }) => {
                        Some(format!("hook_ask_unattended:{reason}"))
                    }
                    Err(error) => Some(format!("hook_blocked:{error}")),
                };
                if let Some(reason) = hook_error {
                    self.record_event(
                        request_id,
                        sequence,
                        "capability.failed",
                        json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                    )
                    .await?;
                    CapabilityResult::failure(request.request_id, reason)
                } else {
                    let cell_lease = match self.begin_cell_capability_from_request(&request).await {
                        Ok(lease) => lease,
                        Err(error) => {
                            let reason = error.to_string();
                            self.record_event(
                                request_id,
                                sequence,
                                "capability.failed",
                                json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                            )
                            .await?;
                            return Ok(Err(reason));
                        }
                    };
                    let authorized =
                        AuthorizedCapabilityRequest::new(authorization_id, request.clone())?;
                    tokio::select! {
                        biased;
                        _ = wait_until_cancelled(cancel_rx) => {
                            if let Some(lease) = cell_lease {
                                self.finish_cell_capability(lease, CapabilityOutcome::Unknown)
                                    .await?;
                            }
                            self.record_event(
                                request_id,
                                sequence,
                                "run.cancelled",
                                json!({ "run_id": run_id, "error": "cancelled:user" }),
                            )
                            .await?;
                            return Ok(Err("cancelled:user".to_owned()));
                        }
                        executed = self.capabilities.execute(authorized) => {
                            match executed {
                                Ok(result) => {
                                    let result = redact_capability_result(result);
                                    let outcome = if result.request_id != request.request_id {
                                        CapabilityOutcome::Unknown
                                    } else if result.success {
                                        CapabilityOutcome::Succeeded
                                    } else {
                                        CapabilityOutcome::Failed
                                    };
                                    if result.request_id != request.request_id {
                                        if let Some(lease) = cell_lease {
                                            self.finish_cell_capability(lease, outcome).await?;
                                        }
                                        return Ok(Err(
                                            "result_unknown:capability_result_mismatch".to_owned(),
                                        ));
                                    }
                                    let result_event = self.record_event(
                                        request_id,
                                        sequence,
                                        if result.success {
                                            "capability.completed"
                                        } else {
                                            "capability.failed"
                                        },
                                        capability_event_payload(
                                            &result.output,
                                            &request,
                                            context,
                                            run_id,
                                        ),
                                    )
                                    .await;
                                    if let Err(error) = result_event {
                                        if let Some(lease) = cell_lease {
                                            self.finish_cell_capability(lease, CapabilityOutcome::Unknown)
                                                .await?;
                                        }
                                        return Ok(Err(format!(
                                            "result_unknown:{}",
                                            redact_event_text(&error.to_string())
                                        )));
                                    }
                                    if let Some(lease) = cell_lease {
                                        self.finish_cell_capability(lease, outcome).await?;
                                    }
                                    result
                                }
                                Err(error) => {
                                    if let Some(lease) = cell_lease {
                                        self.finish_cell_capability(lease, CapabilityOutcome::Unknown)
                                            .await?;
                                    }
                                    let reason = redact_event_text(&error.to_string());
                                    if reason.starts_with("shell_result_unknown:") {
                                        self.record_event(
                                            request_id,
                                            sequence,
                                            "capability.result_unknown",
                                            json!({
                                                "run_id": run_id,
                                                "capability_request_id": request.request_id,
                                                "error": redact_event_text(&reason),
                                            }),
                                        )
                                        .await?;
                                        return Ok(Err(format!("result_unknown:{reason}")));
                                    }
                                    self.record_event(
                                        request_id,
                                        sequence,
                                        "capability.failed",
                                        json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                                    )
                                    .await?;
                                    CapabilityResult::failure(request.request_id, reason)
                                }
                            }
                        }
                    }
                }
            }
            GateDecision::Denied { reason }
                if reason.starts_with("role_") || reason.starts_with("packet_") =>
            {
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({ "run_id": run_id, "reason": &reason }),
                )
                .await?;
                return Ok(Err(reason));
            }
            GateDecision::AwaitingApproval { reason } => {
                let mut approval_context = context.clone();
                approval_context.request_id = request.request_id;
                let challenge = self
                    .approvals
                    .stage(&approval_context, request.clone(), &reason)
                    .await?;
                self.record_event(
                    request_id,
                    sequence,
                    "approval.requested",
                    json!({
                        "run_id": run_id,
                        "approval_id": challenge.approval_id,
                        "request_hash": &challenge.request_hash,
                        "session_id": context.session_id,
                        "actor_id": context.actor_id,
                        "expires_at_unix_ms": challenge.expires_at_unix_ms,
                    }),
                )
                .await?;
                self.approvals.activate(challenge.approval_id).await?;
                self.pending_invocations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(
                        challenge.approval_id,
                        PendingInvocation {
                            approval_id: challenge.approval_id,
                            challenge: challenge.clone(),
                            request_id: request.request_id,
                            event_request_id: request_id,
                            event_sequence: *sequence + 1,
                            run_id,
                            request,
                            context: context.clone(),
                            sandbox: sandbox.to_owned(),
                        },
                    );
                self.record_event(
                    request_id,
                    sequence,
                    "run.awaiting_approval",
                    json!({ "run_id": run_id, "approval_id": challenge.approval_id }),
                )
                .await?;
                return Ok(Ok(None));
            }
            GateDecision::Denied { reason } => {
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({ "run_id": run_id, "reason": &reason }),
                )
                .await?;
                CapabilityResult::failure(
                    request.request_id,
                    format!("capability_blocked:{reason}"),
                )
            }
        };

        let result = redact_capability_result(result);
        match self
            .runner
            .send(RunnerCommand::CapabilityResult { run_id, result })
            .await
        {
            Ok(events) => Ok(Ok(Some(events))),
            Err(error) => {
                let reason = redact_event_text(&error.to_string());
                self.record_event(
                    request_id,
                    sequence,
                    "run.failed",
                    json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                )
                .await?;
                Ok(Err(reason))
            }
        }
    }

    pub async fn send_runner(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, CoreError> {
        Ok(self.runner.send(command).await?)
    }

    fn resolve_run_id(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
    ) -> Result<RunId, &'static str> {
        let sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        let binding = sessions
            .get(context.session_id.as_str())
            .ok_or("session_not_found")?;
        if !Self::same_session_principal(binding, context) {
            return Err("session_owner_mismatch");
        }
        if let Some(requested) = run_id {
            if requested != binding.run_id {
                return Err("run_owner_mismatch");
            }
        }
        Ok(binding.run_id)
    }

    fn session_binding(&self, session_id: &str) -> Option<SessionBinding> {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session_id)
            .cloned()
    }

    fn session_known(&self, session_id: &str) -> bool {
        self.session_run_id(session_id).is_some()
    }

    fn session_run_id(&self, session_id: &str) -> Option<RunId> {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session_id)
            .map(|binding| binding.run_id)
    }

    fn remember_session(&self, context: &RequestContext, run_id: RunId) {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                context.session_id.as_str().to_owned(),
                SessionBinding {
                    run_id,
                    actor_id: context.actor_id.clone(),
                    project_root: context.project_root.clone(),
                    role_id: context.role_id.clone(),
                    department_id: context.department_id.clone(),
                },
            );
    }

    fn forget_session(&self, session_id: &str, run_id: RunId) {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if sessions
            .get(session_id)
            .is_some_and(|binding| binding.run_id == run_id)
        {
            sessions.remove(session_id);
        }
    }

    fn same_session_principal(binding: &SessionBinding, context: &RequestContext) -> bool {
        binding.actor_id == context.actor_id
            && Self::canonical_project_root(&binding.project_root)
                == Self::canonical_project_root(&context.project_root)
            && binding.role_id == context.role_id
            && binding.department_id == context.department_id
    }

    fn canonical_project_root(root: &str) -> std::path::PathBuf {
        let path = Path::new(root);
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    fn acquire_builder_path_locks(
        &self,
        project_root: &str,
        session_id: &str,
        path_allow: &[String],
    ) -> Result<(), &'static str> {
        let paths = builder_lock_paths(path_allow);
        let mut locks = self
            .path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for path in &paths {
            for (held, owner) in locks.iter() {
                if owner != session_id && path_locks_conflict(path, held) {
                    return Err("path_lock_conflict");
                }
            }
        }

        let lock_key = path_lock_session_key(project_root, session_id);
        let durable = match acquire_durable_path_locks(project_root, &paths) {
            Ok(durable) => durable,
            Err(reason) => return Err(reason),
        };
        for path in paths {
            locks.insert(path, session_id.to_owned());
        }
        self.durable_path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(lock_key, durable);
        Ok(())
    }

    fn release_builder_path_locks(&self, project_root: &str, session_id: &str) {
        self.path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, owner| owner != session_id);
        self.durable_path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&path_lock_session_key(project_root, session_id));
    }

    fn watch_cancel(&self, run_id: RunId) -> watch::Receiver<bool> {
        let mut cancellations = self
            .cancellations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = cancellations.get(&run_id) {
            return tx.subscribe();
        }
        let (tx, rx) = watch::channel(false);
        cancellations.insert(run_id, tx);
        rx
    }

    fn signal_cancel(&self, run_id: RunId) -> bool {
        let mut cancellations = self
            .cancellations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = cancellations.get(&run_id) {
            let _ = tx.send(true);
            return true;
        }
        // Cancel arrived before drive_run subscribed: leave a pre-signaled
        // watch so the in-flight capability select! still aborts.
        let (tx, _rx) = watch::channel(true);
        cancellations.insert(run_id, tx);
        false
    }

    fn clear_cancel(&self, run_id: RunId) {
        self.cancellations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&run_id);
    }

    async fn record_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: &mut u64,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        self.append_event(request_id, *sequence, kind, data).await?;
        *sequence += 1;
        Ok(())
    }

    async fn append_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: u64,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        // EventLog is the canonical boundary: generic runner output must be redacted before it
        // can become durable fact or feed a receipt projection.
        let data = redact_event_value(&data);
        let (aggregate_type, aggregate_id) = aggregate_for_event(request_id, &data);
        let idempotency_key =
            format!("{request_id}:{aggregate_type}:{aggregate_id}:{sequence}:{kind}");
        let current_version = self
            .events
            .read_stream(&aggregate_type, &aggregate_id)
            .await?
            .iter()
            .map(|event| event.stream_version.unwrap_or(event.sequence))
            .max()
            .unwrap_or(0);
        self.events
            .append_idempotent_expected(
                RuntimeEvent::new(request_id, sequence, kind, data)?
                    .with_stream_metadata(
                        aggregate_type,
                        aggregate_id,
                        current_version.saturating_add(1),
                    )
                    .with_idempotency_key(idempotency_key),
                Some(current_version),
            )
            .await?;
        Ok(())
    }
}

fn aggregate_for_event(request_id: kiana_domain::RequestId, data: &Value) -> (String, String) {
    if let Some(packet_id) = data
        .get("packet_id")
        .and_then(Value::as_str)
        .filter(|packet_id| !packet_id.trim().is_empty())
    {
        return ("work_packet".to_owned(), packet_id.to_owned());
    }
    if let Some(run_id) = data
        .get("run_id")
        .and_then(Value::as_str)
        .filter(|run_id| !run_id.trim().is_empty())
    {
        return ("run".to_owned(), run_id.to_owned());
    }
    ("request".to_owned(), request_id.to_string())
}

fn run_identity(context: &RequestContext, run_id: RunId, sandbox: &str) -> Value {
    let worker = RoleSpec::lookup(&context.role_id).unwrap_or_else(RoleSpec::builder);
    with_work_packet(
        json!({
            "schema": RUN_RESULT_SCHEMA,
            "run_id": run_id,
            "session_id": context.session_id,
            "harness": HARNESS_ID,
            "sandbox": sandbox,
            "actor_id": context.actor_id,
            "role_id": worker.role_id,
            "department_id": worker.department_id,
            "prompt_hash": worker.prompt_hash,
        }),
        context,
    )
}

fn receipt_from_events(
    context: &RequestContext,
    run_id: RunId,
    sandbox: &str,
    output: Value,
    events: &[RuntimeEvent],
) -> Value {
    let worker = RoleSpec::lookup(&context.role_id).unwrap_or_else(RoleSpec::builder);
    let receipt = with_work_packet(
        json!({
            "schema": RUN_RESULT_SCHEMA,
            "run_id": run_id,
            "session_id": context.session_id,
            "harness": HARNESS_ID,
            "sandbox": sandbox,
            "actor_id": context.actor_id,
            "role_id": worker.role_id,
            "department_id": worker.department_id,
            "prompt_hash": worker.prompt_hash,
            "files_changed": files_changed_from_events(events),
            "memory_hits": memory_hits_from_events(events),
            "compact": compact_from_events(events),
            "capabilities": capabilities_from_events(events),
            "output": output,
        }),
        context,
    );
    // Re-apply the boundary while projecting so legacy events written before centralized
    // redaction cannot reintroduce a credential into a restart receipt.
    redact_event_value(&receipt)
}

fn with_work_packet(mut receipt: Value, context: &RequestContext) -> Value {
    if let Some(work_packet_id) = context
        .work_packet_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        receipt["work_packet_id"] = json!(work_packet_id);
        receipt["input"] = json!("work_packet");
    }
    receipt
}

fn path_lock_session_key(project_root: &str, session_id: &str) -> String {
    format!(
        "{}\0{}",
        ControlPlane::canonical_project_root(project_root).display(),
        session_id
    )
}

fn durable_path_lock_root() -> PathBuf {
    std::env::var_os("KIANA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".kiana")))
        .unwrap_or_else(|| PathBuf::from(".kiana"))
        .join("locks")
}

fn durable_path_lock_path(project_root: &str, path: &str) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ControlPlane::canonical_project_root(project_root)
        .display()
        .to_string()
        .hash(&mut hasher);
    path.hash(&mut hasher);
    durable_path_lock_root().join(format!("{:016x}.lock", hasher.finish()))
}

fn acquire_durable_path_locks(
    project_root: &str,
    paths: &[String],
) -> Result<Vec<PathLockLease>, &'static str> {
    let root = durable_path_lock_root();
    fs::create_dir_all(&root).map_err(|_| "path_lock_unavailable")?;
    let mut leases = Vec::with_capacity(paths.len());
    for path in paths {
        let lock_path = durable_path_lock_path(project_root, path);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "path_lock_unavailable")?;
        if !try_lock_path_file(&file) {
            return Err("path_lock_conflict");
        }
        leases.push(PathLockLease { _file: file });
    }
    Ok(leases)
}

#[cfg(unix)]
fn try_lock_path_file(file: &File) -> bool {
    use std::os::fd::AsRawFd;
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

#[cfg(not(unix))]
fn try_lock_path_file(_file: &File) -> bool {
    true
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn spawn_error_reason(error: &CoreError) -> String {
    error.to_string()
}

fn cell_event_payload(reservation: &kiana_ports::SpawnReservation, lifecycle: &str) -> Value {
    json!({
        "schema": CELL_SCHEMA,
        "cell_id": reservation.cell.cell_id,
        "plan_id": reservation.plan.plan_id,
        "run_id": reservation.cell.root_run_id,
        "role_id": reservation.cell.role_id,
        "lifecycle": lifecycle,
        "owned_paths": reservation.owned_paths,
        "capability_grant_id": reservation.grant.grant_id,
        "budget_lease_id": reservation.budget.lease_id,
        "supervision_lease_id": reservation.supervision.lease_id,
        "replayed": reservation.replayed,
    })
}

fn spawn_event_payload(reservation: &kiana_ports::SpawnReservation) -> Value {
    json!({
        "schema": SPAWN_PLAN_SCHEMA,
        "plan_id": reservation.plan.plan_id,
        "run_id": reservation.cell.root_run_id,
        "cell_id": reservation.cell.cell_id,
        "status": reservation.plan.status,
        "idempotency_key": reservation.plan.idempotency_key,
        "fingerprint": reservation.fingerprint,
        "replayed": reservation.replayed,
    })
}

fn retirement_event_payload(retirement: &kiana_domain::RetirementRecord) -> Value {
    json!({
        "schema": kiana_domain::RETIREMENT_RECORD_SCHEMA,
        "cell_id": retirement.cell_id,
        "grant_id": retirement.grant_id,
        "budget_lease_id": retirement.budget_lease_id,
        "supervision_lease_id": retirement.supervision_lease_id,
        "released_paths": retirement.released_paths,
        "reason": retirement.reason,
        "retired_at_unix_ms": retirement.retired_at_unix_ms,
    })
}

fn cell_event_kind(lifecycle: kiana_domain::CellLifecycle) -> &'static str {
    match lifecycle {
        kiana_domain::CellLifecycle::Validated => "cell.validated",
        kiana_domain::CellLifecycle::Spawning => "cell.spawning",
        kiana_domain::CellLifecycle::Ready => "cell.ready",
        kiana_domain::CellLifecycle::Running => "cell.started",
        kiana_domain::CellLifecycle::WaitingInput => "cell.waiting_input",
        kiana_domain::CellLifecycle::CancelRequested => "cell.cancel_requested",
        kiana_domain::CellLifecycle::Cancelled => "cell.cancelled",
        kiana_domain::CellLifecycle::Blocked => "cell.blocked",
        kiana_domain::CellLifecycle::ReadyToMerge => "cell.ready_to_merge",
        kiana_domain::CellLifecycle::Quarantined => "cell.quarantined",
        kiana_domain::CellLifecycle::Failed => "cell.failed",
        kiana_domain::CellLifecycle::Retired => "cell.retired",
        _ => "cell.lifecycle_changed",
    }
}

fn receipt_owner_mismatch(events: &[RuntimeEvent], context: &RequestContext) -> bool {
    let Some(identity) = events.iter().find(|event| event.kind == "run.authorized") else {
        return false;
    };
    let session_matches = identity
        .data
        .get("session_id")
        .and_then(Value::as_str)
        .is_none_or(|session| session == context.session_id.as_str());
    let project_matches = identity
        .data
        .get("project_root")
        .and_then(Value::as_str)
        .is_none_or(|project| {
            ControlPlane::canonical_project_root(project)
                == ControlPlane::canonical_project_root(&context.project_root)
        });
    let actor_matches = match (
        identity.data.get("actor_id").and_then(Value::as_str),
        context.actor_id.as_deref(),
    ) {
        (Some(actor), Some(expected)) => actor == expected,
        _ => true,
    };
    let role_matches = identity
        .data
        .get("role_id")
        .and_then(Value::as_str)
        .is_none_or(|role| role == context.role_id);
    let department_matches = identity
        .data
        .get("department_id")
        .and_then(Value::as_str)
        .is_none_or(|department| department == context.department_id);
    !(session_matches && project_matches && actor_matches && role_matches && department_matches)
}

fn filter_run_events(events: &[RuntimeEvent], run_id: RunId) -> Vec<RuntimeEvent> {
    let run_id_str = run_id.to_string();
    events
        .iter()
        .filter(|event| {
            let stream = (
                event.aggregate_type.as_deref(),
                event.aggregate_id.as_deref(),
            );
            let exact_run_stream = stream == (Some("run"), Some(run_id_str.as_str()));
            let conflicting_run_stream =
                matches!(stream, (Some("run"), Some(_))) && !exact_run_stream;
            match event.data.get("run_id") {
                Some(Value::String(value)) => value == &run_id_str && !conflicting_run_stream,
                Some(_) => false,
                // Legacy events without a payload run ID are only usable when their
                // durable aggregate metadata identifies this exact run.
                None => exact_run_stream,
            }
        })
        .cloned()
        .collect()
}

fn unique_authorized_run_id(
    events: &[RuntimeEvent],
    reviewer: &RequestContext,
    author_session_id: &str,
) -> Option<RunId> {
    let reviewer_actor_id = reviewer.actor_id.as_deref()?;
    let reviewer_project_root = ControlPlane::canonical_project_root(&reviewer.project_root);
    let candidates: HashSet<_> = events
        .iter()
        .filter_map(|event| {
            if event.kind != "run.authorized"
                || event.data.get("session_id").and_then(Value::as_str) != Some(author_session_id)
                || event.data.get("actor_id").and_then(Value::as_str) != Some(reviewer_actor_id)
                || event.data.get("role_id").and_then(Value::as_str) != Some(ROLE_BUILDER)
                || event.data.get("department_id").and_then(Value::as_str)
                    != Some(DEPARTMENT_EXECUTING)
            {
                return None;
            }
            let project_root = event.data.get("project_root").and_then(Value::as_str)?;
            if ControlPlane::canonical_project_root(project_root) != reviewer_project_root {
                return None;
            }
            event
                .data
                .get("run_id")
                .and_then(Value::as_str)
                .and_then(RunId::parse_str)
        })
        .collect();
    (candidates.len() == 1)
        .then(|| candidates.into_iter().next())
        .flatten()
}

fn capability_event_payload(
    output: &Value,
    request: &CapabilityRequest,
    context: &RequestContext,
    run_id: RunId,
) -> Value {
    let mut payload = redact_event_value(output);
    if !payload.is_object() {
        payload = json!({ "output": payload });
    }
    let object = payload
        .as_object_mut()
        .expect("capability event payload is normalized to an object");
    object.insert("run_id".to_owned(), json!(run_id));
    object.insert("session_id".to_owned(), json!(context.session_id));
    object.insert("capability".to_owned(), json!(request.capability));
    object.insert("operation".to_owned(), json!(request.operation));
    object.insert("cell_id".to_owned(), json!(request.cell_id));
    object.insert(
        "capability_grant_id".to_owned(),
        json!(request.capability_grant_id),
    );
    object.insert("budget_lease_id".to_owned(), json!(request.budget_lease_id));
    object.insert(
        "capability_request_id".to_owned(),
        json!(request.request_id),
    );
    payload
}

fn direct_capability_event_payload(output: &Value, request: &CapabilityRequest) -> Value {
    let mut payload = redact_event_value(output);
    if !payload.is_object() {
        payload = json!({ "output": payload });
    }
    let object = payload
        .as_object_mut()
        .expect("direct capability payload is normalized to an object");
    object.insert("capability".to_owned(), json!(request.capability));
    object.insert("operation".to_owned(), json!(request.operation));
    object.insert("cell_id".to_owned(), json!(request.cell_id));
    object.insert(
        "capability_grant_id".to_owned(),
        json!(request.capability_grant_id),
    );
    object.insert("budget_lease_id".to_owned(), json!(request.budget_lease_id));
    object.insert(
        "capability_request_id".to_owned(),
        json!(request.request_id),
    );
    payload
}

fn redact_event_text(text: &str) -> String {
    let trimmed = text.trim();
    if matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'[')) {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return redact_event_value(&value).to_string();
        }
    }

    const SENSITIVE_MARKERS: &[&str] = &[
        "token=",
        "password=",
        "api_key=",
        "access_key=",
        "private_key=",
        "secret=",
        "bearer ",
        "authorization: bearer ",
        "authorization: basic ",
        "x-api-key:",
        "\"token\":\"",
        "\"password\":\"",
        "\"api_key\":\"",
        "\"access_key\":\"",
        "\"private_key\":\"",
        "\"secret\":\"",
        "\"authorization\":\"bearer ",
    ];
    let mut redacted = text.to_owned();
    for marker in SENSITIVE_MARKERS {
        let marker_lower = marker.to_ascii_lowercase();
        let quoted = marker.ends_with('\"');
        let mut search_from = 0;
        while search_from < redacted.len() {
            let lower = redacted.to_ascii_lowercase();
            let Some(relative_start) = lower[search_from..].find(&marker_lower) else {
                break;
            };
            let start = search_from + relative_start + marker.len();
            let value_start = if quoted {
                start
            } else {
                start
                    + redacted[start..]
                        .chars()
                        .take_while(|character| character.is_whitespace())
                        .map(char::len_utf8)
                        .sum::<usize>()
            };
            let end = if quoted {
                redacted[value_start..]
                    .find('\"')
                    .map_or(redacted.len(), |relative_end| value_start + relative_end)
            } else {
                redacted[value_start..]
                    .find(|character: char| {
                        character.is_whitespace()
                            || matches!(character, '&' | ',' | ';' | '\"' | '}')
                    })
                    .map_or(redacted.len(), |relative_end| value_start + relative_end)
            };
            if end <= value_start {
                break;
            }
            redacted.replace_range(value_start..end, "[REDACTED]");
            search_from = value_start + "[REDACTED]".len();
        }
    }
    redacted
}

fn redact_event_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(redact_event_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let token_metric =
                        matches!(
                            normalized.as_str(),
                            "tokens_before"
                                | "tokens_after"
                                | "input_tokens"
                                | "output_tokens"
                                | "total_tokens"
                                | "cached_tokens"
                                | "reasoning_tokens"
                                | "max_tokens"
                                | "min_tokens"
                                | "token_count"
                                | "token_budget"
                                | "estimated_tokens"
                                | "token_overlap"
                        ) && matches!(value, Value::Number(_) | Value::Bool(_) | Value::Null);
                    let sensitive = normalized != "secret_ref"
                        && ((normalized.contains("token") && !token_metric)
                            || normalized.contains("password")
                            || normalized.contains("api_key")
                            || normalized.contains("access_key")
                            || normalized.contains("private_key")
                            || normalized.contains("secret"));
                    let value = if sensitive {
                        Value::String("[REDACTED]".to_owned())
                    } else {
                        redact_event_value(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::String(text) => Value::String(redact_event_text(text)),
        _ => value.clone(),
    }
}

fn redact_capability_result(result: CapabilityResult) -> CapabilityResult {
    CapabilityResult {
        request_id: result.request_id,
        success: result.success,
        output: redact_event_value(&result.output),
        evidence_refs: result
            .evidence_refs
            .iter()
            .map(|reference| redact_event_text(reference))
            .collect(),
    }
}

fn stamp_request_identity(request: &mut CapabilityRequest, context: &RequestContext) {
    let Some(arguments) = request.arguments.as_object_mut() else {
        return;
    };
    arguments.insert("role_id".to_owned(), json!(context.role_id));
    arguments.insert("department_id".to_owned(), json!(context.department_id));
    arguments.insert("session_id".to_owned(), json!(context.session_id.as_str()));
    arguments.insert("project_root".to_owned(), json!(context.project_root));
}

fn files_changed_from_events(events: &[RuntimeEvent]) -> Vec<String> {
    let mut files = Vec::new();
    for event in events {
        if event.kind != "capability.completed" {
            continue;
        }
        let Some(changed) = event.data.get("changed").and_then(Value::as_array) else {
            continue;
        };
        for item in changed {
            let Some(path) = item.get("path").and_then(Value::as_str) else {
                continue;
            };
            if !path.is_empty() && !files.iter().any(|existing| existing == path) {
                files.push(path.to_owned());
            }
        }
    }
    files
}

fn compact_from_events(events: &[RuntimeEvent]) -> Value {
    let mut count = 0u64;
    let mut last = None;
    for event in events {
        if event.kind != "run.compacted" {
            continue;
        }
        count += 1;
        last = Some(event.data.clone());
    }
    match last {
        Some(data) => json!({
            "applied": true,
            "count": count,
            "tokens_before": data.get("tokens_before"),
            "tokens_after": data.get("tokens_after"),
            "summary_present": data.get("summary_present"),
        }),
        None => json!({
            "applied": false,
            "count": 0,
        }),
    }
}

fn memory_hits_from_events(events: &[RuntimeEvent]) -> Vec<Value> {
    let mut hits = Vec::new();
    for event in events {
        if event.kind != "capability.completed" {
            continue;
        }
        if event.data.get("schema").and_then(Value::as_str) != Some(MEMORY_SEARCH_SCHEMA) {
            continue;
        }
        let Some(items) = event.data.get("hits").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            hits.push(item.clone());
        }
    }
    hits
}

fn capabilities_from_events(events: &[RuntimeEvent]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event.kind == "run.capability_requested")
        .map(|event| {
            json!({
                "capability": event.data.get("capability"),
                "operation": event.data.get("operation"),
            })
        })
        .collect()
}

async fn wait_until_cancelled(rx: &watch::Receiver<bool>) {
    let mut rx = rx.clone();
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}

fn write_symposium_artifacts(
    project_root: &str,
    meeting: &Symposium,
    decision: &DecisionRecord,
    packet: Option<&WorkPacket>,
) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("symposium_artifact_write_failed");
    }
    let decision_path = Path::new(meeting.decision_path());
    let decision_json =
        serde_json::to_string_pretty(decision).map_err(|_| "symposium_artifact_write_failed")?;
    write_project_artifact(
        root,
        decision_path,
        decision_json.as_bytes(),
        "symposium_artifact_write_failed",
    )?;
    if let Some(packet) = packet {
        let packet_json =
            serde_json::to_string_pretty(packet).map_err(|_| "symposium_artifact_write_failed")?;
        write_project_artifact(
            root,
            Path::new(WORK_PACKET_PATH),
            packet_json.as_bytes(),
            "symposium_artifact_write_failed",
        )?;
    }
    Ok(())
}

fn read_review_artifact(project_root: &str) -> Result<ReviewPacket, &'static str> {
    let root = Path::new(project_root);
    if !root.is_dir() {
        return Err("close_project_not_found");
    }
    let raw = read_project_artifact(
        root,
        Path::new(REVIEW_PACKET_PATH),
        "close_review_not_found",
    )?;
    let review: ReviewPacket = serde_json::from_str(&raw).map_err(|_| "close_review_invalid")?;
    review.validate().map_err(|_| "close_review_invalid")?;
    Ok(review)
}

fn write_closing_artifact(
    project_root: &str,
    receipt: &ClosingReceipt,
) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("closing_artifact_write_failed");
    }
    let receipt_json =
        serde_json::to_string_pretty(receipt).map_err(|_| "closing_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(kiana_domain::CLOSING_RECEIPT_PATH),
        receipt_json.as_bytes(),
        "closing_artifact_write_failed",
    )?;
    let mut learned = String::from("# Closing lessons\n\n");
    learned.push_str("The Builder output passed independent Review and was accepted by Closing.\n");
    if !receipt.files_verified.is_empty() {
        learned.push_str("\nVerified files:\n");
        for file in &receipt.files_verified {
            learned.push_str("- ");
            learned.push_str(file);
            learned.push('\n');
        }
    }
    write_project_artifact(
        root,
        Path::new("lessons/LEARNED.md"),
        learned.as_bytes(),
        "closing_artifact_write_failed",
    )
}

fn read_merge_artifact(project_root: &str) -> Result<MergeReceipt, &'static str> {
    let root = Path::new(project_root);
    let raw = read_project_artifact(
        root,
        Path::new(kiana_domain::MERGE_RECEIPT_PATH),
        "close_merge_receipt_not_found",
    )?;
    let merge: MergeReceipt =
        serde_json::from_str(&raw).map_err(|_| "close_merge_receipt_invalid")?;
    merge
        .validate()
        .map_err(|_| "close_merge_receipt_invalid")?;
    Ok(merge)
}

fn write_merge_artifact(project_root: &str, receipt: &MergeReceipt) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("merge_artifact_write_failed");
    }
    let receipt_json =
        serde_json::to_string_pretty(receipt).map_err(|_| "merge_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(kiana_domain::MERGE_RECEIPT_PATH),
        receipt_json.as_bytes(),
        "merge_artifact_write_failed",
    )
}

fn write_review_artifact(project_root: &str, packet: &ReviewPacket) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("review_artifact_write_failed");
    }
    let packet_json =
        serde_json::to_string_pretty(packet).map_err(|_| "review_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(REVIEW_PACKET_PATH),
        packet_json.as_bytes(),
        "review_artifact_write_failed",
    )?;
    Ok(())
}

const ARTIFACT_TEMP_ATTEMPTS: usize = 16;
#[cfg(target_os = "linux")]
const LINUX_RENAME_NOREPLACE: u32 = 1;
#[cfg(target_os = "linux")]
const LINUX_RENAME_EXCHANGE: u32 = 2;

#[cfg(target_os = "linux")]
fn read_project_artifact(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    read_project_artifact_linux(root, relative, error)
}

#[cfg(not(target_os = "linux"))]
fn read_project_artifact(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    let path = confined_artifact_path(root, relative, error)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|_| error)?;
    let mut raw = String::new();
    file.read_to_string(&mut raw).map_err(|_| error)?;
    Ok(raw)
}

#[cfg(target_os = "linux")]
fn write_project_artifact(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<(), &'static str> {
    prepare_project_artifact_linux(root, relative, contents, error)?.commit()
}

#[cfg(not(target_os = "linux"))]
fn write_project_artifact(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<(), &'static str> {
    let path = confined_artifact_path(root, relative, error)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| error)?;
    }
    confined_artifact_path(root, relative, error)?;
    let (temporary, mut file) = create_artifact_temp_sibling(&path, error)?;
    let write_result = (|| {
        file.write_all(contents).map_err(|_| error)?;
        file.flush().map_err(|_| error)?;
        file.sync_all().map_err(|_| error)
    })();
    drop(file);
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
        return write_result;
    }
    if confined_artifact_path(root, relative, error).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn confined_artifact_path(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<PathBuf, &'static str> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(error);
    }
    let path = root.join(relative);
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(error);
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(error),
            Ok(_) => {}
            Err(io_error) if io_error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(error),
        }
    }
    Ok(path)
}

#[cfg(not(target_os = "linux"))]
fn create_artifact_temp_sibling(
    path: &Path,
    error: &'static str,
) -> Result<(PathBuf, File), &'static str> {
    let name = path.file_name().ok_or(error)?.to_string_lossy();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for attempt in 0..ARTIFACT_TEMP_ATTEMPTS {
        let temporary = path.with_file_name(format!(
            ".{name}.kiana-artifact-{}-{stamp}-{attempt}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(io_error) if io_error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(error),
        }
    }
    Err(error)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinuxArtifactTargetState {
    Missing,
    Regular {
        device: u64,
        inode: u64,
        mode: u32,
        links: u64,
        size: u64,
        modified_seconds: i64,
        modified_nanoseconds: i64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
    },
}

#[cfg(target_os = "linux")]
struct LinuxArtifactLocation {
    root_path: PathBuf,
    root: File,
    parent: File,
    parent_components: Vec<OsString>,
    target: CString,
}

#[cfg(target_os = "linux")]
impl LinuxArtifactLocation {
    fn open(
        root_path: &Path,
        relative: &Path,
        create_parents: bool,
        error: &'static str,
    ) -> Result<Self, &'static str> {
        let mut components = validated_artifact_components(relative, error)?;
        let target = components.pop().ok_or(error)?;
        let root = linux_open_artifact_root(root_path).map_err(|_| error)?;
        let mut parent = root.try_clone().map_err(|_| error)?;
        for component in &components {
            parent =
                linux_open_artifact_directory_at(parent.as_raw_fd(), component, create_parents)
                    .map_err(|_| error)?;
        }
        Ok(Self {
            root_path: root_path.to_path_buf(),
            root,
            parent,
            parent_components: components,
            target: linux_artifact_cstring(&target).map_err(|_| error)?,
        })
    }

    fn is_current(&self) -> bool {
        let Ok(root) = linux_open_artifact_root(&self.root_path) else {
            return false;
        };
        if !linux_same_directory(&self.root, &root) {
            return false;
        }
        let mut parent = root;
        for component in &self.parent_components {
            let Ok(next) = linux_open_artifact_directory_at(parent.as_raw_fd(), component, false)
            else {
                return false;
            };
            parent = next;
        }
        linux_same_directory(&self.parent, &parent)
    }
}

#[cfg(target_os = "linux")]
struct PreparedLinuxArtifactWrite {
    location: LinuxArtifactLocation,
    temporary: CString,
    expected: LinuxArtifactTargetState,
    temporary_present: bool,
    error: &'static str,
}

#[cfg(target_os = "linux")]
impl PreparedLinuxArtifactWrite {
    fn commit(mut self) -> Result<(), &'static str> {
        if !self.location.is_current() {
            return Err(self.error);
        }
        let current =
            linux_artifact_target_state(self.location.parent.as_raw_fd(), &self.location.target)
                .map_err(|_| self.error)?;
        if current != self.expected {
            return Err(self.error);
        }

        match self.expected {
            LinuxArtifactTargetState::Missing => {
                linux_artifact_renameat2(
                    self.location.parent.as_raw_fd(),
                    &self.temporary,
                    &self.location.target,
                    LINUX_RENAME_NOREPLACE,
                )
                .map_err(|_| self.error)?;
                self.temporary_present = false;
            }
            LinuxArtifactTargetState::Regular { .. } => {
                linux_artifact_renameat2(
                    self.location.parent.as_raw_fd(),
                    &self.temporary,
                    &self.location.target,
                    LINUX_RENAME_EXCHANGE,
                )
                .map_err(|_| self.error)?;
                let displaced =
                    linux_artifact_target_state(self.location.parent.as_raw_fd(), &self.temporary);
                if !matches!(displaced, Ok(state) if state == self.expected) {
                    if linux_artifact_renameat2(
                        self.location.parent.as_raw_fd(),
                        &self.temporary,
                        &self.location.target,
                        LINUX_RENAME_EXCHANGE,
                    )
                    .is_err()
                    {
                        self.temporary_present = false;
                    }
                    return Err(self.error);
                }
                linux_artifact_unlinkat(self.location.parent.as_raw_fd(), &self.temporary)
                    .map_err(|_| self.error)?;
                self.temporary_present = false;
            }
        }
        let _ = linux_artifact_sync_directory(self.location.parent.as_raw_fd());
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for PreparedLinuxArtifactWrite {
    fn drop(&mut self) {
        if self.temporary_present {
            let _ = linux_artifact_unlinkat(self.location.parent.as_raw_fd(), &self.temporary);
        }
    }
}

#[cfg(target_os = "linux")]
fn read_project_artifact_linux(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    let location = LinuxArtifactLocation::open(root, relative, false, error)?;
    let expected = linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
        .map_err(|_| error)?;
    if matches!(expected, LinuxArtifactTargetState::Missing) {
        return Err(error);
    }
    let fd = unsafe {
        libc::openat(
            location.parent.as_raw_fd(),
            location.target.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(error);
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    if linux_artifact_file_state(&file).map_err(|_| error)? != expected {
        return Err(error);
    }
    let mut raw = String::new();
    file.read_to_string(&mut raw).map_err(|_| error)?;
    if linux_artifact_file_state(&file).map_err(|_| error)? != expected
        || !location.is_current()
        || linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
            .map_err(|_| error)?
            != expected
    {
        return Err(error);
    }
    Ok(raw)
}

#[cfg(target_os = "linux")]
fn prepare_project_artifact_linux(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<PreparedLinuxArtifactWrite, &'static str> {
    let location = LinuxArtifactLocation::open(root, relative, true, error)?;
    let expected = linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
        .map_err(|_| error)?;
    let (temporary, mut file) =
        linux_create_artifact_temp(location.parent.as_raw_fd()).map_err(|_| error)?;
    let result = (|| {
        file.write_all(contents).map_err(|_| error)?;
        file.flush().map_err(|_| error)?;
        file.sync_all().map_err(|_| error)
    })();
    drop(file);
    if let Err(write_error) = result {
        let _ = linux_artifact_unlinkat(location.parent.as_raw_fd(), &temporary);
        return Err(write_error);
    }
    Ok(PreparedLinuxArtifactWrite {
        location,
        temporary,
        expected,
        temporary_present: true,
        error,
    })
}

#[cfg(target_os = "linux")]
fn validated_artifact_components(
    relative: &Path,
    error: &'static str,
) -> Result<Vec<OsString>, &'static str> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(error);
    }
    relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => Ok(value.to_os_string()),
            _ => Err(error),
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_open_artifact_root(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(target_os = "linux")]
fn linux_open_artifact_directory_at(
    parent: RawFd,
    component: &std::ffi::OsStr,
    create: bool,
) -> std::io::Result<File> {
    let component = linux_artifact_cstring(component)?;
    let open = || unsafe {
        libc::openat(
            parent,
            component.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    let mut fd = open();
    if fd < 0 && create && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        let mkdir = unsafe { libc::mkdirat(parent, component.as_ptr(), 0o755) };
        if mkdir != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(std::io::Error::last_os_error());
        }
        fd = open();
    }
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

#[cfg(target_os = "linux")]
fn linux_artifact_target_state(
    parent: RawFd,
    name: &CString,
) -> std::io::Result<LinuxArtifactTargetState> {
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(LinuxArtifactTargetState::Missing)
        } else {
            Err(error)
        };
    }
    let file = unsafe { File::from_raw_fd(fd) };
    linux_artifact_file_state(&file)
}

#[cfg(target_os = "linux")]
fn linux_artifact_file_state(file: &File) -> std::io::Result<LinuxArtifactTargetState> {
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "artifact target is not a single-link regular file",
        ));
    }
    Ok(LinuxArtifactTargetState::Regular {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        links: metadata.nlink(),
        size: metadata.size(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    })
}

#[cfg(target_os = "linux")]
fn linux_same_directory(left: &File, right: &File) -> bool {
    match (left.metadata(), right.metadata()) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

#[cfg(target_os = "linux")]
fn linux_create_artifact_temp(parent: RawFd) -> std::io::Result<(CString, File)> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for attempt in 0..ARTIFACT_TEMP_ATTEMPTS {
        let name = CString::new(format!(
            ".kiana-artifact-{}-{stamp}-{attempt}",
            std::process::id()
        ))
        .expect("generated artifact name cannot contain NUL");
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd >= 0 {
            return Ok((name, unsafe { File::from_raw_fd(fd) }));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(error);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "artifact temporary name attempts exhausted",
    ))
}

#[cfg(target_os = "linux")]
fn linux_artifact_renameat2(
    parent: RawFd,
    source: &CString,
    target: &CString,
    flags: u32,
) -> std::io::Result<()> {
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent,
            source.as_ptr(),
            parent,
            target.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn linux_artifact_unlinkat(parent: RawFd, name: &CString) -> std::io::Result<()> {
    if unsafe { libc::unlinkat(parent, name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn linux_artifact_sync_directory(parent: RawFd) -> std::io::Result<()> {
    if unsafe { libc::fsync(parent) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn linux_artifact_cstring(value: &std::ffi::OsStr) -> std::io::Result<CString> {
    CString::new(value.as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "artifact path contains NUL",
        )
    })
}

fn authorized_harness_sandbox(
    context: &RequestContext,
    requested: Option<&str>,
) -> Result<&'static str, &'static str> {
    let requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("read-only");
    match requested {
        "read-only" => Ok("read-only"),
        "workspace-write" => {
            if context.project_trusted
                && !matches!(context.permission_profile, PermissionProfile::Safe)
            {
                let role = RoleSpec::lookup(&context.role_id).unwrap_or_else(RoleSpec::builder);
                if role.workspace_write_allowed() {
                    Ok("workspace-write")
                } else {
                    Err("role_sandbox_read_only")
                }
            } else {
                Err("workspace_write_requires_trusted_non_safe_profile")
            }
        }
        _ => Err("sandbox_unsupported"),
    }
}

fn normalize_context_query_arguments(
    context: &RequestContext,
    arguments: &Value,
) -> Result<NormalizedContextQuery, ContextQueryIntentError> {
    let arguments = arguments
        .as_object()
        .ok_or(ContextQueryIntentError::Invalid)?;
    if arguments.keys().any(|key| {
        !matches!(
            key.as_str(),
            "operation" | "output" | "options" | "project_root"
        )
    }) {
        return Err(ContextQueryIntentError::Invalid);
    }
    let operation = arguments
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ContextQueryIntentError::Invalid)?;
    let output = arguments
        .get("output")
        .and_then(Value::as_str)
        .ok_or(ContextQueryIntentError::Invalid)?;
    if !matches!(output, "json" | "text") {
        return Err(ContextQueryIntentError::Invalid);
    }
    let options = arguments
        .get("options")
        .and_then(Value::as_object)
        .ok_or(ContextQueryIntentError::Invalid)?;

    let (broker_operation, risk, normalized) = match operation {
        "repo_map" => {
            ensure_option_keys(options, &["max_tokens"])?;
            let max_tokens = optional_bounded_u64(options, "max_tokens", u64::MAX)?;
            (
                CONTEXT_REPO_MAP_OPERATION,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "output": output,
                    "max_tokens": max_tokens,
                }),
            )
        }
        "artifact_graph" | "artifact_readiness" => {
            ensure_option_keys(options, &["root", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = if operation == "artifact_graph" {
                CONTEXT_ARTIFACT_GRAPH_OPERATION
            } else {
                CONTEXT_ARTIFACT_READINESS_OPERATION
            };
            (
                broker_operation,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "index" | "artifacts" | "artifact_store" => {
            ensure_option_keys(options, &["root", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = match operation {
                "index" => CONTEXT_INDEX_OPERATION,
                "artifacts" => CONTEXT_ARTIFACTS_OPERATION,
                _ => CONTEXT_ARTIFACT_STORE_OPERATION,
            };
            (
                broker_operation,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "index_cache_write" | "artifacts_cache_write" | "artifact_store_cache_write" => {
            ensure_option_keys(options, &["root", "cache", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let cache = required_relative_path(options, "cache")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = match operation {
                "index_cache_write" => CONTEXT_INDEX_CACHE_OPERATION,
                "artifacts_cache_write" => CONTEXT_ARTIFACTS_CACHE_OPERATION,
                _ => CONTEXT_ARTIFACT_STORE_CACHE_OPERATION,
            };
            (
                broker_operation,
                RiskLevel::LocalWrite,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "cache": cache,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "artifact_ingest_write" => {
            ensure_option_keys(options, &["root", "source", "store", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let source = required_relative_path(options, "source")?;
            let store = optional_relative_path(options, "store")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            (
                CONTEXT_ARTIFACT_INGEST_OPERATION,
                RiskLevel::LocalWrite,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "source": source,
                    "store": store,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "search" | "vector_search" | "pack" => {
            let allowed = if operation == "pack" {
                &[
                    "query",
                    "root",
                    "limit",
                    "max_bytes_per_file",
                    "max_snippet_lines",
                ][..]
            } else {
                &["query", "root", "limit", "max_bytes_per_file"][..]
            };
            ensure_option_keys(options, allowed)?;
            let query = options
                .get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|query| !query.is_empty())
                .ok_or(ContextQueryIntentError::Invalid)?;
            let root = optional_relative_path(options, "root")?;
            let limit = optional_bounded_u64(options, "limit", MAX_CONTEXT_LIMIT)?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let max_snippet_lines = if operation == "pack" {
                optional_bounded_u64(options, "max_snippet_lines", MAX_CONTEXT_SNIPPET_LINES)?
            } else {
                None
            };
            let broker_operation = match operation {
                "search" => CONTEXT_SEARCH_OPERATION,
                "vector_search" => CONTEXT_VECTOR_SEARCH_OPERATION,
                _ => CONTEXT_PACK_OPERATION,
            };
            let normalized = if operation == "pack" {
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "query": query,
                    "limit": limit,
                    "max_bytes_per_file": max_bytes_per_file,
                    "max_snippet_lines": max_snippet_lines,
                })
            } else {
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "query": query,
                    "limit": limit,
                    "max_bytes_per_file": max_bytes_per_file,
                })
            };
            (broker_operation, RiskLevel::ReadOnly, normalized)
        }
        _ => return Err(ContextQueryIntentError::Unregistered),
    };

    Ok(NormalizedContextQuery {
        operation: broker_operation,
        risk,
        arguments: normalized,
    })
}

fn ensure_option_keys(
    options: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), ContextQueryIntentError> {
    if options.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ContextQueryIntentError::Invalid);
    }
    Ok(())
}

fn optional_bounded_u64(
    options: &serde_json::Map<String, Value>,
    key: &str,
    maximum: u64,
) -> Result<Option<u64>, ContextQueryIntentError> {
    match options.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0 && *value <= maximum)
            .map(Some)
            .ok_or(ContextQueryIntentError::Invalid),
    }
}

fn optional_relative_path(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ContextQueryIntentError> {
    let Some(value) = options.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ContextQueryIntentError::Invalid)?;
    validate_relative_path(value)?;
    Ok(Some(value.to_owned()))
}

fn required_relative_path(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, ContextQueryIntentError> {
    let value = options
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ContextQueryIntentError::Invalid)?;
    validate_relative_path(value)?;
    Ok(value.to_owned())
}

fn validate_relative_path(value: &str) -> Result<(), ContextQueryIntentError> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ContextQueryIntentError::Invalid);
    }
    Ok(())
}

struct NormalizedContextQuery {
    operation: &'static str,
    risk: RiskLevel,
    arguments: Value,
}

enum ContextQueryIntentError {
    Invalid,
    Unregistered,
}

impl ContextQueryIntentError {
    fn reason(&self) -> &'static str {
        match self {
            Self::Invalid => "command_arguments_invalid",
            Self::Unregistered => "command_unregistered",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Domain(#[from] kiana_domain::DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
}

#[cfg(all(test, target_os = "linux"))]
mod artifact_path_tests {
    use super::{prepare_project_artifact_linux, write_project_artifact};
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after epoch")
            .as_nanos();
        for attempt in 0..16 {
            let root = std::env::temp_dir().join(format!(
                "kiana-core-artifact-{label}-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            if fs::create_dir(&root).is_ok() {
                return root;
            }
        }
        panic!("could not create temporary artifact test root");
    }

    #[test]
    fn prepared_artifact_write_rejects_parent_rename_before_commit() {
        let root = temporary_root("parent-rename");
        let outside = temporary_root("parent-rename-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        fs::rename(root.join("gate"), root.join("gate-before-rename")).unwrap();
        symlink(&outside, root.join("gate")).unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert!(!outside.join("REVIEW.json").exists());
        assert!(!root.join("gate-before-rename/REVIEW.json").exists());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn prepared_artifact_write_rejects_final_symlink_replacement_before_commit() {
        let root = temporary_root("target-symlink");
        let outside = temporary_root("target-symlink-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let outside_target = outside.join("outside-target");
        fs::write(&outside_target, "outside sentinel").unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        symlink(&outside_target, root.join("gate/REVIEW.json")).unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert_eq!(
            fs::read_to_string(&outside_target).unwrap(),
            "outside sentinel"
        );
        assert!(fs::symlink_metadata(root.join("gate/REVIEW.json"))
            .unwrap()
            .file_type()
            .is_symlink());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn prepared_artifact_write_rejects_target_version_replacement_before_commit() {
        let root = temporary_root("target-version");
        fs::create_dir(root.join("gate")).unwrap();
        let target = root.join("gate/REVIEW.json");
        fs::write(&target, "original").unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        fs::write(&target, "changed after prepare").unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "changed after prepare"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn artifact_write_rejects_hardlinked_target_without_mutating_source() {
        let root = temporary_root("hardlink");
        let outside = temporary_root("hardlink-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let outside_target = outside.join("outside-target");
        fs::write(&outside_target, "outside sentinel").unwrap();
        fs::hard_link(&outside_target, root.join("gate/REVIEW.json")).unwrap();

        assert_eq!(
            write_project_artifact(
                &root,
                Path::new("gate/REVIEW.json"),
                b"replacement",
                "artifact_write_failed",
            ),
            Err("artifact_write_failed")
        );
        assert_eq!(
            fs::read_to_string(&outside_target).unwrap(),
            "outside sentinel"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}

#[cfg(test)]
mod event_redaction_tests {
    use super::{
        capability_event_payload, direct_capability_event_payload, redact_event_text,
        redact_event_value,
    };
    use kiana_domain::{CapabilityKind, CapabilityRequest, RequestContext, RequestId, RunId};
    use serde_json::json;

    #[test]
    fn capability_result_keeps_cell_scope_correlation() {
        let mut request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            "apply_patch",
            json!({ "patch": "*** Begin Patch" }),
        );
        request.cell_id = Some(kiana_domain::CellId::new());
        request.capability_grant_id = Some(kiana_domain::CapabilityGrantId::new());
        request.budget_lease_id = Some(kiana_domain::BudgetLeaseId::new());
        let context = RequestContext::local("session-1", "/repo");
        let run_id = RunId::new();
        let payload =
            capability_event_payload(&json!({ "changed": true }), &request, &context, run_id);

        assert_eq!(payload["run_id"], json!(run_id));
        assert_eq!(payload["session_id"], "session-1");
        assert_eq!(payload["cell_id"], json!(request.cell_id));
        assert_eq!(
            payload["capability_grant_id"],
            json!(request.capability_grant_id)
        );
        assert_eq!(payload["budget_lease_id"], json!(request.budget_lease_id));
        assert_eq!(payload["capability_request_id"], json!(request.request_id));
    }

    #[test]
    fn scalar_capability_result_is_wrapped_with_scope_correlation() {
        let request =
            CapabilityRequest::new(RequestId::new(), CapabilityKind::Query, "search", json!({}));
        let context = RequestContext::local("session-1", "/repo");
        let run_id = RunId::new();
        let payload = capability_event_payload(&json!(["one", "two"]), &request, &context, run_id);

        assert_eq!(payload["output"], json!(["one", "two"]));
        assert_eq!(payload["run_id"], json!(run_id));
        assert_eq!(payload["capability_request_id"], json!(request.request_id));
        assert_eq!(payload["operation"], "search");
    }

    #[test]
    fn direct_capability_result_keeps_request_scope_correlation() {
        let request =
            CapabilityRequest::new(RequestId::new(), CapabilityKind::Query, "search", json!({}));
        let payload = direct_capability_event_payload(&json!("found"), &request);

        assert_eq!(payload["output"], "found");
        assert_eq!(payload["capability_request_id"], json!(request.request_id));
        assert_eq!(payload["capability"], "query");
        assert_eq!(payload["operation"], "search");
    }

    #[test]
    fn event_error_redaction_preserves_codes_and_masks_secret_parameters() {
        let redacted = redact_event_text("provider_failed token=abc123, retryable=true");
        assert_eq!(redacted, "provider_failed token=[REDACTED], retryable=true");
    }

    #[test]
    fn event_redaction_masks_bearer_and_json_string_secrets() {
        let redacted = redact_event_value(&json!({
            "error": "Authorization: Bearer bearer-secret token=token-secret",
            "nested": ["{\"api_key\":\"json-secret\"}"],
            "secret_ref": "vault://capability",
        }));
        let text = redacted.to_string();
        assert!(!text.contains("bearer-secret"), "{text}");
        assert!(!text.contains("token-secret"), "{text}");
        assert!(!text.contains("json-secret"), "{text}");
        assert!(text.contains("[REDACTED]"), "{text}");
        assert_eq!(redacted["secret_ref"], "vault://capability");
    }

    #[test]
    fn event_redaction_masks_standard_header_and_spaced_json_secrets() {
        let redacted = redact_event_value(&json!({
            "basic": "Authorization: Basic basic-secret",
            "header": "X-Api-Key:  header-secret",
            "json": "{\"api_key\": \"json-secret\", \"secret_ref\": \"vault://kept\"}",
        }));
        let text = redacted.to_string();
        for sentinel in ["basic-secret", "header-secret", "json-secret"] {
            assert!(!text.contains(sentinel), "leaked {sentinel}: {text}");
        }
        assert!(text.contains("[REDACTED]"), "{text}");
        assert!(text.contains("vault://kept"), "{text}");
    }

    #[test]
    fn event_redaction_preserves_references_and_non_sensitive_results() {
        let redacted = redact_event_value(&json!({
            "secret_ref": "provider/anthropic",
            "api_key": "do-not-record",
            "numeric_api_key": 123456,
            "tokens_before": 4096,
            "token_budget": 1000,
            "estimated_tokens": 42,
            "token_overlap": 2,
            "nested": [{"access_token": "also-private", "path": "src/lib.rs"}],
        }));
        assert_eq!(redacted["secret_ref"], "provider/anthropic");
        assert_eq!(redacted["api_key"], "[REDACTED]");
        assert_eq!(redacted["numeric_api_key"], "[REDACTED]");
        assert_eq!(redacted["tokens_before"], 4096);
        assert_eq!(redacted["token_budget"], 1000);
        assert_eq!(redacted["estimated_tokens"], 42);
        assert_eq!(redacted["token_overlap"], 2);
        assert_eq!(redacted["nested"][0]["access_token"], "[REDACTED]");
        assert_eq!(redacted["nested"][0]["path"], "src/lib.rs");
    }
}
