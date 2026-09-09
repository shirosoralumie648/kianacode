/// Example demonstrating the split view system
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Paragraph,
    Frame,
};

use super::{SplitContent, SplitDirection, SplitManager};

/// Simple text content for demonstration
pub struct TextContent {
    text: String,
    background: Color,
}

impl TextContent {
    pub fn new(text: impl Into<String>, background: Color) -> Self {
        Self {
            text: text.into(),
            background,
        }
    }
}

impl SplitContent for TextContent {
    fn render(&mut self, f: &mut Frame, area: Rect, focused: bool) {
        let style = if focused {
            Style::default().fg(Color::White).bg(self.background)
        } else {
            Style::default().fg(Color::Gray).bg(self.background)
        };

        let text = Line::from(self.text.clone());
        let paragraph = Paragraph::new(text).style(style);
        f.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example_split_setup() {
        let mut manager = SplitManager::new();

        // 添加第一个分屏
        let content1 = TextContent::new("Split 1", Color::Blue);
        manager.set_root(Box::new(content1));

        assert_eq!(manager.split_count(), 1);

        // 水平分屏
        let content2 = TextContent::new("Split 2", Color::Green);
        manager.split(SplitDirection::Horizontal, Box::new(content2));

        assert_eq!(manager.split_count(), 2);

        // 垂直分屏
        let content3 = TextContent::new("Split 3", Color::Red);
        manager.split(SplitDirection::Vertical, Box::new(content3));

        assert_eq!(manager.split_count(), 3);
    }

    #[test]
    fn test_example_split_navigation() {
        let mut manager = SplitManager::new();

        let content1 = TextContent::new("Split 1", Color::Blue);
        let id1 = manager.set_root(Box::new(content1));

        let content2 = TextContent::new("Split 2", Color::Green);
        let id2 = manager.split(SplitDirection::Horizontal, Box::new(content2));

        // 当前焦点在 id2
        assert_eq!(manager.focused_id(), id2);

        // 切换焦点
        manager.focus_next();
        assert_eq!(manager.focused_id(), Some(id1));
    }
}
