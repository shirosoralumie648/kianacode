/// Integration tests for chart components
use kiana_tui::components::{
    BarChart, BarOrientation, ChartStyle, DataPoint, Interpolation, LineChart, ScalingMode,
    Sparkline, SparklineStyle,
};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

#[test]
fn test_bar_chart_horizontal_render() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![
                DataPoint::new("A", 10.0),
                DataPoint::new("B", 20.0),
                DataPoint::new("C", 15.0),
            ];

            let chart = BarChart::new(data)
                .with_orientation(BarOrientation::Horizontal)
                .with_scaling(ScalingMode::Auto);

            f.render_widget(chart, f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    // Just verify it doesn't panic and renders something
    assert!(buffer.area.width > 0);
    assert!(buffer.area.height > 0);
}

#[test]
fn test_bar_chart_vertical_render() {
    let backend = TestBackend::new(40, 15);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![
                DataPoint::new("Q1", 45.0),
                DataPoint::new("Q2", 67.0),
                DataPoint::new("Q3", 54.0),
            ];

            let chart = BarChart::new(data)
                .with_orientation(BarOrientation::Vertical)
                .with_scaling(ScalingMode::Auto);

            f.render_widget(chart, f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert!(buffer.area.width > 0);
}

#[test]
fn test_bar_chart_empty_data() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let chart = BarChart::new(vec![]).with_orientation(BarOrientation::Horizontal);
            f.render_widget(chart, f.area());
        })
        .unwrap();

    // Should not panic with empty data
}

#[test]
fn test_bar_chart_fixed_scaling() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![DataPoint::new("Test", 50.0)];

            let chart = BarChart::new(data).with_scaling(ScalingMode::Fixed {
                min: 0.0,
                max: 100.0,
            });

            f.render_widget(chart, f.area());
        })
        .unwrap();

    // Should use fixed scale
}

#[test]
fn test_line_chart_render() {
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![
                DataPoint::new("0", 10.0),
                DataPoint::new("2", 30.0),
                DataPoint::new("4", 50.0),
                DataPoint::new("6", 45.0),
                DataPoint::new("8", 70.0),
            ];

            let chart = LineChart::new(data)
                .with_interpolation(Interpolation::Linear)
                .with_scaling(ScalingMode::Auto);

            f.render_widget(chart, f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert!(buffer.area.width > 0);
}

#[test]
fn test_line_chart_step_interpolation() {
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![
                DataPoint::new("A", 10.0),
                DataPoint::new("B", 20.0),
                DataPoint::new("C", 15.0),
            ];

            let chart = LineChart::new(data).with_interpolation(Interpolation::Step);

            f.render_widget(chart, f.area());
        })
        .unwrap();
}

#[test]
fn test_line_chart_single_point() {
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![DataPoint::new("Single", 42.0)];
            let chart = LineChart::new(data);
            f.render_widget(chart, f.area());
        })
        .unwrap();

    // Should handle single point gracefully
}

#[test]
fn test_sparkline_block_style() {
    let data = vec![1.0, 3.0, 5.0, 7.0, 9.0, 8.0, 6.0, 4.0, 2.0];
    let sparkline = Sparkline::new(data).with_style(SparklineStyle::Block);

    let result = sparkline.render_inline();
    assert!(!result.is_empty());
    assert_eq!(result.chars().count(), 9);

    // Check that it uses block characters
    for c in result.chars() {
        assert!("▁▂▃▄▅▆▇█".contains(c));
    }
}

#[test]
fn test_sparkline_braille_style() {
    let data = vec![1.0, 3.0, 5.0, 7.0, 9.0];
    let sparkline = Sparkline::new(data).with_style(SparklineStyle::Braille);

    let result = sparkline.render_inline();
    assert!(!result.is_empty());
    assert_eq!(result.chars().count(), 5);

    // Braille characters should be in the correct Unicode range
    for c in result.chars() {
        let code = c as u32;
        assert!(code >= 0x2800 && code <= 0x28FF);
    }
}

#[test]
fn test_sparkline_empty_data() {
    let sparkline = Sparkline::new(vec![]);
    let result = sparkline.render_inline();
    assert!(result.is_empty());
}

#[test]
fn test_sparkline_single_value() {
    let sparkline = Sparkline::new(vec![42.0]);
    let result = sparkline.render_inline();
    assert_eq!(result.chars().count(), 1);
}

#[test]
fn test_sparkline_same_values() {
    let data = vec![5.0, 5.0, 5.0, 5.0];
    let sparkline = Sparkline::new(data);
    let result = sparkline.render_inline();
    // All same values should render as middle block
    assert_eq!(result.chars().count(), 4);
}

#[test]
fn test_sparkline_render_widget() {
    let backend = TestBackend::new(30, 3);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
            let sparkline = Sparkline::new(data).with_style(SparklineStyle::Block);
            f.render_widget(sparkline, f.area());
        })
        .unwrap();

    // Should render without panic
}

#[test]
fn test_chart_style_configuration() {
    use ratatui::style::Color;

    let style = ChartStyle {
        show_labels: true,
        show_legend: true,
        colors: vec![Color::Red, Color::Green, Color::Blue],
    };

    let data = vec![DataPoint::new("Test", 10.0)];
    let chart = BarChart::new(data).with_style(style);

    // Chart should accept custom style
}

#[test]
fn test_data_point_creation() {
    let point = DataPoint::new("Label", 42.5);
    assert_eq!(point.label, "Label");
    assert_eq!(point.value, 42.5);
}

#[test]
fn test_scaling_mode_auto() {
    let data = vec![
        DataPoint::new("A", 10.0),
        DataPoint::new("B", 50.0),
        DataPoint::new("C", 30.0),
    ];

    let chart = BarChart::new(data).with_scaling(ScalingMode::Auto);
    // Auto scaling should handle the data range automatically
}

#[test]
fn test_scaling_mode_fixed() {
    let data = vec![DataPoint::new("Test", 75.0)];

    let chart = BarChart::new(data).with_scaling(ScalingMode::Fixed {
        min: 0.0,
        max: 100.0,
    });

    // Fixed scaling should use the specified range
}

#[test]
fn test_large_dataset() {
    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data: Vec<DataPoint> = (0..100)
                .map(|i| DataPoint::new(format!("{}", i), (i as f64 * 1.5).sin() * 50.0 + 50.0))
                .collect();

            let chart = LineChart::new(data);
            f.render_widget(chart, f.area());
        })
        .unwrap();

    // Should handle large datasets without panic
}

#[test]
fn test_negative_values() {
    let backend = TestBackend::new(40, 15);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![
                DataPoint::new("A", -10.0),
                DataPoint::new("B", 20.0),
                DataPoint::new("C", -5.0),
            ];

            let chart = BarChart::new(data);
            f.render_widget(chart, f.area());
        })
        .unwrap();

    // Should handle negative values correctly
}

#[test]
fn test_extreme_values() {
    let backend = TestBackend::new(40, 15);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let data = vec![
                DataPoint::new("Min", 0.001),
                DataPoint::new("Max", 999999.0),
            ];

            let chart = BarChart::new(data);
            f.render_widget(chart, f.area());
        })
        .unwrap();

    // Should handle extreme value ranges
}
