//! TUI 的 prompt 历史选择器，以及搜索词的独立使用历史。
//!
//! 进入链路：REPL 的 `/history` → `App` 发出 `LoadPromptHistory` → entrypoints adapter 从
//! prompt-history JSONL 读取 `HistoryEntry` → `load_entries()` 清理并保存到本地 state。
//! 用户按 Enter 选中一项后，本文件只发出 `RestorePrompt`；`App` 再把文本放回 REPL 草稿，
//! 不会在这里向模型提交请求。
//!
//! 这里还有第二种“历史”：`SearchHistoryManager` 保存用户搜索过的查询词及其次数/时间，
//! 与可恢复的 prompt 内容不是同一份数据，也不会提供 session 恢复能力。该模块属于已 park 的
//! 旧 TUI，不是 DaemonHost 主执行路径。
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::app::AppScreen;
use crate::search_history::{SearchHistoryEntry, SearchHistoryManager};

/// 一条可放回输入框的旧 prompt；实际文件读写由 entrypoints adapter 负责。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// 原始 prompt 文本；serde default 让缺少该字段的旧 JSON 行按空字符串读取。
    #[serde(default)]
    pub prompt: String,
    /// adapter 提供的展示时间文本，不在本 screen 中解析成日期。
    #[serde(default)]
    pub timestamp: String,
}

impl HistoryEntry {
    /// 将 adapter 读出的 prompt 和展示时间组合成一条 UI 数据。
    pub fn new(prompt: String, timestamp: String) -> Self {
        Self { prompt, timestamp }
    }
}

/// 历史 screen 向 App 汇报的结果；它描述用户意图，不直接执行应用操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEvent {
    /// 用户取消浏览，要求 App 返回指定 screen。
    SwitchScreen(AppScreen),
    /// 用户选择的文本交给 App 恢复到 REPL 草稿。
    RestorePrompt(String),
}

/// 历史页交互状态；列表索引始终指向当前过滤结果，而不是原始 entries 的索引。
#[derive(Debug)]
pub struct HistoryState {
    /// entrypoints adapter 加载并按 prompt 去重后的历史，通常新记录排在前面。
    pub entries: Vec<HistoryEntry>,
    /// 当前过滤列表中的选中位置；筛选变化时必须重新裁剪到有效范围。
    pub selected: usize,
    /// Ratatui 用于在列表绘制时同步高亮/视口的状态。
    pub list_state: ListState,
    /// adapter 加载主 prompt 历史时的展示标记。
    pub loading: bool,
    /// 当前输入的过滤词；非空时会即时过滤 prompt 和 timestamp。
    pub search_query: String,
    /// 是否正处于接收搜索字符的模式；为空查询也可以是 active。
    pub search_active: bool,

    /// 搜索词统计管理器；Arc 提供共享所有权，Mutex 保护每次同步读写。
    /// 当前静态调用点都在本 screen 内；如果初始化失败为 None，prompt 历史仍可用，只是不保存搜索词。
    pub search_history: Option<Arc<Mutex<SearchHistoryManager>>>,

    /// 是否覆盖显示搜索词历史面板；当前 Ctrl+R 分支只有在搜索输入模式中可达。
    pub showing_search_history: bool,

    /// 搜索词历史面板中的选中位置，与 prompt 列表的 selected 分开维护。
    pub search_history_selected: usize,

    /// 搜索词历史列表专用的 Ratatui 高亮/视口状态。
    pub search_history_list_state: ListState,
}

impl Default for HistoryState {
    fn default() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let mut search_history_list_state = ListState::default();
        search_history_list_state.select(Some(0));

        // 搜索词统计的存储路径可能无法创建或读取；这里吞掉初始化错误，让主 prompt 历史浏览仍可继续。
        // None 会让相关记录/显示函数退化为空结果，不会影响恢复 prompt 的流程。
        let search_history = SearchHistoryManager::with_default_path()
            .ok()
            .map(|manager| Arc::new(Mutex::new(manager)));

        Self {
            entries: Vec::new(),
            selected: 0,
            list_state,
            loading: false,
            search_query: String::new(),
            search_active: false,
            search_history,
            showing_search_history: false,
            search_history_selected: 0,
            search_history_list_state,
        }
    }
}

impl HistoryState {
    /// 按“搜索词历史面板 → 搜索输入 → 普通列表快捷键”的优先级解释一次按键。
    ///
    /// 【调用者】`App::handle_key()` 仅在 History screen 活动时调用，并解释返回的 `HistoryEvent`。
    /// 【输出】Esc 可请求离开页面，Enter 可请求恢复当前 prompt；其他按键只更新本地选择/查询状态。
    /// 搜索过滤使用 ASCII 小写比较，因此它对英文字母不区分大小写，但不是完整的 Unicode 大小写折叠。
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<HistoryEvent> {
        // 显示搜索历史时的特殊处理
        if self.showing_search_history {
            return self.handle_search_history_key(key);
        }

        if self.search_active {
            match key.code {
                KeyCode::Esc => {
                    self.clear_search();
                    return None;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.normalize_selection();
                    return None;
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.clear_search();
                    return None;
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl+R 显示搜索历史
                    self.showing_search_history = true;
                    self.search_history_selected = 0;
                    self.search_history_list_state.select(Some(0));
                    return None;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search_query.push(c);
                    self.normalize_selection();
                    return None;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => return Some(HistoryEvent::SwitchScreen(AppScreen::Repl)),
            KeyCode::Char('/') => {
                self.search_active = true;
            }
            KeyCode::Enter => {
                if let Some(entry) = self.selected_entry() {
                    // 只记录非空过滤词；选中 prompt 本身不会被复制到搜索词历史。
                    // Mutex 若已 poison，unwrap 会 panic；这里没有把该状态转换成可显示错误。
                    if !self.search_query.is_empty() {
                        if let Some(manager) = &self.search_history {
                            manager
                                .lock()
                                .unwrap()
                                .record_search(&self.search_query, "prompt_history");
                        }
                    }
                    return Some(HistoryEvent::RestorePrompt(entry.prompt.clone()));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.filtered_len() > 0 && self.selected > 0 {
                    self.selected -= 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.filtered_len() {
                    self.selected += 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Home => {
                if self.filtered_len() > 0 {
                    self.selected = 0;
                    self.list_state.select(Some(0));
                }
            }
            KeyCode::End => {
                let len = self.filtered_len();
                if len > 0 {
                    let last = len - 1;
                    self.selected = last;
                    self.list_state.select(Some(last));
                }
            }
            _ => {}
        }
        None
    }

    /// 接收 adapter 读出的 prompt 历史，先去空白/去重，再修正当前列表选择。
    ///
    /// 若新列表比旧列表短，`normalize_selection()` 会把过期索引移回最后一项；空列表则取消高亮。
    pub fn load_entries(&mut self, entries: Vec<HistoryEntry>) {
        self.entries = normalize_history_entries(entries);
        self.normalize_selection();
    }

    /// 把过滤列表中的位置映射回原始 entries；没有选中项时返回 None，Enter 因而不会恢复内容。
    pub fn selected_entry(&self) -> Option<&HistoryEntry> {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.entries.get(*index))
    }

    /// App 切入历史 screen 前清掉旧查询，避免上一次搜索意外限制新一轮浏览。
    pub fn reset_search(&mut self) {
        self.clear_search();
    }

    fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_active = false;
        self.normalize_selection();
    }

    fn normalize_selection(&mut self) {
        let len = self.filtered_len();
        if len == 0 {
            self.selected = 0;
            self.list_state.select(None);
        } else {
            self.selected = self.selected.min(len - 1);
            self.list_state.select(Some(self.selected));
        }
    }

    fn filtered_len(&self) -> usize {
        self.filtered_indices().len()
    }

    fn filtered_entries(&self) -> Vec<&HistoryEntry> {
        self.filtered_indices()
            .into_iter()
            .filter_map(|index| self.entries.get(index))
            .collect()
    }

    fn filtered_indices(&self) -> Vec<usize> {
        // 保存原始下标而不是复制整条记录；显示与 Enter 都通过同一映射取得当前结果。
        let query = self.search_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..self.entries.len()).collect();
        }
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                history_entry_matches_query(entry, &query).then_some(index)
            })
            .collect()
    }

    /// 处理覆盖层中的搜索词选择；Enter 将查询词放回搜索框并再次计入使用次数。
    ///
    /// 每次取历史列表时都在锁内生成拥有所有权的快照，锁释放后才修改 HistoryState，避免同时借用
    /// 管理器与界面字段。Mutex 中毒仍会由 unwrap 转成 panic。
    fn handle_search_history_key(&mut self, key: KeyEvent) -> Option<HistoryEvent> {
        match key.code {
            KeyCode::Esc => {
                self.showing_search_history = false;
                self.search_history_selected = 0;
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.search_history_selected > 0 {
                    self.search_history_selected -= 1;
                    self.search_history_list_state
                        .select(Some(self.search_history_selected));
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(manager) = &self.search_history {
                    let history = manager
                        .lock()
                        .unwrap()
                        .get_sorted_history(Some("prompt_history"));
                    if self.search_history_selected + 1 < history.len() {
                        self.search_history_selected += 1;
                        self.search_history_list_state
                            .select(Some(self.search_history_selected));
                    }
                }
                None
            }
            KeyCode::Enter => {
                if let Some(manager) = &self.search_history {
                    let history = manager
                        .lock()
                        .unwrap()
                        .get_sorted_history(Some("prompt_history"));
                    if let Some(entry) = history.get(self.search_history_selected) {
                        // 选中搜索词只恢复过滤条件，不会直接选择或恢复某条 prompt。
                        self.search_query = entry.query.clone();
                        self.showing_search_history = false;

                        // 再次使用历史查询会更新 last_used/use_count，让 frecency 排序反映本次访问。
                        manager
                            .lock()
                            .unwrap()
                            .record_search(&entry.query, "prompt_history");

                        // 新查询可能排除当前选项，统一把选择裁剪到新的过滤结果范围。
                        self.normalize_selection();
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 返回当前 screen 所属的 `prompt_history` 查询词历史；未能创建 manager 时按空列表显示。
    fn get_search_history_entries(&self) -> Vec<SearchHistoryEntry> {
        if let Some(manager) = &self.search_history {
            manager
                .lock()
                .unwrap()
                .get_sorted_history(Some("prompt_history"))
        } else {
            Vec::new()
        }
    }
}

/// 清理 adapter 读出的 prompt 列表：去掉首尾空白、空内容和重复文本。
///
/// HashSet 只用于判断“是否见过”，输出仍沿用输入顺序；因此 adapter 若按新到旧提供数据，保留的是最新重复项。
pub fn normalize_history_entries(entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter_map(|mut entry| {
            entry.prompt = entry.prompt.trim().to_string();
            if entry.prompt.is_empty() || !seen.insert(entry.prompt.clone()) {
                return None;
            }
            Some(entry)
        })
        .collect()
}

fn history_entry_matches_query(entry: &HistoryEntry, query: &str) -> bool {
    entry.prompt.to_ascii_lowercase().contains(query)
        || entry.timestamp.to_ascii_lowercase().contains(query)
}

pub struct HistoryScreen;

impl HistoryScreen {
    /// 绘制主 prompt 列表、可选的搜索词历史覆盖区域和按键说明；不读写历史文件。
    pub fn draw(frame: &mut Frame, state: &mut HistoryState) {
        let area = frame.area();

        // 如果显示搜索历史，使用不同的布局
        if state.showing_search_history {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(10),
                    Constraint::Length(3),
                ])
                .split(area);

            Self::draw_list(frame, state, chunks[0]);
            Self::draw_search_history(frame, state, chunks[1]);
            Self::draw_help(frame, chunks[2]);
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(5), Constraint::Length(3)])
                .split(area);

            Self::draw_list(frame, state, chunks[0]);
            Self::draw_help(frame, chunks[1]);
        }
    }

    fn draw_list(frame: &mut Frame, state: &mut HistoryState, area: Rect) {
        if state.loading {
            let para = Paragraph::new("Loading prompt history...")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL).title(" History "));
            frame.render_widget(para, area);
            return;
        }

        if state.entries.is_empty() {
            let para = Paragraph::new("No prompt history found.\nPress Esc to return.")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL).title(" History "));
            frame.render_widget(para, area);
            return;
        }

        // `selected` 已归一化为过滤结果下标，渲染列表也按相同顺序构造，二者才能正确对齐。
        let filtered_entries = state.filtered_entries();
        if filtered_entries.is_empty() {
            let para = Paragraph::new(format!(
                "No prompts match \"{}\".\nPress Esc to clear search.",
                state.search_query
            ))
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" History "));
            frame.render_widget(para, area);
            return;
        }

        let title = if state.search_active || !state.search_query.is_empty() {
            format!(" Prompt History /{} ", state.search_query)
        } else {
            " Prompt History ".to_string()
        };

        let items: Vec<ListItem> = filtered_entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let is_selected = idx == state.selected;
                let prefix = if is_selected { "> " } else { "  " };

                let line = Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default().fg(if is_selected {
                            Color::Cyan
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled(
                        prompt_preview(&entry.prompt),
                        Style::default()
                            .fg(if is_selected {
                                Color::White
                            } else {
                                Color::Gray
                            })
                            .add_modifier(if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                ]);

                let info = Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        entry.timestamp.clone(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);

                ListItem::new(vec![line, info])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut state.list_state);
    }

    fn draw_help(frame: &mut Frame, area: Rect) {
        let help = Paragraph::new(
            "Up/Down or j/k: navigate  /: search  Ctrl+R: history  Enter: restore  Esc: cancel",
        )
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(help, area);
    }

    /// 绘制搜索词使用历史；这块数据来自 SearchHistoryManager，而非 prompt-history 文件。
    fn draw_search_history(frame: &mut Frame, state: &mut HistoryState, area: Rect) {
        let history_entries = state.get_search_history_entries();

        if history_entries.is_empty() {
            let para = Paragraph::new("No search history yet.\nPress Esc to cancel.")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Search History ")
                        .title_style(Style::default().fg(Color::Yellow)),
                );
            frame.render_widget(para, area);
            return;
        }

        let items: Vec<ListItem> = history_entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let is_selected = idx == state.search_history_selected;
                let prefix = if is_selected { "> " } else { "  " };

                let query_line = Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default().fg(if is_selected {
                            Color::Yellow
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled(
                        &entry.query,
                        Style::default()
                            .fg(if is_selected {
                                Color::White
                            } else {
                                Color::Gray
                            })
                            .add_modifier(if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                ]);

                let info_line = Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        format!("{} times, {}", entry.use_count, entry.last_used_humanized()),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);

                ListItem::new(vec![query_line, info_line])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Search History (Ctrl+R) ")
                    .title_style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut state.search_history_list_state);
    }
}

fn prompt_preview(prompt: &str) -> String {
    let flattened = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = flattened.chars().take(96).collect::<String>();
    if flattened.chars().count() > 96 {
        preview.push_str("...");
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn sample_state() -> HistoryState {
        let mut state = HistoryState::default();
        state.load_entries(vec![
            HistoryEntry::new("explain the rust workspace".to_string(), "100".to_string()),
            HistoryEntry::new("write focused TUI tests".to_string(), "90".to_string()),
            HistoryEntry::new("explain the rust workspace".to_string(), "80".to_string()),
        ]);
        state
    }

    fn render_history_to_text(state: &mut HistoryState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| HistoryScreen::draw(frame, state))
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
    fn load_entries_dedupes_prompts_and_keeps_newest_first() {
        let state = sample_state();

        let prompts = state
            .entries
            .iter()
            .map(|entry| entry.prompt.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            prompts,
            vec!["explain the rust workspace", "write focused TUI tests"]
        );
        assert_eq!(state.selected_entry().unwrap().timestamp, "100");
    }

    #[test]
    fn search_filters_history_by_prompt_and_enter_restores_selected_prompt() {
        let mut state = sample_state();

        state.handle_key(key(KeyCode::Char('/')));
        for c in "tests".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }

        assert!(state.search_active);
        assert_eq!(state.search_query, "tests");
        assert_eq!(
            state.handle_key(key(KeyCode::Enter)),
            Some(HistoryEvent::RestorePrompt(
                "write focused TUI tests".to_string()
            ))
        );
    }

    #[test]
    fn escape_clears_active_search_before_leaving_history_screen() {
        let mut state = sample_state();

        state.handle_key(key(KeyCode::Char('/')));
        state.handle_key(key(KeyCode::Char('r')));

        assert_eq!(state.handle_key(key(KeyCode::Esc)), None);
        assert!(!state.search_active);
        assert!(state.search_query.is_empty());

        assert_eq!(
            state.handle_key(key(KeyCode::Esc)),
            Some(HistoryEvent::SwitchScreen(AppScreen::Repl))
        );
    }

    #[test]
    fn ctrl_u_clears_search_query() {
        let mut state = sample_state();

        state.handle_key(key(KeyCode::Char('/')));
        state.handle_key(key(KeyCode::Char('t')));
        state.handle_key(ctrl_key(KeyCode::Char('u')));

        assert!(!state.search_active);
        assert!(state.search_query.is_empty());
        assert_eq!(state.filtered_len(), 2);
    }

    #[test]
    fn history_render_shows_search_result_selection_and_help() {
        let mut state = sample_state();
        state.handle_key(key(KeyCode::Char('/')));
        for c in "tests".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }

        let rendered = render_history_to_text(&mut state, 80, 16);

        assert!(rendered.contains("Prompt History /tests"));
        assert!(rendered.contains("write focused TUI tests"));
        assert!(rendered.contains("90"));
        assert!(rendered.contains("Enter: restore"));
        assert!(!rendered.contains("explain the rust workspace"));
    }
}
