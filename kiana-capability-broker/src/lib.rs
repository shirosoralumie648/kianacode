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
/// 路由键：能力类型 + 操作名的组合。
///
/// 【为什么用元组而不是字符串拼接】
/// 用 `"shell:run"` 这样的字符串当键会有歧义 ——
/// 如果能力名或操作名里含有冒号，不同的组合可能拼出同一个键。
///
/// 元组在类型层面就杜绝了这种歧义：两个不同的 `(能力, 操作)` 组合
/// 永远不相等，不需要任何转义规则。
///
/// resolves or returns the raw value; provider/connector adapters keep that operation private to
/// their final request boundary and may pass only this consumed lease's digest to receipts.
/// 在产生副作用的**最后一刻**重新校验并消费凭据租约。
///
/// 【作用】
/// 凭据租约不是"取一次就一直有效"的。
/// 每次真正要用凭据之前，都必须重新确认它还没被撤销、还没过期、
/// 还没被别处消费掉。
///
/// 【⚠ 为什么是"最后一刻"而不是"提前准备"】
/// 因为凭据状态可能在这期间改变：
/// - 用户可能在执行过程中撤销了授权；
/// - 租约可能在这期间过期；
/// - 另一次并发操作可能已经消费了同一个租约。
///
/// 如果提前很久就准备好，实际使用时的状态可能已经变了。
/// **越接近使用点校验越准确。**
///
/// 【⚠ Broker 绝不解析或返回凭据原文】
/// 原始注释特别强调这一点："The broker never resolves or returns the raw value"。
///
/// Broker 只处理**元数据**（租约状态、有效期、消费记录）。
/// 真正的密钥由 provider/connector 适配器在它们的最终请求边界内私有地解析。
///
/// 这样做的好处是：Broker 的代码永远不可能泄漏密钥 ——
/// 因为它根本拿不到。
///
/// 【⚹ 适配器只能传递租约摘要】
/// 适配器在写回执（receipt）时，最多只能传递"已消费的这份租约的摘要"，
/// 不能传递租约本身，更不能传递密钥。
///
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
/// 连接器凭据调用的同类消费接口。
///
/// 【作用】
/// [`consume_credential_lease`] 针对普通能力请求的版本；
/// 这个是针对**连接器调用**的版本。
///
/// 【为什么分成两个】
/// 连接器调用涉及外部服务，需要额外校验一些东西
/// （比如连接器配额、绑定一致性）。
/// 共用一个函数会让参数列表膨胀，而且两种场景的校验项确实不同。
///
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
/// 能力处理器接口。
///
/// 【作用】
/// 一个适配器实现"怎么真正执行某类能力"的接口。
///
/// 【⚠ 这是执行链的终点】
/// Broker 经过一长串检查之后，最终会落到 `execute_cancellable`。
/// 也就是说，**这是整条授权链上最后一个会真正碰到外部世界的环节**。
///
/// 前面所有的策略检查、Gate 检查、许可校验，
/// 都是为了保证"能走到这里的东西是合法的"。
///
pub trait CapabilityHandler: Send + Sync {
    /// 处理器绑定到哪个版本的契约。
    ///
    /// 【作用】
    /// 让 Broker 能确认"这个处理器实现的是哪一版契约"。
    ///
    /// 【⚠ 版本不匹配会导致拒绝】
    /// 如果处理器实现的契约版本与当前期望的不一致，
    /// Broker 会拒绝调用它，而不是"凑合着用"。
    ///
    /// 这防的是：新版接口加了字段或改了语义，
    /// 旧处理器不知道这些变化，凑合执行可能产生错误结果。
    ///
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
/// 扩展准入接口。
///
/// 【作用】
/// 让**插件**（extension）声明它能被哪些能力调用。
///
/// 【⚠ 为什么要单独一个接口】
/// 静态注册的能力（编译期就在代码里的）在注册时就已经确定了权限。
/// 但扩展是动态加载的 —— 它们在运行时才出现。
///
/// 扩展带来的能力必须**再次过审**，不能因为"它已经加载了"就认为可信。
///
/// 【典型实现】
/// 由 `kiana-daemon` 之类的上层提供，判断某个扩展是否有权处理某类请求。
///
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
/// 把扩展包装成标准能力处理器。
///
/// 【作用】
/// 适配器模式。`ExtensionHandler` 让扩展的调用路径
/// 和静态注册的能力走同一套 Broker 逻辑，
/// 避免在 Broker 里写两套分发。
///
/// 【⚠ 但准入检查不能省】
/// 即使走的路径统一了，扩展的准入检查依然要做 ——
/// 这正是上面那个 [`ExtensionAdmission`] 接口存在的意义。
///
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
/// 能力代理 —— 本 crate 的核心类型。
///
/// 【作用】
/// 把"已授权的请求"路由到"能执行它的适配器"，并管理适配器注册表。
///
/// 【⚠ 它是纯路由层，不是授权层】
/// 它**接受**已授权的请求，但**不签发**授权。
/// 授权在上游的 `kiana-core` 完成。
///
/// 这个区分很重要：Broker 不参与"能不能做"的判断，
/// 只负责"找到做这件事的东西并调用它"。
///
pub struct CapabilityBroker {
    /// 以能力种类和精确操作名为键的 Handler 集合。
    handlers: RwLock<HashMap<HandlerKey, Arc<dyn CapabilityHandler>>>,
    extension_admission: Option<Arc<dyn ExtensionAdmission>>,
    permit_verifier: Option<Arc<dyn kiana_ports::ExecutionPermitVerifierPort>>,
    catalog_sealed: bool,
}

impl CapabilityBroker {
    /// Called once by DaemonHost after all static registrations and before any model request.
    /// 校验所有已注册处理器的绑定是否一致。
    ///
    /// 【作用】
    /// 在开始服务之前，一次性检查所有适配器的绑定声明是否有效。
    ///
    /// 【为什么提前一次性检查】
    /// 如果在每次调用时才检查某个处理器，一个配置错误
    /// 可能要等到某个特定请求进来才会暴露。
    /// 提前检查让问题在启动时就暴露。
    ///
    /// 【⚠ 检查是 `&mut self`】
    /// 因为这个方法可能会**修改**内部状态（比如标记无效的处理器）。
    /// 一个只读检查不需要可变借用。
    ///
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

    /// 校验一次已授权请求的形状是否正确。
    ///
    /// 【作用】
    /// 在任何路由发生之前，确认请求本身结构完整。
    ///
    /// 【⚠ 这一步在路由之前】
    /// 如果请求连基本结构都不对（比如缺少必要字段），
    /// 就不应该浪费一次查找去路由它 ——
    /// 先确认"这是一个形状正确的请求"。
    ///
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
    /// 创建一个空的 Broker。
    ///
    /// 【作用】
    /// 构造一个还没有注册任何处理器的 Broker。
    ///
    /// 【⚠ 空的 Broker 什么都做不了】
    /// 没有处理器时，任何请求都会被拒绝。
    /// 这是安全的默认值 —— 未配置 = 不可用。
    ///
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置许可验证器。
    ///
    /// 【作用】
    /// 注入一个"谁来验证单次执行许可"的实现。
    ///
    /// 【为什么是注入而不是内部实现】
    /// 验证许可需要访问 journal（事件账本），
    /// 而 Broker 不应该直接持有账本访问权。
    /// 通过注入，账本访问被限制在验证器内部。
    ///
    pub fn set_permit_verifier(
        &mut self,
        verifier: Arc<dyn kiana_ports::ExecutionPermitVerifierPort>,
    ) {
        self.permit_verifier = Some(verifier);
    }

    /// 设置扩展准入器。
    ///
    /// 【作用】
    /// 注入一个"判断扩展是否有权处理某类请求"的实现。
    ///
    /// 【⚠ 必须在开始服务前设置好】
    /// 和许可验证器一样，扩展准入必须在 Broker 开始工作前配置完成。
    /// 事后再设置会留下一个"准入检查为空"的窗口期。
    ///
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
    /// 注册一个静态能力处理器。
    ///
    /// 【作用】
    /// 把一个处理器加入路由表。
    ///
    /// 【⚠ 重复注册会失败】
    /// 同一个 `(能力, 操作)` 只能有一个处理器。
    /// 如果两个处理器都能处理同一个请求，路由结果就变得不确定 ——
    /// 那是个安全问题，不只是设计问题。
    ///
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
    /// 注册一个扩展提供的静态处理器。
    ///
    /// 【作用】
    /// 和 [`CapabilityBroker::register_static`] 类似，
    /// 但标记这个处理器来自扩展。
    ///
    /// 【为什么扩展要单独注册】
    /// 因为扩展的处理器需要在调用前额外过扩展准入检查。
    /// 分成两个注册入口，可以让 Broker 在内部就知道
    /// "这个处理器来自扩展"，从而在路由时自动加上那层检查。
    ///
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
    /// 注册一个扩展组件提供的处理器。
    ///
    /// 【作用】
    /// 比 [`CapabilityBroker::register_extension_static`] 更具体的注册入口 ——
    /// 注册的是"扩展组件"而非整个扩展。
    ///
    /// 【具体差别在于准入检查的粒度：
    /// 扩展整体准入 vs 单个组件准入。
    /// 后者允许"一个扩展里只有部分组件被允许调用"。
    ///
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

/// 校验一次网络观测记录。
///
/// 【作用】
/// 确认"这次调用确实产生了预期的网络流量"。
///
/// 【⚠ 为什么需要它】
/// 因为"声称调用了网络"和"真的调用了网络"是两回事。
/// 一个适配器可能声称它访问了外部服务，实际却在本地返回了缓存数据。
///
/// 这个校验把观测记录和预期比对，不符就拒绝 ——
/// 防止适配器谎报网络访问。
///
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
/// 连接器效果边界（effect boundary）校验。
///
/// 【作用 —— Broker 最核心的检查之一】
/// 确认"这次连接器调用的实际效果"符合连接器契约的声明。
///
/// 【⚠ 什么是"效果边界"】
/// 它划定了一条线：
/// 在这条线**之内**的副作用是允许的（按契约声明），
/// 在**之外**的副作用是越界的。
///
/// 比如一个声明为"只读查询"的连接器操作，
/// 如果它实际上修改了远端数据，就越界了。
///
/// 【为什么这道检查不可省略】
/// 因为适配器是外部代码，它可能：
/// - 声明只读，实际却写数据；
/// - 声明访问 A 服务端，实际却访问了 B；
/// - 因为上游服务变更而产生了预期外的影响。
///
/// 没有这道检查，声明就只是一句空话。
///
pub fn validate_connector_effect_boundary(
    request: &AuthorizedCapabilityRequest,
    now_unix_ms: u64,
) -> Result<(), PortError> {
    if request.request.operation != kiana_domain::CONNECTOR_INVOKE_OPERATION {
        return Ok(());
    }
    let Some(reservation_value) = request.request.arguments.get("connector_reservation") else {
        return Ok(());
    };
    let permit_value = request
        .request
        .arguments
        .get("connector_permit")
        .ok_or_else(|| PortError::Conflict("connector_permit_required".to_owned()))?;
    let reservation: kiana_domain::ConnectorInvocationReservation =
        serde_json::from_value(reservation_value.clone())
            .map_err(|_| PortError::Conflict("connector_reservation_invalid".to_owned()))?;
    let permit: kiana_domain::ConnectorInvocationPermit =
        serde_json::from_value(permit_value.clone())
            .map_err(|_| PortError::Conflict("connector_permit_invalid".to_owned()))?;
    kiana_domain::connector_effect_admission(&reservation, &permit, now_unix_ms)
        .map_err(PortError::Conflict)?;

    // INT-18 is the final server-owned check. A reservation/legacy permit can never substitute
    // for an effect permit: the latter binds scope, command/payload digests, epochs and expiry.
    let effect_permit_value = request
        .request
        .arguments
        .get("connector_effect_permit")
        .ok_or_else(|| PortError::Conflict("connector_effect_permit_required".to_owned()))?;
    let effect_permit: kiana_domain::ConnectorEffectPermit =
        serde_json::from_value(effect_permit_value.clone())
            .map_err(|_| PortError::Conflict("connector_effect_permit_invalid".to_owned()))?;
    let binding_value = request
        .request
        .arguments
        .get("binding_snapshot")
        .ok_or_else(|| PortError::Conflict("connector_binding_snapshot_required".to_owned()))?;
    let binding: kiana_domain::ConnectorBindingSnapshot =
        serde_json::from_value(binding_value.clone())
            .map_err(|_| PortError::Conflict("connector_binding_snapshot_invalid".to_owned()))?;
    let scope = request
        .request
        .execution_scope
        .as_ref()
        .ok_or_else(|| PortError::Conflict("connector_effect_scope_required".to_owned()))?;
    let configuration_epoch = request
        .request
        .arguments
        .get("connector_configuration_epoch")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| PortError::Conflict("connector_configuration_epoch_required".to_owned()))?;
    let policy_epoch = request
        .request
        .arguments
        .get("connector_policy_epoch")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| PortError::Conflict("connector_policy_epoch_required".to_owned()))?;
    let credential_epoch = request
        .request
        .arguments
        .get("connector_credential_epoch")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| {
            binding
                .binding
                .credential_ref
                .as_ref()
                .map_or(0, |reference| reference.generation)
        });
    let current_scope_digest =
        kiana_domain::connector_effect_scope_digest(&binding, &effect_permit.operation)
            .map_err(PortError::Conflict)?;
    let current = kiana_domain::ConnectorEffectFence::new(
        current_scope_digest,
        scope.authority_epoch,
        configuration_epoch,
        policy_epoch,
        credential_epoch,
        scope.data_epoch,
    )
    .map_err(PortError::Conflict)?;
    effect_permit
        .validate_for_effect(&reservation, &binding, &current, now_unix_ms)
        .map_err(PortError::Conflict)
}

/// Revalidate the optional INT-17 quota envelope at the same effect boundary as the INT-16
/// invocation permit. Quota reservations are server-owned facts; Broker cannot mint a claim,
/// widen a project/account scope or replace a credential generation. Legacy requests without the
/// additive envelope remain readable until ControlPlane emits quota facts for every caller.
/// 连接器配额边界校验。
///
/// 【作用】
/// 确认这次调用没有超过配额上限。
///
/// 【⚠ 为什么配额要在这里检查**
/// 因为这是**副作用即将发生前**的最后一个检查点。
/// 等副作用发生完再检查就晚了 —— 超额消耗已经产生了。
///
/// 【配额的类型】
/// 通常包括速率限制（每秒最多 N 次）和总量限制（每天最多 M 次）。
/// 两种都要在生效前拦住。
///
pub fn validate_connector_quota_boundary(
    request: &AuthorizedCapabilityRequest,
    now_unix_ms: u64,
) -> Result<(), PortError> {
    if request.request.operation != kiana_domain::CONNECTOR_INVOKE_OPERATION {
        return Ok(());
    }
    let reservation = request.request.arguments.get("connector_quota_reservation");
    let has_claim = request
        .request
        .arguments
        .get("connector_quota_claim")
        .is_some();
    let has_policy = request
        .request
        .arguments
        .get("connector_quota_policy")
        .is_some();
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

/// 取当前时间戳（毫秒）。
///
/// 【作用】
/// 给需要时间戳的校验提供统一入口。
///
/// 【⚠ 为什么是 `Result` 而不是直接返回】
/// 因为取系统时间可能失败（比如系统时钟在 1970 年之前）。
/// 虽然极罕见，但它确实可能发生。
///
/// 返回一个 `Result` 让调用方必须处理这个情况，
/// 而不是假设"时间一定能取到"。
///
/// 【为什么不用 `Instant`】
/// `Instant` 是单调递增的，适合测耗时；
/// 但有效期判断需要的是**日历时间**（能被人类理解的时间点），
/// 所以这里用 `SystemTime`。
///
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
/// 用 Broker 测得的耗时覆盖适配器上报的耗时。
///
/// 【作用 —— 防止适配器谎报执行时长】
/// 适配器在回执里会报告"我执行了多久"。
/// 但这个数字是**适配器自己说的**，不可信 ——
/// 它可能为了让某项指标好看而报一个很小的数。
///
/// Broker 用自己测量的 `Instant` 差值**覆盖**那个数字。
///
/// 【为什么必须覆盖而不是核对】
/// 如果只是核对（发现不一致就报错），那么一个老实上报的适配器
/// 在系统繁忙时可能因为调度延迟而报出偏大的耗时，从而被误判。
///
/// 覆盖掉是最简洁的做法：**信任自己测的，不信别人报的。**
///
/// 【⚠ 覆盖掉意味着适配器上报的耗时被完全忽略】
/// 这是有意的。Broker 的测量更接近真实执行时间。
///
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

/// 校验效果用量结果。
///
/// 【作用】
/// 把用量证据重新绑定回服务端持有的事实。
///
/// 【⚠ "重新绑定"是关键**
/// 用量数据（用了多少 token、调了多少次、花了多少钱）
/// 最初是由适配器上报的 —— 那不可信。
///
/// 这个函数把用量数据**重新锚定**到 Broker 已知的事实上：
/// 调用哪个能力、哪个操作、哪个绑定、哪个账号。
///
/// 一份与这些事实对不上的用量记录，就是伪造的。
///
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

/// 把一个处理器插入路由表。
///
/// 【作用】
/// 内部辅助函数，完成实际的插入。
///
/// 【为什么抽成独立函数】
/// 三个注册入口（静态、扩展静态、扩展组件）
/// 都要做插入，但要求的前置检查不同。
/// 抽出插入逻辑，保证它们在"插入"这一步行为完全一致。
///
/// 【⚠ 重复键在这里被拒绝】
/// 插入前检查键是否已存在。
/// 这是路由确定性的保证。
///
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
/// Broker 对外的端口实现。
///
/// 【作用】
/// 实现 `kiana_ports::CapabilityBrokerPort`，
/// 让控制面能通过统一的端口接口调用 Broker。
///
/// 【⚠ 这里是执行链的最终环节】
/// 从控制面到这里，要穿过：
/// 策略 → Gate → 二次 Gate 校验 → 许可签发 → Broker 的九道内部检查
/// → 适配器执行 → 用量证据重绑。
///
/// 这一长串就是架构简报里说的
/// "把执行权交出去之前的所有服务端事实都钉死"。
///
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
    /// 测试用：什么都不做的处理器。
    ///
    /// 【作用】
    /// 用于测试"处理器被正确调用"这类场景 ——
    /// 用一个可预测的空实现，避免真的去执行副作用。
    ///
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
    /// 测试：静态注册时拒绝重复的处理器键。
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
