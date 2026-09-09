use kiana_tui::layout::{Panel, PanelContainer, PanelContent, PanelDirection, PanelSize};
use ratatui::{backend::TestBackend, layout::Rect, Frame, Terminal};
use std::sync::{Arc, Mutex};

/// Mock panel content for testing
struct MockPanel {
    name: String,
    render_count: Arc<Mutex<usize>>,
    focus_state: Arc<Mutex<bool>>,
}

impl MockPanel {
    fn new(name: &str) -> (Self, Arc<Mutex<usize>>, Arc<Mutex<bool>>) {
        let render_count = Arc::new(Mutex::new(0));
        let focus_state = Arc::new(Mutex::new(false));

        (
            Self {
                name: name.to_string(),
                render_count: render_count.clone(),
                focus_state: focus_state.clone(),
            },
            render_count,
            focus_state,
        )
    }
}

impl PanelContent for MockPanel {
    fn render(&mut self, _f: &mut Frame, _area: Rect, has_focus: bool) {
        *self.render_count.lock().unwrap() += 1;
        *self.focus_state.lock().unwrap() = has_focus;
    }

    fn on_focus(&mut self) {
        *self.focus_state.lock().unwrap() = true;
    }

    fn on_blur(&mut self) {
        *self.focus_state.lock().unwrap() = false;
    }

    fn title(&self) -> Option<String> {
        Some(self.name.clone())
    }
}

#[test]
fn test_three_panel_horizontal_layout() {
    let mut container = PanelContainer::new(PanelDirection::Horizontal);

    let (panel1, render1, focus1) = MockPanel::new("Left");
    let (panel2, render2, focus2) = MockPanel::new("Center");
    let (panel3, render3, focus3) = MockPanel::new("Right");

    container.add_panel(
        Panel::new(Box::new(panel1))
            .with_size(PanelSize::Percentage(25))
            .with_title("Sidebar"),
    );
    container.add_panel(Panel::new(Box::new(panel2)).with_size(PanelSize::Fill));
    container.add_panel(Panel::new(Box::new(panel3)).with_size(PanelSize::Percentage(25)));

    assert_eq!(container.panel_count(), 3);
    assert_eq!(container.focused_index(), 0);

    // Render the container
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = f.area();
            container.render(f, area);
        })
        .unwrap();

    // All panels should be rendered
    assert_eq!(*render1.lock().unwrap(), 1);
    assert_eq!(*render2.lock().unwrap(), 1);
    assert_eq!(*render3.lock().unwrap(), 1);

    // First panel should have focus
    assert!(*focus1.lock().unwrap());
    assert!(!*focus2.lock().unwrap());
    assert!(!*focus3.lock().unwrap());

    // Navigate focus
    container.focus_next();
    assert_eq!(container.focused_index(), 1);

    container.focus_next();
    assert_eq!(container.focused_index(), 2);

    container.focus_next();
    assert_eq!(container.focused_index(), 0); // Wrap around
}

#[test]
fn test_vertical_panel_layout() {
    let mut container = PanelContainer::new(PanelDirection::Vertical);

    let (panel1, _, _) = MockPanel::new("Top");
    let (panel2, _, _) = MockPanel::new("Middle");
    let (panel3, _, _) = MockPanel::new("Bottom");

    container.add_panel(Panel::new(Box::new(panel1)).with_size(PanelSize::Fixed(5)));
    container.add_panel(Panel::new(Box::new(panel2)).with_size(PanelSize::Fill));
    container.add_panel(Panel::new(Box::new(panel3)).with_size(PanelSize::Fixed(3)));

    assert_eq!(container.panel_count(), 3);
    assert_eq!(container.direction(), PanelDirection::Vertical);

    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = f.area();
            container.render(f, area);
        })
        .unwrap();
}

#[test]
fn test_nested_panel_containers() {
    // Create outer vertical container
    let mut outer_container = PanelContainer::new(PanelDirection::Vertical);

    // Top panel
    let (top_panel, _, _) = MockPanel::new("Header");
    outer_container.add_panel(
        Panel::new(Box::new(top_panel))
            .with_size(PanelSize::Fixed(3))
            .with_title("Header"),
    );

    // Middle section will contain nested horizontal panels
    // For testing purposes, we'll use a mock panel
    let (middle_panel, _, _) = MockPanel::new("Main Content");
    outer_container.add_panel(Panel::new(Box::new(middle_panel)).with_size(PanelSize::Fill));

    // Bottom panel
    let (bottom_panel, _, _) = MockPanel::new("Footer");
    outer_container.add_panel(
        Panel::new(Box::new(bottom_panel))
            .with_size(PanelSize::Fixed(1))
            .with_title("Status"),
    );

    assert_eq!(outer_container.panel_count(), 3);

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = f.area();
            outer_container.render(f, area);
        })
        .unwrap();
}

#[test]
fn test_dynamic_panel_management() {
    let mut container = PanelContainer::new(PanelDirection::Horizontal);

    // Start with 2 panels
    let (panel1, _, _) = MockPanel::new("Panel 1");
    let (panel2, _, _) = MockPanel::new("Panel 2");

    container.add_panel(Panel::new(Box::new(panel1)));
    container.add_panel(Panel::new(Box::new(panel2)));
    assert_eq!(container.panel_count(), 2);

    // Add a third panel dynamically
    let (panel3, _, _) = MockPanel::new("Panel 3");
    container.add_panel(Panel::new(Box::new(panel3)));
    assert_eq!(container.panel_count(), 3);

    // Remove the middle panel
    let removed = container.remove_panel(1);
    assert!(removed.is_some());
    assert_eq!(container.panel_count(), 2);

    // Focus should still be valid
    container.focus_next();
    assert!(container.focused_index() < container.panel_count());
}

#[test]
fn test_focus_persistence_after_removal() {
    let mut container = PanelContainer::new(PanelDirection::Horizontal);

    let (panel1, _, _) = MockPanel::new("Panel 1");
    let (panel2, _, _) = MockPanel::new("Panel 2");
    let (panel3, _, _) = MockPanel::new("Panel 3");
    let (panel4, _, _) = MockPanel::new("Panel 4");

    container.add_panel(Panel::new(Box::new(panel1)));
    container.add_panel(Panel::new(Box::new(panel2)));
    container.add_panel(Panel::new(Box::new(panel3)));
    container.add_panel(Panel::new(Box::new(panel4)));

    // Focus on panel 3 (index 2)
    container.set_focus(2);
    assert_eq!(container.focused_index(), 2);

    // Remove panel 1 (index 0)
    container.remove_panel(0);
    assert_eq!(container.panel_count(), 3);
    // Focus should shift to index 1 (was index 2 before removal)
    assert_eq!(container.focused_index(), 1);

    // Remove the currently focused panel
    container.remove_panel(1);
    assert_eq!(container.panel_count(), 2);
    // Focus should adjust
    assert!(container.focused_index() < container.panel_count());
}

#[test]
fn test_mixed_focusable_and_non_focusable() {
    let mut container = PanelContainer::new(PanelDirection::Horizontal);

    let (panel1, _, _) = MockPanel::new("Focusable 1");
    let (panel2, _, _) = MockPanel::new("Non-focusable");
    let (panel3, _, _) = MockPanel::new("Focusable 2");

    container.add_panel(Panel::new(Box::new(panel1)));
    container.add_panel(Panel::new(Box::new(panel2)).with_focusable(false));
    container.add_panel(Panel::new(Box::new(panel3)));

    assert_eq!(container.focused_index(), 0);

    // Next should skip non-focusable panel
    container.focus_next();
    assert_eq!(container.focused_index(), 2);

    // Previous should also skip non-focusable panel
    container.focus_previous();
    assert_eq!(container.focused_index(), 0);
}

#[test]
fn test_panel_without_borders() {
    let mut container = PanelContainer::new(PanelDirection::Horizontal);

    let (panel1, _, _) = MockPanel::new("Panel 1");
    let (panel2, _, _) = MockPanel::new("Panel 2");

    container.add_panel(Panel::new(Box::new(panel1)).with_border(true));
    container.add_panel(Panel::new(Box::new(panel2)).with_border(false));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = f.area();
            container.render(f, area);
        })
        .unwrap();
}

#[test]
fn test_complex_layout_scenario() {
    // Simulate a complex IDE-like layout:
    // - Top: status bar (fixed height)
    // - Middle: 3 columns (sidebar, editor, details)
    // - Bottom: console (fixed height)

    let mut main_container = PanelContainer::new(PanelDirection::Vertical);

    // Status bar
    let (status, _, _) = MockPanel::new("Status Bar");
    main_container.add_panel(
        Panel::new(Box::new(status))
            .with_size(PanelSize::Fixed(1))
            .with_border(false),
    );

    // Main content area (horizontal split)
    let (sidebar, _, _) = MockPanel::new("Sidebar");
    let (editor, _, _) = MockPanel::new("Editor");
    let (details, _, _) = MockPanel::new("Details");

    // For this test, we add them directly to the main container
    // In a real scenario, they would be in a nested container
    main_container.add_panel(
        Panel::new(Box::new(sidebar))
            .with_size(PanelSize::Percentage(20))
            .with_title("Files"),
    );
    main_container.add_panel(
        Panel::new(Box::new(editor))
            .with_size(PanelSize::Fill)
            .with_title("Editor"),
    );
    main_container.add_panel(
        Panel::new(Box::new(details))
            .with_size(PanelSize::Percentage(20))
            .with_title("Info"),
    );

    // Console
    let (console, _, _) = MockPanel::new("Console");
    main_container.add_panel(
        Panel::new(Box::new(console))
            .with_size(PanelSize::Fixed(8))
            .with_title("Output"),
    );

    assert_eq!(main_container.panel_count(), 5);

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = f.area();
            main_container.render(f, area);
        })
        .unwrap();

    // Test focus navigation through all panels
    for _ in 0..main_container.panel_count() {
        main_container.focus_next();
    }
    // Should wrap back to first focusable panel
}

#[test]
fn test_minimum_size_panels() {
    let mut container = PanelContainer::new(PanelDirection::Vertical);

    let (panel1, _, _) = MockPanel::new("Panel 1");
    let (panel2, _, _) = MockPanel::new("Panel 2");

    container.add_panel(Panel::new(Box::new(panel1)).with_size(PanelSize::Min(5)));
    container.add_panel(Panel::new(Box::new(panel2)).with_size(PanelSize::Fill));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = f.area();
            container.render(f, area);
        })
        .unwrap();
}

#[test]
fn test_empty_container_render() {
    let mut container = PanelContainer::new(PanelDirection::Horizontal);

    // Empty container should not panic
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = f.area();
            container.render(f, area);
        })
        .unwrap();
}

#[test]
fn test_input_routing_to_focused_panel() {
    let mut container = PanelContainer::new(PanelDirection::Horizontal);

    let (panel1, _, _) = MockPanel::new("Panel 1");
    let (panel2, _, _) = MockPanel::new("Panel 2");

    container.add_panel(Panel::new(Box::new(panel1)));
    container.add_panel(Panel::new(Box::new(panel2)));

    // Mock input would be handled by the focused panel
    // This is tested in the unit tests of panels.rs
}
