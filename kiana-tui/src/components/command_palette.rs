use crate::search::fuzzy::calculate_match_score;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub type CommandAction = Box<dyn Fn() + Send>;

#[derive(Clone)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub shortcut: Option<String>,
    pub category: String,
}

impl Command {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        category: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            shortcut: None,
            category: category.into(),
        }
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }
}

pub struct CommandRegistry {
    commands: Vec<(Command, CommandAction)>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn register<F>(&mut self, command: Command, action: F)
    where
        F: Fn() + Send + 'static,
    {
        self.commands.push((command, Box::new(action)));
    }

    pub fn get_command(&self, name: &str) -> Option<&CommandAction> {
        self.commands
            .iter()
            .find(|(cmd, _)| cmd.name == name)
            .map(|(_, action)| action)
    }

    pub fn commands(&self) -> impl Iterator<Item = &Command> {
        self.commands.iter().map(|(cmd, _)| cmd)
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct SearchResult {
    command: Command,
    score: i32,
    #[allow(dead_code)]
    positions: Vec<usize>,
}

pub struct CommandPalette {
    query: String,
    results: Vec<SearchResult>,
    selected: usize,
    registry: CommandRegistry,
}

impl CommandPalette {
    pub fn new(registry: CommandRegistry) -> Self {
        let mut palette = Self {
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            registry,
        };
        palette.update_results();
        palette
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.update_results();
        self.selected = 0;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    fn update_results(&mut self) {
        if self.query.is_empty() {
            self.results = self
                .registry
                .commands()
                .map(|cmd| SearchResult {
                    command: cmd.clone(),
                    score: 0,
                    positions: Vec::new(),
                })
                .collect();
        } else {
            self.results = self
                .registry
                .commands()
                .filter_map(|cmd| {
                    calculate_match_score(&cmd.name, &self.query).map(|(score, positions)| {
                        SearchResult {
                            command: cmd.clone(),
                            score,
                            positions,
                        }
                    })
                })
                .collect();

            // Sort by score (higher is better - bigger score = better match)
            self.results.sort_by_key(|r| -r.score);
        }

        // Limit results
        self.results.truncate(10);
    }

    pub fn select_next(&mut self) {
        if !self.results.is_empty() {
            self.selected = (self.selected + 1).min(self.results.len() - 1);
        }
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_command(&self) -> Option<&str> {
        self.results
            .get(self.selected)
            .map(|r| r.command.name.as_str())
    }

    pub fn execute_selected(&self) {
        if let Some(result) = self.results.get(self.selected) {
            if let Some(action) = self.registry.get_command(&result.command.name) {
                action();
            }
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let width = 80;
        let height = 20;

        let popup_area = centered_rect(width, height, area);

        // Clear background
        f.render_widget(Clear, popup_area);

        // Create block
        let block = Block::default()
            .title("Command Palette")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(popup_area);
        f.render_widget(block, popup_area);

        // Split into input and results
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(inner);

        // Render search input
        let input_text = if self.query.is_empty() {
            Line::from(Span::styled(
                "Type to search commands...",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(Span::raw(&self.query))
        };

        let input = Paragraph::new(input_text)
            .block(Block::default().borders(Borders::ALL).title("Search"));
        f.render_widget(input, chunks[0]);

        // Render results
        let items: Vec<ListItem> = self
            .results
            .iter()
            .enumerate()
            .map(|(i, result)| {
                let is_selected = i == self.selected;

                let mut spans = vec![Span::styled(
                    &result.command.name,
                    Style::default()
                        .fg(if is_selected {
                            Color::Yellow
                        } else {
                            Color::White
                        })
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                )];

                if let Some(ref shortcut) = result.command.shortcut {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        format!("[{}]", shortcut),
                        Style::default().fg(Color::Cyan),
                    ));
                }

                spans.push(Span::raw(" - "));
                spans.push(Span::styled(
                    &result.command.description,
                    Style::default().fg(Color::DarkGray),
                ));

                let style = if is_selected {
                    Style::default().bg(Color::Blue)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(spans)).style(style)
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            "Results ({}/{})",
            self.results.len(),
            self.registry.commands().count()
        )));

        f.render_widget(list, chunks[1]);
    }
}

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((r.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_command_creation() {
        let cmd = Command::new("test", "Test command", "General").with_shortcut("Ctrl+T");

        assert_eq!(cmd.name, "test");
        assert_eq!(cmd.description, "Test command");
        assert_eq!(cmd.category, "General");
        assert_eq!(cmd.shortcut, Some("Ctrl+T".to_string()));
    }

    #[test]
    fn test_registry_register() {
        let mut registry = CommandRegistry::new();
        let executed = Arc::new(Mutex::new(false));
        let executed_clone = executed.clone();

        registry.register(Command::new("test", "Test", "General"), move || {
            *executed_clone.lock().unwrap() = true;
        });

        assert_eq!(registry.commands().count(), 1);

        let action = registry.get_command("test").unwrap();
        action();
        assert!(*executed.lock().unwrap());
    }

    #[test]
    fn test_palette_query() {
        let registry = CommandRegistry::new();
        let mut palette = CommandPalette::new(registry);

        palette.set_query("test".to_string());
        assert_eq!(palette.query(), "test");
    }

    #[test]
    fn test_palette_navigation() {
        let mut registry = CommandRegistry::new();
        registry.register(Command::new("cmd1", "First", "General"), || {});
        registry.register(Command::new("cmd2", "Second", "General"), || {});
        registry.register(Command::new("cmd3", "Third", "General"), || {});

        let mut palette = CommandPalette::new(registry);
        assert_eq!(palette.selected, 0);

        palette.select_next();
        assert_eq!(palette.selected, 1);

        palette.select_next();
        assert_eq!(palette.selected, 2);

        palette.select_previous();
        assert_eq!(palette.selected, 1);
    }

    #[test]
    fn test_fuzzy_search() {
        let mut registry = CommandRegistry::new();
        registry.register(Command::new("open_file", "Open file", "File"), || {});
        registry.register(Command::new("save_file", "Save file", "File"), || {});
        registry.register(Command::new("quit", "Quit", "App"), || {});

        let mut palette = CommandPalette::new(registry);

        // Search for "file"
        palette.set_query("file".to_string());
        assert_eq!(palette.results.len(), 2);
        assert!(palette.results[0].command.name.contains("file"));
    }

    #[test]
    fn test_empty_query_shows_all() {
        let mut registry = CommandRegistry::new();
        registry.register(Command::new("cmd1", "First", "General"), || {});
        registry.register(Command::new("cmd2", "Second", "General"), || {});

        let palette = CommandPalette::new(registry);
        assert_eq!(palette.results.len(), 2);
    }

    #[test]
    fn test_no_match_returns_empty() {
        let mut registry = CommandRegistry::new();
        registry.register(Command::new("open_file", "Open file", "File"), || {});

        let mut palette = CommandPalette::new(registry);
        palette.set_query("xyz".to_string());
        assert_eq!(palette.results.len(), 0);
    }

    #[test]
    fn test_selected_command() {
        let mut registry = CommandRegistry::new();
        registry.register(Command::new("cmd1", "First", "General"), || {});
        registry.register(Command::new("cmd2", "Second", "General"), || {});

        let mut palette = CommandPalette::new(registry);
        assert_eq!(palette.selected_command(), Some("cmd1"));

        palette.select_next();
        assert_eq!(palette.selected_command(), Some("cmd2"));
    }

    #[test]
    fn test_execute_selected() {
        let mut registry = CommandRegistry::new();
        let executed = Arc::new(Mutex::new(String::new()));
        let executed_clone = executed.clone();

        registry.register(Command::new("test_cmd", "Test", "General"), move || {
            *executed_clone.lock().unwrap() = "executed".to_string();
        });

        let palette = CommandPalette::new(registry);
        palette.execute_selected();
        assert_eq!(*executed.lock().unwrap(), "executed");
    }

    #[test]
    fn test_results_limited_to_10() {
        let mut registry = CommandRegistry::new();
        for i in 0..20 {
            registry.register(Command::new(format!("cmd{}", i), "Test", "General"), || {});
        }

        let palette = CommandPalette::new(registry);
        assert_eq!(palette.results.len(), 10);
    }

    #[test]
    fn test_navigation_boundary() {
        let mut registry = CommandRegistry::new();
        registry.register(Command::new("cmd1", "First", "General"), || {});

        let mut palette = CommandPalette::new(registry);

        // Try to go below 0
        palette.select_previous();
        assert_eq!(palette.selected, 0);

        // Try to go above max
        palette.select_next();
        palette.select_next();
        assert_eq!(palette.selected, 0); // Still at max (0 for single item)
    }

    #[test]
    fn test_search_performance() {
        use std::time::Instant;

        let mut registry = CommandRegistry::new();

        // Create 100 commands as per requirement
        for i in 0..100 {
            registry.register(
                Command::new(
                    format!("command_{}", i),
                    format!("Description for command {}", i),
                    "General",
                ),
                || {},
            );
        }

        let mut palette = CommandPalette::new(registry);

        // Measure search time
        let start = Instant::now();
        palette.set_query("command_5".to_string());
        let duration = start.elapsed();

        // Should be less than 50ms
        assert!(
            duration.as_millis() < 50,
            "Search took {}ms, should be <50ms",
            duration.as_millis()
        );

        // Should find matching commands
        assert!(palette.results.len() > 0);
    }

    #[test]
    fn test_query_reset_on_new_search() {
        let mut registry = CommandRegistry::new();
        registry.register(Command::new("cmd1", "First", "General"), || {});
        registry.register(Command::new("cmd2", "Second", "General"), || {});

        let mut palette = CommandPalette::new(registry);

        palette.set_query("cmd1".to_string());
        palette.select_next();
        assert_eq!(palette.selected, 0);

        // Setting new query should reset selection
        palette.set_query("cmd2".to_string());
        assert_eq!(palette.selected, 0);
    }
}
