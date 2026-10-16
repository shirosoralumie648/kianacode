//! 启动阶段的配置与会话状态兼容层。
//!
//! 本 crate 位于 `kiana-entrypoints` 与较早的 CLI/Runner 代码之间：入口在进程启动时调用
//! `config::load_config()` 读取 TOML/JSON/环境变量设置，把模型、endpoint、超时和界面偏好
//! 交给后续 adapter；需要跨调用保存本地会话统计或 UI 辅助状态的旧路径使用 `state` 模块。
//!
//! ```text
//! 进程入口 / init / runner
//!        -> kiana-bootstrap::config 读取配置文件与环境 overlay
//!        -> runner / model adapter 使用解析后的值
//!        -> kiana-bootstrap::state 保存进程内会话计数、缓存和显示辅助信息
//!        -> 真正的身份、授权、生命周期事实仍由 DaemonHost / ControlPlane / EventStore 管理
//! ```
//!
//! 这里的 `api_key`、`base_url` 和 session 字段只是启动/兼容层数据，不是授权凭证签发器；
//! 读取到一个 token 不表示上游请求已经允许，session id 也不等于 Domain 的 Run/Request ID。
//! `GlobalState` 是进程内 `Arc<Mutex<State>>`，重启不会自动恢复，也不能替代 EventLog、
//! receipt 或服务端投影。新增产品能力应先确认是否应该下沉到 `kiana-domain`/`kiana-core`
//! 的权威契约，而不是把业务状态继续堆进这个兼容层。
//!
//! crate 根只做模块声明和重导出：`pub use` 是调用方便的路径，不代表所有导出都在每个入口
//! 被使用；实际数据流仍要沿 `kiana-entrypoints/src/init.rs`、`runner.rs` 和 `cli.rs` 的调用
//! 点核对。

pub mod config;
pub mod state;

pub use config::{get_api_key, load_config, Config, Settings};
pub use state::{
    ChannelEntry, GlobalState, InvokedSkillInfo, ModelUsage, SessionCronTask, SessionId,
    SlowOperation, State,
};
