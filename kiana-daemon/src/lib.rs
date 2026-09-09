//! Composition root for Kiana control-plane adapters.

mod apply_patch;
mod approval_store;
mod context_query;
mod harness_capabilities;
mod harness_mcp;
mod harness_memory;
mod harness_sandbox;
mod harness_skills;
mod model_client;
mod pre_tool_hooks;
mod run_stream;

use approval_store::{JsonlApprovalStore, MemoryApprovalStore};
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
pub use kiana_domain::StreamingRedactor;
use kiana_domain::{
    CommandIntent, PermissionProfile, RequestContext, RoleSpec, RunId, RuntimeEvent,
};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{PortError, RunnerPort};
use kiana_protocol::{RequestBody, RequestEnvelope, ResponseEnvelope, PROTOCOL_SCHEMA};
use kiana_runner::KianaHarness;
use run_stream::RunStreamBus;
pub use run_stream::RunStreamSubscription;
use std::path::Path;
use std::sync::Arc;

pub struct DaemonHost {
    core: Arc<ControlPlane>,
    principal: AuthenticatedPrincipal,
    project_authority: Arc<dyn ProjectTrustAuthority>,
    run_stream: Arc<RunStreamBus>,
}

pub trait ProjectTrustAuthority: Send + Sync {
    fn project_trusted(&self, project_root: &Path) -> Result<bool, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StoredProjectTrustAuthority;

impl ProjectTrustAuthority for StoredProjectTrustAuthority {
    fn project_trusted(&self, project_root: &Path) -> Result<bool, String> {
        kiana_types::read_project_trust(project_root)
            .map(|trust| trust.is_some_and(kiana_types::ProjectTrust::as_bool))
    }
}

#[derive(Clone, Debug)]
struct AuthenticatedPrincipal {
    actor_id: String,
}

impl AuthenticatedPrincipal {
    fn local() -> Self {
        Self {
            actor_id: "local-user".to_owned(),
        }
    }
}

impl DaemonHost {
    pub fn new(core: Arc<ControlPlane>) -> Self {
        Self::new_with_project_authority(core, Arc::new(StoredProjectTrustAuthority))
    }

    pub fn new_with_project_authority(
        core: Arc<ControlPlane>,
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Self {
        Self::with_run_stream(core, project_authority, Arc::new(RunStreamBus::default()))
    }

    fn with_run_stream(
        core: Arc<ControlPlane>,
        project_authority: Arc<dyn ProjectTrustAuthority>,
        run_stream: Arc<RunStreamBus>,
    ) -> Self {
        Self {
            core,
            principal: AuthenticatedPrincipal::local(),
            project_authority,
            run_stream,
        }
    }

    /// Subscribe to additive run-stream events for one run.
    ///
    /// The subscription must be created before the run starts to observe deltas. It remains
    /// usable until the terminal event or until the receiver is dropped.
    pub fn subscribe_run(&self, run_id: RunId) -> RunStreamSubscription {
        self.run_stream.subscribe(run_id)
    }

    /// 只读地读取本实例账本里的全部事件，供展示层做只读投影。
    ///
    /// 展示层不得自己解析账本文件：路径推导、torn-tail 容忍和 symlink 拒绝都由
    /// `EventStorePort` 的实现负责。`Ok(None)` 表示该存储不支持全量读取；读取失败
    /// 照原样返回，调用方不得把它当成空账本。
    pub async fn persisted_events(&self) -> Result<Option<Vec<RuntimeEvent>>, PortError> {
        self.core
            .persisted_events()
            .await
            .map_err(|error| PortError::Failed(error.to_string()))
    }

    pub fn local() -> Result<Self, PortError> {
        Self::with_runner_events_and_approval(
            Arc::new(KianaHarness::new(model_client::from_env())),
            Arc::new(JsonlEventLog::open_default()?),
            Arc::new(JsonlApprovalStore::open_default()?),
        )
    }

    pub fn local_with_model_config(config: LocalModelConfig) -> Result<Self, PortError> {
        Self::with_runner_events_and_approval(
            Arc::new(KianaHarness::new(model_client::from_config(config))),
            Arc::new(JsonlEventLog::open_default()?),
            Arc::new(JsonlApprovalStore::open_default()?),
        )
    }

    pub fn local_with_project_authority(
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_approval_and_authority(
            Arc::new(KianaHarness::new(model_client::from_env())),
            Arc::new(JsonlEventLog::open_default()?),
            Arc::new(JsonlApprovalStore::open_default()?),
            project_authority,
        )
    }

    pub fn with_harness(harness: KianaHarness) -> Result<Self, PortError> {
        Self::with_runner(Arc::new(harness))
    }

    pub fn with_harness_and_project_authority(
        harness: KianaHarness,
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_approval_and_authority(
            Arc::new(harness),
            Arc::new(MemoryEventLog::new()),
            Arc::new(MemoryApprovalStore::new()),
            project_authority,
        )
    }

    pub fn with_env_harness() -> Result<Self, PortError> {
        Self::with_runner(Arc::new(KianaHarness::new(model_client::from_env())))
    }

    pub fn with_env_harness_and_project_authority(
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_approval_and_authority(
            Arc::new(KianaHarness::new(model_client::from_env())),
            Arc::new(MemoryEventLog::new()),
            Arc::new(MemoryApprovalStore::new()),
            project_authority,
        )
    }

    pub fn with_harness_on_disk(
        harness: KianaHarness,
        events_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, PortError> {
        Self::with_runner_and_events(
            Arc::new(harness),
            Arc::new(JsonlEventLog::open(events_path)?),
        )
    }

    pub fn with_harness_on_disk_and_project_authority(
        harness: KianaHarness,
        events_path: impl AsRef<std::path::Path>,
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_approval_and_authority(
            Arc::new(harness),
            Arc::new(JsonlEventLog::open(events_path)?),
            Arc::new(MemoryApprovalStore::new()),
            project_authority,
        )
    }

    pub fn with_runner(runner: Arc<dyn RunnerPort>) -> Result<Self, PortError> {
        Self::with_runner_and_events(runner, Arc::new(MemoryEventLog::new()))
    }

    pub fn with_runner_and_events(
        runner: Arc<dyn RunnerPort>,
        events: Arc<dyn kiana_ports::EventStorePort>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_and_approval(runner, events, Arc::new(MemoryApprovalStore::new()))
    }

    fn with_runner_events_and_approval(
        runner: Arc<dyn RunnerPort>,
        events: Arc<dyn kiana_ports::EventStorePort>,
        approvals: Arc<dyn kiana_ports::ApprovalStorePort>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_approval_and_authority(
            runner,
            events,
            approvals,
            Arc::new(StoredProjectTrustAuthority),
        )
    }

    fn with_runner_events_approval_and_authority(
        runner: Arc<dyn RunnerPort>,
        events: Arc<dyn kiana_ports::EventStorePort>,
        approvals: Arc<dyn kiana_ports::ApprovalStorePort>,
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        let run_stream = Arc::new(RunStreamBus::default());
        let runner = harness_skills::SkillAwareRunner::wrap(runner);
        let runner = run_stream::RunStreamRunner::wrap(runner, run_stream.clone());
        let mut capabilities = CapabilityBroker::new();
        context_query::register(&mut capabilities)?;
        harness_capabilities::register(&mut capabilities)?;
        harness_mcp::register(&mut capabilities)?;
        harness_memory::register(&mut capabilities)?;
        let core = ControlPlane::with_pre_tool_hooks(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events,
            Arc::new(capabilities),
            approvals,
            runner,
            Arc::new(pre_tool_hooks::QueryPreToolHooks),
        );
        Ok(Self::with_run_stream(
            Arc::new(core),
            project_authority,
            run_stream,
        ))
    }

    pub async fn handle(&self, request: RequestEnvelope) -> ResponseEnvelope {
        let request_id = request.metadata.request_id;
        if request.schema != PROTOCOL_SCHEMA {
            return ResponseEnvelope::rejected(request_id, "protocol_schema_unsupported");
        }
        let mut metadata = request.metadata;
        let permission_profile =
            effective_permission_profile(&request.body, metadata.permission_profile);
        if let RequestBody::ApprovalDecision(decision) = &request.body {
            let proof_missing = decision
                .request_hash
                .as_deref()
                .is_none_or(|proof| proof.trim().is_empty())
                || decision
                    .nonce
                    .as_deref()
                    .is_none_or(|proof| proof.trim().is_empty());
            if proof_missing {
                return ResponseEnvelope::rejected(request_id, "approval_proof_required");
            }
        }
        if matches!(&request.body, RequestBody::ApprovalDecision(_))
            && metadata
                .actor_id
                .as_deref()
                .is_some_and(|actor| actor != self.principal.actor_id)
        {
            return ResponseEnvelope::rejected(request_id, "approval_context_mismatch");
        }
        metadata.actor_id = Some(self.principal.actor_id.clone());
        if metadata.session_id.is_empty() {
            return ResponseEnvelope::rejected(request_id, "session_id_required");
        }
        if metadata.project_root.trim().is_empty() {
            return ResponseEnvelope::rejected(request_id, "project_root_required");
        }
        let project_trusted = match self
            .project_authority
            .project_trusted(Path::new(&metadata.project_root))
        {
            Ok(trusted) => trusted,
            Err(_) => {
                return ResponseEnvelope::rejected(request_id, "project_trust_unavailable");
            }
        };
        if metadata
            .actor_id
            .as_deref()
            .is_none_or(|actor| actor.trim().is_empty())
        {
            return ResponseEnvelope::rejected(request_id, "actor_identity_required");
        }

        let Some(role) = RoleSpec::lookup(&metadata.role_id) else {
            return ResponseEnvelope::rejected(request_id, "role_unknown");
        };
        let department = metadata.department_id.trim();
        if !department.is_empty() && department != role.department_id {
            return ResponseEnvelope::rejected(request_id, "role_department_mismatch");
        }

        let context = RequestContext {
            request_id,
            session_id: metadata.session_id,
            project_root: metadata.project_root,
            actor_id: metadata.actor_id,
            project_trusted,
            permission_profile,
            role_id: role.role_id,
            department_id: role.department_id,
            work_packet_id: None,
            cell_id: None,
            path_allow: Vec::new(),
        };
        let response = match request.body {
            RequestBody::Command(command) => {
                self.core
                    .handle_command(context, CommandIntent::new(command.name, command.arguments))
                    .await
            }
            RequestBody::ApprovalDecision(decision) => {
                self.core
                    .decide_approval_with_proof(
                        &context,
                        decision.approval_id,
                        decision.decision,
                        decision.request_hash.as_deref(),
                        decision.nonce.as_deref(),
                    )
                    .await
            }
            RequestBody::Run(run) => {
                self.core
                    .start_run_with_history(context, run.prompt, run.history, run.sandbox)
                    .await
            }
            RequestBody::Continue(run) => {
                self.core
                    .continue_run(context, run.prompt, run.sandbox, run.run_id)
                    .await
            }
            RequestBody::Cancel(run) => self.core.cancel_run(context, run.run_id, run.reason).await,
            RequestBody::Receipt(receipt) => self.core.read_receipt(context, receipt.run_id).await,
            RequestBody::Spawn(spawn) => {
                self.core
                    .spawn_from_packet(context, spawn.packet, spawn.sandbox)
                    .await
            }
            RequestBody::Symposium(symposium) => {
                self.core
                    .convene_symposium(
                        context,
                        symposium.goal,
                        symposium.anti_meeting,
                        Some(symposium.max_rounds),
                        symposium.sandbox,
                    )
                    .await
            }
            RequestBody::Review(review) => {
                self.core
                    .review_author_run(context, review.author_session_id, review.author_run_id)
                    .await
            }
            RequestBody::Close(close) => {
                self.core
                    .close_author_run(context, close.author_session_id, close.author_run_id)
                    .await
            }
        };
        let response = match response {
            Ok(response) => {
                let mut response = ResponseEnvelope::from_core(response);
                response.request_id = request_id;
                response
            }
            Err(error) => ResponseEnvelope {
                schema: PROTOCOL_SCHEMA.to_owned(),
                request_id,
                status: kiana_domain::ExecutionStatus::Failed,
                output: serde_json::Value::Null,
                error: Some(error.to_string()),
            },
        };
        if response.status.is_terminal() {
            if let Some(run_id) = response
                .output
                .get("run_id")
                .and_then(|value| value.as_str())
                .and_then(RunId::parse_str)
            {
                self.run_stream.publish_terminal(run_id, response.clone());
            }
        }
        response
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalModelConfig {
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

fn effective_permission_profile(
    body: &RequestBody,
    declared: PermissionProfile,
) -> PermissionProfile {
    match body {
        RequestBody::Run(request) => permission_profile_for_sandbox(request.sandbox.as_deref()),
        RequestBody::Continue(request) => {
            permission_profile_for_sandbox(request.sandbox.as_deref())
        }
        RequestBody::Spawn(request) => permission_profile_for_sandbox(request.sandbox.as_deref()),
        RequestBody::Symposium(request) => {
            permission_profile_for_sandbox(request.sandbox.as_deref())
        }
        RequestBody::ApprovalDecision(_) => declared,
        RequestBody::Review(_) | RequestBody::Close(_) => PermissionProfile::Balanced,
        RequestBody::Command(_) | RequestBody::Cancel(_) | RequestBody::Receipt(_) => {
            PermissionProfile::Safe
        }
    }
}

fn permission_profile_for_sandbox(sandbox: Option<&str>) -> PermissionProfile {
    match sandbox.map(str::trim) {
        Some("workspace-write" | "workspace_write" | "workspace") => PermissionProfile::Balanced,
        _ => PermissionProfile::Safe,
    }
}
