use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub struct ListItem {
    pub content: String,
    pub description: Option<String>,
    pub is_focused: bool,
    pub is_selected: bool,
    pub disabled: bool,
    pub jump_number: Option<u8>,
}

impl ListItem {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            description: None,
            is_focused: false,
            is_selected: false,
            disabled: false,
            jump_number: None,
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.is_focused = focused;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.is_selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_jump_number(mut self, jump_number: Option<u8>) -> Self {
        self.jump_number = jump_number;
        self
    }

    fn get_indicator(&self) -> &'static str {
        if self.disabled {
            " "
        } else if self.is_focused {
            "❯"
        } else {
            " "
        }
    }

    fn get_text_color(&self) -> Color {
        if self.disabled {
            Color::DarkGray
        } else if self.is_selected {
            Color::Green
        } else if self.is_focused {
            Color::Cyan
        } else {
            Color::White
        }
    }

    pub fn render(&self) -> Vec<Line<'static>> {
        let mut lines = vec![];
        let mut spans = vec![];

        // Add jump number if present
        if let Some(num) = self.jump_number {
            spans.push(Span::styled(
                format!("{} ", num),
                Style::default().fg(Color::Yellow),
            ));
        }

        // Add indicator
        spans.push(Span::styled(
            self.get_indicator(),
            Style::default().fg(if self.is_focused {
                Color::Cyan
            } else {
                Color::White
            }),
        ));

        // Add content
        spans.push(Span::styled(
            format!(" {}", self.content),
            Style::default().fg(self.get_text_color()),
        ));

        // Add check mark if selected
        if self.is_selected && !self.disabled {
            spans.push(Span::styled(" ✓", Style::default().fg(Color::Green)));
        }

        lines.push(Line::from(spans));

        if let Some(desc) = &self.description {
            lines.push(Line::from(Span::styled(
                format!("  {}", desc),
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines
    }
}

/// Assigns jump numbers (1-9) to a slice of ListItems starting from a given index
pub fn assign_jump_numbers(items: &mut [ListItem], start_idx: usize, visible_count: usize) {
    let max_jump_numbers = 9;
    let count = visible_count
        .min(max_jump_numbers)
        .min(items.len() - start_idx);

    // Clear all jump numbers first
    for item in items.iter_mut() {
        item.jump_number = None;
    }

    // Assign numbers to visible items
    for i in 0..count {
        if start_idx + i < items.len() {
            items[start_idx + i].jump_number = Some((i + 1) as u8);
        }
    }
}

/// Finds the target index for a given jump number
/// Returns None if the jump number is invalid or out of bounds
pub fn find_jump_target(jump_num: u8, start_idx: usize, total_items: usize) -> Option<usize> {
    // Jump numbers must be 1-9
    if jump_num < 1 || jump_num > 9 {
        return None;
    }

    let target_idx = start_idx + (jump_num as usize) - 1;

    if target_idx < total_items {
        Some(target_idx)
    } else {
        None
    }
}
