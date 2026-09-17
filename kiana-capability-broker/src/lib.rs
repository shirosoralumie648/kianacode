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
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, CredentialLease,
    ExtensionExecutionContract,
};
use kiana_ports::{CapabilityBrokerPort, PortError};
use std::collections::HashMap;
use std::sync::Arc;
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
}

struct ExtensionHandler {
    handler: Arc<dyn CapabilityHandler>,
    contract: ExtensionExecutionContract,
    admission: Arc<dyn ExtensionAdmission>,
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
            }),
        )
    }
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
        self.permit_verifier
            .as_ref()
            .ok_or_else(|| PortError::Unavailable("execution_permit_verifier_required".to_owned()))?
            .verify_and_consume(&request)
            .await?;
        handler.execute_cancellable(request, cancellation).await
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
        self.permit_verifier
            .as_ref()
            .ok_or_else(|| PortError::Unavailable("execution_permit_verifier_required".to_owned()))?
            .verify_and_consume(&request)
            .await?;
        handler.execute(request).await
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
