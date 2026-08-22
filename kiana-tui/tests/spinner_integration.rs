//! Integration tests for spinner component

use kiana_tui::components::{Spinner, SpinnerStyle};
use ratatui::{backend::TestBackend, layout::Rect, style::Color, Terminal};
use std::{thread, time::Duration};

#[test]
fn test_spinner_creation() {
    let spinner = Spinner::new();
    assert_eq!(spinner.current_frame(), "⠋");
}

#[test]
fn test_spinner_with_style() {
    let dots = Spinner::with_style(SpinnerStyle::Dots);
    assert_eq!(dots.current_frame(), "⠋");

    let line = Spinner::with_style(SpinnerStyle::Line);
    assert_eq!(line.current_frame(), "|");

    let braille = Spinner::with_style(SpinnerStyle::Braille);
    assert_eq!(braille.current_frame(), "⣾");
}

#[test]
fn test_spinner_frame_sequences() {
    let dots = SpinnerStyle::Dots.frames();
    assert_eq!(dots.len(), 10);
    assert_eq!(dots[0], "⠋");
    assert_eq!(dots[9], "⠏");

    let line = SpinnerStyle::Line.frames();
    assert_eq!(line.len(), 4);
    assert_eq!(line[0], "|");
    assert_eq!(line[1], "/");
    assert_eq!(line[2], "-");
    assert_eq!(line[3], "\\");

    let braille = SpinnerStyle::Braille.frames();
    assert_eq!(braille.len(), 8);
    assert_eq!(braille[0], "⣾");
    assert_eq!(braille[7], "⣷");
}

#[test]
fn test_spinner_frame_duration() {
    let dots_duration = SpinnerStyle::Dots.frame_duration();
    assert_eq!(dots_duration, Duration::from_millis(80));

    let line_duration = SpinnerStyle::Line.frame_duration();
    assert_eq!(line_duration, Duration::from_millis(100));

    let braille_duration = SpinnerStyle::Braille.frame_duration();
    assert_eq!(braille_duration, Duration::from_millis(80));
}

#[test]
fn test_spinner_force_tick() {
    let mut spinner = Spinner::new();
    let initial = spinner.current_frame();

    spinner.force_tick();
    let after_one = spinner.current_frame();
    assert_ne!(initial, after_one);

    spinner.force_tick();
    let after_two = spinner.current_frame();
    assert_ne!(after_one, after_two);
}

#[test]
fn test_spinner_cycle_completion() {
    let mut spinner = Spinner::with_style(SpinnerStyle::Dots);
    let initial = spinner.current_frame();

    // Tick through all frames (10 for Dots style)
    for _ in 0..10 {
        spinner.force_tick();
    }

    // Should cycle back to initial frame
    assert_eq!(spinner.current_frame(), initial);
}

#[test]
fn test_spinner_line_style_cycle() {
    let mut spinner = Spinner::with_style(SpinnerStyle::Line);

    assert_eq!(spinner.current_frame(), "|");
    spinner.force_tick();
    assert_eq!(spinner.current_frame(), "/");
    spinner.force_tick();
    assert_eq!(spinner.current_frame(), "-");
    spinner.force_tick();
    assert_eq!(spinner.current_frame(), "\\");
    spinner.force_tick();
    assert_eq!(spinner.current_frame(), "|"); // Back to start
}

#[test]
fn test_spinner_with_label() {
    let spinner = Spinner::new().with_label("Loading...");
    // Label is stored but we can't directly test rendering without a frame
    let spinner = Spinner::new().with_label("Processing data");
    // Just ensure it doesn't panic
}

#[test]
fn test_spinner_label_setting() {
    let mut spinner = Spinner::new();

    spinner.set_label(Some("Task 1".to_string()));
    spinner.set_label(Some("Task 2".to_string()));
    spinner.set_label(None);
}

#[test]
fn test_spinner_style_change() {
    let mut spinner = Spinner::new();
    assert_eq!(spinner.current_frame(), "⠋"); // Dots default

    spinner.set_style(SpinnerStyle::Line);
    assert_eq!(spinner.current_frame(), "|"); // Line starts at |

    spinner.set_style(SpinnerStyle::Braille);
    assert_eq!(spinner.current_frame(), "⣾"); // Braille starts at ⣾
}

#[test]
fn test_spinner_style_change_resets_frame() {
    let mut spinner = Spinner::with_style(SpinnerStyle::Dots);

    // Advance several frames
    for _ in 0..5 {
        spinner.force_tick();
    }

    // Change style should reset to frame 0
    spinner.set_style(SpinnerStyle::Line);
    assert_eq!(spinner.current_frame(), "|");
}

#[test]
fn test_spinner_timing() {
    let mut spinner = Spinner::new();

    // Should not need update immediately
    assert!(!spinner.should_update());

    // Wait for frame duration
    let duration = spinner.current_frame(); // Store current
    thread::sleep(Duration::from_millis(90)); // Slightly more than 80ms

    // Should now need update
    assert!(spinner.should_update());

    // After ticking, shouldn't need immediate update
    spinner.tick();
    assert!(!spinner.should_update());
}

#[test]
fn test_spinner_builder_pattern() {
    let spinner = Spinner::with_style(SpinnerStyle::Braille)
        .with_label("Working")
        .with_color(Color::Yellow);

    assert_eq!(spinner.current_frame(), "⣾");
}

#[test]
fn test_spinner_rendering_basic() {
    let backend = TestBackend::new(80, 3);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let area = Rect::new(0, 0, 80, 1);
            let spinner = Spinner::new();
            spinner.render(frame, area);
        })
        .unwrap();
}

#[test]
fn test_spinner_rendering_all_styles() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let dots = Spinner::with_style(SpinnerStyle::Dots);
            dots.render(frame, Rect::new(0, 0, 80, 1));

            let line = Spinner::with_style(SpinnerStyle::Line);
            line.render(frame, Rect::new(0, 2, 80, 1));

            let braille = Spinner::with_style(SpinnerStyle::Braille);
            braille.render(frame, Rect::new(0, 4, 80, 1));
        })
        .unwrap();
}

#[test]
fn test_spinner_rendering_with_labels() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let spinner1 = Spinner::new().with_label("Loading...");
            spinner1.render(frame, Rect::new(0, 0, 80, 1));

            let spinner2 = Spinner::with_style(SpinnerStyle::Line).with_label("Processing data...");
            spinner2.render(frame, Rect::new(0, 2, 80, 1));

            let spinner3 =
                Spinner::with_style(SpinnerStyle::Braille).with_label("Building project...");
            spinner3.render(frame, Rect::new(0, 4, 80, 1));
        })
        .unwrap();
}

#[test]
fn test_spinner_rendering_with_colors() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let red = Spinner::new().with_color(Color::Red);
            red.render(frame, Rect::new(0, 0, 80, 1));

            let green = Spinner::new().with_color(Color::Green);
            green.render(frame, Rect::new(0, 2, 80, 1));

            let yellow = Spinner::new().with_color(Color::Yellow);
            yellow.render(frame, Rect::new(0, 4, 80, 1));

            let blue = Spinner::new().with_color(Color::Blue);
            blue.render(frame, Rect::new(0, 6, 80, 1));
        })
        .unwrap();
}

#[test]
fn test_spinner_default() {
    let spinner1 = Spinner::default();
    let spinner2 = Spinner::new();

    assert_eq!(spinner1.current_frame(), spinner2.current_frame());
}

#[test]
fn test_spinner_multiple_instances() {
    let mut spinner1 = Spinner::with_style(SpinnerStyle::Dots);
    let mut spinner2 = Spinner::with_style(SpinnerStyle::Line);
    let mut spinner3 = Spinner::with_style(SpinnerStyle::Braille);

    // Each should maintain independent state
    spinner1.force_tick();
    spinner1.force_tick();

    spinner2.force_tick();

    // No ticks for spinner3

    assert_eq!(spinner1.current_frame(), "⠹"); // 2 ticks from ⠋
    assert_eq!(spinner2.current_frame(), "/"); // 1 tick from |
    assert_eq!(spinner3.current_frame(), "⣾"); // No ticks
}

#[test]
fn test_spinner_rapid_ticking() {
    let mut spinner = Spinner::new();

    // Rapid ticks should cycle through properly
    for _ in 0..100 {
        spinner.force_tick();
    }

    // Should be at frame 0 (100 % 10 = 0 for Dots style)
    assert_eq!(spinner.current_frame(), "⠋");
}

#[test]
fn test_spinner_style_default() {
    let default_style = SpinnerStyle::default();
    assert_eq!(default_style, SpinnerStyle::Dots);
}

#[test]
fn test_spinner_all_frames_unique() {
    for style in [
        SpinnerStyle::Dots,
        SpinnerStyle::Line,
        SpinnerStyle::Braille,
    ] {
        let frames = style.frames();
        let mut seen = std::collections::HashSet::new();

        for frame in frames {
            assert!(seen.insert(frame), "Duplicate frame found: {}", frame);
        }
    }
}
