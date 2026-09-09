use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub struct ConfirmDialog {
    message: String,
    selected_button: Button,
    on_confirm: Option<Box<dyn Fn() + Send>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Yes,
    No,
}

impl ConfirmDialog {
    pub fn new(message: String) -> Self {
        Self {
            message,
            selected_button: Button::No, // Default to safe option
            on_confirm: None,
        }
    }

    pub fn with_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + 'static,
    {
        self.on_confirm = Some(Box::new(callback));
        self
    }

    pub fn next_button(&mut self) {
        self.selected_button = match self.selected_button {
            Button::Yes => Button::No,
            Button::No => Button::Yes,
        };
    }

    pub fn previous_button(&mut self) {
        self.next_button(); // Only 2 buttons, so next = previous
    }

    pub fn confirm(&self) -> bool {
        self.selected_button == Button::Yes
    }

    pub fn execute_callback(&self) {
        if let Some(ref callback) = self.on_confirm {
            callback();
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let dialog_width = 50;
        let dialog_height = 7;

        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        // Clear background
        f.render_widget(Clear, dialog_area);

        // Create block
        let block = Block::default()
            .title("Confirm")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let inner = block.inner(dialog_area);
        f.render_widget(block, dialog_area);

        // Split into message and buttons
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(inner);

        // Render message
        let message = Paragraph::new(self.message.clone())
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(message, chunks[0]);

        // Render buttons
        let button_area = chunks[1];
        let button_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(button_area);

        // Yes button
        let yes_style = if self.selected_button == Button::Yes {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };

        let yes_button = Paragraph::new("[ Yes ]")
            .alignment(Alignment::Center)
            .style(yes_style);
        f.render_widget(yes_button, button_layout[0]);

        // No button
        let no_style = if self.selected_button == Button::No {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };

        let no_button = Paragraph::new("[ No ]")
            .alignment(Alignment::Center)
            .style(no_style);
        f.render_widget(no_button, button_layout[1]);
    }
}

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((r.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialog_creation() {
        let dialog = ConfirmDialog::new("Delete this?".into());
        assert_eq!(dialog.message, "Delete this?");
        assert_eq!(dialog.selected_button, Button::No);
    }

    #[test]
    fn test_button_navigation() {
        let mut dialog = ConfirmDialog::new("Test".into());
        assert_eq!(dialog.selected_button, Button::No);

        dialog.next_button();
        assert_eq!(dialog.selected_button, Button::Yes);

        dialog.next_button();
        assert_eq!(dialog.selected_button, Button::No);
    }

    #[test]
    fn test_confirm_result() {
        let mut dialog = ConfirmDialog::new("Test".into());
        assert!(!dialog.confirm()); // Default is No

        dialog.next_button(); // Switch to Yes
        assert!(dialog.confirm());
    }

    #[test]
    fn test_callback_execution() {
        use std::sync::{Arc, Mutex};

        let executed = Arc::new(Mutex::new(false));
        let executed_clone = executed.clone();

        let dialog = ConfirmDialog::new("Test".into()).with_callback(move || {
            *executed_clone.lock().unwrap() = true;
        });

        assert!(!*executed.lock().unwrap());
        dialog.execute_callback();
        assert!(*executed.lock().unwrap());
    }

    #[test]
    fn test_previous_button() {
        let mut dialog = ConfirmDialog::new("Test".into());
        assert_eq!(dialog.selected_button, Button::No);

        dialog.previous_button();
        assert_eq!(dialog.selected_button, Button::Yes);

        dialog.previous_button();
        assert_eq!(dialog.selected_button, Button::No);
    }

    #[test]
    fn test_centered_rect() {
        let area = Rect::new(0, 0, 100, 50);
        let result = centered_rect(50, 10, area);

        // Should be centered horizontally and vertically
        assert_eq!(result.width, 50);
        assert_eq!(result.height, 10);
        assert_eq!(result.x, (100 - 50) / 2);
        assert_eq!(result.y, (50 - 10) / 2);
    }

    #[test]
    fn test_callback_not_executed_when_none() {
        let dialog = ConfirmDialog::new("Test".into());
        // Should not panic when no callback is set
        dialog.execute_callback();
    }
}
