// kiana-tui/src/overlay/search.rs

use crate::overlay::{Overlay, OverlayAction};
use crate::search::calculate_match_score;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

/// 搜索模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// 仅搜索用户输入
    UserInputs,
    /// 搜索所有消息
    AllMessages,
}

/// 单条搜索结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// 匹配的文本内容
    pub text: String,
    /// 匹配分数（越小越好）
    pub score: usize,
    /// 高亮位置（字符索引）
    pub positions: Vec<usize>,
    /// 原始数据的索引
    pub index: usize,
}

impl SearchResult {
    /// 创建新的搜索结果
    pub fn new(text: String, score: usize, positions: Vec<usize>, index: usize) -> Self {
        Self {
            text,
            score,
            positions,
            index,
        }
    }
}

/// 搜索覆盖层
pub struct SearchOverlay {
    /// 搜索模式
    mode: SearchMode,
    /// 当前查询字符串
    query: String,
    /// 搜索结果列表（按分数排序）
    results: Vec<SearchResult>,
    /// 当前选中的结果索引
    selected_index: usize,
}

impl SearchOverlay {
    /// 创建新的搜索覆盖层
    pub fn new(mode: SearchMode) -> Self {
        Self {
            mode,
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
        }
    }

    /// 获取搜索模式
    pub fn mode(&self) -> SearchMode {
        self.mode
    }

    /// 获取当前查询字符串
    pub fn query(&self) -> &str {
        &self.query
    }

    /// 获取搜索结果列表
    pub fn results(&self) -> &[SearchResult] {
        &self.results
    }

    /// 获取当前选中的索引
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// 设置查询字符串并执行搜索
    pub fn set_query(&mut self, query: String, items: &[String]) {
        self.query = query;
        self.perform_search(items);
    }

    /// 执行搜索并更新结果列表
    ///
    /// 该方法会：
    /// 1. 根据 mode 决定搜索范围（当前仅支持 UserInputs，未来可扩展）
    /// 2. 对每个项目调用 calculate_match_score
    /// 3. 收集并排序结果
    /// 4. 限制结果数量到 50 条
    /// 5. 更新 self.results
    fn perform_search(&mut self, items: &[String]) {
        self.results.clear();
        self.selected_index = 0;

        if self.query.is_empty() {
            return;
        }

        // 收集所有匹配的结果
        let mut matches: Vec<SearchResult> = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                calculate_match_score(item, &self.query).map(|(score, positions)| {
                    SearchResult::new(item.clone(), score, positions, index)
                })
            })
            .collect();

        // 按分数排序（越小越好）
        matches.sort_by_key(|r| r.score);

        // 限制结果数量到 50 条
        matches.truncate(50);

        self.results = matches;
    }

    /// 更新搜索查询并重新执行搜索
    pub fn update_query(&mut self, query: String, items: &[String]) {
        self.query = query;
        self.perform_search(items);
    }

    /// 切换搜索模式
    pub fn toggle_mode(&mut self, items: &[String]) {
        self.mode = match self.mode {
            SearchMode::UserInputs => SearchMode::AllMessages,
            SearchMode::AllMessages => SearchMode::UserInputs,
        };
        self.perform_search(items);
    }

    /// 向上移动选择
    pub fn select_previous(&mut self) {
        if !self.results.is_empty() && self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// 向下移动选择
    pub fn select_next(&mut self) {
        if !self.results.is_empty() && self.selected_index < self.results.len() - 1 {
            self.selected_index += 1;
        }
    }

    /// 获取当前选中的结果
    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected_index)
    }

    /// 获取当前选中的内容文本（Task 6 要求）
    pub fn get_selected_content(&self) -> Option<String> {
        self.selected_result().map(|result| result.text.clone())
    }
}

impl Overlay for SearchOverlay {
    fn render(&self, frame: &mut Frame, area: Rect) {
        // 清除背景区域
        frame.render_widget(Clear, area);

        // 创建一个居中的弹出窗口（80% 宽度，80% 高度）
        let popup_width = (area.width as f32 * 0.8) as u16;
        let popup_height = (area.height as f32 * 0.8) as u16;
        let popup_x = (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = (area.height.saturating_sub(popup_height)) / 2;

        let popup_area = Rect {
            x: area.x + popup_x,
            y: area.y + popup_y,
            width: popup_width,
            height: popup_height,
        };

        // 创建主边框块
        let block = Block::default()
            .title(self.title())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        frame.render_widget(block, popup_area);

        // 计算内部区域（去掉边框）
        let inner_area = Rect {
            x: popup_area.x + 1,
            y: popup_area.y + 1,
            width: popup_area.width.saturating_sub(2),
            height: popup_area.height.saturating_sub(2),
        };

        // 布局：搜索框(1行) + 结果列表(剩余-1行) + 状态栏(1行)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // 搜索输入
                Constraint::Min(3),    // 结果列表
                Constraint::Length(1), // 状态栏
            ])
            .split(inner_area);

        // 渲染搜索输入框
        self.render_search_input(frame, chunks[0]);

        // 渲染结果列表
        self.render_results_list(frame, chunks[1]);

        // 渲染状态栏
        self.render_status_bar(frame, chunks[2]);
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Esc => OverlayAction::Close,
            KeyCode::Enter => {
                if let Some(result) = self.selected_result() {
                    OverlayAction::Submit(result.text.clone())
                } else {
                    OverlayAction::Close
                }
            }
            KeyCode::Up => {
                self.select_previous();
                OverlayAction::Continue
            }
            KeyCode::Down => {
                self.select_next();
                OverlayAction::Continue
            }
            _ => OverlayAction::Continue,
        }
    }

    fn title(&self) -> &str {
        match self.mode {
            SearchMode::UserInputs => "搜索用户输入 (Ctrl+R)",
            SearchMode::AllMessages => "搜索所有消息",
        }
    }
}

impl SearchOverlay {
    /// 渲染搜索输入框
    fn render_search_input(&self, frame: &mut Frame, area: Rect) {
        let search_text = if self.query.is_empty() {
            Line::from(vec![
                Span::styled("搜索: ", Style::default().fg(Color::Yellow)),
                Span::styled("(输入以搜索...)", Style::default().fg(Color::DarkGray)),
            ])
        } else {
            Line::from(vec![
                Span::styled("搜索: ", Style::default().fg(Color::Yellow)),
                Span::styled(&self.query, Style::default().fg(Color::White)),
                Span::styled("_", Style::default().fg(Color::Cyan)),
            ])
        };

        let paragraph = Paragraph::new(search_text);
        frame.render_widget(paragraph, area);
    }

    /// 渲染结果列表
    fn render_results_list(&self, frame: &mut Frame, area: Rect) {
        if self.results.is_empty() {
            // 显示空状态消息
            let empty_msg = if self.query.is_empty() {
                "开始输入以搜索历史消息..."
            } else {
                "未找到匹配结果"
            };

            let paragraph = Paragraph::new(Line::from(vec![Span::styled(
                empty_msg,
                Style::default().fg(Color::DarkGray),
            )]));
            frame.render_widget(paragraph, area);
            return;
        }

        // 创建结果列表项
        let items: Vec<ListItem> = self
            .results
            .iter()
            .enumerate()
            .map(|(idx, result)| {
                let is_selected = idx == self.selected_index;

                // 高亮匹配位置
                let styled_text = self.highlight_matches(&result.text, &result.positions);

                let style = if is_selected {
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(styled_text).style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, area);
    }

    /// 渲染状态栏
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status_text = if self.results.is_empty() {
            Line::from(vec![
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 关闭 | "),
                Span::styled(
                    "Ctrl+U",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 切换模式"),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    format!("{}/{}", self.selected_index + 1, self.results.len()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" | "),
                Span::styled(
                    "↑↓",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 导航 | "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 选择 | "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 取消"),
            ])
        };

        let paragraph = Paragraph::new(status_text);
        frame.render_widget(paragraph, area);
    }

    /// 高亮匹配位置
    fn highlight_matches(&self, text: &str, positions: &[usize]) -> Line<'static> {
        if positions.is_empty() {
            return Line::from(text.to_string());
        }

        let mut spans = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut last_pos = 0;

        for &pos in positions {
            if pos >= chars.len() {
                continue;
            }

            // 添加匹配前的普通文本
            if pos > last_pos {
                let normal_text: String = chars[last_pos..pos].iter().collect();
                spans.push(Span::raw(normal_text));
            }

            // 添加高亮的匹配字符
            let match_char: String = chars[pos..pos + 1].iter().collect();
            spans.push(Span::styled(
                match_char,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));

            last_pos = pos + 1;
        }

        // 添加剩余的文本
        if last_pos < chars.len() {
            let remaining_text: String = chars[last_pos..].iter().collect();
            spans.push(Span::raw(remaining_text));
        }

        Line::from(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_mode_equality() {
        assert_eq!(SearchMode::UserInputs, SearchMode::UserInputs);
        assert_eq!(SearchMode::AllMessages, SearchMode::AllMessages);
        assert_ne!(SearchMode::UserInputs, SearchMode::AllMessages);
    }

    #[test]
    fn test_search_result_creation() {
        let result = SearchResult::new("test content".to_string(), 5, vec![0, 1, 2], 10);
        assert_eq!(result.text, "test content");
        assert_eq!(result.score, 5);
        assert_eq!(result.positions, vec![0, 1, 2]);
        assert_eq!(result.index, 10);
    }

    #[test]
    fn test_search_overlay_creation() {
        let overlay = SearchOverlay::new(SearchMode::UserInputs);
        assert_eq!(overlay.mode(), SearchMode::UserInputs);
        assert_eq!(overlay.query(), "");
        assert_eq!(overlay.results().len(), 0);
        assert_eq!(overlay.selected_index(), 0);
    }

    #[test]
    fn test_search_overlay_with_all_messages_mode() {
        let overlay = SearchOverlay::new(SearchMode::AllMessages);
        assert_eq!(overlay.mode(), SearchMode::AllMessages);
    }

    #[test]
    fn test_perform_search_empty_query() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec!["hello".to_string(), "world".to_string()];
        overlay.set_query("".to_string(), &items);
        assert_eq!(overlay.results().len(), 0);
    }

    #[test]
    fn test_perform_search_with_matches() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec![
            "hello world".to_string(),
            "foo bar".to_string(),
            "hello rust".to_string(),
        ];
        overlay.set_query("hello".to_string(), &items);

        // 应该匹配两个包含 "hello" 的项目
        assert_eq!(overlay.results().len(), 2);

        // 结果应该按分数排序
        assert_eq!(overlay.results()[0].text, "hello world");
        assert_eq!(overlay.results()[1].text, "hello rust");
    }

    #[test]
    fn test_perform_search_no_matches() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec!["hello".to_string(), "world".to_string()];
        overlay.set_query("xyz".to_string(), &items);
        assert_eq!(overlay.results().len(), 0);
    }

    #[test]
    fn test_select_navigation() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec![
            "item1".to_string(),
            "item2".to_string(),
            "item3".to_string(),
        ];
        overlay.set_query("item".to_string(), &items);

        assert_eq!(overlay.selected_index(), 0);

        overlay.select_next();
        assert_eq!(overlay.selected_index(), 1);

        overlay.select_next();
        assert_eq!(overlay.selected_index(), 2);

        // 不应该越界
        overlay.select_next();
        assert_eq!(overlay.selected_index(), 2);

        overlay.select_previous();
        assert_eq!(overlay.selected_index(), 1);

        overlay.select_previous();
        assert_eq!(overlay.selected_index(), 0);

        // 不应该越界
        overlay.select_previous();
        assert_eq!(overlay.selected_index(), 0);
    }

    #[test]
    fn test_selected_result() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec!["first item".to_string(), "second item".to_string()];
        overlay.set_query("item".to_string(), &items);

        let selected = overlay.selected_result();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().text, "first item");

        overlay.select_next();
        let selected = overlay.selected_result();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().text, "second item");
    }

    #[test]
    fn test_overlay_trait_title() {
        let overlay_user = SearchOverlay::new(SearchMode::UserInputs);
        assert_eq!(overlay_user.title(), "搜索用户输入 (Ctrl+R)");

        let overlay_all = SearchOverlay::new(SearchMode::AllMessages);
        assert_eq!(overlay_all.title(), "搜索所有消息");
    }

    #[test]
    fn test_overlay_trait_handle_key_esc() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let key = KeyEvent::from(KeyCode::Esc);
        let action = overlay.handle_key(key);
        assert_eq!(action, OverlayAction::Close);
    }

    #[test]
    fn test_overlay_trait_handle_key_enter_with_result() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec!["test item".to_string()];
        overlay.set_query("test".to_string(), &items);

        let key = KeyEvent::from(KeyCode::Enter);
        let action = overlay.handle_key(key);
        assert_eq!(action, OverlayAction::Submit("test item".to_string()));
    }

    #[test]
    fn test_overlay_trait_handle_key_enter_no_result() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let key = KeyEvent::from(KeyCode::Enter);
        let action = overlay.handle_key(key);
        assert_eq!(action, OverlayAction::Close);
    }

    #[test]
    fn test_overlay_trait_handle_key_navigation() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec!["item1".to_string(), "item2".to_string()];
        overlay.set_query("item".to_string(), &items);

        let up = KeyEvent::from(KeyCode::Up);
        let down = KeyEvent::from(KeyCode::Down);

        assert_eq!(overlay.selected_index(), 0);

        let action = overlay.handle_key(down);
        assert_eq!(action, OverlayAction::Continue);
        assert_eq!(overlay.selected_index(), 1);

        let action = overlay.handle_key(up);
        assert_eq!(action, OverlayAction::Continue);
        assert_eq!(overlay.selected_index(), 0);
    }

    #[test]
    fn test_update_query() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec![
            "hello world".to_string(),
            "goodbye world".to_string(),
            "hello rust".to_string(),
        ];

        overlay.update_query("hello".to_string(), &items);
        assert_eq!(overlay.query(), "hello");
        assert_eq!(overlay.results().len(), 2);
        assert_eq!(overlay.results()[0].text, "hello world");
        assert_eq!(overlay.results()[1].text, "hello rust");
    }

    #[test]
    fn test_toggle_mode() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        assert_eq!(overlay.mode(), SearchMode::UserInputs);

        let items = vec!["test".to_string()];
        overlay.toggle_mode(&items);
        assert_eq!(overlay.mode(), SearchMode::AllMessages);

        overlay.toggle_mode(&items);
        assert_eq!(overlay.mode(), SearchMode::UserInputs);
    }

    #[test]
    fn test_search_results_limited_to_50() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);

        // 创建 100 个匹配项
        let items: Vec<String> = (0..100).map(|i| format!("test item {}", i)).collect();

        overlay.set_query("test".to_string(), &items);

        // 结果应该被限制在 50 条
        assert_eq!(overlay.results().len(), 50);
    }

    #[test]
    fn test_search_sorts_by_score() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec![
            "the hello world".to_string(), // 匹配在位置 4
            "hello".to_string(),           // 匹配在位置 0（最佳）
            "say hello rust".to_string(),  // 匹配在位置 4
        ];

        overlay.set_query("hello".to_string(), &items);

        // 结果应该按分数排序（分数越低越好）
        assert_eq!(overlay.results().len(), 3);
        // "hello" 应该排在第一位（分数为 0）
        assert_eq!(overlay.results()[0].text, "hello");
        assert_eq!(overlay.results()[0].score, 0);
        // 其他两个应该有更高的分数
        assert!(overlay.results()[1].score > 0);
        assert!(overlay.results()[2].score > 0);
    }

    #[test]
    fn test_update_query_resets_selection() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec![
            "item1".to_string(),
            "item2".to_string(),
            "item3".to_string(),
        ];

        overlay.set_query("item".to_string(), &items);
        overlay.select_next();
        overlay.select_next();
        assert_eq!(overlay.selected_index(), 2);

        // 更新查询应该重置选择索引
        overlay.update_query("item2".to_string(), &items);
        assert_eq!(overlay.selected_index(), 0);
    }

    #[test]
    fn test_toggle_mode_resets_selection() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec!["item1".to_string(), "item2".to_string()];

        overlay.set_query("item".to_string(), &items);
        overlay.select_next();
        assert_eq!(overlay.selected_index(), 1);

        // 切换模式应该重置选择索引
        overlay.toggle_mode(&items);
        assert_eq!(overlay.selected_index(), 0);
    }

    #[test]
    fn test_get_selected_content() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec![
            "first item".to_string(),
            "second item".to_string(),
            "third item".to_string(),
        ];
        overlay.set_query("item".to_string(), &items);

        // 测试获取第一个选中的内容
        let content = overlay.get_selected_content();
        assert!(content.is_some());
        let first = content.unwrap();

        // 移动到下一个并测试
        overlay.select_next();
        let content = overlay.get_selected_content();
        assert!(content.is_some());
        let second = content.unwrap();

        // 移动到第三个并测试
        overlay.select_next();
        let content = overlay.get_selected_content();
        assert!(content.is_some());
        let third = content.unwrap();

        // 验证我们有三个不同的结果
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);

        // 验证不会越界（停留在最后一个）
        overlay.select_next();
        let content = overlay.get_selected_content();
        assert!(content.is_some());
        assert_eq!(content.unwrap(), third);
    }

    #[test]
    fn test_get_selected_content_empty() {
        let overlay = SearchOverlay::new(SearchMode::UserInputs);
        let content = overlay.get_selected_content();
        assert!(content.is_none());
    }

    #[test]
    fn test_get_selected_content_after_search() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec![
            "hello world".to_string(),
            "goodbye world".to_string(),
            "hello rust".to_string(),
        ];

        // 搜索 "hello" 应该只匹配两个
        overlay.set_query("hello".to_string(), &items);

        let content = overlay.get_selected_content();
        assert!(content.is_some());
        assert_eq!(content.unwrap(), "hello world");

        overlay.select_next();
        let content = overlay.get_selected_content();
        assert!(content.is_some());
        assert_eq!(content.unwrap(), "hello rust");
    }

    #[test]
    fn test_navigation_wraparound_bounds() {
        let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
        let items = vec!["item1".to_string(), "item2".to_string()];
        overlay.set_query("item".to_string(), &items);

        // 测试向下到达末尾不会越界
        assert_eq!(overlay.selected_index(), 0);
        overlay.select_next();
        assert_eq!(overlay.selected_index(), 1);
        overlay.select_next();
        assert_eq!(overlay.selected_index(), 1); // 应该停留在末尾

        // 测试向上回到开头不会越界
        overlay.select_previous();
        assert_eq!(overlay.selected_index(), 0);
        overlay.select_previous();
        assert_eq!(overlay.selected_index(), 0); // 应该停留在开头
    }
}
