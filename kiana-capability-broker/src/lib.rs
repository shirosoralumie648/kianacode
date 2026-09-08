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
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult};
use kiana_ports::{CapabilityBrokerPort, PortError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

type HandlerKey = (CapabilityKind, String);

#[async_trait]
/// 单个能力操作的受控执行适配器。
///
/// 每个实现只应处理注册时声明的能力种类和精确操作名，并在请求携带的 sandbox、路径
/// 与参数边界内工作。Handler 不接收原始模型调用，因此不能自行绕过控制面创建或扩展
/// [`AuthorizedCapabilityRequest`]。
pub trait CapabilityHandler: Send + Sync {
    /// 执行一项已授权请求并返回与原请求 ID 对应的结构化结果。
    ///
    /// 端口/传输层故障使用 [`PortError`]；能力执行成功或业务失败由
    /// [`CapabilityResult`] 表达。实现不得在结果不可判定时伪造成功证据。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError>;
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
}

impl CapabilityBroker {
    /// 创建一个没有注册任何能力的 Broker。
    ///
    /// 空 Broker 对所有执行请求都会 fail-closed；组合根必须显式注册产品允许的能力面。
    pub fn new() -> Self {
        Self::default()
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
        // 两个公开注册入口共用此函数，确保静态和动态装配不会产生不同的覆盖语义。
        insert_handler(
            self.handlers.get_mut(),
            capability,
            operation.into(),
            handler,
        )
    }
}

fn insert_handler(
    handlers: &mut HashMap<HandlerKey, Arc<dyn CapabilityHandler>>,
    capability: CapabilityKind,
    operation: String,
    handler: Arc<dyn CapabilityHandler>,
) -> Result<(), PortError> {
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
    /// 按能力种类和操作名精确路由并执行请求。
    ///
    /// 未注册键返回 [`PortError::Unavailable`]，不会尝试相近名称或更宽泛 Handler。查找
    /// 完成后先克隆 [`Arc`] 再释放读锁，所以 Handler 的异步执行不会占用注册表锁。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
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
