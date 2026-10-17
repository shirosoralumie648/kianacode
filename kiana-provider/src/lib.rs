//! Provider gateway：把一次已准入的模型调用发送到上游服务商。
//!
//! 本 crate 位于 `kiana-daemon::model_client` 注入的 `ModelClient` 实现之后、具体 HTTP
//! transport 之前。它接收 `kiana-domain` 的统一 `ModelRequest`，由 `request` 模块编译成
//! provider wire body，再由 `transport` 发出，最后由 `response` 归一化为 `ModelReply`。
//! 它不创建 Agent loop、不执行模型工具，也不替代 `kiana-core` 的策略和审批。
//!
//! `ProviderGateway` 只允许经过 `prepare_call`、预算 admission、凭据/配置代次校验后才
//! 进入 `complete_admitted`。直接调用便捷的 `complete` / `complete_prepared` 会显式拒绝，
//! 防止调用方绕过模型调用预算、路由围栏和执行前的凭据检查。daemon 也可以注入
//! `ScriptedModel` 或 `UnavailableModel`；本 crate 的 gateway 源码存在不等于当前状态账本
//! 已证明 live provider 成功。
mod capacity;
mod config;
mod connector_quota;
mod credentials;
mod fallback;
mod oauth;
pub mod replay;
mod request;
mod resolver;
mod response;
mod telemetry;
mod transport;
mod usage;
mod usage_adapters;
use async_trait::async_trait;
pub use config::ProviderConfig;
pub use connector_quota::{
    validate_connector_quota_lease, CONNECTOR_QUOTA_PROVIDER_BOUNDARY_SCHEMA,
};
use credentials::{CredentialRevocations, RevocationFencedStore};
pub use fallback::{validate_fallback_attempt, FALLBACK_PROVIDER_BOUNDARY_SCHEMA};
use kiana_domain::*;
use kiana_ports::{ModelBudgetPort, ModelClient};
pub use resolver::{
    redacted_workspace_value, ConfigResolver, WorkspaceConfig, WorkspaceConfigTrust,
    WorkspaceProfile, MAX_WORKSPACE_CONFIG_BYTES,
};
pub use response::{replay_stream_fixture, replay_stream_fixture_capturing};
use std::collections::BTreeMap;
use std::sync::Arc;
pub use telemetry::{safe_prepared_metadata, MODEL_ATTEMPT_TELEMETRY_SCHEMA};
pub use usage::normalize_model_reply;
pub use usage_adapters::{
    normalize_provider_usage, provider_usage_containment, MAX_PROVIDER_USAGE_TOKENS,
    PROVIDER_USAGE_ADAPTER_SCHEMA,
};

/// 持有已解析 provider 连接和容量状态的网关。
///
/// `state` 以异步读写锁保护当前配置快照：准备调用时读取同一代配置，workspace reload
/// 只能在写锁边界替换候选快照。已准入请求会在网络 effect 前重新确认路由、凭据代次和
/// permit，避免配置热更新把一个旧请求悄悄送到新连接。
pub struct ProviderGateway {
    state: tokio::sync::RwLock<GatewayState>,
    /// Narrow, in-process revocation fence shared with every resolved connection.  It holds
    /// reference metadata only; recording or checking a revocation never reads a secret value.
    revocations: CredentialRevocations,
}

struct GatewayState {
    resolved: resolver::Resolution,
    // Removed scopes remain bounded and retained so removing/re-adding a profile cannot reset
    // its process-local RPM/TPM window or circuit. They contain no credentials or HTTP clients.
    capacities: BTreeMap<String, CapacityState>,
}

#[derive(Clone)]
struct CapacityState {
    policy: Arc<ProviderCapacityPolicy>,
    window: Arc<capacity::CapacityWindow>,
    active: Arc<tokio::sync::Semaphore>,
    queued: Arc<tokio::sync::Semaphore>,
    circuit: Arc<std::sync::Mutex<ProviderCircuitBreaker>>,
}

impl GatewayState {
    fn candidate(
        mut resolved: resolver::Resolution,
        previous: BTreeMap<String, CapacityState>,
        revocations: &CredentialRevocations,
    ) -> Result<Self, ModelError> {
        let mut capacities = previous.clone();
        let mut current = BTreeMap::<String, CapacityState>::new();
        for connection in resolved.connections.values_mut() {
            // Enforce the narrow backend registry and the revocation fence at every read from the
            // protected source, including admission-time revision checks and effect-time issuance.
            let store = connection.credential_store.clone();
            let fenced = RevocationFencedStore::new(store, revocations.clone());
            connection.credential_store = Arc::new(fenced);
            let scope = config::capacity_scope(connection);
            let shared = if let Some(shared) = current.get(&scope) {
                shared.clone()
            } else {
                let retained = previous.get(&scope);
                if let Some(retained) = retained {
                    let old = &retained.policy;
                    let new = &connection.capacity_policy;
                    if (
                        old.max_concurrency,
                        old.max_queue,
                        old.requests_per_minute,
                        old.tokens_per_minute,
                        old.max_queue_wait_ms,
                    ) != (
                        new.max_concurrency,
                        new.max_queue,
                        new.requests_per_minute,
                        new.tokens_per_minute,
                        new.max_queue_wait_ms,
                    ) {
                        return Err(ModelError::invalid("config_capacity_policy_changed"));
                    }
                }
                let prior_circuit = retained
                    .map(|retained| &retained.circuit)
                    .unwrap_or(&connection.circuit);
                // Rebind health to the new revision without resetting failures/open cooldown.
                // Clone first: a later candidate failure must leave the live circuit untouched.
                let mut circuit = prior_circuit
                    .lock()
                    .map_err(|_| ModelError::invalid("provider_circuit_lock_poisoned"))?
                    .clone();
                circuit.config_revision = connection.route.configuration_revision.clone();
                circuit.health_digest = circuit.digest();
                circuit.validate().map_err(ModelError::invalid)?;
                let shared = CapacityState {
                    policy: connection.capacity_policy.clone(),
                    window: retained
                        .map(|state| &state.window)
                        .unwrap_or(&connection.capacity_window)
                        .clone(),
                    active: retained
                        .map(|state| &state.active)
                        .unwrap_or(&connection.capacity)
                        .clone(),
                    queued: retained
                        .map(|state| &state.queued)
                        .unwrap_or(&connection.queue_slots)
                        .clone(),
                    circuit: Arc::new(std::sync::Mutex::new(circuit)),
                };
                current.insert(scope.clone(), shared.clone());
                shared
            };
            connection.capacity_policy = shared.policy.clone();
            connection.capacity_window = shared.window.clone();
            connection.capacity = shared.active.clone();
            connection.queue_slots = shared.queued.clone();
            connection.circuit = shared.circuit.clone();
            capacities.insert(scope, shared);
        }
        if capacities.len() > 256 {
            return Err(ModelError::invalid("config_capacity_scope_limit_exceeded"));
        }
        let candidate = Self {
            resolved,
            capacities,
        };
        candidate.model_catalog()?;
        Ok(candidate)
    }

    fn connection(&self, spec: &ModelCallSpec) -> Result<&config::Connection, ModelError> {
        let assignment = spec
            .assignment
            .as_ref()
            .ok_or_else(|| ModelError::invalid("model_server_assignment_required"))?;
        assignment.validate()?;
        if let Some(connection) = self.resolved.connections.get(&assignment.profile) {
            return Ok(connection);
        }
        if self.resolved.explicit_profiles {
            return Err(ModelError::invalid("model_profile_unconfigured"));
        }
        self.resolved
            .connections
            .get("default")
            .ok_or_else(|| ModelError::invalid("model_default_route_unavailable"))
    }

    fn model_catalog(&self) -> Result<ModelCatalog, ModelError> {
        let entries = self
            .resolved
            .connections
            .values()
            .map(|connection| {
                let entry = ModelCatalogEntry {
                    schema: MODEL_CATALOG_ENTRY_SCHEMA.to_owned(),
                    provider_id: connection.route.provider_id.clone(),
                    connection_id: connection.route.connection_id.clone(),
                    model_id: connection.route.model_id.clone(),
                    capabilities: connection.capabilities.clone(),
                    source: if connection.route.profile == "default" {
                        ModelCatalogSource::Builtin
                    } else {
                        ModelCatalogSource::Configured
                    },
                    catalog_revision: connection.route.configuration_revision.clone(),
                    expires_at_unix_ms: None,
                };
                (
                    (
                        entry.provider_id.clone(),
                        entry.connection_id.clone(),
                        entry.model_id.clone(),
                    ),
                    entry,
                )
            })
            .collect::<BTreeMap<_, _>>();
        // Whole-default profile aliases describe one catalog identity, not duplicate models.
        ModelCatalog::new(entries.into_values().collect()).map_err(ModelError::invalid)
    }
}
impl ProviderGateway {
    /// 从显式配置和进程环境解析 provider，构造第一代连接快照。
    ///
    /// 该方法只解析配置，不发网络请求；配置无效、协议不支持或凭据边界不满足时返回
    /// 结构化 `ModelError`，不会创建一个“先运行再猜”的半有效网关。
    pub fn from_env(config: ProviderConfig) -> Result<Self, ModelError> {
        let resolved = resolver::ConfigResolver::resolve(config)?;
        Self::from_resolution(resolved)
    }

    pub fn from_workspace(
        config: ProviderConfig,
        raw: &str,
        trust: WorkspaceConfigTrust<'_>,
    ) -> Result<Self, ModelError> {
        let resolved = ConfigResolver::resolve_with_workspace(config, raw, trust, 0)?;
        Self::from_resolution(resolved)
    }

    fn from_resolution(resolved: resolver::Resolution) -> Result<Self, ModelError> {
        let revocations = CredentialRevocations::default();
        let state = GatewayState::candidate(resolved, BTreeMap::new(), &revocations)?;
        Ok(Self {
            state: tokio::sync::RwLock::new(state),
            revocations,
        })
    }

    fn snapshot(&self) -> Result<tokio::sync::RwLockReadGuard<'_, GatewayState>, ModelError> {
        self.state
            .try_read()
            .map_err(|_| ModelError::invalid("config_reload_in_progress"))
    }

    /// Publish a completely validated candidate only at an idle effect boundary. `expected_revision`
    /// is the current ProviderConfigSnapshot digest, not a caller-selected replacement revision.
    /// Busy is a retryable refusal: no new revision is published while an admitted effect is pinned.
    /// The caller must supply/revalidate server ProjectTrust; no trust is inferred from config text.
    pub fn reload_workspace(
        &self,
        config: ProviderConfig,
        raw: &str,
        trust: WorkspaceConfigTrust<'_>,
        expected_revision: &str,
    ) -> Result<ProviderConfigSnapshot, ModelError> {
        trust.validate()?;
        let generation = {
            let state = self.snapshot()?;
            if state.resolved.snapshot.snapshot_digest != expected_revision {
                return Err(ModelError::invalid("config_snapshot_revision_changed"));
            }
            state
                .resolved
                .generation
                .checked_add(1)
                .ok_or_else(|| ModelError::invalid("config_reload_generation_overflow"))?
        };
        let resolved = ConfigResolver::resolve_with_workspace(config, raw, trust, generation)?;
        let mut state = self
            .state
            .try_write()
            .map_err(|_| ModelError::invalid("config_reload_busy"))?;
        if state.resolved.snapshot.snapshot_digest != expected_revision {
            return Err(ModelError::invalid("config_snapshot_revision_changed"));
        }
        let previous = state.capacities.clone();
        let candidate = GatewayState::candidate(resolved, previous, &self.revocations)?;
        let snapshot = candidate.resolved.snapshot.clone();
        *state = candidate;
        Ok(snapshot)
    }
    pub fn catalog(&self) -> serde_json::Value {
        let state = match self.snapshot() {
            Ok(state) => state,
            Err(error) => {
                return serde_json::json!({"schema":"kiana.model-catalog.v1","error":{"code":error.code}})
            }
        };
        serde_json::json!({
            "schema":"kiana.model-catalog.v1",
            "connections":state.resolved.connections.values().map(|connection|serde_json::json!({"route":connection.route,"capabilities":connection.capabilities})).collect::<Vec<_>>(),
            "configuration":state.resolved.snapshot,
            "model_catalog":state.model_catalog().ok(),
        })
    }

    pub fn configuration_snapshot(&self) -> Result<ProviderConfigSnapshot, ModelError> {
        Ok(self.snapshot()?.resolved.snapshot.clone())
    }

    /// Record a server-owned revocation for exactly this opaque reference generation.
    ///
    /// After this call, any admission-time revision check or effect-time issuance for a
    /// generation at or below the recorded watermark fails closed with
    /// `credential_secret_ref_revoked` before the protected source is read and before any
    /// provider request is built.  The reference value is never resolved here.  This is the
    /// narrow, in-process fence; durable/event-sourced revocation belongs to the recovery steps.
    pub fn revoke_credential_ref(&self, secret_ref: &SecretRef) -> Result<(), ModelError> {
        self.revocations.revoke(secret_ref)
    }

    /// Canonical, secret-free workspace input and its server trust binding. Actual resolved
    /// precedence is exposed by `configuration_snapshot`, from the same generation.
    pub fn workspace_configuration_snapshot(&self) -> Result<Option<ConfigSnapshot>, ModelError> {
        Ok(self.snapshot()?.resolved.workspace_snapshot.clone())
    }

    /// Return only the secret-free identity that a per-connection live evidence record may bind.
    /// This does not execute a request or turn metadata into provider proof.
    pub fn live_connection_metadata(
        &self,
        profile: &str,
    ) -> Result<ProviderLiveConnectionMetadata, ModelError> {
        let state = self.snapshot()?;
        let connection = state
            .resolved
            .connections
            .get(profile)
            .ok_or_else(|| ModelError::invalid("model_profile_unconfigured"))?;
        ProviderLiveConnectionMetadata::new(
            connection.route.clone(),
            connection.capabilities.clone(),
            state.resolved.snapshot.snapshot_digest.clone(),
            connection.credential_revision.clone(),
            connection.provider_account.clone(),
        )
        .map_err(ModelError::invalid)
    }

    pub fn model_catalog(&self) -> Result<ModelCatalog, ModelError> {
        self.snapshot()?.model_catalog()
    }

    /// Prepare a request with explicitly admitted image artifacts.  The bytes are supplied by the
    /// caller's Artifact/data-governance adapter; this method never resolves a path or fetches a
    /// URL.  A normal `prepare_call` remains deny-first for AttachmentRef values without this
    /// admission list.
    pub fn prepare_call_with_images(
        &self,
        request: ModelRequest,
        spec: ModelCallSpec,
        images: Vec<ImageInputAdmission>,
    ) -> Result<PreparedModelCall, ModelError> {
        let state = self.snapshot()?;
        request::compile_with_images(state.connection(&spec)?, request, spec, &images)
    }

    /// Prepare a request that resumes a provider continuation through a protected replay
    /// adapter. The adapter owns scope, TTL and persistence; this gateway only refuses to
    /// compile when the material is missing, expired, revoked or bound to another route.
    pub fn prepare_call_with_protected_replay(
        &self,
        request: ModelRequest,
        spec: ModelCallSpec,
        images: Vec<ImageInputAdmission>,
        source: &dyn ProtectedReplaySource,
        now_unix_ms: u64,
    ) -> Result<PreparedModelCall, ModelError> {
        let state = self.snapshot()?;
        request::compile_with_protected_replay(
            state.connection(&spec)?,
            request,
            spec,
            &images,
            source,
            now_unix_ms,
        )
    }
}
#[async_trait]
impl ModelClient for ProviderGateway {
    /// 只准备统一模型请求，不执行网络 effect。
    ///
    /// `prepare_call` 会读取当前连接快照并调用 request compiler，返回带 request hash、
    /// route、工具目录摘要和 token budget 的 `PreparedModelCall`。真正发送必须再经过
    /// admission 与 permit 校验。
    fn prepare_call(
        &self,
        request: ModelRequest,
        spec: ModelCallSpec,
    ) -> Result<PreparedModelCall, ModelError> {
        let state = self.snapshot()?;
        request::compile(state.connection(&spec)?, request, spec)
    }
    fn request_context(&self, request: &ModelRequest) -> ModelRequestContext {
        ModelRequestContext::for_request(request)
    }
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        // 这个便利接口没有携带模型调用 permit，拒绝它比隐式绕过预算更安全。
        Err("provider_requires_model_admission".to_owned())
    }
    async fn complete_prepared(
        &self,
        _prepared: PreparedModelCall,
        _sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        Err(ModelError::invalid("provider_requires_model_admission"))
    }
    async fn complete_admitted(
        &self,
        prepared: PreparedModelCall,
        permit: ModelCallPermit,
        admission: &dyn ModelBudgetPort,
        sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        // 这里是唯一进入 provider transport 的边界：先锁住配置代次，再校验 permit、
        // 凭据 revision 和预算，最后才把已经封存的 wire body 交给 transport。
        prepared.validate()?;
        // Validate the allow-listed instrumentation view at the provider boundary.  The value is
        // deliberately not built from wire_body or response content; the runner persists the
        // same safe fields through its committed model-turn event.
        let _telemetry = telemetry::safe_prepared_metadata(&prepared)?;
        // Pin the same generation across budget admission, capacity waits, credential lease
        // issuance and network dispatch. Reload uses try_write, so it cannot commit between
        // this fence and the effect boundary; cancellation drops the Tokio read guard.
        let state = self.state.read().await;
        let connection = state.connection(&prepared.spec)?;
        // Compilation installs the server assignment's profile and may force Gemini streaming.
        // The configuration revision freezes the underlying connection, not those presentation
        // adjustments. Compare the immutable connection identity before permit consumption.
        if connection.route.configuration_revision != prepared.route.configuration_revision
            || connection.route.connection_id != prepared.route.connection_id
            || connection.route.provider_id != prepared.route.provider_id
            || connection.route.model_id != prepared.route.model_id
            || connection.route.protocol != prepared.route.protocol
        {
            return Err(ModelError::invalid("model_route_changed_after_admission"));
        }
        let admission_now = transport::unix_ms()?;
        permit.validate_for_prepared(&prepared, admission_now)?;
        if let Some(secret_ref) = &connection.credential_ref {
            let current_revision = connection.credential_store.current_revision(secret_ref)?;
            if current_revision != connection.credential_revision {
                return Err(ModelError::invalid("model_credential_revision_changed"));
            }
        } else if connection.credential_revision != "none" {
            return Err(ModelError::invalid("model_credential_revision_missing"));
        }
        admission
            .consume_prepared(&prepared, &permit)
            .await
            .map_err(|e| ModelError::invalid(e.to_string()))?;
        transport::send(connection, prepared, sink).await
    }
}
