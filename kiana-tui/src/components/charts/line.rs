/// Line chart component
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use super::common::{
    auto_scale, format_number, generate_ticks, map_value, ChartStyle, DataPoint, ScalingMode,
};

/// Line interpolation style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Step function (no interpolation)
    Step,
    /// Linear interpolation with box drawing characters
    Linear,
}

/// Line chart component
pub struct LineChart {
    data: Vec<DataPoint>,
    scaling: ScalingMode,
    style: ChartStyle,
    interpolation: Interpolation,
}

impl LineChart {
    pub fn new(data: Vec<DataPoint>) -> Self {
        Self {
            data,
            scaling: ScalingMode::Auto,
            style: ChartStyle::default(),
            interpolation: Interpolation::Linear,
        }
    }

    pub fn with_data(mut self, data: Vec<DataPoint>) -> Self {
        self.data = data;
        self
    }

    pub fn with_scaling(mut self, scaling: ScalingMode) -> Self {
        self.scaling = scaling;
        self
    }

    pub fn with_style(mut self, style: ChartStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_interpolation(mut self, interpolation: Interpolation) -> Self {
        self.interpolation = interpolation;
        self
    }

    fn get_scale_range(&self) -> (f64, f64) {
        match self.scaling {
            ScalingMode::Auto => {
                let values: Vec<f64> = self.data.iter().map(|d| d.value).collect();
                auto_scale(&values)
            }
            ScalingMode::Fixed { min, max } => (min, max),
        }
    }
}

impl Widget for LineChart {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.data.is_empty() || area.width < 10 || area.height < 5 {
            return;
        }

        let (min_val, max_val) = self.get_scale_range();

        // Reserve space for axes
        let y_axis_width = 6;
        let x_axis_height = if self.style.show_labels { 2 } else { 1 };

        let plot_width = area.width.saturating_sub(y_axis_width) as usize;
        let plot_height = area.height.saturating_sub(x_axis_height) as usize;

        if plot_width < 2 || plot_height < 2 {
            return;
        }

        let plot_x = area.x + y_axis_width;
        let plot_y = area.y;

        // Draw Y-axis
        let y_ticks = generate_ticks(min_val, max_val, plot_height.min(5));
        for tick_val in y_ticks.iter() {
            let y_pos = plot_height - map_value(*tick_val, min_val, max_val, plot_height);
            if y_pos < plot_height {
                let label = format!("{:>5}", format_number(*tick_val));
                buf.set_string(
                    area.x,
                    plot_y + y_pos as u16,
                    label,
                    Style::default().fg(Color::DarkGray),
                );
                buf.set_string(
                    area.x + 5,
                    plot_y + y_pos as u16,
                    "┤",
                    Style::default().fg(Color::DarkGray),
                );
            }
        }

        // Draw X-axis
        let axis_y = plot_y + plot_height as u16;
        buf.set_string(
            area.x + y_axis_width - 1,
            axis_y,
            "└",
            Style::default().fg(Color::DarkGray),
        );
        for x in 0..plot_width {
            buf.set_string(
                plot_x + x as u16,
                axis_y,
                "─",
                Style::default().fg(Color::DarkGray),
            );
        }

        // Plot data points
        if self.data.len() == 1 {
            // Single point
            let y = plot_height - map_value(self.data[0].value, min_val, max_val, plot_height);
            if y < plot_height {
                buf.set_string(
                    plot_x,
                    plot_y + y as u16,
                    "●",
                    Style::default().fg(self.style.colors[0]),
                );
            }
        } else {
            // Multiple points - draw line
            let step = (self.data.len() - 1) as f64 / plot_width.max(1) as f64;
            let color = self.style.colors[0];

            for x in 0..plot_width {
                let data_idx = (x as f64 * step) as usize;
                if data_idx >= self.data.len() {
                    break;
                }

                let value = self.data[data_idx].value;
                let y = plot_height - map_value(value, min_val, max_val, plot_height);

                if y < plot_height {
                    let ch = match self.interpolation {
                        Interpolation::Step => "█",
                        Interpolation::Linear => {
                            // Use box drawing characters for smoother appearance
                            if x > 0 && data_idx > 0 {
                                let prev_value = self.data[data_idx - 1].value;
                                let prev_y = plot_height
                                    - map_value(prev_value, min_val, max_val, plot_height);

                                if prev_y < y {
                                    "╭" // Going down
                                } else if prev_y > y {
                                    "╰" // Going up
                                } else {
                                    "─" // Flat
                                }
                            } else {
                                "●"
                            }
                        }
                    };

                    buf.set_string(
                        plot_x + x as u16,
                        plot_y + y as u16,
                        ch,
                        Style::default().fg(color),
                    );
                }
            }
        }

        // Draw labels on X-axis
        if self.style.show_labels && !self.data.is_empty() {
            let label_y = axis_y + 1;

            // First label
            let first_label = &self.data[0].label;
            buf.set_string(
                plot_x,
                label_y,
                first_label,
                Style::default().fg(Color::Gray),
            );

            // Last label
            if self.data.len() > 1 {
                let last_label = &self.data[self.data.len() - 1].label;
                let last_x = (plot_x + plot_width as u16).saturating_sub(last_label.len() as u16);
                buf.set_string(
                    last_x,
                    label_y,
                    last_label,
                    Style::default().fg(Color::Gray),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_chart_creation() {
        let data = vec![
            DataPoint::new("0", 10.0),
            DataPoint::new("1", 20.0),
            DataPoint::new("2", 15.0),
        ];

        let chart = LineChart::new(data);
        assert_eq!(chart.data.len(), 3);
        assert_eq!(chart.interpolation, Interpolation::Linear);
    }

    #[test]
    fn test_line_chart_builders() {
        let chart = LineChart::new(vec![])
            .with_data(vec![DataPoint::new("Test", 42.0)])
            .with_interpolation(Interpolation::Step)
            .with_scaling(ScalingMode::Fixed {
                min: 0.0,
                max: 100.0,
            });

        assert_eq!(chart.data.len(), 1);
        assert_eq!(chart.interpolation, Interpolation::Step);
    }
}
