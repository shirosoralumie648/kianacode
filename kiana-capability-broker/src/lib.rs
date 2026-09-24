//! Kiana 已授权能力请求的本地路由 Broker。
//!
//! 控制面完成 trust、角色、策略、Gate 和审批判断后，才会把原始请求封装为
//! [`AuthorizedCapabilityRequest`] 并交给本 crate。Broker 根据
//! `(CapabilityKind, operation)` 精确选择一个适配器，负责路由和调用，但不签发授权、
//! 不推断近似操作名，也不把未知能力回退到通用 shell。
//!
//! 这里的 `authorization_id` 目前是控制面传入的关联标识，并非密码学签名。Broker 接受
//! 已授权类型不等于授权已经 durable 或可跨进程验证；这类证明仍属于控制面、事件存储
//! 和具体授权实现的责任。

use async_trait::async_trait;
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, ConnectorBindingSnapshot,
    ConnectorCredentialEvidence, ConnectorCredentialInvocation, CredentialLease,
    ExtensionAdapterDescriptor, ExtensionExecutionContract, RequestId,
};
use kiana_ports::{CapabilityBrokerPort, PortError};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

type HandlerKey = (CapabilityKind, String);

/// Revalidate and consume credential metadata immediately before an effect.  The broker never
/// resolves or returns the raw value; provider/connector adapters keep that operation private to
/// their final request boundary and may pass only this consumed lease's digest to receipts.
pub fn consume_credential_lease(
    lease: &mut CredentialLease,
    now_unix_ms: u64,
    provider_account: &str,
    purpose: &str,
    audience: &str,
    endpoint_digest: &str,
) -> Result<(), PortError> {
    lease
        .validate_for(
            now_unix_ms,
            provider_account,
            purpose,
            audience,
            endpoint_digest,
        )
        .map_err(PortError::Failed)?;
    lease.consume(now_unix_ms).map_err(PortError::Failed)
}

/// Consume connector credential metadata only after the current server-owned binding has been
/// revalidated at the adapter effect boundary. Raw secret resolution remains adapter-private.
#[allow(clippy::too_many_arguments)]
pub fn consume_connector_credential_invocation(
    invocation: &mut ConnectorCredentialInvocation,
    binding: &ConnectorBindingSnapshot,
    operation: &str,
    invocation_id: RequestId,
    idempotency_key: &str,
    now_unix_ms: u64,
) -> Result<ConnectorCredentialEvidence, PortError> {
    invocation
        .consume_for(
            binding,
            operation,
            invocation_id,
            idempotency_key,
            now_unix_ms,
        )
        .map_err(PortError::Failed)
}

#[async_trait]
/// 单个能力操作的受控执行适配器。
///
/// 每个实现只应处理注册时声明的能力种类和精确操作名，并在请求携带的 sandbox、路径
/// 与参数边界内工作。Handler 不接收原始模型调用，因此不能自行绕过控制面创建或扩展
/// [`AuthorizedCapabilityRequest`]。
pub trait CapabilityHandler: Send + Sync {
    fn binding_version(&self) -> &'static str {
        kiana_domain::ACTION_HANDLER_BINDING_VERSION
    }
    /// 执行一项已授权请求并返回与原请求 ID 对应的结构化结果。
    ///
    /// 端口/传输层故障使用 [`PortError`]；能力执行成功或业务失败由
    /// [`CapabilityResult`] 表达。实现不得在结果不可判定时伪造成功证据。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError>;

    async fn execute_cancellable(
        &self,
        request: AuthorizedCapabilityRequest,
        cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        if *cancellation.borrow() {
            return Ok(CapabilityResult::success(
                request.request.request_id,
                serde_json::json!({"cancelled":true,"not_executed":true,"stop_confirmed":true}),
            ));
        }
        let request_id = request.request.request_id;
        // Await ownership of blocking writers and child cleanup. Dropping an execute future
        // does not stop spawn_blocking work and cannot release its execution reservation.
        let mut result =
            kiana_domain::normalize_capability_result(request_id, self.execute(request).await?);
        if *cancellation.borrow()
            && !result
                .failure_code()
                .is_some_and(|code| code.policy().requires_reconciliation)
        {
            if !result.output.is_object() {
                result.output = serde_json::json!({"detail":result.output});
            }
            result.output["completed_before_cancel"] = serde_json::json!(result.success);
            result.output["cancelled"] = serde_json::json!(true);
            result.output["stop_confirmed"] = serde_json::json!(true);
            result = kiana_domain::normalize_capability_result(request_id, result);
        }
        Ok(result)
    }
}

/// Installed extension admission is rechecked immediately before dispatch, so a cached
/// descriptor cannot survive revocation or silently change to a newer package version.
#[async_trait]
pub trait ExtensionAdmission: Send + Sync {
    async fn check(
        &self,
        request: &AuthorizedCapabilityRequest,
        contract: &ExtensionExecutionContract,
    ) -> Result<(), PortError>;

    /// Component adapters remain metadata until this final binding check succeeds. The default
    /// implementation preserves compatibility for admissions that only manage legacy extension
    /// execution contracts; daemon-owned admissions should override it when a component registry
    /// is available.
    async fn check_component_adapter(
        &self,
        _request: &AuthorizedCapabilityRequest,
        descriptor: &ExtensionAdapterDescriptor,
    ) -> Result<(), PortError> {
        if descriptor.status != kiana_domain::ExtensionAdapterStatus::Available {
            return Err(PortError::Failed(format!(
                "extension_adapter_not_available:{}",
                descriptor.reason
            )));
        }
        Ok(())
    }
}

struct ExtensionHandler {
    handler: Arc<dyn CapabilityHandler>,
    contract: ExtensionExecutionContract,
    admission: Arc<dyn ExtensionAdmission>,
    component: Option<ExtensionAdapterDescriptor>,
}

#[async_trait]
impl CapabilityHandler for ExtensionHandler {
    fn binding_version(&self) -> &'static str {
        self.handler.binding_version()
    }
    async fn execute_cancellable(
        &self,
        request: AuthorizedCapabilityRequest,
        cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        self.contract
            .check(&request.request)
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        self.admission.check(&request, &self.contract).await?;
        if let Some(component) = &self.component {
            self.admission
                .check_component_adapter(&request, component)
                .await?;
        }
        self.handler
            .execute_cancellable(request, cancellation)
            .await
    }
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.contract
            .check(&request.request)
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        self.admission.check(&request, &self.contract).await?;
        if let Some(component) = &self.component {
            self.admission
                .check_component_adapter(&request, component)
                .await?;
        }
        self.handler.execute(request).await
    }
}

#[derive(Default)]
/// 将已授权请求路由到唯一 Handler 的进程内注册表。
///
/// 注册表受异步读写锁保护：执行路径只短暂读取并克隆 [`Arc`]，随后在不持锁的情况下
/// `await` Handler，避免长时间工具调用阻塞其他注册读取。当前类型不提供覆盖或注销；
/// 重复键会被拒绝，从而防止后注册适配器悄悄改变同一操作的执行含义。
pub struct CapabilityBroker {
    /// 以能力种类和精确操作名为键的 Handler 集合。
    handlers: RwLock<HashMap<HandlerKey, Arc<dyn CapabilityHandler>>>,
    extension_admission: Option<Arc<dyn ExtensionAdmission>>,
    permit_verifier: Option<Arc<dyn kiana_ports::ExecutionPermitVerifierPort>>,
    catalog_sealed: bool,
}

impl CapabilityBroker {
    /// Called once by DaemonHost after all static registrations and before any model request.
    pub fn validate_catalog_bindings(&mut self) -> Result<(), PortError> {
        kiana_domain::validate_action_catalog().map_err(PortError::Failed)?;
        let handlers = self.handlers.get_mut();
        for operation in kiana_domain::ACTION_OPERATIONS {
            let descriptor = kiana_domain::capability_action_descriptor(operation)
                .ok_or_else(|| PortError::Failed("capability_descriptor_missing".to_owned()))?;
            kiana_domain::validate_schema_contract(&descriptor.argument_schema)
                .map_err(PortError::Failed)?;
            kiana_domain::validate_schema_contract(&descriptor.result_schema)
                .map_err(PortError::Failed)?;
            let handler = handlers
                .get(&(descriptor.capability, (*operation).to_owned()))
                .ok_or_else(|| {
                    PortError::Unavailable(format!("capability_binding_missing:{operation}"))
                })?;
            if handler.binding_version() != descriptor.binding_version {
                return Err(PortError::Failed(
                    "capability_binding_version_mismatch".to_owned(),
                ));
            }
        }
        if handlers.len() != kiana_domain::ACTION_OPERATIONS.len() {
            return Err(PortError::Failed(
                "capability_binding_catalog_mismatch".to_owned(),
            ));
        }
        self.catalog_sealed = true;
        Ok(())
    }

    fn validate_action(&self, request: &AuthorizedCapabilityRequest) -> Result<(), PortError> {
        if !self.catalog_sealed {
            return Err(PortError::Unavailable(
                "capability_catalog_unvalidated".to_owned(),
            ));
        }
        kiana_domain::validate_action_catalog().map_err(PortError::Failed)?;
        let mut normalized = request.request.clone();
        kiana_domain::normalize_capability_action(&mut normalized)
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        if normalized != request.request {
            return Err(PortError::Failed(
                "capability_action_not_prepared".to_owned(),
            ));
        }
        let scope = request
            .request
            .execution_scope
            .as_ref()
            .ok_or_else(|| PortError::Failed("execution_scope_required".to_owned()))?;
        scope
            .validate_for_request(&request.request)
            .map_err(PortError::Failed)?;
        validate_network_observation(&request.request, scope)?;
        Ok(())
    }
    async fn admit_extensions(
        &self,
        request: &AuthorizedCapabilityRequest,
    ) -> Result<(), PortError> {
        if let Some(scopes) = kiana_domain::request_extension_scopes(&request.request)
            .map_err(|e| PortError::Failed(e.to_owned()))?
        {
            for scope in scopes {
                let admission = self.extension_admission.as_ref().ok_or_else(|| {
                    PortError::Unavailable("extension_admission_unavailable".to_owned())
                })?;
                // Concrete built-in read behavior is classified by the broker, never by
                // the model or skill manifest. Shell remains potentially mutating.
                let effect = if request.request.capability == CapabilityKind::Query
                    && matches!(
                        request.request.operation.as_str(),
                        "memory.search" | "context.repo_map" | "context.search"
                    ) {
                    kiana_domain::ExtensionEffect::ReadOnly
                } else {
                    kiana_domain::ExtensionEffect::ReadWrite
                };
                let contract = scope.contract(effect);
                contract
                    .check(&request.request)
                    .map_err(|e| PortError::Failed(e.to_owned()))?;
                admission.check(&request, &contract).await?;
            }
        }
        Ok(())
    }

    /// 创建一个没有注册任何能力的 Broker。
    ///
    /// 空 Broker 对所有执行请求都会 fail-closed；组合根必须显式注册产品允许的能力面。
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_permit_verifier(
        &mut self,
        verifier: Arc<dyn kiana_ports::ExecutionPermitVerifierPort>,
    ) {
        self.permit_verifier = Some(verifier);
    }

    pub fn set_extension_admission(&mut self, admission: Arc<dyn ExtensionAdmission>) {
        self.extension_admission = Some(admission);
    }

    /// 在共享 Broker 已投入使用后，以异步写锁注册一个 Handler。
    ///
    /// 路由键不会做大小写转换、去空白或别名展开。完全相同的键已经存在时返回
    /// [`PortError::Conflict`]，原 Handler 保持不变。
    pub async fn register(
        &self,
        capability: CapabilityKind,
        operation: impl Into<String>,
        handler: Arc<dyn CapabilityHandler>,
    ) -> Result<(), PortError> {
        if self.catalog_sealed {
            return Err(PortError::Failed("capability_catalog_sealed".to_owned()));
        }
        let mut handlers = self.handlers.write().await;
        insert_handler(&mut handlers, capability, operation.into(), handler)
    }

    /// 在组合根尚持有 `&mut self` 时同步注册一个 Handler。
    ///
    /// 该入口避免 daemon 启动装配阶段进入异步锁；它与 [`Self::register`] 使用完全相同
    /// 的重复键规则。调用者拥有独占可变借用，因此不能与执行并发发生。
    pub fn register_static(
        &mut self,
        capability: CapabilityKind,
        operation: impl Into<String>,
        handler: Arc<dyn CapabilityHandler>,
    ) -> Result<(), PortError> {
        if self.catalog_sealed {
            return Err(PortError::Failed("capability_catalog_sealed".to_owned()));
        }
        // 两个公开注册入口共用此函数，确保静态和动态装配不会产生不同的覆盖语义。
        insert_handler(
            self.handlers.get_mut(),
            capability,
            operation.into(),
            handler,
        )
    }

    /// Register an explicitly reviewed host adapter for an installed package. This grants
    /// no new policy permission: callers still arrive through ControlPlane, and manifest
    /// limits are an additional intersection applied inside the broker.
    pub fn register_extension_static(
        &mut self,
        capability: CapabilityKind,
        operation: impl Into<String>,
        handler: Arc<dyn CapabilityHandler>,
        contract: ExtensionExecutionContract,
        admission: Arc<dyn ExtensionAdmission>,
    ) -> Result<(), PortError> {
        self.register_static(
            capability,
            operation,
            Arc::new(ExtensionHandler {
                handler,
                contract,
                admission,
                component: None,
            }),
        )
    }

    /// Register a component adapter without giving it a second execution path. The descriptor is
    /// checked by the existing ExtensionAdmission immediately before the wrapped handler runs.
    pub fn register_extension_component_static(
        &mut self,
        capability: CapabilityKind,
        operation: impl Into<String>,
        handler: Arc<dyn CapabilityHandler>,
        contract: ExtensionExecutionContract,
        admission: Arc<dyn ExtensionAdmission>,
        component: ExtensionAdapterDescriptor,
    ) -> Result<(), PortError> {
        component.validate().map_err(PortError::Failed)?;
        self.register_static(
            capability,
            operation,
            Arc::new(ExtensionHandler {
                handler,
                contract,
                admission,
                component: Some(component),
            }),
        )
    }
}

fn validate_network_observation(
    request: &kiana_domain::CapabilityRequest,
    scope: &kiana_domain::ExecutionScope,
) -> Result<(), PortError> {
    let Some(endpoint) = request
        .arguments
        .get("endpoint")
        .and_then(|value| value.as_str())
    else {
        return Ok(());
    };
    if request.capability != CapabilityKind::Network {
        return Err(PortError::Failed(
            "network_endpoint_on_non_network_action".to_owned(),
        ));
    }
    let addresses = request
        .arguments
        .get("resolved_addresses")
        .and_then(|value| value.as_array())
        .ok_or_else(|| PortError::Failed("network_resolution_observation_required".to_owned()))?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                PortError::Failed("network_resolution_observation_invalid".to_owned())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let policy = kiana_domain::NetworkPolicy::from_scope_hosts(&scope.network_policy)
        .map_err(PortError::Failed)?;
    let observation = policy
        .observe(endpoint, &addresses)
        .map_err(PortError::Failed)?;
    let expected = request
        .arguments
        .get("network_policy_digest")
        .and_then(|value| value.as_str())
        .ok_or_else(|| PortError::Failed("network_policy_digest_required".to_owned()))?;
    if expected != observation.policy_digest {
        return Err(PortError::Failed(
            "network_policy_digest_mismatch".to_owned(),
        ));
    }
    Ok(())
}

/// Revalidate the optional connector reservation envelope immediately before the existing
/// ExecutionPermitVerifier/handler boundary.  The Broker cannot mint or commit a reservation; an
/// uncommitted reservation, stale permit, lease/fence drift or expiry therefore produces zero
/// adapter effect.  Legacy connector requests without the additive envelope continue through the
/// pre-INT-16 compatibility path until ControlPlane emits the reservation event.
pub fn validate_connector_effect_boundary(
    request: &AuthorizedCapabilityRequest,
    now_unix_ms: u64,
) -> Result<(), PortError> {
    if request.request.operation != kiana_domain::CONNECTOR_INVOKE_OPERATION {
        return Ok(());
    }
    let Some(reservation) = request.request.arguments.get("connector_reservation") else {
        return Ok(());
    };
    let permit = request
        .request
        .arguments
        .get("connector_permit")
        .ok_or_else(|| PortError::Conflict("connector_permit_required".to_owned()))?;
    let reservation: kiana_domain::ConnectorInvocationReservation =
        serde_json::from_value(reservation.clone())
            .map_err(|_| PortError::Conflict("connector_reservation_invalid".to_owned()))?;
    let permit: kiana_domain::ConnectorInvocationPermit = serde_json::from_value(permit.clone())
        .map_err(|_| PortError::Conflict("connector_permit_invalid".to_owned()))?;
    kiana_domain::connector_effect_admission(&reservation, &permit, now_unix_ms)
        .map_err(PortError::Conflict)
}

/// Revalidate the optional INT-17 quota envelope at the same effect boundary as the INT-16
/// invocation permit. Quota reservations are server-owned facts; Broker cannot mint a claim,
/// widen a project/account scope or replace a credential generation. Legacy requests without the
/// additive envelope remain readable until ControlPlane emits quota facts for every caller.
pub fn validate_connector_quota_boundary(
    request: &AuthorizedCapabilityRequest,
    now_unix_ms: u64,
) -> Result<(), PortError> {
    if request.request.operation != kiana_domain::CONNECTOR_INVOKE_OPERATION {
        return Ok(());
    }
    let reservation = request.request.arguments.get("connector_quota_reservation");
    let has_claim = request.request.arguments.contains_key("connector_quota_claim");
    let has_policy = request.request.arguments.contains_key("connector_quota_policy");
    if reservation.is_none() {
        if has_claim || has_policy {
            return Err(PortError::Conflict(
                "connector_quota_reservation_required".to_owned(),
            ));
        }
        return Ok(());
    }
    let reservation = reservation.expect("checked above");
    let claim = request
        .request
        .arguments
        .get("connector_quota_claim")
        .ok_or_else(|| PortError::Conflict("connector_quota_claim_required".to_owned()))?;
    let policy = request
        .request
        .arguments
        .get("connector_quota_policy")
        .ok_or_else(|| PortError::Conflict("connector_quota_policy_required".to_owned()))?;
    let reservation: kiana_domain::ConnectorQuotaReservation =
        serde_json::from_value(reservation.clone())
            .map_err(|_| PortError::Conflict("connector_quota_reservation_invalid".to_owned()))?;
    let claim: kiana_domain::ConnectorQuotaClaim = serde_json::from_value(claim.clone())
        .map_err(|_| PortError::Conflict("connector_quota_claim_invalid".to_owned()))?;
    let policy: kiana_domain::ConnectorQuotaPolicy = serde_json::from_value(policy.clone())
        .map_err(|_| PortError::Conflict("connector_quota_policy_invalid".to_owned()))?;
    kiana_domain::connector_quota_effect_admission(&reservation, &claim, &policy, now_unix_ms)
        .map_err(PortError::Conflict)
}

fn connector_now_unix_ms() -> Result<u64, PortError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| PortError::Failed("connector_clock_untrusted".to_owned()))?
        .as_millis()
        .try_into()
        .map_err(|_| PortError::Failed("connector_clock_overflow".to_owned()))
}

/// Validate optional handler-produced BQ-15 usage evidence at the Broker boundary. The handler
/// may report bytes and timing, but it cannot choose a different run, owner, lease, resource or
/// attempt. Rejected/not-started results must carry no effect and cannot be upgraded to success.
fn seal_effect_usage_wall_time(
    mut result: CapabilityResult,
    elapsed: std::time::Duration,
) -> Result<CapabilityResult, PortError> {
    let Some(value) = result.output.get("effect_usage").cloned() else {
        return Ok(result);
    };
    let usage =
        kiana_domain::EffectUsageObservation::from_json(&value).map_err(PortError::Failed)?;
    let elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
    let usage = usage
        .with_server_wall_time_ms(elapsed_ms)
        .map_err(PortError::Failed)?;
    let output = result
        .output
        .as_object_mut()
        .ok_or_else(|| PortError::Failed("effect_usage_output_object_required".to_owned()))?;
    output.insert(
        "effect_usage".to_owned(),
        serde_json::to_value(usage)
            .map_err(|_| PortError::Failed("effect_usage_encode_failed".to_owned()))?,
    );
    Ok(result)
}

fn validate_effect_usage_result(
    request: &AuthorizedCapabilityRequest,
    result: &CapabilityResult,
) -> Result<(), PortError> {
    let Some(value) = result.output.get("effect_usage") else {
        return Ok(());
    };
    if result.request_id != request.request.request_id {
        return Err(PortError::Failed(
            "effect_usage_request_mismatch".to_owned(),
        ));
    }
    let usage =
        kiana_domain::EffectUsageObservation::from_json(value).map_err(PortError::Failed)?;
    let scope = request
        .request
        .execution_scope
        .as_ref()
        .ok_or_else(|| PortError::Failed("effect_usage_scope_required".to_owned()))?;
    let run_id = scope
        .run_id
        .ok_or_else(|| PortError::Failed("effect_usage_run_required".to_owned()))?;
    let lease_digest = request
        .request
        .arguments
        .get("lease_digest")
        .or_else(|| request.request.arguments.get("resource_lease_digest"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| PortError::Failed("effect_usage_lease_required".to_owned()))?;
    let resource_digest = request
        .request
        .arguments
        .get("resource_digest")
        .or_else(|| request.request.arguments.get("path_digest"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| PortError::Failed("effect_usage_resource_required".to_owned()))?;
    let expected_attempt = request
        .request
        .arguments
        .get("attempt")
        .and_then(serde_json::Value::as_u64)
        .and_then(|attempt| u32::try_from(attempt).ok())
        .unwrap_or(1);
    if usage.attempt != expected_attempt {
        return Err(PortError::Failed(
            "effect_usage_attempt_mismatch".to_owned(),
        ));
    }
    usage
        .validate_binding(
            run_id,
            kiana_domain::InvocationId::from_uuid(request.request.request_id.as_uuid()),
            usage.attempt_id,
            expected_attempt,
            &scope.scope_digest,
            lease_digest,
            resource_digest,
        )
        .map_err(PortError::Failed)?;
    let dimensions = result.dimensions();
    if result.success != usage.state.is_success()
        || result.output.get("not_executed") == Some(&serde_json::Value::Bool(true))
            && !usage.state.is_not_started()
        || dimensions.effect == kiana_domain::CapabilityEffectState::NotStarted
            && !usage.state.is_not_started()
    {
        return Err(PortError::Failed(
            "effect_usage_result_state_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn insert_handler(
    handlers: &mut HashMap<HandlerKey, Arc<dyn CapabilityHandler>>,
    capability: CapabilityKind,
    operation: String,
    handler: Arc<dyn CapabilityHandler>,
) -> Result<(), PortError> {
    let descriptor = kiana_domain::capability_action_descriptor(&operation)
        .ok_or_else(|| PortError::Failed("capability_operation_unknown".to_owned()))?;
    if descriptor.operation != operation || descriptor.capability != capability {
        return Err(PortError::Failed(
            "capability_binding_catalog_mismatch".to_owned(),
        ));
    }
    if descriptor.binding_version != handler.binding_version() {
        return Err(PortError::Failed(
            "capability_binding_version_mismatch".to_owned(),
        ));
    }
    let key = (capability, operation);
    // 禁止静默替换：路由目标变化必须在组合代码中显式解决，而不能取决于注册顺序。
    if handlers.contains_key(&key) {
        return Err(PortError::Conflict(
            "capability_handler_already_registered".to_owned(),
        ));
    }
    handlers.insert(key, handler);
    Ok(())
}

#[async_trait]
impl CapabilityBrokerPort for CapabilityBroker {
    async fn execute_cancellable(
        &self,
        request: AuthorizedCapabilityRequest,
        cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        self.validate_action(&request)?;
        self.admit_extensions(&request).await?;
        let key = (
            request.request.capability.clone(),
            request.request.operation.clone(),
        );
        let handler = self
            .handlers
            .read()
            .await
            .get(&key)
            .cloned()
            .ok_or_else(|| {
                PortError::Unavailable(format!("capability_unregistered:{:?}:{}", key.0, key.1))
            })?;
        if *cancellation.borrow() {
            return Err(PortError::Failed("cancelled:before_broker".to_owned()));
        }
        validate_connector_effect_boundary(&request, connector_now_unix_ms()?)?;
        validate_connector_quota_boundary(&request, connector_now_unix_ms()?)?;
        self.permit_verifier
            .as_ref()
            .ok_or_else(|| PortError::Unavailable("execution_permit_verifier_required".to_owned()))?
            .verify_and_consume(&request)
            .await?;
        let started_at = Instant::now();
        let result = handler
            .execute_cancellable(request.clone(), cancellation)
            .await?;
        let result = seal_effect_usage_wall_time(result, started_at.elapsed())?;
        validate_effect_usage_result(&request, &result)?;
        Ok(result)
    }
    /// 按能力种类和操作名精确路由并执行请求。
    ///
    /// 未注册键返回 [`PortError::Unavailable`]，不会尝试相近名称或更宽泛 Handler。查找
    /// 完成后先克隆 [`Arc`] 再释放读锁，所以 Handler 的异步执行不会占用注册表锁。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.validate_action(&request)?;
        self.admit_extensions(&request).await?;
        let key = (
            request.request.capability.clone(),
            request.request.operation.clone(),
        );
        // 只在 HashMap 查找期间持锁；不能把外部 I/O 的 await 放在锁保护区内。
        let handler = self.handlers.read().await.get(&key).cloned();
        let Some(handler) = handler else {
            return Err(PortError::Unavailable(format!(
                "capability_unregistered:{:?}:{}",
                key.0, key.1
            )));
        };
        validate_connector_effect_boundary(&request, connector_now_unix_ms()?)?;
        validate_connector_quota_boundary(&request, connector_now_unix_ms()?)?;
        self.permit_verifier
            .as_ref()
            .ok_or_else(|| PortError::Unavailable("execution_permit_verifier_required".to_owned()))?
            .verify_and_consume(&request)
            .await?;
        let started_at = Instant::now();
        let result = handler.execute(request.clone()).await?;
        let result = seal_effect_usage_wall_time(result, started_at.elapsed())?;
        validate_effect_usage_result(&request, &result)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{CapabilityRequest, RequestId};
    use serde_json::Value;

    struct NoopHandler;

    #[async_trait]
    impl CapabilityHandler for NoopHandler {
        async fn execute(
            &self,
            request: AuthorizedCapabilityRequest,
        ) -> Result<CapabilityResult, PortError> {
            Ok(CapabilityResult::success(
                request.request.request_id,
                Value::Null,
            ))
        }
    }

    #[tokio::test]
    async fn unregistered_capability_fails_closed() {
        let broker = CapabilityBroker::new();
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "search",
            Value::Null,
        );
        let request = AuthorizedCapabilityRequest::new("policy:test", request).unwrap();
        assert!(matches!(
            broker.execute(request).await,
            Err(PortError::Unavailable(_))
        ));
    }

    #[test]
    fn static_registration_rejects_duplicate_handler_keys() {
        let mut broker = CapabilityBroker::new();
        broker
            .register_static(CapabilityKind::Query, "search", Arc::new(NoopHandler))
            .unwrap();
        assert_eq!(
            broker
                .register_static(CapabilityKind::Query, "search", Arc::new(NoopHandler))
                .unwrap_err(),
            PortError::Conflict("capability_handler_already_registered".to_owned())
        );
    }
}
