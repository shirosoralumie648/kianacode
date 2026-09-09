use super::{LogBuffer, LogEntry, LogLevel, LogParser};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::collections::HashSet;

pub struct LogViewer {
    buffer: LogBuffer,
    scroll_offset: usize,
    auto_scroll: bool,
    level_filter: HashSet<LogLevel>,
    search_query: Option<String>,
    wrap_lines: bool,
    show_timestamps: bool,
    paused: bool,
}

impl LogViewer {
    pub fn new(max_capacity: usize) -> Self {
        let mut level_filter = HashSet::new();
        // 默认显示所有级别
        level_filter.insert(LogLevel::Trace);
        level_filter.insert(LogLevel::Debug);
        level_filter.insert(LogLevel::Info);
        level_filter.insert(LogLevel::Warn);
        level_filter.insert(LogLevel::Error);
        level_filter.insert(LogLevel::Fatal);

        Self {
            buffer: LogBuffer::new(max_capacity),
            scroll_offset: 0,
            auto_scroll: true,
            level_filter,
            search_query: None,
            wrap_lines: true,
            show_timestamps: true,
            paused: false,
        }
    }

    pub fn add_line(&mut self, line: &str) {
        if !self.paused {
            let entry = LogParser::parse(line);
            self.buffer.push(entry);

            if self.auto_scroll {
                // 自动滚动到底部
                self.scroll_to_bottom();
            }
        }
    }

    pub fn toggle_auto_scroll(&mut self) {
        self.auto_scroll = !self.auto_scroll;
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn toggle_level(&mut self, level: LogLevel) {
        if self.level_filter.contains(&level) {
            self.level_filter.remove(&level);
        } else {
            self.level_filter.insert(level);
        }
    }

    pub fn set_search(&mut self, query: Option<String>) {
        self.search_query = query;
    }

    pub fn toggle_wrap(&mut self) {
        self.wrap_lines = !self.wrap_lines;
    }

    pub fn toggle_timestamps(&mut self) {
        self.show_timestamps = !self.show_timestamps;
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
        // 检查是否滚动到底部
        let filtered_count = self.filtered_entries().count();
        if self.scroll_offset >= filtered_count.saturating_sub(1) {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll_offset = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        let filtered_count = self.filtered_entries().count();
        self.scroll_offset = filtered_count.saturating_sub(1);
        self.auto_scroll = true;
    }

    pub fn filtered_entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.buffer.entries().iter().filter(move |entry| {
            // 级别过滤
            let level_match = entry
                .level
                .map_or(true, |lvl| self.level_filter.contains(&lvl));

            // 搜索过滤
            let search_match = self.search_query.as_ref().map_or(true, |query| {
                entry.message.to_lowercase().contains(&query.to_lowercase())
            });

            level_match && search_match
        })
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let filtered: Vec<&LogEntry> = self.filtered_entries().collect();

        // 计算可见区域
        let visible_height = area.height.saturating_sub(2) as usize; // 减去边框
        let start_idx = self.scroll_offset.min(filtered.len().saturating_sub(1));
        let end_idx = (start_idx + visible_height).min(filtered.len());

        let visible_entries = if filtered.is_empty() {
            &[]
        } else {
            &filtered[start_idx..end_idx]
        };

        // 构建显示行
        let mut lines: Vec<Line> = Vec::new();

        for entry in visible_entries {
            let mut spans = Vec::new();

            // 时间戳
            if self.show_timestamps {
                if let Some(ts) = &entry.timestamp {
                    spans.push(Span::styled(
                        format!("[{}] ", ts),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }

            // 级别
            if let Some(level) = entry.level {
                let level_str = format!("{:5} ", level.as_str());
                spans.push(Span::styled(
                    level_str,
                    Style::default()
                        .fg(level.color())
                        .add_modifier(Modifier::BOLD),
                ));
            }

            // 消息（高亮搜索）
            if let Some(query) = &self.search_query {
                let message_lower = entry.message.to_lowercase();
                let query_lower = query.to_lowercase();

                let mut last_end = 0;
                for (idx, _) in message_lower.match_indices(&query_lower) {
                    // 非匹配部分
                    if idx > last_end {
                        spans.push(Span::raw(&entry.message[last_end..idx]));
                    }
                    // 匹配部分（高亮）
                    spans.push(Span::styled(
                        &entry.message[idx..idx + query.len()],
                        Style::default().bg(Color::Yellow).fg(Color::Black),
                    ));
                    last_end = idx + query.len();
                }
                // 剩余部分
                if last_end < entry.message.len() {
                    spans.push(Span::raw(&entry.message[last_end..]));
                }
            } else {
                spans.push(Span::raw(&entry.message));
            }

            lines.push(Line::from(spans));
        }

        // 构建状态栏信息
        let stats = self.buffer.stats();
        let title = format!(
            " Log Viewer [{}/{}] {}{}{}",
            filtered.len(),
            stats.total,
            if self.paused { "⏸ " } else { "" },
            if self.auto_scroll { "↓ " } else { "" },
            if self.search_query.is_some() {
                "🔍 "
            } else {
                ""
            }
        );

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = if self.wrap_lines {
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false })
        } else {
            Paragraph::new(lines).block(block)
        };

        f.render_widget(paragraph, area);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            // 滚动控制
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                self.scroll_up(1);
                true
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                self.scroll_down(1);
                true
            }
            (KeyCode::PageUp, _) => {
                self.scroll_up(10);
                true
            }
            (KeyCode::PageDown, _) => {
                self.scroll_down(10);
                true
            }
            (KeyCode::Home | KeyCode::Char('g'), _) => {
                self.scroll_to_top();
                true
            }
            (KeyCode::End | KeyCode::Char('G'), KeyModifiers::SHIFT) => {
                self.scroll_to_bottom();
                true
            }

            // 自动滚动切换
            (KeyCode::Char('a'), _) => {
                self.toggle_auto_scroll();
                true
            }

            // 暂停/继续
            (KeyCode::Char(' '), _) => {
                self.toggle_pause();
                true
            }

            // 换行模式切换
            (KeyCode::Char('w'), _) => {
                self.toggle_wrap();
                true
            }

            // 时间戳切换
            (KeyCode::Char('t'), _) => {
                self.toggle_timestamps();
                true
            }

            // 级别过滤切换
            (KeyCode::Char('1'), _) => {
                self.toggle_level(LogLevel::Trace);
                true
            }
            (KeyCode::Char('2'), _) => {
                self.toggle_level(LogLevel::Debug);
                true
            }
            (KeyCode::Char('3'), _) => {
                self.toggle_level(LogLevel::Info);
                true
            }
            (KeyCode::Char('4'), _) => {
                self.toggle_level(LogLevel::Warn);
                true
            }
            (KeyCode::Char('5'), _) => {
                self.toggle_level(LogLevel::Error);
                true
            }
            (KeyCode::Char('6'), _) => {
                self.toggle_level(LogLevel::Fatal);
                true
            }

            // 清空日志
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.buffer.clear();
                self.scroll_offset = 0;
                true
            }

            _ => false,
        }
    }

    pub fn buffer(&self) -> &LogBuffer {
        &self.buffer
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn auto_scroll_enabled(&self) -> bool {
        self.auto_scroll
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewer_creation() {
        let viewer = LogViewer::new(100);
        assert!(viewer.auto_scroll_enabled());
        assert!(!viewer.is_paused());
    }

    #[test]
    fn test_add_line() {
        let mut viewer = LogViewer::new(100);
        viewer.add_line("INFO: Test message");
        assert_eq!(viewer.buffer().len(), 1);
    }

    #[test]
    fn test_auto_scroll_behavior() {
        let mut viewer = LogViewer::new(100);
        assert!(viewer.auto_scroll_enabled());

        viewer.scroll_up(5);
        assert!(!viewer.auto_scroll_enabled());

        viewer.scroll_to_bottom();
        assert!(viewer.auto_scroll_enabled());
    }

    #[test]
    fn test_pause() {
        let mut viewer = LogViewer::new(100);
        viewer.add_line("INFO: Line 1");
        assert_eq!(viewer.buffer().len(), 1);

        viewer.toggle_pause();
        viewer.add_line("INFO: Line 2");
        assert_eq!(viewer.buffer().len(), 1); // 暂停时不添加

        viewer.toggle_pause();
        viewer.add_line("INFO: Line 3");
        assert_eq!(viewer.buffer().len(), 2);
    }

    #[test]
    fn test_level_filter() {
        let mut viewer = LogViewer::new(100);
        viewer.add_line("INFO: info message");
        viewer.add_line("ERROR: error message");
        viewer.add_line("DEBUG: debug message");

        // 只显示 ERROR
        viewer.toggle_level(LogLevel::Info);
        viewer.toggle_level(LogLevel::Debug);
        viewer.toggle_level(LogLevel::Trace);
        viewer.toggle_level(LogLevel::Warn);
        viewer.toggle_level(LogLevel::Fatal);

        let filtered: Vec<_> = viewer.filtered_entries().collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].level, Some(LogLevel::Error));
    }

    #[test]
    fn test_search_filter() {
        let mut viewer = LogViewer::new(100);
        viewer.add_line("INFO: hello world");
        viewer.add_line("ERROR: something failed");
        viewer.add_line("INFO: hello again");

        viewer.set_search(Some("hello".to_string()));
        let filtered: Vec<_> = viewer.filtered_entries().collect();
        assert_eq!(filtered.len(), 2);
    }
}
