//! 部署进程监督的窄 adapter 选择边界，属于 `kiana-daemon`。
//!
//! 【预期位置与数据流】
//! ```text
//! 部署协调器构造 SupervisorRequest
//!     -> SupervisorPort::{start,stop,restart}
//!     -> 本文件检查请求和 backend/action 绑定
//!     -> 未来的平台实现返回 SupervisorObservation
//!     -> Core 校验 observation 是否对应原请求
//! ```
//!
//! 当前实现只校验请求并返回 `Unavailable`，没有 systemd、launchd、Windows Service、容器或 fake 运行时。
//! 因此 `SUPPORTED_SUPERVISOR_BACKENDS` 表示协议认识这些 backend 名称，不表示它们已安装、被探测或可以执行。
//! 此处没有 shell、模型、Capability Broker 或 caller PID 后备路径；有效请求也不会触发真实启停操作。

use async_trait::async_trait;
use kiana_domain::{SupervisorAction, SupervisorBackend, SupervisorObservation, SupervisorRequest};
use kiana_ports::{PortError, SupervisorPort};

/// 请求合同中可识别的 backend 枚举集合；当前列表用于选择/兼容检查，不代表对应 adapter 可用。
pub const SUPPORTED_SUPERVISOR_BACKENDS: &[SupervisorBackend] = &[
    SupervisorBackend::Systemd,
    SupervisorBackend::Launchd,
    SupervisorBackend::WindowsService,
    SupervisorBackend::Container,
    SupervisorBackend::Fake,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// 绑定一种 backend 的窄监督 adapter。
///
/// 该值只保存选择结果，不持有服务句柄、lease、锁或平台客户端。当前创建它也不会探测 backend；三个端口方法
/// 对校验通过的请求都会返回 `PortError::Unavailable`。
pub struct NarrowSupervisorAdapter {
    /// 本 adapter 接受的 backend；请求中的 backend 必须与它完全一致。
    backend: SupervisorBackend,
}

impl NarrowSupervisorAdapter {
    /// 创建一个只绑定 backend 名称的 adapter 值。
    ///
    /// 【调用者】预期由 DaemonHost 的组合逻辑为选定平台构造；当前未发现生产构造调用。
    /// 【输入/输出】输入 backend enum，返回保存该值的轻量对象。
    /// 【边界】不会探测系统服务、验证配置或检查 `SUPPORTED_SUPERVISOR_BACKENDS` 中的运行时能力。
    pub const fn new(backend: SupervisorBackend) -> Self {
        Self { backend }
    }

    /// 读取该对象绑定的 backend，供装配或诊断代码比较选择结果。
    pub const fn backend(&self) -> SupervisorBackend {
        self.backend
    }

    /// 校验端口请求的基本合同后，以稳定原因报告当前没有可执行实现。
    ///
    /// 【输入】由 `SupervisorPort` 方法取得的 request，以及该方法期望的 action。
    /// 【核心顺序】先调用 Domain request 校验；再比较请求 backend 与本 adapter；最后比较 action。
    /// 因此畸形请求返回 `Failed`，路由到错误 adapter 或调用错误动作返回 `Conflict`，只有合同有效且绑定一致时
    /// 才返回 `Unavailable`。
    /// 【失败与副作用】无效/冲突原因用稳定字符串表达；有效请求当前必定得到 backend-specific `Unavailable`。
    /// 本方法不重试、不记录日志、不取 lease、不查服务目录、不发送信号，也不构造成功 observation。
    fn unavailable(
        &self,
        request: SupervisorRequest,
        expected_action: SupervisorAction,
    ) -> Result<SupervisorObservation, PortError> {
        request.validate().map_err(PortError::Failed).and_then(|_| {
            if request.backend != self.backend {
                return Err(PortError::Conflict(
                    "supervisor_backend_binding_mismatch".to_owned(),
                ));
            }
            if request.action != expected_action {
                return Err(PortError::Conflict(
                    "supervisor_action_binding_mismatch".to_owned(),
                ));
            }
            Err(PortError::Unavailable(format!(
                "supervisor_{}_adapter_unavailable",
                self.backend.adapter_name()
            )))
        })
    }
}

#[async_trait]
// `async_trait` 将 trait 的 async 方法转换为兼容 dyn-dispatch 的返回形式；这里保留端口接口约定，
// 但当前实现不等待 I/O，验证后立即返回。将来接入平台调用时，取消与 deadline 仍需由真实实现负责。
impl SupervisorPort for NarrowSupervisorAdapter {
    /// 通过统一端口请求启动；当前只验证 backend/action 绑定后返回 Unavailable。
    async fn start(&self, request: SupervisorRequest) -> Result<SupervisorObservation, PortError> {
        self.unavailable(request, SupervisorAction::Start)
    }

    /// 通过统一端口请求停止；当前不会发送 TERM/KILL，也不会生成停止回执。
    async fn stop(&self, request: SupervisorRequest) -> Result<SupervisorObservation, PortError> {
        self.unavailable(request, SupervisorAction::Stop)
    }

    /// 通过统一端口请求重启；当前不会先停后启，也不会改变服务代次。
    async fn restart(
        &self,
        request: SupervisorRequest,
    ) -> Result<SupervisorObservation, PortError> {
        self.unavailable(request, SupervisorAction::Restart)
    }
}
