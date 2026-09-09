use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub claude: Color,
    pub claude_shimmer: Color,
    pub text: Color,
    pub inactive: Color,
    pub suggestion: Color,
    pub success: Color,
    pub error: Color,
    pub permission: Color,
    pub subtle: Color,
    pub background: Color,
}

impl Theme {
    pub fn default() -> Self {
        Self {
            claude: Color::Rgb(204, 143, 92),
            claude_shimmer: Color::Rgb(255, 179, 115),
            text: Color::White,
            inactive: Color::Gray,
            suggestion: Color::Cyan,
            success: Color::Green,
            error: Color::Red,
            permission: Color::Yellow,
            subtle: Color::DarkGray,
            background: Color::Black,
        }
    }

    pub fn resolve_color(&self, key: &str) -> Color {
        match key {
            "claude" => self.claude,
            "claudeShimmer" => self.claude_shimmer,
            "text" => self.text,
            "inactive" => self.inactive,
            "suggestion" => self.suggestion,
            "success" => self.success,
            "error" => self.error,
            "permission" => self.permission,
            "subtle" => self.subtle,
            "background" => self.background,
            _ => self.text,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThemedStyle {
    pub color: Option<Color>,
    pub bg_color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
}

impl ThemedStyle {
    pub fn new() -> Self {
        Self {
            color: None,
            bg_color: None,
            bold: false,
            italic: false,
            underline: false,
            dim: false,
        }
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn bg_color(mut self, color: Color) -> Self {
        self.bg_color = Some(color);
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub fn to_ratatui(&self, _theme: &Theme) -> Style {
        let mut style = Style::default();

        if let Some(color) = self.color {
            style = style.fg(color);
        }

        if let Some(bg) = self.bg_color {
            style = style.bg(bg);
        }

        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }

        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }

        if self.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }

        if self.dim {
            style = style.add_modifier(Modifier::DIM);
        }

        style
    }
}

impl Default for ThemedStyle {
    fn default() -> Self {
        Self::new()
    }
}
