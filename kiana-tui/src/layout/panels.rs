use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders},
    Frame,
};

/// 面板内容的 trait
pub trait PanelContent {
    /// 渲染面板内容
    fn render(&mut self, f: &mut Frame, area: Rect, has_focus: bool);

    /// 处理输入（仅当面板有焦点时）
    fn handle_input(&mut self, _key: crossterm::event::KeyEvent) -> bool {
        false
    }

    /// 面板获得焦点时调用
    fn on_focus(&mut self) {}

    /// 面板失去焦点时调用
    fn on_blur(&mut self) {}

    /// 获取面板的首选标题（可选）
    fn title(&self) -> Option<String> {
        None
    }
}

/// 面板大小约束
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSize {
    /// 固定大小（行数或列数）
    Fixed(u16),
    /// 百分比大小 (0-100)
    Percentage(u16),
    /// 最小大小
    Min(u16),
    /// 自动填充剩余空间
    Fill,
}

impl From<PanelSize> for Constraint {
    fn from(size: PanelSize) -> Self {
        match size {
            PanelSize::Fixed(n) => Constraint::Length(n),
            PanelSize::Percentage(p) => Constraint::Percentage(p),
            PanelSize::Min(n) => Constraint::Min(n),
            PanelSize::Fill => Constraint::Min(0),
        }
    }
}

/// 单个面板
pub struct Panel {
    content: Box<dyn PanelContent>,
    size: PanelSize,
    title: Option<String>,
    show_border: bool,
    focusable: bool,
}

impl Panel {
    pub fn new(content: Box<dyn PanelContent>) -> Self {
        Self {
            content,
            size: PanelSize::Fill,
            title: None,
            show_border: true,
            focusable: true,
        }
    }

    pub fn with_size(mut self, size: PanelSize) -> Self {
        self.size = size;
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_border(mut self, show: bool) -> Self {
        self.show_border = show;
        self
    }

    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    pub fn size(&self) -> PanelSize {
        self.size
    }

    pub fn is_focusable(&self) -> bool {
        self.focusable
    }
}

/// 面板容器方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelDirection {
    Horizontal,
    Vertical,
}

impl From<PanelDirection> for Direction {
    fn from(dir: PanelDirection) -> Self {
        match dir {
            PanelDirection::Horizontal => Direction::Horizontal,
            PanelDirection::Vertical => Direction::Vertical,
        }
    }
}

/// 多面板容器
pub struct PanelContainer {
    panels: Vec<Panel>,
    direction: PanelDirection,
    focused_index: usize,
}

impl PanelContainer {
    pub fn new(direction: PanelDirection) -> Self {
        Self {
            panels: Vec::new(),
            direction,
            focused_index: 0,
        }
    }

    /// 添加面板
    pub fn add_panel(&mut self, panel: Panel) {
        self.panels.push(panel);
    }

    /// 移除面板
    pub fn remove_panel(&mut self, index: usize) -> Option<Panel> {
        if index < self.panels.len() {
            let panel = self.panels.remove(index);

            // 调整焦点索引
            if !self.panels.is_empty() {
                // 如果移除的面板在焦点之前，焦点索引需要减 1
                if index < self.focused_index {
                    self.focused_index -= 1;
                }
                // 如果焦点索引超出范围，调整到最后一个面板
                else if self.focused_index >= self.panels.len() {
                    self.focused_index = self.panels.len() - 1;
                }
                // 确保焦点在可聚焦的面板上
                self.ensure_valid_focus();
            } else {
                self.focused_index = 0;
            }

            Some(panel)
        } else {
            None
        }
    }

    /// 确保焦点在可聚焦的面板上
    fn ensure_valid_focus(&mut self) {
        if self.panels.is_empty() {
            return;
        }

        // 如果当前焦点面板可聚焦，无需调整
        if self.panels[self.focused_index].focusable {
            return;
        }

        // 查找下一个可聚焦的面板
        let start = self.focused_index;
        loop {
            self.focused_index = (self.focused_index + 1) % self.panels.len();

            if self.panels[self.focused_index].focusable {
                return;
            }

            // 如果所有面板都不可聚焦，保持在原位
            if self.focused_index == start {
                return;
            }
        }
    }

    /// 切换到下一个可聚焦的面板
    pub fn focus_next(&mut self) {
        if self.panels.is_empty() {
            return;
        }

        let start = self.focused_index;
        let prev_focus = self.focused_index;

        loop {
            self.focused_index = (self.focused_index + 1) % self.panels.len();

            if self.panels[self.focused_index].focusable {
                break;
            }

            // 防止无限循环（所有面板都不可聚焦）
            if self.focused_index == start {
                self.focused_index = prev_focus;
                return;
            }
        }

        // 触发焦点变化事件
        if prev_focus != self.focused_index {
            if let Some(panel) = self.panels.get_mut(prev_focus) {
                panel.content.on_blur();
            }
            if let Some(panel) = self.panels.get_mut(self.focused_index) {
                panel.content.on_focus();
            }
        }
    }

    /// 切换到上一个可聚焦的面板
    pub fn focus_previous(&mut self) {
        if self.panels.is_empty() {
            return;
        }

        let start = self.focused_index;
        let prev_focus = self.focused_index;

        loop {
            self.focused_index = if self.focused_index == 0 {
                self.panels.len() - 1
            } else {
                self.focused_index - 1
            };

            if self.panels[self.focused_index].focusable {
                break;
            }

            // 防止无限循环
            if self.focused_index == start {
                self.focused_index = prev_focus;
                return;
            }
        }

        // 触发焦点变化事件
        if prev_focus != self.focused_index {
            if let Some(panel) = self.panels.get_mut(prev_focus) {
                panel.content.on_blur();
            }
            if let Some(panel) = self.panels.get_mut(self.focused_index) {
                panel.content.on_focus();
            }
        }
    }

    /// 设置焦点到指定面板
    pub fn set_focus(&mut self, index: usize) -> bool {
        if index < self.panels.len() && self.panels[index].focusable && index != self.focused_index
        {
            if let Some(panel) = self.panels.get_mut(self.focused_index) {
                panel.content.on_blur();
            }

            self.focused_index = index;

            if let Some(panel) = self.panels.get_mut(self.focused_index) {
                panel.content.on_focus();
            }

            true
        } else {
            false
        }
    }

    /// 获取焦点面板索引
    pub fn focused_index(&self) -> usize {
        self.focused_index
    }

    /// 获取面板数量
    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    /// 获取布局方向
    pub fn direction(&self) -> PanelDirection {
        self.direction
    }

    /// 设置布局方向
    pub fn set_direction(&mut self, direction: PanelDirection) {
        self.direction = direction;
    }

    /// 处理输入（路由到焦点面板）
    pub fn handle_input(&mut self, key: crossterm::event::KeyEvent) -> bool {
        if let Some(panel) = self.panels.get_mut(self.focused_index) {
            panel.content.handle_input(key)
        } else {
            false
        }
    }

    /// 渲染面板容器
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        if self.panels.is_empty() {
            return;
        }

        // 构建约束
        let constraints: Vec<Constraint> = self.panels.iter().map(|p| p.size.into()).collect();

        // 分割区域
        let chunks = Layout::default()
            .direction(self.direction.into())
            .constraints(constraints)
            .split(area);

        // 渲染每个面板
        for (i, (panel, &chunk)) in self.panels.iter_mut().zip(chunks.iter()).enumerate() {
            let has_focus = i == self.focused_index;

            // 应用边框
            let render_area = if panel.show_border {
                let title = panel
                    .title
                    .clone()
                    .or_else(|| panel.content.title())
                    .unwrap_or_default();

                let border_style = if has_focus {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(title);

                let inner = block.inner(chunk);
                f.render_widget(block, chunk);
                inner
            } else {
                chunk
            };

            // 渲染面板内容
            panel.content.render(f, render_area, has_focus);
        }
    }
}

impl Default for PanelContainer {
    fn default() -> Self {
        Self::new(PanelDirection::Horizontal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // Mock PanelContent for testing
    struct MockPanelContent {
        render_count: Arc<Mutex<usize>>,
        focus_count: Arc<Mutex<usize>>,
        blur_count: Arc<Mutex<usize>>,
        input_count: Arc<Mutex<usize>>,
        test_title: Option<String>,
    }

    impl MockPanelContent {
        fn new() -> (
            Self,
            Arc<Mutex<usize>>,
            Arc<Mutex<usize>>,
            Arc<Mutex<usize>>,
            Arc<Mutex<usize>>,
        ) {
            let render_count = Arc::new(Mutex::new(0));
            let focus_count = Arc::new(Mutex::new(0));
            let blur_count = Arc::new(Mutex::new(0));
            let input_count = Arc::new(Mutex::new(0));

            (
                Self {
                    render_count: render_count.clone(),
                    focus_count: focus_count.clone(),
                    blur_count: blur_count.clone(),
                    input_count: input_count.clone(),
                    test_title: None,
                },
                render_count,
                focus_count,
                blur_count,
                input_count,
            )
        }

        fn with_title(mut self, title: String) -> Self {
            self.test_title = Some(title);
            self
        }
    }

    impl PanelContent for MockPanelContent {
        fn render(&mut self, _f: &mut Frame, _area: Rect, _has_focus: bool) {
            *self.render_count.lock().unwrap() += 1;
        }

        fn on_focus(&mut self) {
            *self.focus_count.lock().unwrap() += 1;
        }

        fn on_blur(&mut self) {
            *self.blur_count.lock().unwrap() += 1;
        }

        fn handle_input(&mut self, _key: crossterm::event::KeyEvent) -> bool {
            *self.input_count.lock().unwrap() += 1;
            true
        }

        fn title(&self) -> Option<String> {
            self.test_title.clone()
        }
    }

    #[test]
    fn test_panel_size_conversion() {
        assert_eq!(
            Constraint::from(PanelSize::Fixed(10)),
            Constraint::Length(10)
        );
        assert_eq!(
            Constraint::from(PanelSize::Percentage(50)),
            Constraint::Percentage(50)
        );
        assert_eq!(Constraint::from(PanelSize::Min(5)), Constraint::Min(5));
        assert_eq!(Constraint::from(PanelSize::Fill), Constraint::Min(0));
    }

    #[test]
    fn test_panel_creation() {
        let (content, _, _, _, _) = MockPanelContent::new();
        let panel = Panel::new(Box::new(content));

        assert_eq!(panel.size(), PanelSize::Fill);
        assert!(panel.show_border);
        assert!(panel.is_focusable());
        assert!(panel.title.is_none());
    }

    #[test]
    fn test_panel_with_size() {
        let (content, _, _, _, _) = MockPanelContent::new();
        let panel = Panel::new(Box::new(content)).with_size(PanelSize::Fixed(20));

        assert_eq!(panel.size(), PanelSize::Fixed(20));
    }

    #[test]
    fn test_panel_with_title() {
        let (content, _, _, _, _) = MockPanelContent::new();
        let panel = Panel::new(Box::new(content)).with_title("Test Panel");

        assert_eq!(panel.title, Some("Test Panel".to_string()));
    }

    #[test]
    fn test_panel_with_border() {
        let (content, _, _, _, _) = MockPanelContent::new();
        let panel = Panel::new(Box::new(content)).with_border(false);

        assert!(!panel.show_border);
    }

    #[test]
    fn test_panel_with_focusable() {
        let (content, _, _, _, _) = MockPanelContent::new();
        let panel = Panel::new(Box::new(content)).with_focusable(false);

        assert!(!panel.is_focusable());
    }

    #[test]
    fn test_container_creation() {
        let container = PanelContainer::new(PanelDirection::Horizontal);

        assert_eq!(container.panel_count(), 0);
        assert_eq!(container.focused_index(), 0);
        assert_eq!(container.direction(), PanelDirection::Horizontal);
    }

    #[test]
    fn test_container_add_panel() {
        let mut container = PanelContainer::new(PanelDirection::Vertical);
        let (content, _, _, _, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content)));

        assert_eq!(container.panel_count(), 1);
        assert_eq!(container.focused_index(), 0);
    }

    #[test]
    fn test_container_remove_panel() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _, _, _) = MockPanelContent::new();
        let (content2, _, _, _, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)));
        container.add_panel(Panel::new(Box::new(content2)));

        assert_eq!(container.panel_count(), 2);

        let removed = container.remove_panel(0);
        assert!(removed.is_some());
        assert_eq!(container.panel_count(), 1);
        assert_eq!(container.focused_index(), 0);
    }

    #[test]
    fn test_focus_next() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _focus1, blur1, _) = MockPanelContent::new();
        let (content2, _, focus2, _blur2, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)));
        container.add_panel(Panel::new(Box::new(content2)));

        assert_eq!(container.focused_index(), 0);

        container.focus_next();
        assert_eq!(container.focused_index(), 1);
        assert_eq!(*blur1.lock().unwrap(), 1);
        assert_eq!(*focus2.lock().unwrap(), 1);

        // Wrap around
        container.focus_next();
        assert_eq!(container.focused_index(), 0);
    }

    #[test]
    fn test_focus_previous() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _, _, _) = MockPanelContent::new();
        let (content2, _, _, _, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)));
        container.add_panel(Panel::new(Box::new(content2)));

        assert_eq!(container.focused_index(), 0);

        container.focus_previous();
        assert_eq!(container.focused_index(), 1); // Wrap to last

        container.focus_previous();
        assert_eq!(container.focused_index(), 0);
    }

    #[test]
    fn test_set_focus() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _, _, _) = MockPanelContent::new();
        let (content2, _, focus2, _, _) = MockPanelContent::new();
        let (content3, _, focus3, _, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)));
        container.add_panel(Panel::new(Box::new(content2)));
        container.add_panel(Panel::new(Box::new(content3)));

        assert!(container.set_focus(2));
        assert_eq!(container.focused_index(), 2);
        assert_eq!(*focus3.lock().unwrap(), 1);

        // Set to same panel
        assert!(!container.set_focus(2));

        // Out of bounds
        assert!(!container.set_focus(10));

        assert!(container.set_focus(1));
        assert_eq!(*focus2.lock().unwrap(), 1);
    }

    #[test]
    fn test_focus_skip_non_focusable() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _, _, _) = MockPanelContent::new();
        let (content2, _, _, _, _) = MockPanelContent::new();
        let (content3, _, focus3, _, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)));
        container.add_panel(Panel::new(Box::new(content2)).with_focusable(false));
        container.add_panel(Panel::new(Box::new(content3)));

        assert_eq!(container.focused_index(), 0);

        container.focus_next();
        // Should skip panel 1 (non-focusable) and go to panel 2
        assert_eq!(container.focused_index(), 2);
        assert_eq!(*focus3.lock().unwrap(), 1);
    }

    #[test]
    fn test_set_focus_non_focusable() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _, _, _) = MockPanelContent::new();
        let (content2, _, _, _, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)));
        container.add_panel(Panel::new(Box::new(content2)).with_focusable(false));

        // Cannot set focus to non-focusable panel
        assert!(!container.set_focus(1));
        assert_eq!(container.focused_index(), 0);
    }

    #[test]
    fn test_input_routing() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _, _, input1) = MockPanelContent::new();
        let (content2, _, _, _, input2) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)));
        container.add_panel(Panel::new(Box::new(content2)));

        let key = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('a'));

        // Input goes to focused panel (0)
        assert!(container.handle_input(key));
        assert_eq!(*input1.lock().unwrap(), 1);
        assert_eq!(*input2.lock().unwrap(), 0);

        container.focus_next();

        // Input goes to new focused panel (1)
        assert!(container.handle_input(key));
        assert_eq!(*input1.lock().unwrap(), 1);
        assert_eq!(*input2.lock().unwrap(), 1);
    }

    #[test]
    fn test_empty_container() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        // These should not panic
        container.focus_next();
        container.focus_previous();
        assert_eq!(container.focused_index(), 0);
        assert_eq!(container.panel_count(), 0);

        let key = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('a'));
        assert!(!container.handle_input(key));
    }

    #[test]
    fn test_single_panel_navigation() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);
        let (content, _, _, _, _) = MockPanelContent::new();
        container.add_panel(Panel::new(Box::new(content)));

        // Should stay on the same panel
        container.focus_next();
        assert_eq!(container.focused_index(), 0);

        container.focus_previous();
        assert_eq!(container.focused_index(), 0);
    }

    #[test]
    fn test_all_panels_non_focusable() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _, _, _) = MockPanelContent::new();
        let (content2, _, _, _, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)).with_focusable(false));
        container.add_panel(Panel::new(Box::new(content2)).with_focusable(false));

        let initial = container.focused_index();

        // Should not move if all panels are non-focusable
        container.focus_next();
        assert_eq!(container.focused_index(), initial);

        container.focus_previous();
        assert_eq!(container.focused_index(), initial);
    }

    #[test]
    fn test_remove_focused_panel() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);

        let (content1, _, _, _, _) = MockPanelContent::new();
        let (content2, _, _, _, _) = MockPanelContent::new();
        let (content3, _, _, _, _) = MockPanelContent::new();

        container.add_panel(Panel::new(Box::new(content1)));
        container.add_panel(Panel::new(Box::new(content2)));
        container.add_panel(Panel::new(Box::new(content3)));

        container.set_focus(2);
        assert_eq!(container.focused_index(), 2);

        // Remove the focused panel
        container.remove_panel(2);
        assert_eq!(container.panel_count(), 2);
        // Focus should adjust to last panel
        assert_eq!(container.focused_index(), 1);
    }

    #[test]
    fn test_direction_conversion() {
        assert_eq!(
            Direction::from(PanelDirection::Horizontal),
            Direction::Horizontal
        );
        assert_eq!(
            Direction::from(PanelDirection::Vertical),
            Direction::Vertical
        );
    }

    #[test]
    fn test_set_direction() {
        let mut container = PanelContainer::new(PanelDirection::Horizontal);
        assert_eq!(container.direction(), PanelDirection::Horizontal);

        container.set_direction(PanelDirection::Vertical);
        assert_eq!(container.direction(), PanelDirection::Vertical);
    }

    #[test]
    fn test_panel_content_title() {
        let (mut content, _, _, _, _) = MockPanelContent::new();
        content = content.with_title("Content Title".to_string());
        let panel = Panel::new(Box::new(content));

        // Panel has no explicit title, should use content's title
        assert!(panel.title.is_none());
    }
}
