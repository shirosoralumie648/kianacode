use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

/// Scrollbar orientation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarOrientation {
    Vertical,
    Horizontal,
}

/// Scrollbar state tracking
#[derive(Debug, Clone)]
pub struct ScrollbarState {
    /// Total content length (lines or columns)
    pub content_length: usize,
    /// Current scroll position (0-based)
    pub position: usize,
    /// Visible viewport size
    pub viewport_size: usize,
}

impl ScrollbarState {
    pub fn new(content_length: usize) -> Self {
        Self {
            content_length,
            position: 0,
            viewport_size: 0,
        }
    }

    pub fn viewport_size(mut self, size: usize) -> Self {
        self.viewport_size = size;
        self
    }

    pub fn position(mut self, pos: usize) -> Self {
        self.position = pos.min(self.max_scroll());
        self
    }

    /// Maximum scroll position
    pub fn max_scroll(&self) -> usize {
        self.content_length.saturating_sub(self.viewport_size)
    }

    /// Calculate thumb position in the track
    pub fn thumb_position(&self, track_size: usize) -> usize {
        if self.content_length == 0 || track_size == 0 {
            return 0;
        }

        let max_scroll = self.max_scroll();
        if max_scroll == 0 {
            return 0;
        }

        // Position as ratio of total scrollable content
        let ratio = self.position as f64 / max_scroll as f64;
        let thumb_size = self.thumb_size(track_size);
        let max_thumb_pos = track_size.saturating_sub(thumb_size);
        (ratio * max_thumb_pos as f64).round() as usize
    }

    /// Calculate thumb size based on visible ratio
    pub fn thumb_size(&self, track_size: usize) -> usize {
        if self.content_length == 0 {
            return track_size;
        }

        let ratio = self.viewport_size as f64 / self.content_length as f64;
        let size = (ratio * track_size as f64).round() as usize;
        size.max(1).min(track_size)
    }

    /// Check if scrollbar should be visible
    pub fn is_visible(&self) -> bool {
        self.content_length > self.viewport_size
    }
}

/// Scrollbar widget
pub struct Scrollbar {
    orientation: ScrollbarOrientation,
    track_symbol: &'static str,
    thumb_symbol: &'static str,
    track_style: Style,
    thumb_style: Style,
}

impl Scrollbar {
    pub fn new(orientation: ScrollbarOrientation) -> Self {
        Self {
            orientation,
            track_symbol: match orientation {
                ScrollbarOrientation::Vertical => "│",
                ScrollbarOrientation::Horizontal => "─",
            },
            thumb_symbol: match orientation {
                ScrollbarOrientation::Vertical => "█",
                ScrollbarOrientation::Horizontal => "█",
            },
            track_style: Style::default().fg(Color::DarkGray),
            thumb_style: Style::default().fg(Color::Gray),
        }
    }

    pub fn track_symbol(mut self, symbol: &'static str) -> Self {
        self.track_symbol = symbol;
        self
    }

    pub fn thumb_symbol(mut self, symbol: &'static str) -> Self {
        self.thumb_symbol = symbol;
        self
    }

    pub fn track_style(mut self, style: Style) -> Self {
        self.track_style = style;
        self
    }

    pub fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = style;
        self
    }

    pub fn render(self, area: Rect, buf: &mut Buffer, state: &ScrollbarState) {
        if !state.is_visible() {
            return;
        }

        match self.orientation {
            ScrollbarOrientation::Vertical => self.render_vertical(area, buf, state),
            ScrollbarOrientation::Horizontal => self.render_horizontal(area, buf, state),
        }
    }

    fn render_vertical(&self, area: Rect, buf: &mut Buffer, state: &ScrollbarState) {
        let track_height = area.height as usize;
        let thumb_size = state.thumb_size(track_height);
        let thumb_pos = state.thumb_position(track_height);

        for y in 0..area.height {
            let cell_y = area.y + y;
            let y_pos = y as usize;
            let is_thumb = y_pos >= thumb_pos && y_pos < thumb_pos + thumb_size;

            let (symbol, style) = if is_thumb {
                (self.thumb_symbol, self.thumb_style)
            } else {
                (self.track_symbol, self.track_style)
            };

            buf.set_string(area.x, cell_y, symbol, style);
        }
    }

    fn render_horizontal(&self, area: Rect, buf: &mut Buffer, state: &ScrollbarState) {
        let track_width = area.width as usize;
        let thumb_size = state.thumb_size(track_width);
        let thumb_pos = state.thumb_position(track_width);

        for x in 0..area.width {
            let cell_x = area.x + x;
            let x_pos = x as usize;
            let is_thumb = x_pos >= thumb_pos && x_pos < thumb_pos + thumb_size;

            let (symbol, style) = if is_thumb {
                (self.thumb_symbol, self.thumb_style)
            } else {
                (self.track_symbol, self.track_style)
            };

            buf.set_string(cell_x, area.y, symbol, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrollbar_state_thumb_size() {
        // Viewport shows 50% of content
        let state = ScrollbarState::new(100).viewport_size(50).position(0);

        let track_size = 20;
        assert_eq!(state.thumb_size(track_size), 10); // 50% of 20
    }

    #[test]
    fn test_scrollbar_state_thumb_position() {
        // Scrolled to middle
        let state = ScrollbarState::new(100).viewport_size(20).position(40); // Middle of scrollable range (0-80)

        let track_size = 20;
        let thumb_size = state.thumb_size(track_size); // 4
        let max_thumb_pos = track_size - thumb_size; // 16
        let expected_pos = 8; // Middle of available positions (40/80 * 16)

        assert_eq!(state.thumb_position(track_size), expected_pos);
    }

    #[test]
    fn test_scrollbar_visibility() {
        // Content fits viewport
        let state = ScrollbarState::new(10).viewport_size(20);
        assert!(!state.is_visible());

        // Content exceeds viewport
        let state = ScrollbarState::new(20).viewport_size(10);
        assert!(state.is_visible());
    }

    #[test]
    fn test_scrollbar_edge_cases() {
        // Empty content
        let state = ScrollbarState::new(0).viewport_size(10);
        assert_eq!(state.thumb_size(20), 20);
        assert_eq!(state.thumb_position(20), 0);

        // Zero track size
        let state = ScrollbarState::new(100).viewport_size(10);
        assert_eq!(state.thumb_size(0), 0);
        assert_eq!(state.thumb_position(0), 0);
    }

    #[test]
    fn test_max_scroll() {
        let state = ScrollbarState::new(100).viewport_size(20);
        assert_eq!(state.max_scroll(), 80);

        // Content smaller than viewport
        let state = ScrollbarState::new(10).viewport_size(20);
        assert_eq!(state.max_scroll(), 0);
    }

    #[test]
    fn test_scrollbar_at_start() {
        let state = ScrollbarState::new(100).viewport_size(20).position(0);
        let track_size = 10;

        assert_eq!(state.thumb_position(track_size), 0);
    }

    #[test]
    fn test_scrollbar_at_end() {
        let state = ScrollbarState::new(100).viewport_size(20).position(80);
        let track_size = 10;
        let thumb_size = state.thumb_size(track_size); // 2

        // Should be at end of track
        assert_eq!(state.thumb_position(track_size), track_size - thumb_size);
    }

    #[test]
    fn test_position_clamping() {
        // Position beyond max scroll should be clamped
        let state = ScrollbarState::new(100).viewport_size(20).position(200);
        assert_eq!(state.position, 80); // max_scroll
    }
}
