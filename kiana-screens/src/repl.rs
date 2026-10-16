//! 旧 TUI 的对话输入状态、用户意图生成和 Ratatui 渲染。
//!
//! 主要数据流：键盘事件 → `ReplState::handle_key()` 编辑本地草稿/生成 `ReplEvent` → `App` 转成
//! `AppAction` → `kiana-entrypoints/src/tui.rs` adapter 调用旧 SDK、命令或审批通道 → 事件回填消息、
//! usage 与权限面板 → `ReplScreen::draw()` 绘制。这个 crate 不发网络请求、不执行工具，也不决定权限。
//!
//! 本地 `history` 是 REPL 的 ↑/↓ 回忆列表；持久化 prompt 列表由另一个 History screen/adapter 管理。
//! 屏幕上显示的 transcript 是 UI 投影，不是 EventLog 或运行状态的权威记录。整个旧 TUI 已 park，
//! 不能把此处的对话循环当成当前 DaemonHost/KianaHarness 主路径。
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::AppScreen;
use crate::history::HistoryEntry;

// ─── Message entry ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
/// 一条屏幕 transcript 项；内容由本地用户输入或 adapter 回传的模型/工具/系统事件产生。
pub struct ConversationMessage {
    /// 展示角色；用于标签和颜色，不是权限身份。
    pub role: MessageRole,
    /// 面向 UI 的文本，可能是流式片段累加后的内容或工具结果摘要。
    pub content: String,
    /// producer 提供的展示时间文本；当前本地新增消息使用 `HH:MM:SS`，并非 ISO-8601。
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq)]
/// transcript 的 UI 分类；颜色和标签由本模块映射，不能据此判断消息是否可信。
pub enum MessageRole {
    /// 用户提交的 prompt。
    User,
    /// 助手返回或流式生成的文本。
    Assistant,
    /// 供用户阅读的运行状态、失败或提示。
    System,
    /// 工具调用及其结果的显示项。
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 一次待审批请求的展示副本；它只提供阅读上下文，真正的待处理 oneshot 保存在外层 adapter。
pub struct ReplPermissionPanel {
    /// 请求涉及的工具名，用来帮助用户辨认本次操作。
    pub tool_name: String,
    /// 工具调用 ID；用于把展示信息和对应请求关联，不是可单独验证的授权凭证。
    pub tool_use_id: String,
    /// adapter 从决策原因中提取的可选说明文本。
    pub reason: Option<String>,
    /// 被拒绝或等待审批的目标路径摘要。
    pub blocked_path: Option<String>,
    /// 输入 JSON 的截断预览，不保证展示了完整输入。
    pub input_preview: Option<String>,
    /// 权限建议的截断预览；展示建议不等于建议已被接受。
    pub suggestions_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ReplState 交给 App 的交互结果；App 再将它路由到外部 adapter 或切换 screen。
pub enum ReplEvent {
    /// 请求切换到另一个 screen。
    SwitchScreen(AppScreen),
    /// 请求开始处理一个用户 prompt；只发意图，不代表请求已成功。
    SubmitPrompt(String),
    /// 当前有工作进行时，把后续 prompt 交给 adapter 排队。
    QueuePrompt(String),
    /// 把本地 slash command 名称及参数交给兼容命令 adapter。
    RunSlashCommand { name: String, args: String },
}

impl MessageRole {
    /// 返回显示在消息头部的固定标签；不会读取或校验消息来源。
    fn label(&self) -> &'static str {
        match self {
            MessageRole::User => "You",
            MessageRole::Assistant => "Kiana",
            MessageRole::System => "System",
            MessageRole::Tool => "Tool",
        }
    }

    /// 为不同 UI 角色选取终端文字颜色；颜色只是视觉区分。
    fn color(&self) -> Color {
        match self {
            MessageRole::User => Color::Cyan,
            MessageRole::Assistant => Color::Green,
            MessageRole::System => Color::DarkGray,
            MessageRole::Tool => Color::Yellow,
        }
    }
}

// ─── Spinner state ───────────────────────────────────────────────────────────

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ─── REPL state ──────────────────────────────────────────────────────────────

#[derive(Debug)]
/// Repl screen 的输入、展示和局部导航状态；adapter 仍拥有实际会话/异步任务。
pub struct ReplState {
    /// 当前屏幕显示的消息序列；异步流式输出会由 adapter 找到助手项并追加内容。
    pub messages: Vec<ConversationMessage>,
    /// 用户正在编辑、尚未提交的草稿。
    pub input: String,
    /// UTF-8 字节偏移而非字符数/终端列数；必须保持在字符边界，绘制时再换算为屏幕宽度。
    pub cursor: usize,
    /// 当前进程内的 ↑/↓ 回忆列表，顺序为较旧到较新；持久化由 entrypoints 另行完成。
    pub history: Vec<String>,
    /// 当前回忆到 history 的位置；None 表示显示普通草稿而非历史项。
    pub history_idx: Option<usize>,
    /// 第一次按 Up 前保存的草稿，按 Down 越过最新历史项时恢复。
    pub history_draft: String,
    /// 外层 prompt、命令或恢复工作正在进行；该展示状态也决定普通 prompt 是否进入等待队列。
    pub is_loading: bool,
    /// 外层有一个当前待处理的权限请求；App 用它截获 Ctrl-Y/N/C 并转成交互 action。
    pub permission_request_active: bool,
    /// 当前请求之后排队等待展示的权限请求数，不包含当前面板中的请求。
    pub permission_request_queue_len: usize,
    /// 当前审批面板的展示字段；None 时不分配面板空间。
    pub permission_panel: Option<ReplPermissionPanel>,
    /// 当前 spinner 帧索引；只改变动画显示，不表示模型 token 或请求阶段。
    pub spinner_tick: usize,
    /// 距离 transcript 底部的视觉行偏移；0 表示显示最新内容，PageUp 每次增加 5 行。
    pub scroll_offset: usize,
    /// 交给 Ratatui stateful List 的 UI 状态；消息滚动窗口另由 scroll_offset 计算。
    pub list_state: ListState,
    /// 状态栏展示的累计美元成本；当前 entrypoints TUI 事件回填没有赋值点，默认保持 0.0。
    pub session_cost: f64,
    /// 最近收到 usage 事件中的输入 token 数；缺失字段不会清零旧值。
    pub input_tokens: u64,
    /// 最近收到 usage 事件中的输出 token 数；缺失字段不会清零旧值。
    pub output_tokens: u64,
    /// 最近 usage 事件提供的模型标签；构造时的字符串只是初始显示值，不是模型选择配置。
    pub model_name: String,
}

impl Default for ReplState {
    fn default() -> Self {
        let mut list_state = ListState::default();
        list_state.select(None);
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_idx: None,
            history_draft: String::new(),
            is_loading: false,
            permission_request_active: false,
            permission_request_queue_len: 0,
            permission_panel: None,
            spinner_tick: 0,
            scroll_offset: 0,
            list_state,
            session_cost: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            model_name: "claude-sonnet-4-5".to_owned(),
        }
    }
}

impl ReplState {
    /// 处理当前 screen 收到的按键：编辑草稿、浏览本地历史、调整滚动或返回一个待路由事件。
    ///
    /// 【调用者】`App::handle_key()` 在先处理全局 Ctrl-Y/N/C 后调用本函数。
    /// 【返回值】`None` 表示只更新本地 UI 状态；`Some(ReplEvent)` 交由 App 转成 action/screen 切换，
    /// 再由 entrypoints adapter 执行。这里的 `SubmitPrompt` 不是成功回执。
    /// 【顺序】普通 Enter 先记录回忆、清空输入，再把用户消息加入 transcript，最后发出 SubmitPrompt。
    /// 加载中普通文本走 QueuePrompt；加载中只有 allow/approve/deny/reject/cancel/stop 这组 slash 命令通过。
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ReplEvent> {
        match key.code {
            // ── Submit ────────────────────────────────────────────────────
            KeyCode::Enter => {
                if is_newline_key(key) {
                    self.insert_char('\n');
                    return None;
                }
                let text = self.input.trim().to_owned();
                if text.is_empty() {
                    return None;
                }
                let slash_command = split_slash_command(&text);
                if self.is_loading {
                    if let Some((name, args)) = slash_command.clone() {
                        if is_loading_slash_command(&name) {
                            self.record_history(&text);
                            self.clear_input_after_submit();
                            return Some(ReplEvent::RunSlashCommand { name, args });
                        }
                        return None;
                    }
                    self.record_history(&text);
                    self.clear_input_after_submit();
                    return Some(ReplEvent::QueuePrompt(text));
                }

                if let Some((name, args)) = slash_command {
                    if name == "history" {
                        self.clear_input_after_submit();
                        return Some(ReplEvent::SwitchScreen(AppScreen::History));
                    }
                    self.record_history(&text);
                    self.clear_input_after_submit();
                    return match name.as_str() {
                        "doctor" => Some(ReplEvent::SwitchScreen(AppScreen::Doctor)),
                        "resume" => Some(ReplEvent::SwitchScreen(AppScreen::ResumeConversation)),
                        "settings" => Some(ReplEvent::SwitchScreen(AppScreen::Settings)),
                        _ => Some(ReplEvent::RunSlashCommand { name, args }),
                    };
                }

                self.record_history(&text);
                self.clear_input_after_submit();

                // 先把用户输入作为 transcript 项显示；adapter 随后收到相同文本并启动请求。
                // UI 中出现用户消息只证明本地已接收输入，不证明 SDK 或上游已接受请求。
                self.push_message(MessageRole::User, text);
                return Some(ReplEvent::SubmitPrompt(
                    self.messages
                        .last()
                        .map(|message| message.content.clone())
                        .unwrap_or_default(),
                ));
            }

            // ── Editing ───────────────────────────────────────────────────
            KeyCode::Char(c) => {
                // Ctrl-J 是另一种插入换行的按键表示。
                if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'j' {
                    self.insert_char('\n');
                } else if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' {
                    // 这里删除光标之前的前缀并保留光标之后的尾部，并非无条件清空整行。
                    let tail = self.input[self.cursor..].to_owned();
                    self.input = tail;
                    self.cursor = 0;
                } else if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'w' {
                    // 从光标向左删掉一段非空白字符和其前面的空白；右侧草稿保留。
                    let head = &self.input[..self.cursor];
                    let new_end = head
                        .trim_end_matches(|c: char| !c.is_whitespace())
                        .trim_end_matches(char::is_whitespace)
                        .len();
                    self.input =
                        format!("{}{}", &self.input[..new_end], &self.input[self.cursor..]);
                    self.cursor = new_end;
                } else if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'a' {
                    self.cursor = 0;
                } else if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'e' {
                    self.cursor = self.input.len();
                } else {
                    self.insert_char(c);
                }
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    // cursor 是 UTF-8 字节位置；用 char_indices 找前一个字符边界，避免 remove 落在多字节字符中间。
                    let prev = self.input[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.remove(prev);
                    self.cursor = prev;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = self.input[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                if self.cursor < self.input.len() {
                    let ch = self.input[self.cursor..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    self.cursor += ch;
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),

            // ── History navigation ────────────────────────────────────────
            KeyCode::Up => {
                if self.history.is_empty() {
                    return None;
                }
                if self.history_idx.is_none() {
                    // 只在进入回忆模式时保存草稿；连续 Up 不覆盖它，便于 Down 时恢复。
                    self.history_draft = self.input.clone();
                }
                let new_idx = self
                    .history_idx
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(self.history.len() - 1);
                self.history_idx = Some(new_idx);
                self.input = self.history[new_idx].clone();
                self.cursor = self.input.len();
            }
            KeyCode::Down => {
                if let Some(idx) = self.history_idx {
                    if idx + 1 >= self.history.len() {
                        self.history_idx = None;
                        self.input = self.history_draft.clone();
                    } else {
                        let new_idx = idx + 1;
                        self.history_idx = Some(new_idx);
                        self.input = self.history[new_idx].clone();
                    }
                    self.cursor = self.input.len();
                }
            }

            // ── Scroll ────────────────────────────────────────────────────
            KeyCode::PageUp => {
                // 偏移单位是绘制后的视觉行而不是消息数；实际最大值在 draw_messages 中按窗口高度裁剪。
                self.scroll_offset = self.scroll_offset.saturating_add(5);
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
            }

            _ => {}
        }
        None
    }

    pub fn push_message(&mut self, role: MessageRole, content: String) {
        self.messages.push(ConversationMessage {
            role,
            content,
            timestamp: chrono_now(),
        });
        // 新消息加入后回到最新内容；offset 的 0 代表视口贴近 transcript 底部。
        self.scroll_offset = 0;
    }

    /// 推进一个 spinner 帧；App 只在 is_loading 时调用，帧数变化会影响取模结果。
    pub fn tick_spinner(&mut self) {
        self.spinner_tick = (self.spinner_tick + 1) % SPINNER_FRAMES.len();
    }

    /// 把 adapter 收到的当前审批摘要同步到 UI 状态；不发送 Allow/Deny，也不改变授权记录。
    pub fn set_permission_request(&mut self, panel: ReplPermissionPanel, queued_count: usize) {
        self.permission_request_active = true;
        self.permission_request_queue_len = queued_count;
        self.permission_panel = Some(panel);
    }

    /// adapter 已没有当前等待展示的请求时清除面板和计数。
    pub fn clear_permission_request(&mut self) {
        self.permission_request_active = false;
        self.permission_request_queue_len = 0;
        self.permission_panel = None;
    }

    /// 把外层按“新到旧”加载的持久化条目倒转成“旧到新”的键盘回忆顺序。
    ///
    /// 这样第一次 Up 取到向量末尾时正好是最新 prompt；此操作只改变内存导航列表。
    pub fn load_persisted_history(&mut self, entries: &[HistoryEntry]) {
        self.history = entries
            .iter()
            .rev()
            .map(|entry| entry.prompt.clone())
            .collect();
        self.history_idx = None;
        self.history_draft.clear();
    }

    /// 用 History screen 选中的 prompt 替换草稿，并退出旧的 ↑/↓ 浏览位置。
    pub fn restore_draft(&mut self, prompt: String) {
        self.input = prompt;
        self.cursor = self.input.len();
        self.history_idx = None;
        self.history_draft.clear();
    }

    /// 将已提交的 prompt/slash 行加入本地回忆：重复文本先移除旧位置，再放到最新端。
    ///
    /// 这只维护 ReplState 中当前进程的 ↑/↓ 列表；持久化 prompt-history 是 entrypoints adapter 的另一条写入路径。
    fn record_history(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if let Some(index) = self.history.iter().position(|entry| entry == text) {
            self.history.remove(index);
        }
        self.history.push(text.to_string());
    }

    /// 提交/切换屏幕后复位编辑导航，避免旧光标或历史草稿污染下一次输入。
    fn clear_input_after_submit(&mut self) {
        self.history_idx = None;
        self.history_draft.clear();
        self.input.clear();
        self.cursor = 0;
    }

    /// 在 UTF-8 字节偏移处插入一个 Unicode 标量，并将 cursor 按其编码字节数向后移动。
    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }
}

/// Shift/Alt+Enter 表示在 prompt 内插入换行；普通 Enter 才尝试提交。
fn is_newline_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && (key.modifiers.contains(KeyModifiers::SHIFT)
            || key.modifiers.contains(KeyModifiers::ALT))
}

/// 仅解析以 `/` 开头的整行；命令名转为 ASCII 小写，空白后的剩余文本保留为 args。
///
/// 这里负责识别和路由，不检查命令是否存在；命令注册表和错误提示由 entrypoints adapter 负责。
fn split_slash_command(input: &str) -> Option<(String, String)> {
    let command_line = input.strip_prefix('/')?;
    let mut parts = command_line.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let args = parts.next().unwrap_or_default().trim().to_string();
    Some((name, args))
}

/// 加载期间允许继续路由的少数控制类命令名称；这只是输入层白名单，真正动作仍由外层处理。
fn is_loading_slash_command(name: &str) -> bool {
    matches!(
        name,
        "allow" | "approve" | "deny" | "reject" | "cancel" | "stop"
    )
}

// ─── Rendering ───────────────────────────────────────────────────────────────

pub struct ReplScreen;

impl ReplScreen {
    /// 按 terminal 当前帧尺寸组合消息、可选审批面板、状态行和 prompt 输入框。
    ///
    /// 布局优先给消息保留剩余空间；审批面板和多行输入会按可用高度计算，绘制只操作 UI 状态。
    pub fn draw(frame: &mut Frame, state: &mut ReplState) {
        let area = frame.area();
        let permission_height = permission_panel_height(state, area.width, area.height);
        let input_height = input_panel_height(
            state,
            area.width,
            area.height.saturating_sub(permission_height),
        );

        if state.permission_panel.is_some() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(permission_height),
                    Constraint::Length(1),
                    Constraint::Length(input_height),
                ])
                .split(area);

            Self::draw_messages(frame, state, chunks[0]);
            Self::draw_permission_panel(frame, state, chunks[1]);
            Self::draw_status(frame, state, chunks[2]);
            Self::draw_input(frame, state, chunks[3]);
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),    // message list
                    Constraint::Length(1), // status line
                    Constraint::Length(input_height),
                ])
                .split(area);

            Self::draw_messages(frame, state, chunks[0]);
            Self::draw_status(frame, state, chunks[1]);
            Self::draw_input(frame, state, chunks[2]);
        }
    }

    fn draw_messages(frame: &mut Frame, state: &mut ReplState, area: Rect) {
        let height = area.height as usize;

        // 每条消息会展开成标题、正文视觉行和空白分隔，因此下面的滚动计数以行项为单位，而非消息条数。
        let items: Vec<ListItem> = state
            .messages
            .iter()
            .flat_map(|msg| message_to_lines(msg, area.width as usize))
            .collect();

        let total = items.len();
        // 调整窗口尺寸或收到新消息后，旧偏移可能已经超出可滚动范围；在绘制前将其限制到合法上限。
        let max_offset = total.saturating_sub(height);
        if state.scroll_offset > max_offset {
            state.scroll_offset = max_offset;
        }

        // offset 为 0 时取尾部最新高度行；偏移增大则从更早的内容截取同样大小的窗口。
        let visible: Vec<ListItem> = if total <= height {
            items
        } else {
            let start = total - height - state.scroll_offset;
            items.into_iter().skip(start).take(height).collect()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Kiana ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );

        let list = List::new(visible).block(block);
        frame.render_stateful_widget(list, area, &mut state.list_state);
    }

    fn draw_status(frame: &mut Frame, state: &ReplState, area: Rect) {
        let para = Paragraph::new(status_text(state)).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(para, area);
    }

    fn draw_permission_panel(frame: &mut Frame, state: &ReplState, area: Rect) {
        let para = Paragraph::new(permission_panel_text(state))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Approval ")
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::White));
        frame.render_widget(para, area);
    }

    fn draw_input(frame: &mut Frame, state: &ReplState, area: Rect) {
        // 仅为多行文本添加可视前缀，不把提示符字符写回真正要提交的 input。
        let display_text = input_display_text(&state.input);

        let para = Paragraph::new(display_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Prompt ")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(para, area);

        // Paragraph 的边框与首行提示符占据终端列；按同一布局换算 byte cursor 后再加上边框偏移。
        let content_width = area.width.saturating_sub(2) as usize;
        let (cursor_row, cursor_column) =
            input_cursor_position(&state.input, state.cursor, content_width);
        let cursor_x = area.x + 1 + cursor_column as u16;
        let cursor_y = area.y + 1 + cursor_row as u16;
        if cursor_x < area.x + area.width.saturating_sub(1)
            && cursor_y < area.y + area.height.saturating_sub(1)
        {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// 将一条消息拆成标题、折行正文和分隔空行，供消息列表按终端行滚动。
///
/// `width` 来自消息区域宽度；正文额外扣除两列作为左右边框余量。截断/换行只影响显示，不改原消息内容。
fn message_to_lines<'a>(msg: &'a ConversationMessage, width: usize) -> Vec<ListItem<'a>> {
    let header = Line::from(vec![
        Span::styled(
            format!("[{}] ", msg.role.label()),
            Style::default()
                .fg(msg.role.color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(msg.timestamp.clone(), Style::default().fg(Color::DarkGray)),
    ]);

    // 正文用终端显示宽度换行，CJK 字符通常占两列；消息原文继续保存在 content 中。
    let body_width = width.saturating_sub(2);
    let lines = wrap_text(&msg.content, body_width);

    let mut items = vec![ListItem::new(header)];
    for line in lines {
        items.push(ListItem::new(Line::from(Span::raw(format!("  {}", line)))));
    }
    // 每条消息之间保留一个视觉空行，使相邻角色的标题与正文容易区分。
    items.push(ListItem::new(Line::from("")));
    items
}

/// 按空白分词并按终端显示列数折行，不做连字符处理，也不会拆分一个超宽的单词。
///
/// `split_whitespace()` 会合并连续空白；显式的 `\n` 先分段，因此完全空段会保留为空行。
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        let mut current_width = 0usize;
        for word in paragraph.split_whitespace() {
            let word_width = word.width();
            if current.is_empty() {
                current.push_str(word);
                current_width = word_width;
            } else if current_width + 1 + word_width <= width {
                current.push(' ');
                current.push_str(word);
                current_width += 1 + word_width;
            } else {
                lines.push(current.clone());
                current = word.to_owned();
                current_width = word_width;
            }
        }
        if !current.is_empty() || paragraph.is_empty() {
            lines.push(current);
        }
    }
    lines
}

#[cfg(test)]
fn input_display_width_to_cursor(input: &str, cursor: usize) -> usize {
    input
        .get(..cursor)
        .unwrap_or(input)
        .chars()
        .map(|ch| ch.width().unwrap_or(0))
        .sum()
}

fn input_display_text(input: &str) -> String {
    // 每个逻辑输入行加一个纯展示前缀；这是 cursor 位置计算时要同步考虑的列宽。
    let mut output = String::new();
    for (index, line) in input.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
            output.push_str("  ");
        } else {
            output.push_str("> ");
        }
        output.push_str(line);
    }
    output
}

fn status_text(state: &ReplState) -> String {
    // 这行把 adapter 回填的用量和 UI 当前阶段拼成提示；key hint 不执行 cancel/approval。
    let spinner = if state.is_loading {
        format!("{} ", SPINNER_FRAMES[state.spinner_tick])
    } else {
        String::new()
    };

    let key_hint = if state.permission_request_active {
        let mut hint = "[Ctrl-Y allow] [Ctrl-N deny] [Ctrl-C cancel]".to_string();
        if state.permission_request_queue_len > 0 {
            hint.push_str(&format!(
                " [queued permissions: {}]",
                state.permission_request_queue_len
            ));
        }
        hint
    } else if state.is_loading {
        "[Ctrl-C cancel]".to_string()
    } else {
        "[Shift+Enter newline] [Ctrl-C exit]".to_string()
    };

    format!(
        "{}model: {}  in: {}  out: {}  cost: ${:.4}  [/doctor] [/resume] [/history] [/settings] {}",
        spinner,
        state.model_name,
        state.input_tokens,
        state.output_tokens,
        state.session_cost,
        key_hint,
    )
}

fn permission_panel_text(state: &ReplState) -> String {
    // Option 为空代表没有面板；可选字段为空时不占行。所有内容均为预览文本，不构成授权证明。
    let Some(panel) = &state.permission_panel else {
        return String::new();
    };
    let mut lines = vec![
        format!("Tool: {}", panel.tool_name),
        format!("tool_use_id: {}", panel.tool_use_id),
    ];
    if let Some(reason) = panel.reason.as_deref().filter(|value| !value.is_empty()) {
        lines.push(format!("Reason: {reason}"));
    }
    if let Some(path) = panel
        .blocked_path
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Path: {path}"));
    }
    if let Some(input) = panel
        .input_preview
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Input: {input}"));
    }
    if let Some(suggestions) = panel
        .suggestions_preview
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Suggestions: {suggestions}"));
    }
    lines.push("[Ctrl-Y allow] [Ctrl-N deny] [Ctrl-C cancel]".to_string());
    if state.permission_request_queue_len > 0 {
        lines.push(format!("Queued: {}", state.permission_request_queue_len));
    }
    lines.join("\n")
}

fn permission_panel_height(state: &ReplState, area_width: u16, area_height: u16) -> u16 {
    if state.permission_panel.is_none() {
        return 0;
    }
    let content_width = area_width.saturating_sub(2) as usize;
    let content_rows = permission_panel_text(state)
        .lines()
        .map(|line| wrap_text(line, content_width).len().max(1))
        .sum::<usize>();
    // 额外两行用于边框；面板最小 4 行、最大 12 行，避免无限挤压消息区。
    let preferred = (content_rows as u16).saturating_add(2).clamp(4, 12);
    let available = area_height.saturating_sub(5).max(4);
    preferred.min(available)
}

fn input_panel_height(state: &ReplState, area_width: u16, area_height: u16) -> u16 {
    let content_width = area_width.saturating_sub(2) as usize;
    let content_rows = input_cursor_position(&state.input, state.input.len(), content_width).0 + 1;
    // 保留边框所需高度，并把输入区约束在 3 到 8 行；终端更小时再按可用区域缩小。
    let preferred = (content_rows as u16).saturating_add(2).clamp(3, 8);
    let available = area_height.saturating_sub(4).max(3);
    preferred.min(available)
}

fn input_cursor_position(input: &str, cursor: usize, content_width: usize) -> (usize, usize) {
    // cursor 是 UTF-8 byte offset；get() 只接受字符边界，异常偏移时退回整段输入末尾以免切片 panic。
    let before_cursor = input.get(..cursor).unwrap_or(input);
    let mut row = 0usize;
    let mut column = 0usize;

    for (line_index, line) in before_cursor.split('\n').enumerate() {
        if line_index > 0 {
            row += 1;
        }
        column = if line_index == 0 {
            "> ".width()
        } else {
            "  ".width()
        };
        for ch in line.chars() {
            advance_wrapped_position(
                &mut row,
                &mut column,
                ch.width().unwrap_or(0),
                content_width,
            );
        }
    }

    (row, column)
}

fn advance_wrapped_position(
    row: &mut usize,
    column: &mut usize,
    width: usize,
    content_width: usize,
) {
    // 按单个字符的 terminal column width 模拟 ratatui 的换行，返回光标前的行列；宽度为 0 的组合字符不占列。
    if width == 0 {
        return;
    }
    if content_width == 0 {
        *column += width;
        return;
    }
    if *column > 0 && column.saturating_add(width) > content_width {
        *row += 1;
        *column = 0;
    }
    *column += width;
    if *column >= content_width {
        *row += 1;
        *column = 0;
    }
}

fn chrono_now() -> String {
    // 生成 Unix epoch 当天的 UTC 时分秒展示串，不含日期/时区；系统时间早于 epoch 时按 0 处理。
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[cfg(test)]
// 这些局部用例覆盖输入状态、Unicode 布局和 widget 文本，不验证 entrypoints 的异步 SDK/授权接线。
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render_repl_to_text(state: &mut ReplState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| ReplScreen::draw(frame, state))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .fold(String::new(), |mut output, cell| {
                output.push_str(cell.symbol());
                output
            })
    }

    #[test]
    fn cursor_display_width_handles_cjk_input() {
        assert_eq!(input_display_width_to_cursor("abc", 3), 3);
        assert_eq!(input_display_width_to_cursor("你", "你".len()), 2);
        assert_eq!(input_display_width_to_cursor("a你b", "a你".len()), 3);
    }

    #[test]
    fn wrap_text_uses_terminal_display_width() {
        assert_eq!(wrap_text("你好 世界", 5), vec!["你好", "世界"]);
        assert_eq!(wrap_text("hello world", 8), vec!["hello", "world"]);
    }

    #[test]
    fn newline_keys_insert_multiline_prompt_without_submitting() {
        let mut state = ReplState::default();
        state.input = "first".to_string();
        state.cursor = state.input.len();

        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            None
        );
        state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));

        assert_eq!(state.input, "first\ns");
        assert_eq!(state.cursor, state.input.len());
    }

    #[test]
    fn ctrl_j_inserts_newline() {
        let mut state = ReplState::default();
        state.input = "first".to_string();
        state.cursor = state.input.len();

        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));

        assert_eq!(state.input, "first\n");
        assert_eq!(state.cursor, state.input.len());
    }

    #[test]
    fn enter_submits_multiline_prompt() {
        let mut state = ReplState::default();
        state.input = "first\nsecond".to_string();
        state.cursor = state.input.len();

        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(ReplEvent::SubmitPrompt("first\nsecond".to_string()))
        );
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].content, "first\nsecond");
        assert!(state.input.is_empty());
    }

    #[test]
    fn submitted_prompt_history_dedupes_and_keeps_latest_last() {
        let mut state = ReplState::default();
        for prompt in ["first", "second", "first"] {
            state.input = prompt.to_string();
            state.cursor = state.input.len();
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }

        assert_eq!(state.history, vec!["second", "first"]);
    }

    #[test]
    fn persisted_history_entries_feed_up_down_recall_newest_first() {
        let mut state = ReplState::default();
        state.load_persisted_history(&[
            HistoryEntry::new("newest prompt".to_string(), "2".to_string()),
            HistoryEntry::new("older prompt".to_string(), "1".to_string()),
        ]);

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.input, "newest prompt");

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.input, "older prompt");

        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.input, "newest prompt");
    }

    #[test]
    fn restore_draft_replaces_input_and_resets_history_navigation() {
        let mut state = ReplState::default();
        state.history = vec!["old".to_string()];
        state.input = "draft".to_string();
        state.cursor = state.input.len();
        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        state.restore_draft("restored prompt".to_string());

        assert_eq!(state.input, "restored prompt");
        assert_eq!(state.cursor, state.input.len());
        assert_eq!(state.history_idx, None);
        assert!(state.history_draft.is_empty());
    }

    #[test]
    fn input_cursor_position_handles_multiline_and_wide_chars() {
        assert_eq!(input_cursor_position("hi", 2, 20), (0, 4));
        assert_eq!(input_cursor_position("hi\n你", "hi\n你".len(), 20), (1, 4));
    }

    #[test]
    fn input_panel_height_expands_for_multiline_prompt() {
        let mut state = ReplState::default();
        state.input = "one\ntwo\nthree".to_string();
        state.cursor = state.input.len();

        assert_eq!(input_panel_height(&state, 80, 24), 5);
    }

    #[test]
    fn status_text_reports_queued_permission_requests() {
        let mut state = ReplState::default();
        state.is_loading = true;
        state.permission_request_active = true;
        state.permission_request_queue_len = 2;

        let status = status_text(&state);

        assert!(status.contains("[Ctrl-Y allow]"));
        assert!(status.contains("queued permissions: 2"));
    }

    #[test]
    fn permission_panel_text_renders_structured_request_context() {
        let mut state = ReplState::default();
        state.set_permission_request(
            ReplPermissionPanel {
                tool_name: "Bash".to_string(),
                tool_use_id: "toolu_bash".to_string(),
                reason: Some("Command requires approval in ask mode.".to_string()),
                blocked_path: Some("src/main.rs".to_string()),
                input_preview: Some("{\"command\":\"git status\"}".to_string()),
                suggestions_preview: Some("[\"allow once\"]".to_string()),
            },
            2,
        );

        let text = permission_panel_text(&state);

        assert!(text.contains("Tool: Bash"));
        assert!(text.contains("tool_use_id: toolu_bash"));
        assert!(text.contains("Reason: Command requires approval"));
        assert!(text.contains("Path: src/main.rs"));
        assert!(text.contains("Input: {\"command\":\"git status\"}"));
        assert!(text.contains("Suggestions: [\"allow once\"]"));
        assert!(text.contains("Queued: 2"));
        assert!(text.contains("Ctrl-Y allow"));
        assert!(permission_panel_height(&state, 80, 24) >= 4);
    }

    #[test]
    fn repl_render_keeps_approval_status_and_prompt_visible() {
        let mut state = ReplState::default();
        state.is_loading = true;
        state.model_name = "llama3.1".to_string();
        state.input = "follow up".to_string();
        state.cursor = state.input.len();
        state.push_message(MessageRole::Assistant, "Previous answer".to_string());
        state.set_permission_request(
            ReplPermissionPanel {
                tool_name: "Bash".to_string(),
                tool_use_id: "toolu_bash".to_string(),
                reason: Some("Command requires approval in ask mode.".to_string()),
                blocked_path: Some("src/main.rs".to_string()),
                input_preview: Some("{\"command\":\"git status\"}".to_string()),
                suggestions_preview: Some("[\"allow once\"]".to_string()),
            },
            1,
        );

        let rendered = render_repl_to_text(&mut state, 120, 24);

        assert!(rendered.contains("Kiana"));
        assert!(rendered.contains("Previous answer"));
        assert!(rendered.contains("Approval"));
        assert!(rendered.contains("Tool: Bash"));
        assert!(rendered.contains("Path: src/main.rs"));
        assert!(rendered.contains("Queued: 1"));
        assert!(rendered.contains("[Ctrl-Y allow]"));
        assert!(rendered.contains("Prompt"));
        assert!(rendered.contains("> follow up"));
    }

    #[test]
    fn clear_permission_request_removes_panel_state() {
        let mut state = ReplState::default();
        state.set_permission_request(
            ReplPermissionPanel {
                tool_name: "Read".to_string(),
                tool_use_id: "toolu_read".to_string(),
                reason: None,
                blocked_path: None,
                input_preview: None,
                suggestions_preview: None,
            },
            0,
        );

        state.clear_permission_request();

        assert!(!state.permission_request_active);
        assert_eq!(state.permission_request_queue_len, 0);
        assert!(state.permission_panel.is_none());
        assert_eq!(permission_panel_height(&state, 80, 24), 0);
    }
}
