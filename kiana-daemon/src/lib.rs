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
use kiana_core::{ControlPlane, ControlPlaneRuntimeConfig};
pub use kiana_domain::StreamingRedactor;
use kiana_domain::{
    CommandIntent, PermissionProfile, RequestContext, RoleSpec, RunId, RuntimeEvent,
};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{PortError, RunnerPort};
use kiana_protocol::{RequestBody, RequestEnvelope, ResponseEnvelope, PROTOCOL_SCHEMA};
use kiana_runner::{KianaHarness, RuntimeConfig};
use run_stream::RunStreamBus;
pub use run_stream::RunStreamSubscription;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const ENV_HARNESS_MAX_STEPS: &str = "KIANA_HARNESS_MAX_STEPS";
const ENV_HARNESS_WALL_TIME_MS: &str = "KIANA_HARNESS_WALL_TIME_MS";

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

    /// 只读解析一个角色的 harness runtime 配置。
    ///
    /// 显式环境开关 > 角色快照 > harness 默认值。
    pub fn harness_runtime_config(
        &self,
        role: Option<RoleSpec>,
    ) -> Result<RuntimeConfig, PortError> {
        harness_runtime_config_from_env().map(|config| config.into_runtime_config(role))
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
            Arc::new(configured_env_harness()?),
            Arc::new(JsonlEventLog::open_default()?),
            Arc::new(JsonlApprovalStore::open_default()?),
        )
    }

    pub fn local_with_model_config(config: LocalModelConfig) -> Result<Self, PortError> {
        Self::with_runner_events_and_approval(
            Arc::new(KianaHarness::with_config(
                model_client::from_config(config),
                harness_runtime_config_from_env()?.into_runtime_config(None),
            )),
            Arc::new(JsonlEventLog::open_default()?),
            Arc::new(JsonlApprovalStore::open_default()?),
        )
    }

    pub fn local_with_project_authority(
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_approval_and_authority(
            Arc::new(configured_env_harness()?),
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
        Self::with_runner(Arc::new(configured_env_harness()?))
    }

    pub fn with_env_harness_and_project_authority(
        project_authority: Arc<dyn ProjectTrustAuthority>,
    ) -> Result<Self, PortError> {
        Self::with_runner_events_approval_and_authority(
            Arc::new(configured_env_harness()?),
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
        let runtime_config = harness_runtime_config_from_env()?;
        let runner = harness_skills::SkillAwareRunner::wrap(runner);
        let runner = run_stream::RunStreamRunner::wrap(runner, run_stream.clone());
        let mut capabilities = CapabilityBroker::new();
        context_query::register(&mut capabilities)?;
        harness_capabilities::register(&mut capabilities)?;
        harness_mcp::register(&mut capabilities)?;
        harness_memory::register(&mut capabilities)?;
        let core = ControlPlane::with_pre_tool_hooks_and_runtime_config(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events,
            Arc::new(capabilities),
            approvals,
            runner,
            Arc::new(pre_tool_hooks::QueryPreToolHooks),
            ControlPlaneRuntimeConfig {
                max_steps_per_turn: runtime_config.into_runtime_config(None).max_steps_per_turn,
            },
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

/// Harness runtime inputs resolved when a daemon is composed.
///
/// A value is set only by the explicit environment override. A role supplies
/// its snapshot only when this override is absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HarnessRuntimeConfig {
    pub max_steps_override: Option<u32>,
}

impl HarnessRuntimeConfig {
    fn into_runtime_config(self, role: Option<RoleSpec>) -> RuntimeConfig {
        let mut config = RuntimeConfig::default();
        if let Some(max_steps) = self
            .max_steps_override
            .or_else(|| role.map(|role| role.max_steps))
        {
            config.max_steps_per_turn = max_steps;
        }
        config
    }
}

fn configured_env_harness() -> Result<KianaHarness, PortError> {
    let config = runtime_config_from_env()?;
    Ok(KianaHarness::with_config(model_client::from_env(), config))
}

pub fn runtime_config_from_env() -> Result<RuntimeConfig, PortError> {
    runtime_config_from_lookup(|name| std::env::var(name))
}

fn harness_runtime_config_from_env() -> Result<HarnessRuntimeConfig, PortError> {
    harness_runtime_config_from_lookup(|name| std::env::var(name))
}

fn harness_runtime_config_from_lookup(
    mut lookup: impl FnMut(&str) -> Result<String, std::env::VarError>,
) -> Result<HarnessRuntimeConfig, PortError> {
    match lookup(ENV_HARNESS_MAX_STEPS) {
        Ok(raw) => {
            let max_steps = raw
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid_runtime_config(ENV_HARNESS_MAX_STEPS))?;
            if max_steps == 0 {
                return Err(invalid_runtime_config(ENV_HARNESS_MAX_STEPS));
            }
            Ok(HarnessRuntimeConfig {
                max_steps_override: Some(max_steps),
            })
        }
        Err(std::env::VarError::NotPresent) => Ok(HarnessRuntimeConfig::default()),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(invalid_runtime_config(ENV_HARNESS_MAX_STEPS))
        }
    }
}

fn runtime_config_from_lookup(
    mut lookup: impl FnMut(&str) -> Result<String, std::env::VarError>,
) -> Result<RuntimeConfig, PortError> {
    let mut config = RuntimeConfig::default();

    match lookup(ENV_HARNESS_MAX_STEPS) {
        Ok(raw) => {
            let max_steps = raw
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid_runtime_config(ENV_HARNESS_MAX_STEPS))?;
            if max_steps == 0 {
                return Err(invalid_runtime_config(ENV_HARNESS_MAX_STEPS));
            }
            config.max_steps_per_turn = max_steps;
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(invalid_runtime_config(ENV_HARNESS_MAX_STEPS));
        }
    }

    match lookup(ENV_HARNESS_WALL_TIME_MS) {
        Ok(raw) => {
            let wall_time_ms = raw
                .trim()
                .parse::<u64>()
                .map_err(|_| invalid_runtime_config(ENV_HARNESS_WALL_TIME_MS))?;
            if wall_time_ms == 0 {
                return Err(invalid_runtime_config(ENV_HARNESS_WALL_TIME_MS));
            }
            config.wall_time_budget = Some(Duration::from_millis(wall_time_ms));
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(invalid_runtime_config(ENV_HARNESS_WALL_TIME_MS));
        }
    }

    Ok(config)
}

fn invalid_runtime_config(name: &str) -> PortError {
    PortError::Failed(format!(
        "runtime_config_invalid:{name}:expected_positive_integer"
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_runner::UnavailableModel;

    fn harness_with_config(config: RuntimeConfig) -> KianaHarness {
        KianaHarness::with_config(Arc::new(UnavailableModel::default()), config)
    }

    #[test]
    fn unset_harness_environment_uses_default_runtime_config() {
        let config = runtime_config_from_lookup(|_| Err(std::env::VarError::NotPresent)).unwrap();
        let runtime_config = harness_with_config(config).config();

        assert_eq!(runtime_config, RuntimeConfig::default());
        assert_eq!(runtime_config.max_steps_per_turn, 32);
        assert_eq!(runtime_config.wall_time_budget, None);
    }

    #[test]
    fn harness_max_steps_environment_override_reaches_runtime_config() {
        let config = runtime_config_from_lookup(|name| {
            if name == ENV_HARNESS_MAX_STEPS {
                Ok("5".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap();

        assert_eq!(
            harness_with_config(config).config(),
            RuntimeConfig {
                max_steps_per_turn: 5,
                ..RuntimeConfig::default()
            }
        );
    }

    #[test]
    fn harness_wall_time_environment_override_reaches_runtime_config() {
        let config = runtime_config_from_lookup(|name| {
            if name == ENV_HARNESS_WALL_TIME_MS {
                Ok("1000".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap();

        assert_eq!(
            harness_with_config(config).config(),
            RuntimeConfig {
                wall_time_budget: Some(Duration::from_secs(1)),
                ..RuntimeConfig::default()
            }
        );
    }

    #[test]
    fn invalid_role_harness_environment_value_fails_closed() {
        let error = harness_runtime_config_from_lookup(|name| {
            if name == ENV_HARNESS_MAX_STEPS {
                Ok("abc".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap_err();

        assert_eq!(
            error,
            PortError::Failed(format!(
                "runtime_config_invalid:{ENV_HARNESS_MAX_STEPS}:expected_positive_integer"
            ))
        );
    }

    #[test]
    fn non_numeric_harness_environment_value_fails_closed() {
        for name in [ENV_HARNESS_MAX_STEPS, ENV_HARNESS_WALL_TIME_MS] {
            let error = runtime_config_from_lookup(|candidate| {
                if candidate == name {
                    Ok("abc".to_owned())
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .unwrap_err();

            assert_eq!(
                error,
                PortError::Failed(format!(
                    "runtime_config_invalid:{name}:expected_positive_integer"
                ))
            );
        }
    }

    #[test]
    fn zero_harness_environment_value_fails_closed() {
        for name in [ENV_HARNESS_MAX_STEPS, ENV_HARNESS_WALL_TIME_MS] {
            let error = runtime_config_from_lookup(|candidate| {
                if candidate == name {
                    Ok("0".to_owned())
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .unwrap_err();

            assert_eq!(
                error,
                PortError::Failed(format!(
                    "runtime_config_invalid:{name}:expected_positive_integer"
                ))
            );
        }
    }
}
