/// Sparkline component for compact trend visualization
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// Sparkline style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SparklineStyle {
    /// Block characters: ▁▂▃▄▅▆▇█
    Block,
    /// Braille characters for high density
    Braille,
}

/// Sparkline component (compact trend visualization)
pub struct Sparkline {
    data: Vec<f64>,
    style: SparklineStyle,
    color: Color,
}

impl Sparkline {
    pub fn new(data: Vec<f64>) -> Self {
        Self {
            data,
            style: SparklineStyle::Block,
            color: Color::Cyan,
        }
    }

    pub fn with_style(mut self, style: SparklineStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Render inline (returns string for embedding in text)
    pub fn render_inline(&self) -> String {
        if self.data.is_empty() {
            return String::new();
        }

        let chars = match self.style {
            SparklineStyle::Block => Self::render_block(&self.data),
            SparklineStyle::Braille => Self::render_braille(&self.data),
        };

        chars
    }

    fn render_block(data: &[f64]) -> String {
        const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

        if data.is_empty() {
            return String::new();
        }

        // Find min/max for normalization
        let mut min = data[0];
        let mut max = data[0];
        for &v in data {
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }

        let range = max - min;

        data.iter()
            .map(|&v| {
                if range < f64::EPSILON {
                    BLOCKS[4] // Middle block
                } else {
                    let normalized = ((v - min) / range * 7.0).round() as usize;
                    BLOCKS[normalized.min(7)]
                }
            })
            .collect()
    }

    fn render_braille(data: &[f64]) -> String {
        // Braille patterns for vertical bars
        // Using a simplified approach with vertical braille dots
        const BRAILLE_BASE: u32 = 0x2800;
        const BRAILLE_PATTERNS: [u32; 8] = [
            0x00, // ⠀ empty
            0x40, // ⡀ dot 7
            0x44, // ⡄ dots 3,7
            0x46, // ⡆ dots 2,3,7
            0x47, // ⡇ dots 1,2,3,7
            0x67, // ⡧ dots 1,2,3,6,7
            0x77, // ⡷ dots 1,2,3,5,6,7
            0x7F, // ⡿ all dots
        ];

        if data.is_empty() {
            return String::new();
        }

        // Find min/max for normalization
        let mut min = data[0];
        let mut max = data[0];
        for &v in data {
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }

        let range = max - min;

        data.iter()
            .map(|&v| {
                if range < f64::EPSILON {
                    std::char::from_u32(BRAILLE_BASE + BRAILLE_PATTERNS[4]).unwrap_or('⠀')
                } else {
                    let normalized = ((v - min) / range * 7.0).round() as usize;
                    std::char::from_u32(BRAILLE_BASE + BRAILLE_PATTERNS[normalized.min(7)])
                        .unwrap_or('⠀')
                }
            })
            .collect()
    }
}

impl Widget for Sparkline {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.data.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }

        let sparkline_str = self.render_inline();
        let display_str = if sparkline_str.len() > area.width as usize {
            &sparkline_str[..area.width as usize]
        } else {
            &sparkline_str
        };

        buf.set_string(area.x, area.y, display_str, Style::default().fg(self.color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparkline_creation() {
        let data = vec![1.0, 2.0, 3.0, 2.0, 1.0];
        let sparkline = Sparkline::new(data);
        assert_eq!(sparkline.data.len(), 5);
        assert_eq!(sparkline.style, SparklineStyle::Block);
    }

    #[test]
    fn test_sparkline_render_inline_block() {
        let data = vec![1.0, 3.0, 5.0, 7.0, 9.0];
        let sparkline = Sparkline::new(data).with_style(SparklineStyle::Block);
        let result = sparkline.render_inline();
        assert!(!result.is_empty());
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn test_sparkline_render_inline_braille() {
        let data = vec![1.0, 3.0, 5.0, 7.0, 9.0];
        let sparkline = Sparkline::new(data).with_style(SparklineStyle::Braille);
        let result = sparkline.render_inline();
        assert!(!result.is_empty());
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn test_sparkline_empty_data() {
        let sparkline = Sparkline::new(vec![]);
        let result = sparkline.render_inline();
        assert!(result.is_empty());
    }

    #[test]
    fn test_sparkline_single_value() {
        let data = vec![42.0];
        let sparkline = Sparkline::new(data);
        let result = sparkline.render_inline();
        assert_eq!(result.chars().count(), 1);
    }

    #[test]
    fn test_sparkline_same_values() {
        let data = vec![5.0, 5.0, 5.0, 5.0];
        let sparkline = Sparkline::new(data);
        let result = sparkline.render_inline();
        // All same values should render middle block
        assert!(result.chars().all(|c| c == '▄' || c == '▅'));
    }
}
