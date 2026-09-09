use ratatui::style::Style as RatatuiStyle;

#[derive(Debug, Clone, Default)]
pub struct Cell {
    pub content: String,
    pub style_id: usize,
}

pub struct Screen {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Vec<Cell>>,
}

impl Screen {
    pub fn new(width: usize, height: usize) -> Self {
        let cells = vec![vec![Cell::default(); width]; height];
        Self {
            width,
            height,
            cells,
        }
    }

    pub fn set_cell(&mut self, x: usize, y: usize, content: String, style_id: usize) {
        if y < self.height && x < self.width {
            self.cells[y][x] = Cell { content, style_id };
        }
    }

    pub fn write_text(&mut self, x: usize, y: usize, text: &str, style_id: usize) {
        let mut col = x;
        for ch in text.chars() {
            if ch == '\n' {
                break;
            }
            if col >= self.width {
                break;
            }
            self.set_cell(col, y, ch.to_string(), style_id);
            col += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row {
                *cell = Cell::default();
            }
        }
    }
}

pub struct StylePool {
    styles: Vec<RatatuiStyle>,
}

impl StylePool {
    pub fn new() -> Self {
        Self {
            styles: vec![RatatuiStyle::default()],
        }
    }

    pub fn intern(&mut self, style: RatatuiStyle) -> usize {
        if let Some(pos) = self.styles.iter().position(|s| s == &style) {
            return pos;
        }
        self.styles.push(style);
        self.styles.len() - 1
    }

    pub fn get(&self, id: usize) -> Option<&RatatuiStyle> {
        self.styles.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_resets_screen_cells() {
        let mut screen = Screen::new(4, 2);
        screen.set_cell(1, 1, "x".to_string(), 3);

        screen.clear();

        assert_eq!(screen.cells[1][1].content, "");
        assert_eq!(screen.cells[1][1].style_id, 0);
    }
}
