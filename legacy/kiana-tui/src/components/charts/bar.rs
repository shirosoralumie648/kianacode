/// Bar chart component
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use super::common::{auto_scale, format_number, map_value, ChartStyle, DataPoint, ScalingMode};

/// Bar chart orientation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarOrientation {
    Horizontal,
    Vertical,
}

/// Bar chart component
pub struct BarChart {
    data: Vec<DataPoint>,
    orientation: BarOrientation,
    scaling: ScalingMode,
    style: ChartStyle,
}

impl BarChart {
    pub fn new(data: Vec<DataPoint>) -> Self {
        Self {
            data,
            orientation: BarOrientation::Horizontal,
            scaling: ScalingMode::Auto,
            style: ChartStyle::default(),
        }
    }

    pub fn horizontal() -> Self {
        Self::new(vec![]).with_orientation(BarOrientation::Horizontal)
    }

    pub fn vertical() -> Self {
        Self::new(vec![]).with_orientation(BarOrientation::Vertical)
    }

    pub fn with_data(mut self, data: Vec<DataPoint>) -> Self {
        self.data = data;
        self
    }

    pub fn with_orientation(mut self, orientation: BarOrientation) -> Self {
        self.orientation = orientation;
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

impl Widget for BarChart {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.data.is_empty() || area.width < 2 || area.height < 2 {
            return;
        }

        match self.orientation {
            BarOrientation::Horizontal => render_horizontal(self, area, buf),
            BarOrientation::Vertical => render_vertical(self, area, buf),
        }
    }
}

fn render_horizontal(chart: BarChart, area: Rect, buf: &mut Buffer) {
    let (min_val, max_val) = chart.get_scale_range();

    // Calculate label width
    let max_label_width = chart
        .data
        .iter()
        .map(|d| d.label.len())
        .max()
        .unwrap_or(0)
        .min(20);

    let label_width = if chart.style.show_labels {
        max_label_width + 1
    } else {
        0
    };

    let value_width = 8; // Space for value display
    let bar_width = area
        .width
        .saturating_sub(label_width as u16)
        .saturating_sub(value_width);

    if bar_width < 2 {
        return;
    }

    let available_rows = area.height as usize;
    let items_to_show = available_rows.min(chart.data.len());

    for (i, point) in chart.data.iter().take(items_to_show).enumerate() {
        let y = area.y + i as u16;
        let mut x = area.x;

        // Render label
        if chart.style.show_labels {
            let label = if point.label.len() > max_label_width {
                format!("{:width$.width$}", point.label, width = max_label_width)
            } else {
                format!("{:width$}", point.label, width = max_label_width)
            };
            buf.set_string(x, y, label, Style::default().fg(Color::Gray));
            x += label_width as u16;
        }

        // Calculate bar length
        let bar_len = map_value(point.value, min_val, max_val, bar_width as usize);

        // Render bar
        let color = chart.style.colors[i % chart.style.colors.len()];
        let bar = "█".repeat(bar_len);
        buf.set_string(x, y, bar, Style::default().fg(color));
        x += bar_width;

        // Render value
        let value_str = format!(" {}", format_number(point.value));
        buf.set_string(x, y, value_str, Style::default().fg(Color::White));
    }
}

fn render_vertical(chart: BarChart, area: Rect, buf: &mut Buffer) {
    let (min_val, max_val) = chart.get_scale_range();

    if area.height < 3 {
        return;
    }

    let bar_height = if chart.style.show_labels {
        area.height.saturating_sub(2)
    } else {
        area.height
    } as usize;

    let available_width = area.width as usize;
    let bar_count = chart.data.len().min(available_width / 2); // At least 2 chars per bar

    if bar_count == 0 {
        return;
    }

    let bar_spacing = available_width / bar_count;

    // Render bars
    for (i, point) in chart.data.iter().take(bar_count).enumerate() {
        let x = area.x + (i * bar_spacing) as u16;
        let height = map_value(point.value, min_val, max_val, bar_height);

        let color = chart.style.colors[i % chart.style.colors.len()];

        // Draw bar from bottom up
        for h in 0..height {
            let y = area.y + (bar_height - h - 1) as u16;
            buf.set_string(x, y, "█", Style::default().fg(color));
        }

        // Render label below bar
        if chart.style.show_labels {
            let label = if point.label.len() > bar_spacing {
                &point.label[..bar_spacing.min(point.label.len())]
            } else {
                &point.label
            };
            let label_y = area.y + bar_height as u16;
            buf.set_string(x, label_y, label, Style::default().fg(Color::Gray));
        }
    }

    // Render axis
    let axis_y = area.y + bar_height as u16 - 1;
    for x in area.x..area.x + area.width {
        buf.set_string(x, axis_y + 1, "─", Style::default().fg(Color::DarkGray));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_chart_creation() {
        let data = vec![
            DataPoint::new("A", 10.0),
            DataPoint::new("B", 20.0),
            DataPoint::new("C", 15.0),
        ];

        let chart = BarChart::new(data);
        assert_eq!(chart.data.len(), 3);
        assert_eq!(chart.orientation, BarOrientation::Horizontal);
    }

    #[test]
    fn test_bar_chart_builders() {
        let chart = BarChart::horizontal()
            .with_data(vec![DataPoint::new("Test", 42.0)])
            .with_scaling(ScalingMode::Fixed {
                min: 0.0,
                max: 100.0,
            });

        assert_eq!(chart.orientation, BarOrientation::Horizontal);
        assert_eq!(chart.data.len(), 1);
    }
}
