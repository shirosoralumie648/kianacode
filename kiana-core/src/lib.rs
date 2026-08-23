//! The single command and capability control plane for Kiana.

use kiana_domain::{
    ApprovalDecision, ApprovalId, AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest,
    CapabilityResult, CommandIntent, CoreResponse, DecisionRecord, ExecutionStatus, GateDecision,
    PermissionProfile, RequestContext, RequestId, ReviewPacket, RiskLevel, RoleSpec, RunId,
    RuntimeEvent, Symposium, SymposiumClaim, WorkPacket, DECISION_RECORD_PATH,
    MEMORY_SEARCH_SCHEMA, MONITORING_PATH_GATE, PLANNING_PATH_PACKET, PLANNING_PATH_PLAN,
    REVIEW_PACKET_PATH, REVIEW_RESULT_SCHEMA, ROLE_ARCHITECT, ROLE_BUILDER, ROLE_PM, ROLE_REVIEWER,
    SYMPOSIUM_RESULT_SCHEMA, WORK_PACKET_PATH,
};
use kiana_gates::GateEngine;
use kiana_policy::PolicyEngine;
use kiana_ports::{ApprovalStorePort, CapabilityBrokerPort, EventStorePort, PortError, RunnerPort};
use kiana_query::{run_pre_tool_use_hooks, PreToolUseHookContext, ToolHookDecision};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use kiana_types::ProjectTrust;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};
use tokio::sync::{watch, Notify};

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

pub struct ControlPlane {
    policy: Arc<dyn PolicyEngine>,
    gates: Arc<dyn GateEngine>,
    events: Arc<dyn EventStorePort>,
    capabilities: Arc<dyn CapabilityBrokerPort>,
    approvals: Arc<dyn ApprovalStorePort>,
    runner: Arc<dyn RunnerPort>,
    sessions: Mutex<HashMap<String, RunId>>,
    cancellations: Mutex<HashMap<RunId, watch::Sender<bool>>>,
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
        Self {
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            sessions: Mutex::new(HashMap::new()),
            cancellations: Mutex::new(HashMap::new()),
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

        let policy = self.policy.evaluate(context, &request);
        let gate = self.gates.evaluate(&policy);
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
        let pending = match self.approvals.consume(context, approval_id).await {
            Ok(pending) => pending,
            Err(error) => {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
        };
        let request_id = pending.request.request_id;
        let event_kind = match decision {
            ApprovalDecision::Approve => "approval.approved",
            ApprovalDecision::Deny => "approval.denied",
        };
        self.append_event(
            request_id,
            4,
            event_kind,
            json!({
                "approval_id": approval_id,
                "request_hash": pending.challenge.request_hash,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
            }),
        )
        .await?;

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

    async fn execute_authorized_request(
        &self,
        request: CapabilityRequest,
        authorization_id: String,
        result_sequence: u64,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = request.request_id;
        let authorized = AuthorizedCapabilityRequest::new(authorization_id, request)?;
        match self.capabilities.execute(authorized).await {
            Ok(result) => {
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
                        output.clone(),
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
                let reason = error.to_string();
                self.append_event(
                    request_id,
                    result_sequence,
                    "capability.failed",
                    json!({ "error": &reason }),
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

    pub async fn spawn_from_packet(
        &self,
        mut context: RequestContext,
        packet: WorkPacket,
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
        context.assign_role(&RoleSpec::builder());
        context.work_packet_id = Some(packet.id.clone());
        self.start_run(context, packet.as_prompt(), sandbox).await
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
            .events_for_author(&author_session_id, author_run_id)
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

        let run_id = RunId::parse_str(context.session_id.as_str()).unwrap_or_else(RunId::new);
        self.remember_session(context.session_id.as_str(), run_id);
        self.record_event(
            request_id,
            &mut sequence,
            "review.closed",
            json!({
                "author_session_id": author_session_id,
                "reviewer_session_id": context.session_id,
                "verdict": verdict,
                "builder_present": false,
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
                "packet": packet,
            }),
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
        if chair.role_id != ROLE_PM {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "symposium_chair_must_be_pm" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "symposium_chair_must_be_pm",
            ));
        }
        context.assign_role(&RoleSpec::pm());

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

        let mut meeting = Symposium::planning(request_id.to_string(), goal, max_rounds);
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
            for round in 0..max_rounds {
                for role_id in [ROLE_PM, ROLE_ARCHITECT] {
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

        if let Err(reason) = write_planning_artifacts(&context.project_root, &decision, &packet) {
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
                "skipped_meeting": anti_meeting,
                "builder_present": false,
                "decision_id": decision.id,
                "work_packet_id": packet.id,
            }),
        )
        .await?;

        Ok(CoreResponse::completed(
            request_id,
            json!({
                "schema": SYMPOSIUM_RESULT_SCHEMA,
                "harness": HARNESS_ID,
                "symposium_id": meeting.id,
                "status": meeting.status,
                "builder_present": false,
                "skipped_meeting": anti_meeting,
                "decision": decision,
                "packet": packet,
                "decision_path": DECISION_RECORD_PATH,
                "packet_path": WORK_PACKET_PATH,
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
        let request_id = context.request_id;
        let run_id = RunId::parse_str(context.session_id.as_str()).unwrap_or_else(RunId::new);
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.start",
                "run_id": run_id,
                "session_id": context.session_id,
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

        if RoleSpec::lookup(&context.role_id).is_none() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
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
                "role_id": context.role_id,
                "harness": HARNESS_ID,
                "sandbox": sandbox,
                "capability_mode": "brokered",
            }),
        )
        .await?;

        // Session index and cancel watch must exist before the first model step
        // so an in-flight cancel can resolve session-1 and abort shell.exec.
        self.remember_session(context.session_id.as_str(), run_id);
        let _cancel_rx = self.watch_cancel(run_id);

        let pending_events = match self
            .runner
            .send(RunnerCommand::start_in_with_instructions(
                run_id,
                prompt,
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
                let reason = error.to_string();
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.failed",
                    json!({ "error": &reason }),
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
                let reason = error.to_string();
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.failed",
                    json!({ "error": &reason }),
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

        let inflight = self.signal_cancel(run_id);
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
                let reason = error.to_string();
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.failed",
                    json!({ "error": &reason }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, "read-only"),
                    error: Some(reason),
                });
            }
        };

        let error = events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::Failed { error, .. } => Some(error.clone()),
                _ => None,
            })
            .unwrap_or_else(|| format!("cancelled:{reason}"));
        if error == "run_not_found" && !inflight {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "run_not_found" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "run_not_found"));
        }

        self.forget_session(context.session_id.as_str(), run_id);
        let cancelled = if error == "run_not_found" {
            format!("cancelled:{reason}")
        } else {
            error
        };
        self.record_event(
            request_id,
            &mut sequence,
            "run.cancelled",
            json!({ "error": &cancelled }),
        )
        .await?;
        Ok(CoreResponse {
            request_id,
            status: ExecutionStatus::Failed,
            output: run_identity(&context, run_id, "read-only"),
            error: Some(cancelled),
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
                RunnerEvent::Delta { text, .. } => {
                    self.record_event(request_id, sequence, "run.delta", json!({ "text": text }))
                        .await?;
                }
                RunnerEvent::CapabilityRequested { run_id, request } => {
                    match self
                        .broker_harness_capability(
                            context, request_id, run_id, sequence, request, &cancel_rx,
                        )
                        .await?
                    {
                        Ok(events) => pending_events.extend(events),
                        Err(reason) => {
                            failed = Some(reason);
                            break;
                        }
                    }
                }
                RunnerEvent::Completed {
                    output: harness_output,
                    ..
                } => {
                    output = harness_output;
                    completed = true;
                    self.record_event(request_id, sequence, "run.completed", output.clone())
                        .await?;
                }
                RunnerEvent::Failed { error, .. } => {
                    failed = Some(error.clone());
                    self.record_event(
                        request_id,
                        sequence,
                        "run.failed",
                        json!({ "error": error }),
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
            return Ok(CoreResponse {
                request_id,
                status: if error == "run_not_found" {
                    ExecutionStatus::Blocked
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
                "run.failed",
                json!({ "error": reason }),
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Failed,
                output: run_identity(context, run_id, sandbox),
                error: Some(reason.to_owned()),
            });
        }

        self.remember_session(context.session_id.as_str(), run_id);
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
        let run_id = match run_id.or_else(|| RunId::parse_str(context.session_id.as_str())) {
            Some(run_id) => run_id,
            None => {
                return Ok(CoreResponse::blocked(request_id, "receipt_not_found"));
            }
        };
        let events = self.events_for_run(&context, run_id).await?;
        if events.is_empty() {
            return Ok(CoreResponse::blocked(request_id, "receipt_not_found"));
        }
        let sandbox = events
            .iter()
            .rev()
            .find_map(|event| event.data.get("sandbox").and_then(Value::as_str))
            .unwrap_or("read-only");
        if let Some(error) = events.iter().rev().find_map(|event| {
            if event.kind == "run.failed" || event.kind == "run.cancelled" {
                event
                    .data
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            }
        }) {
            if !events.iter().any(|event| event.kind == "run.completed") {
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                    error: Some(error),
                });
            }
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
        let events = self.events_for_run(context, run_id).await?;
        Ok(receipt_from_events(
            context, run_id, sandbox, output, &events,
        ))
    }

    async fn events_for_run(
        &self,
        context: &RequestContext,
        run_id: RunId,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        match self.events.read_all().await {
            Ok(all) => Ok(filter_run_events(&all, run_id, context.session_id.as_str())),
            Err(_) => Ok(self.events.read_request(&context.request_id).await?),
        }
    }

    async fn events_for_author(
        &self,
        author_session_id: &str,
        author_run_id: Option<RunId>,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        let run_id = author_run_id
            .or_else(|| RunId::parse_str(author_session_id))
            .or_else(|| self.session_run_id(author_session_id));
        match self.events.read_all().await {
            Ok(all) => Ok(filter_session_events(&all, run_id, author_session_id)),
            Err(_) => Ok(Vec::new()),
        }
    }

    async fn broker_harness_capability(
        &self,
        context: &RequestContext,
        request_id: kiana_domain::RequestId,
        run_id: RunId,
        sequence: &mut u64,
        mut request: CapabilityRequest,
        cancel_rx: &watch::Receiver<bool>,
    ) -> Result<Result<Vec<RunnerEvent>, String>, CoreError> {
        stamp_request_identity(&mut request, context);
        self.record_event(
            request_id,
            sequence,
            "run.capability_requested",
            json!({
                "request_id": request.request_id,
                "capability": request.capability,
                "operation": request.operation,
                "risk": request.risk,
                "arguments": request.arguments,
            }),
        )
        .await?;

        let policy = self.policy.evaluate(context, &request);
        let gate = self.gates.evaluate(&policy);
        self.record_event(
            request_id,
            sequence,
            "capability.decision",
            json!({ "policy": &policy, "gate": &gate }),
        )
        .await?;

        let result = match gate {
            GateDecision::Allowed { authorization_id } => {
                if let Some(reason) = pre_tool_hook_block(context, &request).await {
                    self.record_event(
                        request_id,
                        sequence,
                        "capability.failed",
                        json!({ "error": &reason }),
                    )
                    .await?;
                    CapabilityResult::failure(request.request_id, reason)
                } else {
                    let authorized =
                        AuthorizedCapabilityRequest::new(authorization_id, request.clone())?;
                    tokio::select! {
                        _ = wait_until_cancelled(cancel_rx) => {
                            self.record_event(
                                request_id,
                                sequence,
                                "run.cancelled",
                                json!({ "error": "cancelled:user" }),
                            )
                            .await?;
                            return Ok(Err("cancelled:user".to_owned()));
                        }
                        executed = self.capabilities.execute(authorized) => {
                            match executed {
                                Ok(result) => {
                                    self.record_event(
                                        request_id,
                                        sequence,
                                        if result.success {
                                            "capability.completed"
                                        } else {
                                            "capability.failed"
                                        },
                                        result.output.clone(),
                                    )
                                    .await?;
                                    result
                                }
                                Err(error) => {
                                    let reason = error.to_string();
                                    self.record_event(
                                        request_id,
                                        sequence,
                                        "capability.failed",
                                        json!({ "error": &reason }),
                                    )
                                    .await?;
                                    CapabilityResult::failure(request.request_id, reason)
                                }
                            }
                        }
                    }
                }
            }
            GateDecision::Denied { reason } if reason.starts_with("role_") => {
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({ "reason": &reason }),
                )
                .await?;
                return Ok(Err(reason));
            }
            GateDecision::AwaitingApproval { reason } | GateDecision::Denied { reason } => {
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({ "reason": &reason }),
                )
                .await?;
                CapabilityResult::failure(
                    request.request_id,
                    format!("capability_blocked:{reason}"),
                )
            }
        };

        match self
            .runner
            .send(RunnerCommand::CapabilityResult { run_id, result })
            .await
        {
            Ok(events) => Ok(Ok(events)),
            Err(error) => {
                let reason = error.to_string();
                self.record_event(
                    request_id,
                    sequence,
                    "run.failed",
                    json!({ "error": &reason }),
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
        if let Some(run_id) = run_id {
            return Ok(run_id);
        }
        if let Some(parsed) = RunId::parse_str(context.session_id.as_str()) {
            return Ok(parsed);
        }
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(context.session_id.as_str())
            .copied()
            .ok_or("session_not_found")
    }

    fn session_known(&self, session_id: &str) -> bool {
        self.session_run_id(session_id).is_some()
    }

    fn session_run_id(&self, session_id: &str) -> Option<RunId> {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session_id)
            .copied()
    }

    fn remember_session(&self, session_id: &str, run_id: RunId) {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session_id.to_owned(), run_id);
    }

    fn forget_session(&self, session_id: &str, run_id: RunId) {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if sessions.get(session_id) == Some(&run_id) {
            sessions.remove(session_id);
        }
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
        self.events
            .append(RuntimeEvent::new(request_id, sequence, kind, data)?)
            .await?;
        Ok(())
    }
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
    with_work_packet(
        json!({
            "schema": RUN_RESULT_SCHEMA,
            "run_id": run_id,
            "session_id": context.session_id,
            "harness": HARNESS_ID,
            "sandbox": sandbox,
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
    )
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

fn filter_run_events(
    events: &[RuntimeEvent],
    run_id: RunId,
    session_id: &str,
) -> Vec<RuntimeEvent> {
    let run_id_str = run_id.to_string();
    let request_ids: HashSet<_> = events
        .iter()
        .filter(|event| {
            event
                .data
                .get("run_id")
                .and_then(Value::as_str)
                .is_some_and(|value| value == run_id_str || value == session_id)
        })
        .map(|event| event.request_id)
        .collect();
    events
        .iter()
        .filter(|event| request_ids.contains(&event.request_id))
        .cloned()
        .collect()
}

fn filter_session_events(
    events: &[RuntimeEvent],
    run_id: Option<RunId>,
    session_id: &str,
) -> Vec<RuntimeEvent> {
    if let Some(run_id) = run_id {
        return filter_run_events(events, run_id, session_id);
    }
    let request_ids: HashSet<_> = events
        .iter()
        .filter(|event| {
            let run = event.data.get("run_id").and_then(Value::as_str);
            let session = event.data.get("session_id").and_then(Value::as_str);
            run == Some(session_id) || session == Some(session_id)
        })
        .map(|event| event.request_id)
        .collect();
    events
        .iter()
        .filter(|event| request_ids.contains(&event.request_id))
        .cloned()
        .collect()
}

fn stamp_request_identity(request: &mut CapabilityRequest, context: &RequestContext) {
    let Some(arguments) = request.arguments.as_object_mut() else {
        return;
    };
    arguments
        .entry("role_id")
        .or_insert_with(|| json!(context.role_id));
    arguments
        .entry("department_id")
        .or_insert_with(|| json!(context.department_id));
    arguments
        .entry("session_id")
        .or_insert_with(|| json!(context.session_id.as_str()));
    arguments
        .entry("project_root")
        .or_insert_with(|| json!(context.project_root));
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

fn write_planning_artifacts(
    project_root: &str,
    decision: &DecisionRecord,
    packet: &WorkPacket,
) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("symposium_artifact_write_failed");
    }
    std::fs::create_dir_all(root.join(PLANNING_PATH_PLAN))
        .map_err(|_| "symposium_artifact_write_failed")?;
    std::fs::create_dir_all(root.join(PLANNING_PATH_PACKET))
        .map_err(|_| "symposium_artifact_write_failed")?;
    let decision_json =
        serde_json::to_string_pretty(decision).map_err(|_| "symposium_artifact_write_failed")?;
    let packet_json =
        serde_json::to_string_pretty(packet).map_err(|_| "symposium_artifact_write_failed")?;
    std::fs::write(root.join(DECISION_RECORD_PATH), decision_json)
        .map_err(|_| "symposium_artifact_write_failed")?;
    std::fs::write(root.join(WORK_PACKET_PATH), packet_json)
        .map_err(|_| "symposium_artifact_write_failed")?;
    Ok(())
}

fn write_review_artifact(project_root: &str, packet: &ReviewPacket) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("review_artifact_write_failed");
    }
    std::fs::create_dir_all(root.join(MONITORING_PATH_GATE))
        .map_err(|_| "review_artifact_write_failed")?;
    let packet_json =
        serde_json::to_string_pretty(packet).map_err(|_| "review_artifact_write_failed")?;
    std::fs::write(root.join(REVIEW_PACKET_PATH), packet_json)
        .map_err(|_| "review_artifact_write_failed")?;
    Ok(())
}

async fn pre_tool_hook_block(
    context: &RequestContext,
    request: &CapabilityRequest,
) -> Option<String> {
    let project_trust = if context.project_trusted {
        ProjectTrust::Trusted
    } else {
        ProjectTrust::Untrusted
    };
    let decision = run_pre_tool_use_hooks(PreToolUseHookContext {
        abort_signal: Arc::new(Notify::new()),
        cwd: PathBuf::from(&context.project_root),
        project_trust,
        permission_mode: permission_mode_label(context.permission_profile),
        query_source: HARNESS_ID.to_owned(),
        tool_name: hook_tool_name(&request.operation),
        tool_input: request.arguments.clone(),
        tool_use_id: request
            .arguments
            .get("call_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
    .await;
    match decision {
        ToolHookDecision::Allow | ToolHookDecision::UpdateInput(_) => None,
        ToolHookDecision::Block(reason) => Some(format!("hook_blocked:{reason}")),
        ToolHookDecision::Ask { reason, .. } => Some(format!("hook_ask_unattended:{reason}")),
    }
}

fn hook_tool_name(operation: &str) -> String {
    match operation {
        "apply_patch" | "file_change" => "apply_patch".to_owned(),
        "shell.exec" | "shell" | "bash" | "exec" | "command_execution" => "shell".to_owned(),
        "mcp.call" | "mcp" => "mcp".to_owned(),
        other => other.to_owned(),
    }
}

fn permission_mode_label(profile: PermissionProfile) -> String {
    match profile {
        PermissionProfile::Safe => "safe".to_owned(),
        PermissionProfile::Balanced => "balanced".to_owned(),
        PermissionProfile::Autonomous => "autonomous".to_owned(),
    }
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
