//! CAP-01 的 Capability Broker 注册边界测试。
//!
//! 此处直接在内存中创建 Broker 和测试 handler，验证 operation 注册键唯一、注册键与
//! Domain action descriptor 一致、handler binding version 匹配，以及未注册 operation 不会
//! 自动回退到 shell。它不经过 DaemonHost/ControlPlane 的完整授权流程，也不执行真实外部副作用。
//! 生产接线可继续查看 `kiana-capability-broker/src/lib.rs` 的注册、catalog 校验和 dispatch。
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, RequestId};
use kiana_ports::{CapabilityBrokerPort, PortError};
use serde_json::Value;
use std::sync::Arc;

/// 注册测试使用的最小 handler；执行时原样回传 request id 和空结果。
///
/// `async_trait` 宏把 trait 中的异步方法转换为 Rust trait object 可调用的形式；`Arc` 则让
/// Broker 能共享 handler 实例。本用例主要测试注册，通常不会调用这个 handler 的 `execute()`。
struct NoopHandler;

// 通过 `CapabilityHandler` trait 把测试对象接入 Broker 的 handler 注册接口。
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

/// binding 版本固定返回旧版本字符串，用来模拟 handler 与权威 descriptor 发布版本不一致。
struct MismatchedHandler;

// execute 与 NoopHandler 一样只是满足 trait 合同；拒绝应发生在注册校验阶段。
#[async_trait]
impl CapabilityHandler for MismatchedHandler {
    fn binding_version(&self) -> &'static str {
        "kiana.handler-binding.f0"
    }

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

#[test]
/// 同一个 operation 不能重复注册，工具 alias 也不能冒充 canonical operation 注册键。
///
/// 先成功注册 `shell.exec`，再确认第二次注册得到冲突；随后用 Domain descriptor 证明 `shell`
/// 是指向 `shell.exec` 的模型工具 alias，并确认 Broker 拒绝以 alias 作为静态 handler operation。
/// 这避免注册表出现两个名字指向同一执行动作、却难以审计实际 handler 归属的情况。
fn duplicate_alias_or_operation_is_rejected() {
    let mut broker = CapabilityBroker::new();
    broker
        .register_static(CapabilityKind::Process, "shell.exec", Arc::new(NoopHandler))
        .unwrap();
    assert_eq!(
        broker
            .register_static(CapabilityKind::Process, "shell.exec", Arc::new(NoopHandler),)
            .unwrap_err(),
        PortError::Conflict("capability_handler_already_registered".to_owned())
    );
    let shell_alias = kiana_domain::capability_action_descriptor("shell").unwrap();
    assert_eq!(shell_alias.operation, "shell.exec");
    assert_eq!(
        broker
            .register_static(CapabilityKind::Process, "shell", Arc::new(NoopHandler))
            .unwrap_err(),
        PortError::Failed("capability_binding_catalog_mismatch".to_owned())
    );
}

#[test]
/// handler 声明的 binding version 与当前动作目录不一致时，注册必须失败。
///
/// 此测试在 dispatch 前调用 `register_static()` 并断言稳定错误码；不会调用
/// `MismatchedHandler::execute()`，也不会将版本不符降级为普通可运行 handler。
fn descriptor_binding_version_mismatch_never_dispatches() {
    let mut broker = CapabilityBroker::new();
    assert_eq!(
        broker
            .register_static(
                CapabilityKind::Process,
                "shell.exec",
                Arc::new(MismatchedHandler),
            )
            .unwrap_err(),
        PortError::Failed("capability_binding_version_mismatch".to_owned())
    );
}

#[tokio::test]
/// 未注册的 operation 必须返回 unavailable，不能把请求错误地交给 shell handler。
///
/// fixture 构造带 policy id 的 `AuthorizedCapabilityRequest`，但直接调用的是空 Broker，
/// 用来隔离测试 Broker 的 handler 查找/拒绝边界；它不证明该请求通过了 ControlPlane 授权。
/// async 测试运行时提供 `.await` 所需的 Tokio 执行环境，测试中的 operation 是 `process.poll`，
/// 且 request arguments 的 `process_id` 只是合成值，不会操作真实进程。
async fn unregistered_operation_does_not_fallback_to_shell() {
    let broker = CapabilityBroker::new();
    let request = AuthorizedCapabilityRequest::new(
        "policy:cap01",
        kiana_domain::CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Process,
            "process.poll",
            serde_json::json!({"process_id":"p"}),
        ),
    )
    .unwrap();
    assert!(matches!(
        broker.execute(request).await,
        Err(PortError::Unavailable(_))
    ));
}
