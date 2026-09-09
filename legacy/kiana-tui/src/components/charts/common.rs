/// Common types and utilities for chart components
use ratatui::style::Color;

/// Chart data point with label and value
#[derive(Debug, Clone)]
pub struct DataPoint {
    pub label: String,
    pub value: f64,
}

impl DataPoint {
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

/// Chart scaling mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalingMode {
    /// Auto-scale to fit data range with padding
    Auto,
    /// Fixed range [min, max]
    Fixed { min: f64, max: f64 },
}

impl Default for ScalingMode {
    fn default() -> Self {
        Self::Auto
    }
}

/// Chart style configuration
#[derive(Debug, Clone)]
pub struct ChartStyle {
    /// Show axis labels
    pub show_labels: bool,
    /// Show legend
    pub show_legend: bool,
    /// Color scheme
    pub colors: Vec<Color>,
}

impl Default for ChartStyle {
    fn default() -> Self {
        Self {
            show_labels: true,
            show_legend: false,
            colors: vec![Color::Cyan, Color::Green, Color::Yellow, Color::Magenta],
        }
    }
}

/// Auto-scale data to fit range with padding
pub fn auto_scale(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 1.0);
    }

    let mut min = values[0];
    let mut max = values[0];

    for &v in values.iter() {
        if v < min {
            min = v;
        }
        if v > max {
            max = v;
        }
    }

    // If all values are the same, create a small range
    if (max - min).abs() < f64::EPSILON {
        if min.abs() < f64::EPSILON {
            return (0.0, 1.0);
        }
        let padding = min.abs() * 0.1;
        return (min - padding, max + padding);
    }

    // Add 5% padding to avoid touching boundaries
    let range = max - min;
    let padding = range * 0.05;
    (min - padding, max + padding)
}

/// Map value to coordinate within range
pub fn map_value(value: f64, min: f64, max: f64, target_size: usize) -> usize {
    if target_size == 0 {
        return 0;
    }

    if (max - min).abs() < f64::EPSILON {
        return target_size / 2;
    }

    let normalized = (value - min) / (max - min);
    let mapped = (normalized * target_size as f64).round() as i64;
    mapped.clamp(0, target_size as i64) as usize
}

/// Format number for display (with appropriate precision)
pub fn format_number(value: f64) -> String {
    if value.abs() < f64::EPSILON {
        return "0".to_string();
    }

    if value.abs() >= 1000000.0 {
        format!("{:.1}M", value / 1000000.0)
    } else if value.abs() >= 1000.0 {
        format!("{:.1}K", value / 1000.0)
    } else if value.abs() >= 100.0 {
        format!("{:.0}", value)
    } else if value.abs() >= 10.0 {
        format!("{:.1}", value)
    } else {
        format!("{:.2}", value)
    }
}

/// Generate nice tick marks for axis
pub fn generate_ticks(min: f64, max: f64, target_count: usize) -> Vec<f64> {
    if target_count == 0 {
        return vec![];
    }

    let range = max - min;
    if range < f64::EPSILON {
        return vec![min];
    }

    // Find a nice step size
    let rough_step = range / target_count as f64;
    let magnitude = 10_f64.powf(rough_step.log10().floor());
    let normalized_step = rough_step / magnitude;

    let nice_step = if normalized_step <= 1.0 {
        magnitude
    } else if normalized_step <= 2.0 {
        2.0 * magnitude
    } else if normalized_step <= 5.0 {
        5.0 * magnitude
    } else {
        10.0 * magnitude
    };

    // Generate ticks
    let start = (min / nice_step).ceil() * nice_step;
    let mut ticks = vec![];
    let mut current = start;

    while current <= max && ticks.len() < target_count * 2 {
        ticks.push(current);
        current += nice_step;
    }

    if ticks.is_empty() {
        ticks.push(min);
    }

    ticks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_scale_normal() {
        let values = vec![10.0, 20.0, 30.0, 40.0];
        let (min, max) = auto_scale(&values);
        assert!(min < 10.0);
        assert!(max > 40.0);
        assert!(max - min > 30.0);
    }

    #[test]
    fn test_auto_scale_empty() {
        let values = vec![];
        let (min, max) = auto_scale(&values);
        assert_eq!(min, 0.0);
        assert_eq!(max, 1.0);
    }

    #[test]
    fn test_auto_scale_single() {
        let values = vec![42.0];
        let (min, max) = auto_scale(&values);
        assert!(min < 42.0);
        assert!(max > 42.0);
    }

    #[test]
    fn test_auto_scale_same_values() {
        let values = vec![10.0, 10.0, 10.0];
        let (min, max) = auto_scale(&values);
        assert!(min < 10.0);
        assert!(max > 10.0);
    }

    #[test]
    fn test_map_value() {
        let mapped = map_value(50.0, 0.0, 100.0, 100);
        assert_eq!(mapped, 50);

        let mapped = map_value(0.0, 0.0, 100.0, 100);
        assert_eq!(mapped, 0);

        let mapped = map_value(100.0, 0.0, 100.0, 100);
        assert_eq!(mapped, 100);
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(5.5), "5.50");
        assert_eq!(format_number(55.5), "55.5");
        assert_eq!(format_number(555.0), "555");
        assert_eq!(format_number(5555.0), "5.6K");
        assert_eq!(format_number(5555555.0), "5.6M");
    }

    #[test]
    fn test_generate_ticks() {
        let ticks = generate_ticks(0.0, 100.0, 5);
        assert!(!ticks.is_empty());
        assert!(ticks[0] >= 0.0);
        assert!(ticks[ticks.len() - 1] <= 100.0);
    }
}
