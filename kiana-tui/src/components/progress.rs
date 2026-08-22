//! Progress bar component for determinate progress display
//!
//! This module provides a progress bar component that displays determinate
//! progress from 0% to 100% with customizable styling and state-based coloring.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Progress state affects the color of the progress bar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    /// Normal progress (green)
    Normal,
    /// Warning state (yellow)
    Warning,
    /// Error state (red)
    Error,
}

impl ProgressState {
    /// Returns the color associated with this progress state
    pub fn color(&self) -> Color {
        match self {
            Self::Normal => Color::Green,
            Self::Warning => Color::Yellow,
            Self::Error => Color::Red,
        }
    }
}

/// Style configuration for progress bar
#[derive(Debug, Clone)]
pub struct ProgressStyle {
    /// Width of the progress bar (if None, fills available space)
    pub width: Option<u16>,
    /// Show percentage text
    pub show_percentage: bool,
    /// Show label text
    pub show_label: bool,
    /// Character for filled portion of bar
    pub filled_char: char,
    /// Character for empty portion of bar
    pub empty_char: char,
}

impl Default for ProgressStyle {
    fn default() -> Self {
        Self {
            width: None,
            show_percentage: true,
            show_label: true,
            filled_char: '█',
            empty_char: '░',
        }
    }
}

/// Progress bar component for displaying determinate progress
#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// Current progress value (0.0 to 1.0)
    progress: f64,
    /// Label text
    label: String,
    /// Progress state (affects color)
    state: ProgressState,
    /// Style configuration
    style: ProgressStyle,
}

impl ProgressBar {
    /// Creates a new progress bar with the given label
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            progress: 0.0,
            label: label.into(),
            state: ProgressState::Normal,
            style: ProgressStyle::default(),
        }
    }

    /// Sets the progress value (0.0 to 1.0 or 0 to 100)
    /// Values > 1.0 are treated as percentages (0-100)
    pub fn progress(mut self, value: f64) -> Self {
        self.set_progress(value);
        self
    }

    /// Sets the progress state
    pub fn state(mut self, state: ProgressState) -> Self {
        self.state = state;
        self
    }

    /// Sets the style configuration
    pub fn style(mut self, style: ProgressStyle) -> Self {
        self.style = style;
        self
    }

    /// Updates the progress value
    /// Values 0.0-1.0 are treated as decimal (0%-100%)
    /// Values > 1.0 are treated as percentage (0-100) and divided by 100
    /// All values are clamped to 0.0-1.0 range
    pub fn set_progress(&mut self, value: f64) {
        // Handle negative values
        if value < 0.0 {
            self.progress = 0.0;
            return;
        }

        // Values > 1.0 are treated as percentages
        if value > 1.0 {
            self.progress = (value / 100.0).min(1.0);
        } else {
            // 0.0-1.0 range
            self.progress = value;
        }
    }

    /// Updates the progress state
    pub fn set_state(&mut self, state: ProgressState) {
        self.state = state;
    }

    /// Updates the label
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    /// Gets the current progress as a percentage (0-100)
    pub fn percentage(&self) -> u8 {
        (self.progress * 100.0).round() as u8
    }

    /// Renders the progress bar to the given frame area
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let color = self.state.color();

        // Calculate bar width
        let bar_width = self.style.width.unwrap_or(area.width) as usize;
        let filled_width = (bar_width as f64 * self.progress).round() as usize;
        let empty_width = bar_width.saturating_sub(filled_width);

        // Build progress bar string
        let bar = format!(
            "{}{}",
            self.style.filled_char.to_string().repeat(filled_width),
            self.style.empty_char.to_string().repeat(empty_width)
        );

        // Build complete line with label and percentage
        let mut spans = Vec::new();

        if self.style.show_label && !self.label.is_empty() {
            spans.push(Span::raw(&self.label));
            spans.push(Span::raw(" "));
        }

        spans.push(Span::styled(&bar, Style::default().fg(color)));

        if self.style.show_percentage {
            spans.push(Span::raw(format!(" {}%", self.percentage())));
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line);

        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_normalization() {
        let mut bar = ProgressBar::new("Test");

        // Test decimal values
        bar.set_progress(0.0);
        assert_eq!(bar.percentage(), 0);

        bar.set_progress(0.5);
        assert_eq!(bar.percentage(), 50);

        bar.set_progress(1.0);
        assert_eq!(bar.percentage(), 100);

        // Test percentage values
        bar.set_progress(25.0);
        assert_eq!(bar.percentage(), 25);

        bar.set_progress(75.0);
        assert_eq!(bar.percentage(), 75);

        bar.set_progress(100.0);
        assert_eq!(bar.percentage(), 100);
    }

    #[test]
    fn test_progress_clamping() {
        let mut bar = ProgressBar::new("Test");

        // Test negative values
        bar.set_progress(-0.5);
        assert_eq!(bar.percentage(), 0);

        // Test over-range values (>100 as percentage)
        bar.set_progress(1.5);
        assert_eq!(bar.percentage(), 2); // 1.5% rounded to 2%

        bar.set_progress(150.0);
        assert_eq!(bar.percentage(), 100); // 150% clamped to 100%
    }

    #[test]
    fn test_state_colors() {
        assert_eq!(ProgressState::Normal.color(), Color::Green);
        assert_eq!(ProgressState::Warning.color(), Color::Yellow);
        assert_eq!(ProgressState::Error.color(), Color::Red);
    }

    #[test]
    fn test_builder_pattern() {
        let bar = ProgressBar::new("Test")
            .progress(0.5)
            .state(ProgressState::Warning);

        assert_eq!(bar.percentage(), 50);
        assert_eq!(bar.state, ProgressState::Warning);
    }
}
