use crate::screen::{Screen, StylePool};
use crate::styles::TextStyle;
use ratatui::style::{Modifier, Style as RatatuiStyle};

pub struct Output {
    screen: Screen,
    style_pool: StylePool,
}

impl Output {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            screen: Screen::new(width, height),
            style_pool: StylePool::new(),
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.screen.clear();
    }

    pub fn write_text(&mut self, x: usize, y: usize, text: &str, text_style: &TextStyle) {
        let mut style = RatatuiStyle::default();

        if let Some(color) = &text_style.color {
            style = style.fg(color.to_ratatui());
        }
        if let Some(bg) = &text_style.bg_color {
            style = style.bg(bg.to_ratatui());
        }
        if text_style.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if text_style.dim {
            style = style.add_modifier(Modifier::DIM);
        }
        if text_style.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if text_style.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if text_style.strikethrough {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }

        let style_id = self.style_pool.intern(style);
        self.screen.write_text(x, y, text, style_id);
    }

    pub fn get_screen(&self) -> &Screen {
        &self.screen
    }

    pub fn get_style(&self, id: usize) -> Option<&RatatuiStyle> {
        self.style_pool.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styles::TextStyle;

    #[test]
    fn clear_resets_rendered_output() {
        let mut output = Output::new(4, 2);
        output.write_text(0, 0, "x", &TextStyle::default());

        output.clear();

        assert_eq!(output.get_screen().cells[0][0].content, "");
    }
}
