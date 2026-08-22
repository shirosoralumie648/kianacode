/// Searchable prompt history picker for the TUI.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub timestamp: String,
}

impl HistoryEntry {
    pub fn new(prompt: String, timestamp: String) -> Self {
        Self { prompt, timestamp }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEvent {
    SwitchScreen(AppScreen),
    RestorePrompt(String),
}

#[derive(Debug)]
pub struct HistoryState {
    pub entries: Vec<HistoryEntry>,
    pub selected: usize,
    pub list_state: ListState,
    pub loading: bool,
    pub search_query: String,
    pub search_active: bool,

    /// 搜索历史管理器
    pub search_history: Option<Arc<Mutex<SearchHistoryManager>>>,

    /// 是否显示搜索历史（Ctrl+R 触发）
    pub showing_search_history: bool,

    /// 搜索历史选中索引
    pub search_history_selected: usize,

    /// 搜索历史列表状态
    pub search_history_list_state: ListState,
}

impl Default for HistoryState {
    fn default() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        let mut search_history_list_state = ListState::default();
        search_history_list_state.select(Some(0));

        // 尝试初始化搜索历史管理器
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
                    // 记录到搜索历史
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

    pub fn load_entries(&mut self, entries: Vec<HistoryEntry>) {
        self.entries = normalize_history_entries(entries);
        self.normalize_selection();
    }

    pub fn selected_entry(&self) -> Option<&HistoryEntry> {
        self.filtered_indices()
            .get(self.selected)
            .and_then(|index| self.entries.get(*index))
    }

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

    /// 处理搜索历史键盘事件
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
                        // 填充到搜索框
                        self.search_query = entry.query.clone();
                        self.showing_search_history = false;

                        // 记录这次使用
                        manager
                            .lock()
                            .unwrap()
                            .record_search(&entry.query, "prompt_history");

                        // 重新过滤结果
                        self.normalize_selection();
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 获取搜索历史条目用于显示
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

    /// 绘制搜索历史列表
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
