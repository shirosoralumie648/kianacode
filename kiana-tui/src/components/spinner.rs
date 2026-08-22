//! Spinner component for indeterminate loading animation
//!
//! This module provides a spinner component that displays an animated loading
//! indicator with multiple style options for indeterminate progress scenarios.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::time::{Duration, Instant};

/// Spinner animation style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinnerStyle {
    /// Braille dots animation: ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏
    Dots,
    /// Simple line animation: |/-\
    Line,
    /// Braille blocks animation: ⣾⣽⣻⢿⡿⣟⣯⣷
    Braille,
}

impl SpinnerStyle {
    /// Returns the frame sequence for this spinner style
    pub fn frames(&self) -> &'static [&'static str] {
        match self {
            Self::Dots => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            Self::Line => &["|", "/", "-", "\\"],
            Self::Braille => &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"],
        }
    }

    /// Returns the duration each frame should be displayed
    pub fn frame_duration(&self) -> Duration {
        match self {
            Self::Dots => Duration::from_millis(80),
            Self::Line => Duration::from_millis(100),
            Self::Braille => Duration::from_millis(80),
        }
    }
}

impl Default for SpinnerStyle {
    fn default() -> Self {
        Self::Dots
    }
}

/// Spinner component for displaying indeterminate loading state
#[derive(Debug, Clone)]
pub struct Spinner {
    /// Current animation frame index
    frame_index: usize,
    /// Animation style
    style: SpinnerStyle,
    /// Last update time
    last_update: Instant,
    /// Optional label text
    label: Option<String>,
    /// Color for the spinner
    color: Color,
}

impl Spinner {
    /// Creates a new spinner with default style
    pub fn new() -> Self {
        Self {
            frame_index: 0,
            style: SpinnerStyle::default(),
            last_update: Instant::now(),
            label: None,
            color: Color::Cyan,
        }
    }

    /// Creates a spinner with the specified style
    pub fn with_style(style: SpinnerStyle) -> Self {
        Self {
            frame_index: 0,
            style,
            last_update: Instant::now(),
            label: None,
            color: Color::Cyan,
        }
    }

    /// Sets the label for this spinner
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the color for this spinner
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the label
    pub fn set_label(&mut self, label: Option<String>) {
        self.label = label;
    }

    /// Sets the style
    pub fn set_style(&mut self, style: SpinnerStyle) {
        self.style = style;
        self.frame_index = 0; // Reset frame when style changes
    }

    /// Checks if the spinner should update to the next frame
    pub fn should_update(&self) -> bool {
        self.last_update.elapsed() >= self.style.frame_duration()
    }

    /// Advances to the next frame if enough time has elapsed
    pub fn tick(&mut self) {
        if self.should_update() {
            let frames = self.style.frames();
            self.frame_index = (self.frame_index + 1) % frames.len();
            self.last_update = Instant::now();
        }
    }

    /// Forces an immediate frame advance (useful for testing)
    pub fn force_tick(&mut self) {
        let frames = self.style.frames();
        self.frame_index = (self.frame_index + 1) % frames.len();
        self.last_update = Instant::now();
    }

    /// Gets the current frame character
    pub fn current_frame(&self) -> &'static str {
        let frames = self.style.frames();
        frames[self.frame_index]
    }

    /// Renders the spinner to the given frame area
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mut spans = Vec::new();

        // Add spinner animation
        spans.push(Span::styled(
            self.current_frame(),
            Style::default().fg(self.color),
        ));

        // Add label if present
        if let Some(ref label) = self.label {
            spans.push(Span::raw(" "));
            spans.push(Span::raw(label));
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line);

        frame.render_widget(paragraph, area);
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_spinner_frames() {
        let dots = SpinnerStyle::Dots.frames();
        assert_eq!(dots.len(), 10);
        assert_eq!(dots[0], "⠋");

        let line = SpinnerStyle::Line.frames();
        assert_eq!(line.len(), 4);
        assert_eq!(line[0], "|");

        let braille = SpinnerStyle::Braille.frames();
        assert_eq!(braille.len(), 8);
        assert_eq!(braille[0], "⣾");
    }

    #[test]
    fn test_spinner_cycle() {
        let mut spinner = Spinner::new();
        let initial_frame = spinner.current_frame();

        // Force several ticks
        for _ in 0..10 {
            spinner.force_tick();
        }

        // Should cycle back to start
        assert_eq!(spinner.current_frame(), initial_frame);
    }

    #[test]
    fn test_spinner_style_change() {
        let mut spinner = Spinner::new();
        assert_eq!(spinner.style, SpinnerStyle::Dots);

        spinner.set_style(SpinnerStyle::Line);
        assert_eq!(spinner.style, SpinnerStyle::Line);
        assert_eq!(spinner.frame_index, 0); // Should reset
    }

    #[test]
    fn test_spinner_timing() {
        let mut spinner = Spinner::new();

        // Should not update immediately
        assert!(!spinner.should_update());

        // Wait for frame duration
        thread::sleep(spinner.style.frame_duration() + Duration::from_millis(10));

        // Should now update
        assert!(spinner.should_update());
    }

    #[test]
    fn test_spinner_label() {
        let spinner = Spinner::new().with_label("Loading...");
        assert_eq!(spinner.label, Some("Loading...".to_string()));

        let mut spinner = Spinner::new();
        spinner.set_label(Some("Processing".to_string()));
        assert_eq!(spinner.label, Some("Processing".to_string()));
    }

    #[test]
    fn test_builder_pattern() {
        let spinner = Spinner::with_style(SpinnerStyle::Braille)
            .with_label("Working")
            .with_color(Color::Yellow);

        assert_eq!(spinner.style, SpinnerStyle::Braille);
        assert_eq!(spinner.label, Some("Working".to_string()));
        assert_eq!(spinner.color, Color::Yellow);
    }
}
