use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};

pub struct Dialog {
    pub title: String,
    pub subtitle: Option<String>,
    pub content: Vec<String>,
    pub color: Color,
    pub hide_border: bool,
}

impl Dialog {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            content: vec![],
            color: Color::Yellow,
            hide_border: false,
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn content(mut self, content: Vec<String>) -> Self {
        self.content = content;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn hide_border(mut self) -> Self {
        self.hide_border = true;
        self
    }

    pub fn render(&self) -> Vec<Line<'static>> {
        let mut lines = vec![];

        // Title
        lines.push(Line::from(Span::styled(
            self.title.clone(),
            Style::default()
                .fg(self.color)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));

        // Subtitle if present
        if let Some(subtitle) = &self.subtitle {
            lines.push(Line::from(Span::styled(
                subtitle.clone(),
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Empty line
        lines.push(Line::from(""));

        // Content
        for line in &self.content {
            lines.push(Line::from(line.clone()));
        }

        lines
    }

    pub fn render_with_border(&self, _area: Rect) -> Block<'static> {
        if self.hide_border {
            Block::default()
        } else {
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.color))
        }
    }
}
