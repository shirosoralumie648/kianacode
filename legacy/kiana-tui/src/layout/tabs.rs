use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// 标签页内容的 trait
pub trait TabContent {
    /// 渲染标签页内容
    fn render(&mut self, f: &mut Frame, area: Rect);

    /// 处理输入（可选）
    fn handle_input(&mut self, _key: crossterm::event::KeyEvent) -> bool {
        false
    }

    /// 标签页被激活时调用
    fn on_focus(&mut self) {}

    /// 标签页失去焦点时调用
    fn on_blur(&mut self) {}
}

/// 单个标签页
pub struct Tab {
    pub title: String,
    pub content: Box<dyn TabContent>,
    pub closeable: bool,
}

impl Tab {
    pub fn new(title: impl Into<String>, content: Box<dyn TabContent>) -> Self {
        Self {
            title: title.into(),
            content,
            closeable: true,
        }
    }

    pub fn with_closeable(mut self, closeable: bool) -> Self {
        self.closeable = closeable;
        self
    }
}

/// 标签页容器
pub struct TabContainer {
    tabs: Vec<Tab>,
    active: usize,
    tab_bar_height: u16,
}

impl TabContainer {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            tab_bar_height: 1,
        }
    }

    pub fn with_tab_bar_height(mut self, height: u16) -> Self {
        self.tab_bar_height = height;
        self
    }

    /// 添加标签页
    pub fn add_tab(&mut self, tab: Tab) {
        self.tabs.push(tab);
    }

    /// 移除标签页
    pub fn remove_tab(&mut self, index: usize) -> Option<Tab> {
        if index < self.tabs.len() && self.tabs[index].closeable {
            let tab = self.tabs.remove(index);

            // 调整活动标签页索引
            if self.active >= self.tabs.len() && self.active > 0 {
                self.active = self.tabs.len() - 1;
            }

            Some(tab)
        } else {
            None
        }
    }

    /// 切换到下一个标签页
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            let prev_active = self.active;
            self.active = (self.active + 1) % self.tabs.len();

            if prev_active != self.active {
                if let Some(tab) = self.tabs.get_mut(prev_active) {
                    tab.content.on_blur();
                }
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.content.on_focus();
                }
            }
        }
    }

    /// 切换到上一个标签页
    pub fn previous_tab(&mut self) {
        if !self.tabs.is_empty() {
            let prev_active = self.active;
            self.active = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };

            if prev_active != self.active {
                if let Some(tab) = self.tabs.get_mut(prev_active) {
                    tab.content.on_blur();
                }
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.content.on_focus();
                }
            }
        }
    }

    /// 切换到指定索引的标签页
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.tabs.len() && index != self.active {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.content.on_blur();
            }

            self.active = index;

            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.content.on_focus();
            }

            true
        } else {
            false
        }
    }

    /// 获取活动标签页索引
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// 获取标签页数量
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// 获取活动标签页的可变引用
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    /// 渲染标签页容器
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        // 分割区域：标签栏 + 内容区域
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(self.tab_bar_height), Constraint::Min(0)])
            .split(area);

        // 渲染标签栏
        self.render_tab_bar(f, chunks[0]);

        // 渲染活动标签页内容
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.content.render(f, chunks[1]);
        }
    }

    /// 渲染标签栏
    fn render_tab_bar(&self, f: &mut Frame, area: Rect) {
        let mut spans = Vec::new();

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active;

            // 添加分隔符（除了第一个标签）
            if i > 0 {
                spans.push(Span::raw(" "));
            }

            // 数字标签（1-9）
            if i < 9 {
                spans.push(Span::styled(
                    format!("{}:", i + 1),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            // 标签标题
            let style = if is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            spans.push(Span::styled(format!(" {} ", tab.title), style));

            // 可关闭标记
            if tab.closeable {
                spans.push(Span::styled(
                    "×",
                    Style::default().fg(if is_active {
                        Color::Red
                    } else {
                        Color::DarkGray
                    }),
                ));
            }
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).style(Style::default().bg(Color::Black));

        f.render_widget(paragraph, area);
    }
}

impl Default for TabContainer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // Mock TabContent for testing
    struct MockTabContent {
        render_count: Arc<Mutex<usize>>,
        focus_count: Arc<Mutex<usize>>,
        blur_count: Arc<Mutex<usize>>,
    }

    impl MockTabContent {
        fn new() -> (
            Self,
            Arc<Mutex<usize>>,
            Arc<Mutex<usize>>,
            Arc<Mutex<usize>>,
        ) {
            let render_count = Arc::new(Mutex::new(0));
            let focus_count = Arc::new(Mutex::new(0));
            let blur_count = Arc::new(Mutex::new(0));

            (
                Self {
                    render_count: render_count.clone(),
                    focus_count: focus_count.clone(),
                    blur_count: blur_count.clone(),
                },
                render_count,
                focus_count,
                blur_count,
            )
        }
    }

    impl TabContent for MockTabContent {
        fn render(&mut self, _f: &mut Frame, _area: Rect) {
            *self.render_count.lock().unwrap() += 1;
        }

        fn on_focus(&mut self) {
            *self.focus_count.lock().unwrap() += 1;
        }

        fn on_blur(&mut self) {
            *self.blur_count.lock().unwrap() += 1;
        }
    }

    #[test]
    fn test_tab_creation() {
        let (content, _, _, _) = MockTabContent::new();
        let tab = Tab::new("Test", Box::new(content));

        assert_eq!(tab.title, "Test");
        assert!(tab.closeable);
    }

    #[test]
    fn test_tab_with_closeable() {
        let (content, _, _, _) = MockTabContent::new();
        let tab = Tab::new("Test", Box::new(content)).with_closeable(false);

        assert!(!tab.closeable);
    }

    #[test]
    fn test_container_add_tab() {
        let mut container = TabContainer::new();
        assert_eq!(container.tab_count(), 0);

        let (content, _, _, _) = MockTabContent::new();
        container.add_tab(Tab::new("Tab 1", Box::new(content)));

        assert_eq!(container.tab_count(), 1);
        assert_eq!(container.active_index(), 0);
    }

    #[test]
    fn test_container_next_tab() {
        let mut container = TabContainer::new();

        let (content1, _, _focus1, blur1) = MockTabContent::new();
        let (content2, _, focus2, _blur2) = MockTabContent::new();

        container.add_tab(Tab::new("Tab 1", Box::new(content1)));
        container.add_tab(Tab::new("Tab 2", Box::new(content2)));

        assert_eq!(container.active_index(), 0);

        container.next_tab();
        assert_eq!(container.active_index(), 1);
        assert_eq!(*blur1.lock().unwrap(), 1);
        assert_eq!(*focus2.lock().unwrap(), 1);

        // Wrap around
        container.next_tab();
        assert_eq!(container.active_index(), 0);
    }

    #[test]
    fn test_container_previous_tab() {
        let mut container = TabContainer::new();

        let (content1, _, _, _) = MockTabContent::new();
        let (content2, _, _, _) = MockTabContent::new();

        container.add_tab(Tab::new("Tab 1", Box::new(content1)));
        container.add_tab(Tab::new("Tab 2", Box::new(content2)));

        assert_eq!(container.active_index(), 0);

        container.previous_tab();
        assert_eq!(container.active_index(), 1); // Wrap to last

        container.previous_tab();
        assert_eq!(container.active_index(), 0);
    }

    #[test]
    fn test_container_switch_to() {
        let mut container = TabContainer::new();

        let (content1, _, _, _) = MockTabContent::new();
        let (content2, _, _, _) = MockTabContent::new();
        let (content3, _, focus3, _) = MockTabContent::new();

        container.add_tab(Tab::new("Tab 1", Box::new(content1)));
        container.add_tab(Tab::new("Tab 2", Box::new(content2)));
        container.add_tab(Tab::new("Tab 3", Box::new(content3)));

        assert!(container.switch_to(2));
        assert_eq!(container.active_index(), 2);
        assert_eq!(*focus3.lock().unwrap(), 1);

        // Switch to same tab
        assert!(!container.switch_to(2));

        // Out of bounds
        assert!(!container.switch_to(10));
    }

    #[test]
    fn test_container_remove_tab() {
        let mut container = TabContainer::new();

        let (content1, _, _, _) = MockTabContent::new();
        let (content2, _, _, _) = MockTabContent::new();

        container.add_tab(Tab::new("Tab 1", Box::new(content1)));
        container.add_tab(Tab::new("Tab 2", Box::new(content2)));

        container.switch_to(1);
        assert_eq!(container.active_index(), 1);

        let removed = container.remove_tab(1);
        assert!(removed.is_some());
        assert_eq!(container.tab_count(), 1);
        assert_eq!(container.active_index(), 0); // Adjusted
    }

    #[test]
    fn test_container_remove_non_closeable() {
        let mut container = TabContainer::new();

        let (content, _, _, _) = MockTabContent::new();
        container.add_tab(Tab::new("Tab 1", Box::new(content)).with_closeable(false));

        let removed = container.remove_tab(0);
        assert!(removed.is_none());
        assert_eq!(container.tab_count(), 1);
    }

    #[test]
    fn test_switch_to_number_key() {
        let mut container = TabContainer::new();

        for i in 1..=5 {
            let (content, _, _, _) = MockTabContent::new();
            container.add_tab(Tab::new(format!("Tab {}", i), Box::new(content)));
        }

        // Simulate number key press (1-5 -> index 0-4)
        // First tab (index 0) is already active, so switching to it returns false
        assert!(!container.switch_to(0)); // Already active
        assert_eq!(container.active_index(), 0);

        for key in 2..=5 {
            assert!(container.switch_to(key - 1));
            assert_eq!(container.active_index(), key - 1);
        }
    }

    #[test]
    fn test_empty_container() {
        let mut container = TabContainer::new();

        // These should not panic
        container.next_tab();
        container.previous_tab();
        assert_eq!(container.active_index(), 0);
        assert_eq!(container.tab_count(), 0);
        assert!(container.active_tab_mut().is_none());
    }

    #[test]
    fn test_single_tab_navigation() {
        let mut container = TabContainer::new();
        let (content, _, _, _) = MockTabContent::new();
        container.add_tab(Tab::new("Tab 1", Box::new(content)));

        // Should stay on the same tab
        container.next_tab();
        assert_eq!(container.active_index(), 0);

        container.previous_tab();
        assert_eq!(container.active_index(), 0);
    }

    #[test]
    fn test_focus_blur_on_switch() {
        let mut container = TabContainer::new();

        let (content1, _, focus1, blur1) = MockTabContent::new();
        let (content2, _, focus2, blur2) = MockTabContent::new();

        container.add_tab(Tab::new("Tab 1", Box::new(content1)));
        container.add_tab(Tab::new("Tab 2", Box::new(content2)));

        // Switch to tab 2
        container.switch_to(1);
        assert_eq!(*blur1.lock().unwrap(), 1);
        assert_eq!(*focus2.lock().unwrap(), 1);

        // Switch back to tab 1
        container.switch_to(0);
        assert_eq!(*blur1.lock().unwrap(), 1);
        assert_eq!(*blur2.lock().unwrap(), 1);
        assert_eq!(*focus1.lock().unwrap(), 1);
    }
}
