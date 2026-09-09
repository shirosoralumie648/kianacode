//! Integration tests for progress bar component

use kiana_tui::components::{ProgressBar, ProgressState, ProgressStyle};
use ratatui::{backend::TestBackend, layout::Rect, Terminal};

#[test]
fn test_progress_bar_creation() {
    let progress = ProgressBar::new("Test");
    assert_eq!(progress.percentage(), 0);
}

#[test]
fn test_progress_value_setting() {
    let mut progress = ProgressBar::new("Test");

    // Test decimal values (0.0-1.0)
    progress.set_progress(0.0);
    assert_eq!(progress.percentage(), 0);

    progress.set_progress(0.25);
    assert_eq!(progress.percentage(), 25);

    progress.set_progress(0.5);
    assert_eq!(progress.percentage(), 50);

    progress.set_progress(0.75);
    assert_eq!(progress.percentage(), 75);

    progress.set_progress(1.0);
    assert_eq!(progress.percentage(), 100);
}

#[test]
fn test_progress_percentage_values() {
    let mut progress = ProgressBar::new("Test");

    // Test percentage values (0-100)
    progress.set_progress(0.0);
    assert_eq!(progress.percentage(), 0);

    progress.set_progress(25.0);
    assert_eq!(progress.percentage(), 25);

    progress.set_progress(50.0);
    assert_eq!(progress.percentage(), 50);

    progress.set_progress(75.0);
    assert_eq!(progress.percentage(), 75);

    progress.set_progress(100.0);
    assert_eq!(progress.percentage(), 100);
}

#[test]
fn test_progress_clamping() {
    let mut progress = ProgressBar::new("Test");

    // Test negative values
    progress.set_progress(-1.0);
    assert_eq!(progress.percentage(), 0);

    progress.set_progress(-0.5);
    assert_eq!(progress.percentage(), 0);

    // Test over 100% (values > 100 are treated as percentages and clamped)
    progress.set_progress(1.5);
    assert_eq!(progress.percentage(), 2); // 1.5% rounded to 2%

    progress.set_progress(150.0);
    assert_eq!(progress.percentage(), 100); // 150% clamped to 100%

    progress.set_progress(200.0);
    assert_eq!(progress.percentage(), 100); // 200% clamped to 100%
}

#[test]
fn test_progress_state_colors() {
    use ratatui::style::Color;

    assert_eq!(ProgressState::Normal.color(), Color::Green);
    assert_eq!(ProgressState::Warning.color(), Color::Yellow);
    assert_eq!(ProgressState::Error.color(), Color::Red);
}

#[test]
fn test_progress_state_changes() {
    let mut progress = ProgressBar::new("Test").progress(0.5);

    progress.set_state(ProgressState::Normal);
    progress.set_state(ProgressState::Warning);
    progress.set_state(ProgressState::Error);

    // Should not affect progress value
    assert_eq!(progress.percentage(), 50);
}

#[test]
fn test_progress_builder_pattern() {
    let progress = ProgressBar::new("Building")
        .progress(0.75)
        .state(ProgressState::Warning);

    assert_eq!(progress.percentage(), 75);
}

#[test]
fn test_progress_style_configuration() {
    let style = ProgressStyle {
        width: Some(50),
        show_percentage: false,
        show_label: false,
        filled_char: '=',
        empty_char: '-',
    };

    let progress = ProgressBar::new("Test").progress(0.5).style(style.clone());

    assert_eq!(progress.percentage(), 50);
}

#[test]
fn test_progress_label_update() {
    let mut progress = ProgressBar::new("Initial");
    progress.set_label("Updated");
    progress.set_label("Final");

    assert_eq!(progress.percentage(), 0);
}

#[test]
fn test_progress_rendering_basic() {
    let backend = TestBackend::new(80, 3);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let area = Rect::new(0, 0, 80, 1);
            let progress = ProgressBar::new("Test").progress(0.5);
            progress.render(frame, area);
        })
        .unwrap();
}

#[test]
fn test_progress_rendering_all_states() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let normal = ProgressBar::new("Normal")
                .progress(0.5)
                .state(ProgressState::Normal);
            normal.render(frame, Rect::new(0, 0, 80, 1));

            let warning = ProgressBar::new("Warning")
                .progress(0.5)
                .state(ProgressState::Warning);
            warning.render(frame, Rect::new(0, 2, 80, 1));

            let error = ProgressBar::new("Error")
                .progress(0.5)
                .state(ProgressState::Error);
            error.render(frame, Rect::new(0, 4, 80, 1));
        })
        .unwrap();
}

#[test]
fn test_progress_rendering_custom_styles() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            // No percentage
            let style1 = ProgressStyle {
                show_percentage: false,
                ..Default::default()
            };
            let progress1 = ProgressBar::new("No %").progress(0.5).style(style1);
            progress1.render(frame, Rect::new(0, 0, 80, 1));

            // No label
            let style2 = ProgressStyle {
                show_label: false,
                ..Default::default()
            };
            let progress2 = ProgressBar::new("").progress(0.5).style(style2);
            progress2.render(frame, Rect::new(0, 2, 80, 1));

            // Custom chars
            let style3 = ProgressStyle {
                filled_char: '=',
                empty_char: '-',
                ..Default::default()
            };
            let progress3 = ProgressBar::new("Custom").progress(0.5).style(style3);
            progress3.render(frame, Rect::new(0, 4, 80, 1));
        })
        .unwrap();
}

#[test]
fn test_progress_boundary_values() {
    let mut progress = ProgressBar::new("Test");

    // Test exact boundaries
    progress.set_progress(0.0);
    assert_eq!(progress.percentage(), 0);

    progress.set_progress(1.0);
    assert_eq!(progress.percentage(), 100);

    // Test just inside boundaries
    progress.set_progress(0.001);
    assert_eq!(progress.percentage(), 0);

    progress.set_progress(0.999);
    assert_eq!(progress.percentage(), 100);
}

#[test]
fn test_progress_incremental_updates() {
    let mut progress = ProgressBar::new("Loading");

    // Simulate incremental progress
    for i in 0..=10 {
        progress.set_progress(i as f64 / 10.0);
        assert_eq!(progress.percentage(), i * 10);
    }
}

#[test]
fn test_progress_default_style() {
    let style = ProgressStyle::default();

    assert_eq!(style.width, None);
    assert!(style.show_percentage);
    assert!(style.show_label);
    assert_eq!(style.filled_char, '█');
    assert_eq!(style.empty_char, '░');
}

#[test]
fn test_progress_multiple_instances() {
    let progress1 = ProgressBar::new("Task 1").progress(0.25);
    let progress2 = ProgressBar::new("Task 2").progress(0.50);
    let progress3 = ProgressBar::new("Task 3").progress(0.75);

    assert_eq!(progress1.percentage(), 25);
    assert_eq!(progress2.percentage(), 50);
    assert_eq!(progress3.percentage(), 75);
}
