//! 旧 TUI 的 session 恢复选择器，只负责过滤、选择和展示会话摘要。
//!
//! 进入时由 `App` 发出 `LoadResumeSessions`，entrypoints adapter 调用 SDK 列表接口；有 cwd 元数据时按
//! 当前工作目录过滤，若整批 session 都没有 cwd 元数据则保留 SDK 返回的全部条目，再转换为 `SessionEntry`。
//! 该 screen 不读取日志文件，也不执行恢复；Enter 只返回
//! `ResumeSession(session_id)`，随后 App/adapter 再异步读取该 session 的详情和消息。
//!
//! 当前渲染顺序沿用 adapter 传入的顺序；本模块没有按 mtime 排序。搜索匹配标题、session ID 和时间文本。
//! 这个兼容 TUI 已 park，不属于 DaemonHost 主执行路径。
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::AppScreen;

/// adapter 为选择器准备的一条会话摘要，不包含完整消息或恢复 checkpoint。
#[derive(Debug, Clone)]
pub struct SessionEntry {
    /// 供 SDK 后续查询会话详情的唯一会话标识，不是列表位置。
    pub session_id: String,
    /// 外层提供的会话标题；screen 不自行从消息推导标题。
    pub title: String,
    /// 外层提供的更新时间展示文本；此模块只用于显示和搜索。
    pub timestamp: String,
    /// 外层统计的消息数，仅作摘要展示，不代表可恢复性校验。
    pub message_count: usize,
}

/// Resume screen 交给 App 的导航/恢复意图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeEvent {
    /// 用户取消浏览时切回指定页面。
    SwitchScreen(AppScreen),
    /// 把所选 session ID 交给 App 和 adapter 继续加载完整会话。
    ResumeSession(String),
}

/// 会话选择页状态；列表只保存摘要，搜索结果由原列表下标映射得到。
#[derive(Debug)]
pub struct ResumeState {
    /// adapter 已加载且按工作目录过滤后的会话摘要，顺序由 adapter/SDK 输入决定。
    pub sessions: Vec<SessionEntry>,
    /// 当前过滤结果中的索引；不是 session ID。
    pub selected: usize,
    /// Ratatui 列表绘制用的选中项与视口状态。
    pub list_state: ListState,
    /// 外层加载器正在读取会话目录/列表时展示 loading 提示。
    pub loading: bool,
    /// 即时过滤条件，会匹配标题、ID 和 timestamp。
    pub search_query: String,
    /// 当前是否接收搜索输入；清空查询后仍可能退出搜索模式。
    pub search_active: bool,
}

impl Default for ResumeState {
    fn default() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            sessions: Vec::new(),
            selected: 0,
            list_state,
            loading: false,
            search_query: String::new(),
            search_active: false,
        }
    }
}

impl ResumeState {
    /// 解释一次按键：先处理活动搜索框，再处理取消、筛选列表导航和确认恢复。
    ///
    /// 【调用者】`App::handle_key()` 仅在 ResumeConversation screen 活动时调用。
    /// 【输出】Enter 返回被选会话的 ID，App 把它交给异步 adapter；Esc 在搜索模式下第一次清空查询，
    /// 再按一次才离开 screen。空过滤结果没有可选 session，因此 Enter 返回 None。
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ResumeEvent> {
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
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search_query.push(c);
                    self.normalize_selection();
                    return None;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                // 退出选择器只切回 REPL，不会清除、取消或恢复任何 session。
                return Some(ResumeEvent::SwitchScreen(AppScreen::Repl));
            }
            KeyCode::Char('/') => {
                self.search_active = true;
            }
            KeyCode::Enter => {
                if let Some(entry) = self.selected_session() {
                    return Some(ResumeEvent::ResumeSession(entry.session_id.clone()));
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

    /// 用 adapter 返回的新摘要列表替换旧数据，并修复可能已经越界的选择索引。
    pub fn load_sessions(&mut self, sessions: Vec<SessionEntry>) {
        self.sessions = sessions;
        self.normalize_selection();
    }

    /// 将过滤结果中的 selected 映射回原始摘要；没有匹配项时阻止产生恢复事件。
    pub fn selected_session(&self) -> Option<&SessionEntry> {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.sessions.get(*index))
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

    fn filtered_sessions(&self) -> Vec<&SessionEntry> {
        self.filtered_indices()
            .into_iter()
            .filter_map(|index| self.sessions.get(index))
            .collect()
    }

    fn filtered_indices(&self) -> Vec<usize> {
        // 只保存命中项在原列表中的位置，使列表渲染和 session ID 查找共用同一排序。
        let query = self.search_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..self.sessions.len()).collect();
        }
        self.sessions
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| session_matches_query(entry, &query).then_some(index))
            .collect()
    }
}

fn session_matches_query(entry: &SessionEntry, query: &str) -> bool {
    entry.title.to_ascii_lowercase().contains(query)
        || entry.session_id.to_ascii_lowercase().contains(query)
        || entry.timestamp.to_ascii_lowercase().contains(query)
}

pub struct ResumeScreen;

impl ResumeScreen {
    /// 把加载提示、空结果、搜索结果或会话摘要绘制到当前 frame；不触碰 SDK。
    pub fn draw(frame: &mut Frame, state: &mut ResumeState) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        Self::draw_list(frame, state, chunks[0]);
        Self::draw_help(frame, chunks[1]);
    }

    fn draw_list(frame: &mut Frame, state: &mut ResumeState, area: Rect) {
        if state.loading {
            let para = Paragraph::new("Loading conversations…")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL).title(" Resume "));
            frame.render_widget(para, area);
            return;
        }

        if state.sessions.is_empty() {
            let para = Paragraph::new("No conversations found to resume.\nPress Esc to return.")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL).title(" Resume "));
            frame.render_widget(para, area);
            return;
        }

        // state.selected 是过滤后的位置；filtered_sessions 也按该位置构建，保证高亮与 ID 对齐。
        let filtered_sessions = state.filtered_sessions();
        if filtered_sessions.is_empty() {
            let para = Paragraph::new(format!(
                "No conversations match \"{}\".\nPress Esc to clear search.",
                state.search_query
            ))
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" Resume "));
            frame.render_widget(para, area);
            return;
        }

        let title = if state.search_active || !state.search_query.is_empty() {
            format!(" Resume Conversation /{} ", state.search_query)
        } else {
            " Resume Conversation ".to_string()
        };

        let items: Vec<ListItem> = filtered_sessions
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let is_selected = idx == state.selected;
                let prefix = if is_selected { "▶ " } else { "  " };

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
                        entry.title.clone(),
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
                        format!(
                            "{} · {} messages · {}",
                            entry.timestamp, entry.message_count, entry.session_id
                        ),
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
        let help = Paragraph::new("↑/↓ or j/k: navigate  /: search  Enter: resume  Esc: cancel")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(help, area);
    }
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

    fn sample_state() -> ResumeState {
        let mut state = ResumeState::default();
        state.load_sessions(vec![
            SessionEntry {
                session_id: "session-alpha".to_string(),
                title: "Refactor remote bridge".to_string(),
                timestamp: "2026-06-14".to_string(),
                message_count: 12,
            },
            SessionEntry {
                session_id: "session-beta".to_string(),
                title: "Polish TUI resume".to_string(),
                timestamp: "2026-06-15".to_string(),
                message_count: 4,
            },
        ]);
        state
    }

    fn render_resume_to_text(state: &mut ResumeState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| ResumeScreen::draw(frame, state))
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
    fn search_filters_resume_sessions_by_title_and_session_id() {
        let mut state = sample_state();

        state.handle_key(key(KeyCode::Char('/')));
        for c in "tui".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }

        assert!(state.search_active);
        assert_eq!(state.search_query, "tui");
        assert_eq!(
            state
                .selected_session()
                .map(|entry| entry.session_id.as_str()),
            Some("session-beta")
        );

        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        state.handle_key(key(KeyCode::Backspace));
        for c in "alpha".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }

        assert_eq!(
            state
                .selected_session()
                .map(|entry| entry.session_id.as_str()),
            Some("session-alpha")
        );
    }

    #[test]
    fn search_no_match_clears_selection_and_enter_does_not_resume() {
        let mut state = sample_state();

        state.handle_key(key(KeyCode::Char('/')));
        for c in "missing".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }

        assert_eq!(
            state.selected_session().map(|entry| &entry.session_id),
            None
        );
        assert_eq!(state.list_state.selected(), None);
        assert_eq!(state.handle_key(key(KeyCode::Enter)), None);
    }

    #[test]
    fn escape_clears_active_search_before_leaving_resume_screen() {
        let mut state = sample_state();

        state.handle_key(key(KeyCode::Char('/')));
        state.handle_key(key(KeyCode::Char('t')));

        assert_eq!(state.handle_key(key(KeyCode::Esc)), None);
        assert!(!state.search_active);
        assert!(state.search_query.is_empty());
        assert_eq!(
            state
                .selected_session()
                .map(|entry| entry.session_id.as_str()),
            Some("session-alpha")
        );

        assert_eq!(
            state.handle_key(key(KeyCode::Esc)),
            Some(ResumeEvent::SwitchScreen(AppScreen::Repl))
        );
    }

    #[test]
    fn resume_render_shows_search_result_selection_and_help() {
        let mut state = sample_state();
        state.handle_key(key(KeyCode::Char('/')));
        for c in "tui".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }

        let rendered = render_resume_to_text(&mut state, 80, 16);

        assert!(rendered.contains("Resume Conversation /tui"));
        assert!(rendered.contains("Polish TUI resume"));
        assert!(rendered.contains("session-beta"));
        assert!(rendered.contains("2026-06-15"));
        assert!(rendered.contains("4 messages"));
        assert!(rendered.contains("Enter: resume"));
        assert!(!rendered.contains("Refactor remote bridge"));
    }

    #[test]
    fn ctrl_u_clears_search_query() {
        let mut state = sample_state();

        state.handle_key(key(KeyCode::Char('/')));
        state.handle_key(key(KeyCode::Char('b')));
        state.handle_key(ctrl_key(KeyCode::Char('u')));

        assert!(!state.search_active);
        assert!(state.search_query.is_empty());
        assert_eq!(state.filtered_len(), 2);
    }
}
