//! # 文件职责
//!
//! 本 crate 保存 parked TUI 的 screen 状态、键盘事件转发和 Ratatui 绘制逻辑。它把交互意图转换成
//! `AppAction` 交给外层 adapter，并根据 adapter 写回的状态重绘；它不负责请求模型、访问 ControlPlane
//! 或执行命令。
//!
//! ```text
//! kiana-entrypoints/src/tui.rs（旧 TUI adapter / runtime）
//!   -> run_app_with_handlers(action_handler, tick_handler)
//!   -> App.handle_key() 路由到当前 Screen
//!   -> Screen 返回 AppAction / 更新纯 UI state
//!   -> adapter 执行异步或旧 SDK 工作并写回结果
//!   -> App.draw() -> Ratatui widgets -> Crossterm terminal
//! ```
//!
//! `kiana tui` 当前在产品计划中 park，仍沿用 legacy SDK/stream，不是 DaemonHost 主路径。Ratatui
//! 负责布局/控件缓冲区，Crossterm 负责终端原始模式和事件；状态结构通常只承载展示数据，外部效果
//! 由 entrypoints adapter 处理。

pub mod app;
pub mod doctor;
pub mod history;
pub mod repl;
pub mod resume_conversation;
pub mod search_history;
pub mod settings;

pub use app::{
    run_app, run_app_with_action_handler, run_app_with_handlers, App, AppAction, AppScreen,
};
