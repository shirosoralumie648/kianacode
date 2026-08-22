use kiana_tui::layout::{SplitContent, SplitDirection, SplitManager};
use ratatui::{layout::Rect, style::Color, text::Line, widgets::Paragraph, Frame};

struct TestContent {
    text: String,
}

impl SplitContent for TestContent {
    fn render(&mut self, f: &mut Frame, area: Rect, _focused: bool) {
        let paragraph = Paragraph::new(Line::from(self.text.clone()));
        f.render_widget(paragraph, area);
    }
}

#[test]
fn test_split_manager_basic() {
    let mut manager = SplitManager::new();

    let content = TestContent {
        text: "Test".to_string(),
    };

    let id = manager.set_root(Box::new(content));
    assert_eq!(manager.split_count(), 1);
    assert_eq!(manager.focused_id(), Some(id));
}

#[test]
fn test_split_horizontal() {
    let mut manager = SplitManager::new();

    let content1 = TestContent {
        text: "Split 1".to_string(),
    };
    manager.set_root(Box::new(content1));

    let content2 = TestContent {
        text: "Split 2".to_string(),
    };
    let new_id = manager.split(SplitDirection::Horizontal, Box::new(content2));

    assert!(new_id.is_some());
    assert_eq!(manager.split_count(), 2);
}

#[test]
fn test_split_vertical() {
    let mut manager = SplitManager::new();

    let content1 = TestContent {
        text: "Split 1".to_string(),
    };
    manager.set_root(Box::new(content1));

    let content2 = TestContent {
        text: "Split 2".to_string(),
    };
    let new_id = manager.split(SplitDirection::Vertical, Box::new(content2));

    assert!(new_id.is_some());
    assert_eq!(manager.split_count(), 2);
}
