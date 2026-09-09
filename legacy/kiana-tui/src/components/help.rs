use crate::components::command_palette::CommandRegistry;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

pub struct HelpOverlay {
    shortcuts: Vec<(String, String, String)>, // (category, key, description)
    scroll_offset: usize,
    filter: String,
}

impl HelpOverlay {
    pub fn from_registry(registry: &CommandRegistry) -> Self {
        let mut shortcuts = Vec::new();

        for cmd in registry.commands() {
            if let Some(ref shortcut) = cmd.shortcut {
                shortcuts.push((
                    cmd.category.clone(),
                    shortcut.clone(),
                    cmd.description.clone(),
                ));
            }
        }

        shortcuts.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        Self {
            shortcuts,
            scroll_offset: 0,
            filter: String::new(),
        }
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let max_offset = self.filtered_shortcuts().len().saturating_sub(10);
        self.scroll_offset = (self.scroll_offset + amount).min(max_offset);
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.scroll_offset = 0;
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    fn filtered_shortcuts(&self) -> Vec<&(String, String, String)> {
        if self.filter.is_empty() {
            self.shortcuts.iter().collect()
        } else {
            self.shortcuts
                .iter()
                .filter(|(_, key, desc)| {
                    key.to_lowercase().contains(&self.filter.to_lowercase())
                        || desc.to_lowercase().contains(&self.filter.to_lowercase())
                })
                .collect()
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        f.render_widget(Clear, area);

        let block = Block::default()
            .title("Keyboard Shortcuts (Press '?' to close)")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        f.render_widget(block, area);

        let filtered = self.filtered_shortcuts();
        let visible_start = self.scroll_offset;
        let visible_end = (visible_start + inner.height as usize).min(filtered.len());

        let mut current_category = String::new();
        let mut items = Vec::new();

        for shortcut in &filtered[visible_start..visible_end] {
            if shortcut.0 != current_category {
                current_category = shortcut.0.clone();
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("\n{}", current_category),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))));
            }

            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:15}", shortcut.1),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(shortcut.2.clone()),
            ])));
        }

        let list = List::new(items);
        f.render_widget(list, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::command_palette::{Command, CommandRegistry};

    #[test]
    fn test_help_from_registry() {
        let mut registry = CommandRegistry::new();
        registry.register(
            Command::new("quit", "Quit app", "App").with_shortcut("q"),
            || {},
        );

        let help = HelpOverlay::from_registry(&registry);
        assert_eq!(help.shortcuts.len(), 1);
    }

    #[test]
    fn test_scroll() {
        let mut registry = CommandRegistry::new();
        for i in 0..20 {
            registry.register(
                Command::new(&format!("cmd{}", i), "Test", "General")
                    .with_shortcut(&format!("Ctrl+{}", i)),
                || {},
            );
        }

        let mut help = HelpOverlay::from_registry(&registry);
        assert_eq!(help.scroll_offset, 0);

        help.scroll_down(5);
        assert_eq!(help.scroll_offset, 5);

        help.scroll_up(2);
        assert_eq!(help.scroll_offset, 3);
    }

    #[test]
    fn test_filter() {
        let mut registry = CommandRegistry::new();
        registry.register(
            Command::new("open", "Open file", "File").with_shortcut("Ctrl+O"),
            || {},
        );
        registry.register(
            Command::new("quit", "Quit", "App").with_shortcut("q"),
            || {},
        );

        let mut help = HelpOverlay::from_registry(&registry);
        assert_eq!(help.filtered_shortcuts().len(), 2);

        help.set_filter("file".to_string());
        assert_eq!(help.filtered_shortcuts().len(), 1);
    }

    #[test]
    fn test_empty_registry() {
        let registry = CommandRegistry::new();
        let help = HelpOverlay::from_registry(&registry);
        assert_eq!(help.shortcuts.len(), 0);
    }

    #[test]
    fn test_commands_without_shortcuts() {
        let mut registry = CommandRegistry::new();
        registry.register(Command::new("cmd1", "Command 1", "General"), || {});
        registry.register(
            Command::new("cmd2", "Command 2", "General").with_shortcut("Ctrl+2"),
            || {},
        );

        let help = HelpOverlay::from_registry(&registry);
        // Only cmd2 has a shortcut
        assert_eq!(help.shortcuts.len(), 1);
        assert_eq!(help.shortcuts[0].1, "Ctrl+2");
    }

    #[test]
    fn test_category_sorting() {
        let mut registry = CommandRegistry::new();
        registry.register(
            Command::new("quit", "Quit", "Zebra").with_shortcut("q"),
            || {},
        );
        registry.register(
            Command::new("open", "Open", "Apple").with_shortcut("o"),
            || {},
        );

        let help = HelpOverlay::from_registry(&registry);
        // Should be sorted by category (Apple before Zebra)
        assert_eq!(help.shortcuts[0].0, "Apple");
        assert_eq!(help.shortcuts[1].0, "Zebra");
    }

    #[test]
    fn test_filter_case_insensitive() {
        let mut registry = CommandRegistry::new();
        registry.register(
            Command::new("open", "Open File", "File").with_shortcut("Ctrl+O"),
            || {},
        );

        let mut help = HelpOverlay::from_registry(&registry);

        help.set_filter("FILE".to_string());
        assert_eq!(help.filtered_shortcuts().len(), 1);

        help.set_filter("ctrl".to_string());
        assert_eq!(help.filtered_shortcuts().len(), 1);
    }

    #[test]
    fn test_filter_resets_scroll() {
        let mut registry = CommandRegistry::new();
        for i in 0..20 {
            registry.register(
                Command::new(&format!("cmd{}", i), "Test", "General")
                    .with_shortcut(&format!("Ctrl+{}", i)),
                || {},
            );
        }

        let mut help = HelpOverlay::from_registry(&registry);
        help.scroll_down(5);
        assert_eq!(help.scroll_offset, 5);

        help.set_filter("test".to_string());
        assert_eq!(help.scroll_offset, 0);
    }

    #[test]
    fn test_scroll_boundaries() {
        let mut registry = CommandRegistry::new();
        registry.register(
            Command::new("cmd1", "Test", "General").with_shortcut("1"),
            || {},
        );

        let mut help = HelpOverlay::from_registry(&registry);

        // Try to scroll up when at top
        help.scroll_up(1);
        assert_eq!(help.scroll_offset, 0);

        // Try to scroll down with limited content
        help.scroll_down(100);
        // Should not exceed max
        assert!(help.scroll_offset <= help.shortcuts.len());
    }

    #[test]
    fn test_filter_getter() {
        let registry = CommandRegistry::new();
        let mut help = HelpOverlay::from_registry(&registry);

        assert_eq!(help.filter(), "");

        help.set_filter("test".to_string());
        assert_eq!(help.filter(), "test");
    }
}
