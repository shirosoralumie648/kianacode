//! 受 Kiana Runner 协议约束的模型循环运行时。
//!
//! 生产 `KianaHarness` 归本 crate 所有；inbox、compact 和工具协议在这里统一定义，避免
//! daemon 或入口层再包一层第二模型循环。模型只能看到固定的 `shell`、`apply_patch`、
//! `mcp`、`memory.search`、`memory.write` 工具名；每次调用都会回到 ControlPlane 形成
//! `CapabilityRequest`，真正执行仍由受控 Broker 完成。
//!
//! 当前模型客户端可以是脚本或显式不可用实现；脚本通过测试/本地 cassette 证明行为，
//! 不等于已接入 live provider。`reference/` 中的上游源码只作为审计对照，不是 workspace
//! 成员或运行时依赖。

mod compact;
mod harness;
mod inbox;
mod model;
mod protocol_runner;
mod tools;

pub use harness::{
    KianaHarness, KianaHarnessError, RuntimeConfig, HARNESS_ID, HARNESS_RESULT_SCHEMA,
};
pub use inbox::{Inbox, InboxMessage, InboxTarget};
pub use model::{
    ModelClient, ModelMessage, ModelOutput, ModelRequest, ModelRole, ModelToolCall, ModelUsage,
    ScriptedModel, UnavailableModel,
};
pub use protocol_runner::ProtocolRunner;
