//! 查询循环使用的窄范围依赖注入容器。
//!
//! `QueryDeps` 只抽象当前需要替换的随机 UUID 生成器；它不是通用服务定位器，也不拥有
//! 文件、网络、模型或权限执行权。测试可以注入确定性闭包，生产组合根则显式选择真实
//! 实现，从而让纯 reducer 的测试不依赖进程全局随机状态。

use std::sync::Arc;
use uuid::Uuid;

/// 查询循环当前抽象出的可替换函数。
///
/// 字段用 `Arc` 包装，便于在多个轮次间廉价克隆；闭包必须是 `Send + Sync`，以满足可能由
/// Tokio 任务持有的生命周期。当前范围刻意保持很小，避免把所有 I/O 重新藏进一个可变
/// 全局；该函数只生成标识符，不应承担授权或副作用。
pub struct QueryDeps {
    /// 生成新的 UUID，例如工具调用 ID。
    pub uuid: Arc<dyn Fn() -> Uuid + Send + Sync>,
}

impl QueryDeps {
    /// 创建连接到生产 UUID 生成器的依赖容器。
    pub fn production() -> Self {
        QueryDeps {
            uuid: Arc::new(Uuid::new_v4),
        }
    }

    /// 通过注入的工厂生成一个 UUID。
    ///
    /// 调用方不应把 UUID 生成器当作事实存储；它只提供新标识，是否已被事件账本接受要由
    /// 对应的持久化流程证明。
    pub fn new_uuid(&self) -> Uuid {
        (self.uuid)()
    }
}

impl Default for QueryDeps {
    fn default() -> Self {
        Self::production()
    }
}

impl std::fmt::Debug for QueryDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryDeps").finish_non_exhaustive()
    }
}
