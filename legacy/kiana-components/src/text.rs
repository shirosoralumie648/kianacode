use crate::theme::{Theme, ThemedStyle};
use ratatui::text::{Line, Span};

pub struct Text {
    content: String,
    style: ThemedStyle,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: ThemedStyle::new(),
        }
    }

    pub fn color(mut self, color: ratatui::style::Color) -> Self {
        self.style = self.style.color(color);
        self
    }

    pub fn bold(mut self) -> Self {
        self.style = self.style.bold();
        self
    }

    pub fn dim(mut self) -> Self {
        self.style = self.style.dim();
        self
    }

    pub fn italic(mut self) -> Self {
        self.style = self.style.italic();
        self
    }

    pub fn to_span(&self, theme: &Theme) -> Span<'_> {
        Span::styled(self.content.clone(), self.style.to_ratatui(theme))
    }

    pub fn to_line(&self, theme: &Theme) -> Line<'_> {
        Line::from(self.to_span(theme))
    }
}
