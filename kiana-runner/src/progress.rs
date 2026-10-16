//! Runner-facing facade for the shared bounded progress reducer.
//!
//! 本模块只 re-export `kiana-domain::progress_stall` 的公开类型和函数，不转换数据，
//! 也不消除 Runner 对 domain 的依赖。进度判断来自有界证据窗口，不是任务完成百分比。
//!
//! ## 主要对象
//!
//! - `ProgressEvidence`：观察摘要、workspace/artifact revision、验证引用摘要及 job cursor
//!   等证据；模型自称“已完成”不单独证明有进展。
//! - `ProgressInput`：结构体，携带证据、可选失败摘要、空 turn 和 stop-hook 反馈，
//!   不是 ModelAttempt/ToolCall/Reject 这样的事件枚举。
//! - `ProgressTracker::observe()`：更新窗口，识别停滞、重复失败和 A→B→A 循环，
//!   并返回 `ProgressDecision`；没有单独的 `decide()` 方法。
//! - `ProgressAction`：Continue、FeedbackOnce、RequestClarification 或 Blocked。
//!   这些是 reducer 的诊断结果，不是授权或 sandbox 许可。
//! - `failure_digest()`：校验失败原因的长度和基本格式后生成摘要，不执行内容脱敏。
//!
//! ## 当前接线边界
//!
//! `ActiveRun` 持有 tracker，harness 在 checkpoint/restore 时复制、校验和恢复它；
//! 当前 `harness.rs` 没有调用 `progress_tracker.observe()`。实际已接入的连续重复工具
//! 调用检查是 `KianaHarness::record_tool_call()`，不能与此 reducer 的证据窗口混为一谈。
//! checkpoint 支持也不等于本模块自行写入 EventLog 或已具备持久恢复闭环。

pub use kiana_domain::{
    failure_digest, ProgressAction, ProgressDecision, ProgressEvidence, ProgressInput,
    ProgressTracker, DEFAULT_NO_PROGRESS_LIMIT, DEFAULT_PROGRESS_WINDOW,
};
