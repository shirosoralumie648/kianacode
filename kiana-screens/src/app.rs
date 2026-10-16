//! 屏幕状态容器、键盘事件路由和终端主循环。
//!
//! 外层 `kiana-entrypoints/src/tui.rs` 提供 action/tick callback：本文件把用户输入路由到当前屏幕，
//! 将需要外部 I/O 的意图排入 `AppAction`，再由 adapter 消费并回填结果。这里不连接模型或服务。
//! 该 TUI 目前已 park；不要将此循环误认为当前 Harness 的第二条执行路径。

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

use crate::doctor::{DoctorScreen, DoctorState};
use crate::history::{HistoryScreen, HistoryState};
use crate::repl::{ReplScreen, ReplState};
use crate::resume_conversation::{ResumeScreen, ResumeState};
use crate::settings::{SettingsScreen, SettingsState};

/// 当前由 App 接收键盘事件并负责绘制的屏幕。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppScreen {
    /// 旧交互 REPL；输入意图会变成 AppAction 交给外层 runtime。
    Repl,
    /// 环境诊断展示页。
    Doctor,
    /// 从外层提供的会话条目中选择恢复目标。
    ResumeConversation,
    /// 浏览并选择先前保存的 prompt。
    History,
    /// 展示外层提供的只读设置/readiness 摘要。
    Settings,
}

/// UI 不能自行完成的应用意图，由外层 shell/runtime 收取并执行。
///
/// 例如数据加载、提交/取消 prompt、slash command 和 session resume 都会先成为 action；
/// `take_actions()` 把这些值交给 `run_app_with_handlers()` 的 callback。它们是消息，不是执行回执。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    /// 外层加载 Doctor 输出并写回 DoctorState。
    LoadDoctor,
    /// 外层异步读取可恢复会话列表。
    LoadResumeSessions,
    /// 外层加载 prompt 历史。
    LoadPromptHistory,
    /// 外层汇总 Settings/readiness 展示数据。
    LoadSettings,
    /// 提交用户新 prompt。
    SubmitPrompt(String),
    /// 将 prompt 放入外层等待队列。
    QueuePrompt(String),
    /// 请求外层取消当前 prompt 操作。
    CancelPrompt,
    /// 把 slash command 的名字和原始参数交给兼容命令层。
    RunSlashCommand { name: String, args: String },
    /// 请求外层恢复指定 session ID。
    ResumeSession(String),
}

/// UI 根状态：保存当前屏幕、各 screen 的展示状态以及尚未交给 adapter 的 action 队列。
///
/// 这些字段是终端交互层状态，不是 EventLog、Core lifecycle 或恢复 checkpoint 的权威副本。
pub struct App {
    /// 当前活动 screen。
    pub screen: AppScreen,
    /// REPL 输入、消息和加载状态。
    pub repl: ReplState,
    /// Doctor 诊断展示数据。
    pub doctor: DoctorState,
    /// Resume session picker 数据。
    pub resume: ResumeState,
    /// Prompt history picker 数据。
    pub history: HistoryState,
    /// Settings/readiness 展示数据。
    pub settings: SettingsState,
    /// 用户已请求结束本地终端循环。
    pub should_quit: bool,
    /// 等待 adapter 收取的外部动作队列。
    actions: Vec<AppAction>,
}

impl App {
    /// 创建初始界面：从 REPL screen 开始，各 screen 使用各自默认状态。
    pub fn new() -> Self {
        Self {
            screen: AppScreen::Repl,
            repl: ReplState::default(),
            doctor: DoctorState::default(),
            resume: ResumeState::default(),
            history: HistoryState::default(),
            settings: SettingsState::default(),
            should_quit: false,
            actions: Vec::new(),
        }
    }

    /// 先处理全局 Ctrl 快捷键，再把其他按键交给当前 screen。
    ///
    /// 【优先顺序】权限卡片 Ctrl-Y/N → loading 时 Ctrl-C 取消 → Ctrl-C/Ctrl-D 退出 → screen 自己处理。
    /// screen 切换到需要数据的页面时，这里先把 loading 状态设好并排入 Load action；真正的 I/O 由 adapter 完成。
    /// Submit/Queue/Resume/Slash 等结果不会在本函数直接执行，而是排入 `actions` 等主循环回调消费。
    pub fn handle_key(&mut self, key: event::KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.screen == AppScreen::Repl && self.repl.permission_request_active {
                match key.code {
                    KeyCode::Char('y') => {
                        self.actions.push(AppAction::RunSlashCommand {
                            name: "allow".to_string(),
                            args: String::new(),
                        });
                        return;
                    }
                    KeyCode::Char('n') => {
                        self.actions.push(AppAction::RunSlashCommand {
                            name: "deny".to_string(),
                            args: String::new(),
                        });
                        return;
                    }
                    _ => {}
                }
            }
            if key.code == KeyCode::Char('c')
                && self.screen == AppScreen::Repl
                && self.repl.is_loading
            {
                self.actions.push(AppAction::CancelPrompt);
                return;
            }
            if key.code == KeyCode::Char('c') || key.code == KeyCode::Char('d') {
                self.should_quit = true;
                return;
            }
        }

        match self.screen {
            AppScreen::Repl => {
                if let Some(event) = self.repl.handle_key(key) {
                    match event {
                        crate::repl::ReplEvent::SwitchScreen(next) => {
                            if next == AppScreen::Doctor {
                                self.actions.push(AppAction::LoadDoctor);
                            }
                            if next == AppScreen::ResumeConversation {
                                self.actions.push(AppAction::LoadResumeSessions);
                            }
                            if next == AppScreen::History {
                                self.history.reset_search();
                                self.history.loading = true;
                                self.actions.push(AppAction::LoadPromptHistory);
                            }
                            if next == AppScreen::Settings {
                                self.settings.set_loading();
                                self.actions.push(AppAction::LoadSettings);
                            }
                            self.screen = next;
                        }
                        crate::repl::ReplEvent::SubmitPrompt(prompt) => {
                            self.actions.push(AppAction::SubmitPrompt(prompt));
                        }
                        crate::repl::ReplEvent::QueuePrompt(prompt) => {
                            self.actions.push(AppAction::QueuePrompt(prompt));
                        }
                        crate::repl::ReplEvent::RunSlashCommand { name, args } => {
                            self.actions.push(AppAction::RunSlashCommand { name, args });
                        }
                    }
                }
            }
            AppScreen::Doctor => {
                if self.doctor.handle_key(key) {
                    // Enter / Esc → back to REPL
                    self.screen = AppScreen::Repl;
                }
            }
            AppScreen::ResumeConversation => {
                if let Some(event) = self.resume.handle_key(key) {
                    match event {
                        crate::resume_conversation::ResumeEvent::SwitchScreen(next) => {
                            self.screen = next;
                        }
                        crate::resume_conversation::ResumeEvent::ResumeSession(session_id) => {
                            self.actions.push(AppAction::ResumeSession(session_id));
                            self.screen = AppScreen::Repl;
                        }
                    }
                }
            }
            AppScreen::History => {
                if let Some(event) = self.history.handle_key(key) {
                    match event {
                        crate::history::HistoryEvent::SwitchScreen(next) => {
                            self.screen = next;
                        }
                        crate::history::HistoryEvent::RestorePrompt(prompt) => {
                            self.repl.restore_draft(prompt);
                            self.screen = AppScreen::Repl;
                        }
                    }
                }
            }
            AppScreen::Settings => {
                if self.settings.handle_key(key) {
                    self.screen = AppScreen::Repl;
                }
            }
        }
    }

    /// 一次性取走所有待处理 action，避免同一 UI 意图在后续循环中重复执行。
    pub fn take_actions(&mut self) -> Vec<AppAction> {
        std::mem::take(&mut self.actions)
    }

    /// 根据当前 screen 选择对应绘制器；只把现有 state 渲染到本帧，不读取外部服务。
    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        match self.screen {
            AppScreen::Repl => ReplScreen::draw(frame, &mut self.repl),
            AppScreen::Doctor => DoctorScreen::draw(frame, &self.doctor),
            AppScreen::ResumeConversation => ResumeScreen::draw(frame, &mut self.resume),
            AppScreen::History => HistoryScreen::draw(frame, &mut self.history),
            AppScreen::Settings => SettingsScreen::draw(frame, &mut self.settings),
        }
    }

    /// 推进本地动画状态；模型/网络进度由外层 tick handler 更新。
    pub fn tick(&mut self) {
        if self.repl.is_loading {
            self.repl.tick_spinner();
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// 用空 action handler 启动终端循环；因此 UI 发出的应用 action 会被消费但不会执行外部工作。
///
/// 这是便捷/展示入口，真正接应用逻辑的调用者应使用 `run_app_with_action_handler()` 或
/// `run_app_with_handlers()`。仓库产品状态将旧 TUI 标记为 parked。
pub fn run_app() -> Result<()> {
    run_app_with_action_handler(|_, _| Ok(()))
}

/// 启动终端循环，并把每个 `AppAction` 交给一个应用层 callback。
///
/// callback 可以基于 action 更新传入的 App；本函数不规定其如何访问服务或保存状态。
pub fn run_app_with_action_handler<H>(mut action_handler: H) -> Result<()>
where
    H: FnMut(AppAction, &mut App) -> Result<()>,
{
    run_app_with_handlers(|action, app| action_handler(action, app), |_| Ok(()))
}

/// 初始化 Crossterm/Ratatui，循环执行 tick、绘制、键盘轮询和外层 callback。
///
/// 【核心顺序】开 raw mode → alternate screen/mouse capture → 建立 Terminal → 每轮更新/绘制 →
/// 最多等待 16 ms 读取事件 → 处理 key 并逐个交付 action → 用户退出后恢复终端。
/// poll 的 16 ms 是一次键盘等待上限，不是网络 timeout。
///
/// ⚠ 正常 break 后会恢复 raw mode、alternate screen、mouse capture 和 cursor；启用 raw mode 之后，
/// 初始化或循环中的任意 `?` 错误都可能提前返回，当前没有 Drop guard 来保证恢复终端状态。
/// 此外 mouse capture 已开启，但事件循环只分发 `Event::Key`，没有处理 Mouse 事件。
pub fn run_app_with_handlers<H, T>(mut action_handler: H, mut tick_handler: T) -> Result<()>
where
    H: FnMut(AppAction, &mut App) -> Result<()>,
    T: FnMut(&mut App) -> Result<()>,
{
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut app = App::new();

    loop {
        app.tick();
        tick_handler(&mut app)?;
        terminal.draw(|f| app.draw(f))?;

        // 限制无输入时一次等待的时长，让下一轮仍能刷新 spinner 和运行 tick_handler。
        if event::poll(std::time::Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
                for action in app.take_actions() {
                    action_handler(action, &mut app)?;
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    // 仅正常退出会走到这里；上面的任何 Err 都不会执行以下终端恢复操作。
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryEntry;
    use crate::resume_conversation::SessionEntry;

    fn key(code: KeyCode) -> event::KeyEvent {
        event::KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn repl_enter_emits_submit_prompt_action() {
        let mut app = App::new();
        app.repl.input = "inspect workspace".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::SubmitPrompt("inspect workspace".to_string())]
        );
        assert_eq!(app.repl.messages.len(), 1);
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_enter_while_loading_does_not_submit_again() {
        let mut app = App::new();
        app.repl.is_loading = true;
        app.repl.input = "second prompt".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::QueuePrompt("second prompt".to_string())]
        );
        assert_eq!(app.repl.messages.len(), 0);
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_enter_while_loading_allows_permission_response_command() {
        let mut app = App::new();
        app.repl.is_loading = true;
        app.repl.input = "/allow".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "allow".to_string(),
                args: String::new()
            }]
        );
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_enter_while_loading_allows_cancel_command() {
        let mut app = App::new();
        app.repl.is_loading = true;
        app.repl.input = "/cancel".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "cancel".to_string(),
                args: String::new()
            }]
        );
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_permission_shortcuts_emit_allow_and_deny_actions() {
        let mut app = App::new();
        app.repl.is_loading = true;
        app.repl.permission_request_active = true;

        app.handle_key(event::KeyEvent::new(
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        ));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "allow".to_string(),
                args: String::new()
            }]
        );

        app.handle_key(event::KeyEvent::new(
            KeyCode::Char('n'),
            KeyModifiers::CONTROL,
        ));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "deny".to_string(),
                args: String::new()
            }]
        );
    }

    #[test]
    fn ctrl_c_cancels_loading_repl_prompt_instead_of_quitting() {
        let mut app = App::new();
        app.repl.is_loading = true;

        app.handle_key(event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ));

        assert!(!app.should_quit);
        assert_eq!(app.take_actions(), vec![AppAction::CancelPrompt]);
    }

    #[test]
    fn ctrl_c_quits_when_repl_is_idle() {
        let mut app = App::new();

        app.handle_key(event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ));

        assert!(app.should_quit);
        assert!(app.take_actions().is_empty());
    }

    #[test]
    fn app_tick_advances_loading_spinner_only() {
        let mut app = App::new();
        app.tick();
        assert_eq!(app.repl.spinner_tick, 0);

        app.repl.is_loading = true;
        app.tick();

        assert_eq!(app.repl.spinner_tick, 1);
    }

    #[test]
    fn repl_resume_command_switches_screen_without_action() {
        let mut app = App::new();
        app.repl.input = "/resume".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::ResumeConversation);
        assert_eq!(app.take_actions(), vec![AppAction::LoadResumeSessions]);
    }

    #[test]
    fn repl_history_command_switches_screen_and_requests_history_load() {
        let mut app = App::new();
        app.history.search_active = true;
        app.history.search_query = "old filter".to_string();
        app.repl.input = "/history".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::History);
        assert!(app.history.loading);
        assert!(!app.history.search_active);
        assert!(app.history.search_query.is_empty());
        assert_eq!(app.take_actions(), vec![AppAction::LoadPromptHistory]);
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_settings_command_switches_screen_and_requests_load() {
        let mut app = App::new();
        app.repl.input = "/settings".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Settings);
        assert!(app.settings.loading);
        assert_eq!(app.take_actions(), vec![AppAction::LoadSettings]);
    }

    #[test]
    fn repl_doctor_command_switches_screen_and_requests_load() {
        let mut app = App::new();
        app.repl.input = "/doctor".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Doctor);
        assert_eq!(app.take_actions(), vec![AppAction::LoadDoctor]);
    }

    #[test]
    fn repl_help_command_emits_slash_action_without_model_submit() {
        let mut app = App::new();
        app.repl.input = "/help".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Repl);
        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "help".to_string(),
                args: String::new()
            }]
        );
        assert!(app.repl.messages.is_empty());
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_slash_command_preserves_arguments_for_runtime_registry() {
        let mut app = App::new();
        app.repl.input = "/config set theme dark".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "config".to_string(),
                args: "set theme dark".to_string()
            }]
        );
    }

    #[test]
    fn resume_enter_emits_resume_session_action() {
        let mut app = App::new();
        app.screen = AppScreen::ResumeConversation;
        app.resume.load_sessions(vec![SessionEntry {
            session_id: "session-1".to_string(),
            title: "Recent work".to_string(),
            timestamp: "2026-06-12 10:00:00".to_string(),
            message_count: 3,
        }]);

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Repl);
        assert_eq!(
            app.take_actions(),
            vec![AppAction::ResumeSession("session-1".to_string())]
        );
    }

    #[test]
    fn history_enter_restores_selected_prompt_as_repl_draft() {
        let mut app = App::new();
        app.screen = AppScreen::History;
        app.history.load_entries(vec![
            HistoryEntry::new("first prompt".to_string(), "2".to_string()),
            HistoryEntry::new("second prompt".to_string(), "1".to_string()),
        ]);
        app.history.selected = 1;
        app.history.list_state.select(Some(1));

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Repl);
        assert_eq!(app.repl.input, "second prompt");
        assert_eq!(app.repl.cursor, app.repl.input.len());
        assert!(app.take_actions().is_empty());
    }
}
