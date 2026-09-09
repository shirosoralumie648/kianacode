use kiana_components::scrollbar::{Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::{
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    Terminal,
};

#[test]
fn test_vertical_scrollbar_renders_correctly() {
    let backend = TestBackend::new(1, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 1, 10);
            let state = ScrollbarState::new(100).viewport_size(10).position(0);

            Scrollbar::new(ScrollbarOrientation::Vertical).render(area, f.buffer_mut(), &state);
        })
        .unwrap();

    // Scrollbar should be visible (100 items, 10 viewport)
    // Thumb should be at top (position 0)
    let buffer = terminal.backend().buffer();

    // First cell should be thumb (█)
    assert_eq!(buffer.get(0, 0).symbol(), "█");
}

#[test]
fn test_horizontal_scrollbar_renders_correctly() {
    let backend = TestBackend::new(10, 1);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 10, 1);
            let state = ScrollbarState::new(100).viewport_size(10).position(0);

            Scrollbar::new(ScrollbarOrientation::Horizontal).render(area, f.buffer_mut(), &state);
        })
        .unwrap();

    // Scrollbar should be visible
    let buffer = terminal.backend().buffer();

    // First cell should be thumb
    assert_eq!(buffer.get(0, 0).symbol(), "█");
}

#[test]
fn test_scrollbar_not_visible_when_content_fits() {
    let backend = TestBackend::new(1, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 1, 10);
            // Content fits viewport (5 items, 10 viewport)
            let state = ScrollbarState::new(5).viewport_size(10).position(0);

            Scrollbar::new(ScrollbarOrientation::Vertical).render(area, f.buffer_mut(), &state);
        })
        .unwrap();

    // Scrollbar should not render anything when content fits
    let buffer = terminal.backend().buffer();

    // Cell should be empty (default)
    assert_eq!(buffer.get(0, 0).symbol(), " ");
}

#[test]
fn test_scrollbar_thumb_position_changes() {
    let backend = TestBackend::new(1, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    // Test at start
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 1, 10);
            let state = ScrollbarState::new(100).viewport_size(10).position(0);

            Scrollbar::new(ScrollbarOrientation::Vertical)
                .track_symbol("│")
                .thumb_symbol("█")
                .render(area, f.buffer_mut(), &state);
        })
        .unwrap();

    let buffer_start = terminal.backend().buffer().clone();
    assert_eq!(buffer_start.get(0, 0).symbol(), "█"); // Thumb at top

    // Test at middle
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 1, 10);
            let state = ScrollbarState::new(100).viewport_size(10).position(45);

            Scrollbar::new(ScrollbarOrientation::Vertical)
                .track_symbol("│")
                .thumb_symbol("█")
                .render(area, f.buffer_mut(), &state);
        })
        .unwrap();

    let buffer_middle = terminal.backend().buffer().clone();
    // Thumb should be somewhere in middle (position 45 out of 90 scrollable = 50%)
    assert_eq!(buffer_middle.get(0, 5).symbol(), "█");
}

#[test]
fn test_scrollbar_custom_styling() {
    let backend = TestBackend::new(1, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 1, 10);
            let state = ScrollbarState::new(100).viewport_size(10).position(0);

            Scrollbar::new(ScrollbarOrientation::Vertical)
                .track_symbol("┊")
                .thumb_symbol("●")
                .track_style(Style::default().fg(Color::Red))
                .thumb_style(Style::default().fg(Color::Green))
                .render(area, f.buffer_mut(), &state);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();

    // Check custom thumb symbol
    assert_eq!(buffer.get(0, 0).symbol(), "●");

    // Check custom thumb color
    assert_eq!(buffer.get(0, 0).fg, Color::Green);

    // Check track symbol
    assert_eq!(buffer.get(0, 9).symbol(), "┊");

    // Check track color
    assert_eq!(buffer.get(0, 9).fg, Color::Red);
}
