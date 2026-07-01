use ratatui::style::Color;
use ratatui::text::Span;
use std::time::{Duration, Instant};

pub struct Spinner {
    frames: Vec<&'static str>,
    current_frame: usize,
    last_update: Instant,
    interval: Duration,
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            current_frame: 0,
            last_update: Instant::now(),
            interval: Duration::from_millis(80),
        }
    }

    pub fn tick(&mut self) {
        if self.last_update.elapsed() >= self.interval {
            self.current_frame = (self.current_frame + 1) % self.frames.len();
            self.last_update = Instant::now();
        }
    }

    pub fn render(&self, color: Color) -> Span<'static> {
        Span::styled(
            self.frames[self.current_frame].to_string(),
            ratatui::style::Style::default().fg(color),
        )
    }

    pub fn current_frame(&self) -> &str {
        self.frames[self.current_frame]
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinnerMode {
    Loading,
    Thinking,
    Working,
}

pub struct SpinnerWithVerb {
    spinner: Spinner,
    mode: SpinnerMode,
    verb: String,
}

impl SpinnerWithVerb {
    pub fn new(verb: impl Into<String>) -> Self {
        Self {
            spinner: Spinner::new(),
            mode: SpinnerMode::Working,
            verb: verb.into(),
        }
    }

    pub fn mode(mut self, mode: SpinnerMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn tick(&mut self) {
        self.spinner.tick();
    }

    pub fn render(&self, color: Color) -> Vec<Span<'static>> {
        vec![
            self.spinner.render(color),
            Span::raw(" "),
            Span::raw(format!("{}…", self.verb)),
        ]
    }
}
