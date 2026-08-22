/// Integration example for the tab system
///
/// This module demonstrates how to integrate the tab system into the main application.
///
/// # Usage
///
/// ```rust,no_run
/// use kiana_tui::layout::{Tab, TabContainer, TabContent};
///
/// // Create a tab container
/// let mut tab_container = TabContainer::new();
///
/// // Add tabs
/// tab_container.add_tab(Tab::new("Messages", Box::new(MessagesTab::new())));
/// tab_container.add_tab(Tab::new("Config", Box::new(ConfigTab::new())));
/// tab_container.add_tab(Tab::new("Sessions", Box::new(SessionsTab::new())));
///
/// // Handle keyboard events
/// match key.code {
///     KeyCode::Tab if key.modifiers.contains(KeyModifiers::CONTROL) => {
///         if key.modifiers.contains(KeyModifiers::SHIFT) {
///             tab_container.previous_tab();
///         } else {
///             tab_container.next_tab();
///         }
///     }
///     KeyCode::Char(c) if c.is_ascii_digit() => {
///         let num = c.to_digit(10).unwrap() as usize;
///         if num >= 1 && num <= 9 {
///             tab_container.switch_to(num - 1);
///         }
///     }
///     _ => {
///         // Forward to active tab
///         if let Some(tab) = tab_container.active_tab_mut() {
///             tab.content.handle_input(key);
///         }
///     }
/// }
///
/// // Render
/// tab_container.render(f, area);
/// ```
///
/// # Keyboard Shortcuts
///
/// - `1-9`: Switch to tab by number
/// - `Ctrl+Tab`: Next tab
/// - `Ctrl+Shift+Tab`: Previous tab

#[cfg(test)]
mod tests {
    use super::super::example_tabs::TextTab;
    use super::super::{Tab, TabContainer};

    #[test]
    fn test_integration_example() {
        let mut container = TabContainer::new();

        // Add example tabs
        container.add_tab(Tab::new(
            "Messages",
            Box::new(TextTab::new("Messages", "Message content here")),
        ));
        container.add_tab(Tab::new(
            "Config",
            Box::new(TextTab::new("Config", "Configuration settings")),
        ));
        container.add_tab(Tab::new(
            "Sessions",
            Box::new(TextTab::new("Sessions", "Active sessions")),
        ));

        assert_eq!(container.tab_count(), 3);
        assert_eq!(container.active_index(), 0);

        // Simulate Ctrl+Tab
        container.next_tab();
        assert_eq!(container.active_index(), 1);

        // Simulate number key "1" (switch to index 0)
        container.switch_to(0);
        assert_eq!(container.active_index(), 0);

        // Simulate number key "3" (switch to index 2)
        container.switch_to(2);
        assert_eq!(container.active_index(), 2);
    }
}
