/// Example tab contents for demonstrating the tab system
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::layout::TabContent;

/// Simple text display tab
pub struct TextTab {
    title: String,
    content: String,
    focused: bool,
}

impl TextTab {
    pub fn new(title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            content: content.into(),
            focused: false,
        }
    }
}

impl TabContent for TextTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::White
        };

        let block = Block::default()
            .title(self.title.clone())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new(self.content.clone())
            .block(block)
            .style(Style::default().fg(Color::White));

        f.render_widget(paragraph, area);
    }

    fn on_focus(&mut self) {
        self.focused = true;
    }

    fn on_blur(&mut self) {
        self.focused = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn test_text_tab_focus_state() {
        let mut tab = TextTab::new("Test", "Content");
        assert!(!tab.focused);

        tab.on_focus();
        assert!(tab.focused);

        tab.on_blur();
        assert!(!tab.focused);
    }

    #[test]
    fn test_text_tab_render() {
        let mut tab = TextTab::new("Test Tab", "Hello, World!");
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = f.area();
                tab.render(f, area);
            })
            .unwrap();

        // Just verify it doesn't panic
    }
}
