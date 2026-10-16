//! Query 状态流程可注入的窄范围依赖。
//!
//! 【文件职责与系统位置】
//! 本文件把 UUID 生成函数包装为 `QueryDeps`，让调用方可以明确选择默认随机实现或注入一个
//! 可重复的实现。预期数据流是组合代码创建依赖集合，查询逻辑调用 `new_uuid()`，生成的 ID
//! 再由具体事件/命令携带；ID 是否被事实系统接受由后续流程决定。
//!
//! 仓内调用核对：`QueryDeps` 目前只有定义和 `kiana-query` 根重导出，没有找到生产代码或
//! 测试消费者。因此这里描述的是公开 API 的能力，不代表 UUID 注入已经接入查询主路径。
//! 本容器不是通用服务定位器，不持有文件、网络、模型或授权执行能力。
//!
//! 【设计原因】
//! 若状态逻辑直接从全局随机源取 ID，测试就难以控制输入，也不容易重复重放同一场景。
//! 把唯一当前依赖显式放在这里，调用边界更清楚；未来若增加其他依赖，也应逐项说明来源和
//! 能力边界，而不是把任意 I/O 隐藏到一个万能对象中。

use std::sync::Arc;
use uuid::Uuid;

/// 查询依赖集合，目前只包含 UUID 工厂。
///
/// `Arc`（原子引用计数指针）允许多个所有者共享同一个闭包；克隆 `Arc` 只增加引用计数，
/// 不复制闭包捕获的数据。外层 `QueryDeps` 本身没有实现 `Clone`，调用方若要共享字段，需要
/// 显式克隆 `uuid` 这个 Arc。
///
/// `dyn Fn() -> Uuid` 是运行时可调用的 trait object：不同闭包可通过同一个字段类型注入。
/// `Send + Sync` 要求闭包可以在线程间移动并可被多个线程安全共享，便于异步任务持有；它们
/// 不保证闭包纯净或不会产生副作用。生产闭包生成随机 v4 UUID，测试闭包可以返回固定值。
/// 该依赖只负责给出候选标识，不能证明标识已写入、已唯一落库或已获授权。
pub struct QueryDeps {
    /// 按需生成 UUID 的函数；自定义实现可能带有自身副作用，类型约束不会替它做隔离。
    pub uuid: Arc<dyn Fn() -> Uuid + Send + Sync>,
}

impl QueryDeps {
    /// 创建使用 UUID 库随机 v4 生成器的默认依赖集合。
    ///
    /// 【调用者】期望由组合层在创建 Query 工作流时选择；当前仓内没有找到直接调用。
    /// 【输入/输出】无输入，返回 UUID 工厂已绑定到 `Uuid::new_v4` 的 `QueryDeps`。
    /// 【副作用】构造时只保存生成函数；随机值在后续调用 `new_uuid()` 时才生成。
    /// 【失败情况】构造本身不返回错误；若注入函数在调用时 panic，panic 会沿调用栈传播。
    /// 【设计原因】默认路径不要求调用方每次手工配置，而公开字段又允许测试替换为确定性工厂。
    pub fn production() -> Self {
        QueryDeps {
            uuid: Arc::new(Uuid::new_v4),
        }
    }

    /// 调用当前注入的工厂生成一个 UUID。
    ///
    /// 【调用者】未来持有 `QueryDeps` 的 Query 外层逻辑；目前仓内没有直接消费者。
    /// 【输入】无显式参数，实际输入来自闭包捕获的环境。
    /// 【输出】返回闭包给出的 UUID。默认实现每次产生新的随机 v4 值；注入实现可以为了测试
    /// 返回固定或按序生成的值。
    /// 【副作用/失败】本方法不捕获 panic，也不校验两个调用是否得到相同 UUID；若自定义闭包
    /// 有副作用或 panic，其行为会直接传递给调用者。
    /// 【重要边界】UUID 只是候选身份，不是持久化事实。是否被事件账本接受，要看后续业务流程。
    pub fn new_uuid(&self) -> Uuid {
        (self.uuid)()
    }
}

impl Default for QueryDeps {
    /// `default()` 与 `production()` 选择相同的随机 v4 UUID 实现。
    fn default() -> Self {
        Self::production()
    }
}

impl std::fmt::Debug for QueryDeps {
    /// 只打印容器类型，不展开 trait object 或其捕获内容。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryDeps").finish_non_exhaustive()
    }
}
