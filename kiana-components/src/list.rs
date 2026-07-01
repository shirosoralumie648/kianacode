use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub struct ListItem {
    pub content: String,
    pub description: Option<String>,
    pub is_focused: bool,
    pub is_selected: bool,
    pub disabled: bool,
}

impl ListItem {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            description: None,
            is_focused: false,
            is_selected: false,
            disabled: false,
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

        let indicator = Span::styled(
            self.get_indicator(),
            Style::default().fg(if self.is_focused {
                Color::Cyan
            } else {
                Color::White
            }),
        );

        let content = Span::styled(
            format!(" {}", self.content),
            Style::default().fg(self.get_text_color()),
        );

        let check = if self.is_selected && !self.disabled {
            Span::styled(" ✓", Style::default().fg(Color::Green))
        } else {
            Span::raw("")
        };

        lines.push(Line::from(vec![indicator, content, check]));

        if let Some(desc) = &self.description {
            lines.push(Line::from(Span::styled(
                format!("  {}", desc),
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines
    }
}
