// kiana-tui/src/overlay/search.rs

use crate::overlay::{Overlay, OverlayAction};
use crate::search::calculate_match_score;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

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

    /// 执行搜索（占位符实现）
    ///
    /// 在完整实现中，此方法会：
    /// 1. 根据 mode 决定搜索范围
    /// 2. 对每个项目调用 calculate_match_score
    /// 3. 收集并排序结果
    /// 4. 更新 self.results
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

        self.results = matches;
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
}

impl Overlay for SearchOverlay {
    fn render(&self, _frame: &mut Frame, _area: Rect) {
        // 占位符实现 - 将在后续任务中实现
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
        let result = SearchResult::new(
            "test content".to_string(),
            5,
            vec![0, 1, 2],
            10,
        );
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
}
