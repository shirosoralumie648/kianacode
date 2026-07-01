use ratatui::style::{Color, Style};
use ratatui::widgets::{Gauge, LineGauge};

pub struct ProgressBar {
    pub progress: f64,
    pub label: Option<String>,
    pub color: Color,
}

impl ProgressBar {
    pub fn new(progress: f64) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            label: None,
            color: Color::Cyan,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn render(&self) -> Gauge<'static> {
        let ratio = (self.progress * 100.0) as u16;
        let label = self.label.clone().unwrap_or_else(|| format!("{}%", ratio));

        Gauge::default()
            .gauge_style(Style::default().fg(self.color))
            .label(label)
            .ratio(self.progress)
    }

    pub fn render_line(&self) -> LineGauge<'static> {
        let ratio = (self.progress * 100.0) as u16;
        let label = self.label.clone().unwrap_or_else(|| format!("{}%", ratio));

        LineGauge::default()
            .gauge_style(Style::default().fg(self.color))
            .label(label)
            .ratio(self.progress)
    }
}
