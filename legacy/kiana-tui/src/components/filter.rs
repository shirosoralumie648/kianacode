/// Filter input UI component
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::filter::{parse_filter, FieldRegistry, FilterExpr};

/// Actions that can be triggered by the filter input
#[derive(Debug, Clone, PartialEq)]
pub enum FilterAction {
    /// No action
    None,

    /// Apply the current filter expression
    Apply(String),

    /// Clear the filter
    Clear,

    /// Save current filter as preset
    SavePreset,

    /// Load presets dialog
    LoadPresets,

    /// Cancel filter input
    Cancel,

    /// Show field suggestions
    ShowSuggestions,
}

/// Filter input widget
pub struct FilterInput {
    /// Current input text
    input: String,

    /// Cursor position in the input
    cursor_position: usize,

    /// Whether the filter input is active
    active: bool,

    /// Parse error message
    error: Option<String>,

    /// Number of matches (set externally)
    matches_count: Option<usize>,

    /// Total number of items (set externally)
    total_count: Option<usize>,

    /// Field suggestions
    suggestions: Vec<String>,

    /// Field registry for auto-completion
    field_registry: Option<FieldRegistry>,

    /// Whether to show suggestions
    show_suggestions: bool,
}

impl FilterInput {
    /// Create a new filter input
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
            active: false,
            error: None,
            matches_count: None,
            total_count: None,
            suggestions: Vec::new(),
            field_registry: None,
            show_suggestions: false,
        }
    }

    /// Set the field registry for auto-completion
    pub fn with_field_registry(mut self, registry: FieldRegistry) -> Self {
        self.field_registry = Some(registry);
        self
    }

    /// Activate the filter input
    pub fn activate(&mut self) {
        self.active = true;
        self.error = None;
    }

    /// Deactivate the filter input
    pub fn deactivate(&mut self) {
        self.active = false;
        self.show_suggestions = false;
    }

    /// Check if the filter input is active
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Set the filter expression
    pub fn set_expression(&mut self, expr: String) {
        self.input = expr;
        self.cursor_position = self.input.len();
        self.validate_internal();
        self.update_suggestions();
    }

    /// Get the current expression
    pub fn get_expression(&self) -> &str {
        &self.input
    }

    /// Set match count
    pub fn set_matches(&mut self, matches: usize, total: usize) {
        self.matches_count = Some(matches);
        self.total_count = Some(total);
    }

    /// Clear match count
    pub fn clear_matches(&mut self) {
        self.matches_count = None;
        self.total_count = None;
    }

    /// Validate the current expression and return the AST
    pub fn validate(&mut self) -> Result<FilterExpr, String> {
        parse_filter(&self.input).map_err(|e| {
            let msg = e.to_string();
            self.error = Some(msg.clone());
            msg
        })
    }

    /// Internal validation (updates error state)
    fn validate_internal(&mut self) {
        if self.input.is_empty() {
            self.error = None;
            return;
        }

        match parse_filter(&self.input) {
            Ok(_) => self.error = None,
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Update field suggestions based on current input
    fn update_suggestions(&mut self) {
        if let Some(ref registry) = self.field_registry {
            // Find the last word being typed (potential field name)
            let words: Vec<&str> = self.input.split_whitespace().collect();
            if let Some(last_word) = words.last() {
                // Check if it looks like a field name (before : = ~ or comparison)
                let field_part = last_word
                    .split(&[':', '=', '~', '>', '<', '!'][..])
                    .next()
                    .unwrap_or("");

                if !field_part.is_empty() {
                    self.suggestions = registry.suggest(field_part);
                } else {
                    self.suggestions.clear();
                }
            } else {
                self.suggestions.clear();
            }
        }
    }

    /// Handle keyboard input
    pub fn handle_key(&mut self, key: KeyEvent) -> FilterAction {
        match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => match c {
                'c' => {
                    self.deactivate();
                    FilterAction::Cancel
                }
                's' => FilterAction::SavePreset,
                'l' => FilterAction::LoadPresets,
                _ => FilterAction::None,
            },
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_position, c);
                self.cursor_position += 1;
                self.validate_internal();
                self.update_suggestions();
                FilterAction::None
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.input.remove(self.cursor_position - 1);
                    self.cursor_position -= 1;
                    self.validate_internal();
                    self.update_suggestions();
                }
                FilterAction::None
            }
            KeyCode::Delete => {
                if self.cursor_position < self.input.len() {
                    self.input.remove(self.cursor_position);
                    self.validate_internal();
                    self.update_suggestions();
                }
                FilterAction::None
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
                FilterAction::None
            }
            KeyCode::Right => {
                if self.cursor_position < self.input.len() {
                    self.cursor_position += 1;
                }
                FilterAction::None
            }
            KeyCode::Home => {
                self.cursor_position = 0;
                FilterAction::None
            }
            KeyCode::End => {
                self.cursor_position = self.input.len();
                FilterAction::None
            }
            KeyCode::Enter => {
                if self.error.is_none() && !self.input.is_empty() {
                    FilterAction::Apply(self.input.clone())
                } else {
                    FilterAction::None
                }
            }
            KeyCode::Esc => {
                if self.input.is_empty() {
                    self.deactivate();
                    FilterAction::Cancel
                } else {
                    self.input.clear();
                    self.cursor_position = 0;
                    self.error = None;
                    self.clear_matches();
                    FilterAction::Clear
                }
            }
            KeyCode::Tab => {
                self.show_suggestions = !self.show_suggestions;
                FilterAction::ShowSuggestions
            }
            _ => FilterAction::None,
        }
    }

    /// Render the filter input
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.active {
            return;
        }

        // Create layout for input and suggestions
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        // Render input box
        self.render_input(f, chunks[0]);

        // Render suggestions if enabled
        if self.show_suggestions && !self.suggestions.is_empty() {
            self.render_suggestions(f, chunks[1]);
        }
    }

    fn render_input(&self, f: &mut Frame, area: Rect) {
        let mut spans = Vec::new();

        // Syntax highlighting (basic)
        let input_parts = self.highlight_syntax();
        for (text, style) in input_parts {
            spans.push(Span::styled(text, style));
        }

        // Add cursor
        if self.active {
            spans.push(Span::styled("█", Style::default().fg(Color::White)));
        }

        let input_line = Line::from(spans);

        // Status indicator
        let status = if let Some(error) = &self.error {
            format!(" ✗ Error: {}", error)
        } else if let (Some(matches), Some(total)) = (self.matches_count, self.total_count) {
            format!(" ✓ {} / {} matches", matches, total)
        } else if !self.input.is_empty() && self.error.is_none() {
            " ✓ Valid".to_string()
        } else {
            String::new()
        };

        let status_style = if self.error.is_some() {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title("Filter")
            .title_bottom(Line::from(vec![
                Span::styled(status, status_style),
                Span::raw(" [Enter] Apply [Esc] Clear/Cancel [Tab] Suggestions"),
            ]));

        let paragraph = Paragraph::new(input_line).block(block);

        f.render_widget(paragraph, area);
    }

    fn render_suggestions(&self, f: &mut Frame, area: Rect) {
        if self.suggestions.is_empty() {
            return;
        }

        let suggestions_text: Vec<Line> = self
            .suggestions
            .iter()
            .take(5) // Show max 5 suggestions
            .map(|s| Line::from(format!("  • {}", s)))
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .title("Field Suggestions");

        let paragraph = Paragraph::new(suggestions_text).block(block);

        // Only use a small portion of the area
        let suggestion_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: (self.suggestions.len().min(5) as u16) + 2,
        };

        f.render_widget(paragraph, suggestion_area);
    }

    fn highlight_syntax(&self) -> Vec<(String, Style)> {
        let mut result = Vec::new();
        let mut current = String::new();
        let mut current_style = Style::default();

        for (i, ch) in self.input.chars().enumerate() {
            let style = if ch == '(' || ch == ')' {
                Style::default().fg(Color::Yellow)
            } else if ch == ':' || ch == '=' || ch == '~' {
                Style::default().fg(Color::Cyan)
            } else if ch == '>' || ch == '<' || ch == '!' {
                Style::default().fg(Color::Magenta)
            } else if ch == '"' || ch == '\'' {
                Style::default().fg(Color::Green)
            } else if i < self.cursor_position {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };

            if style != current_style {
                if !current.is_empty() {
                    result.push((current.clone(), current_style));
                    current.clear();
                }
                current_style = style;
            }

            current.push(ch);
        }

        if !current.is_empty() {
            result.push((current, current_style));
        }

        result
    }
}

impl Default for FilterInput {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_input_creation() {
        let input = FilterInput::new();
        assert!(!input.is_active());
        assert_eq!(input.get_expression(), "");
    }

    #[test]
    fn test_activate_deactivate() {
        let mut input = FilterInput::new();
        assert!(!input.is_active());

        input.activate();
        assert!(input.is_active());

        input.deactivate();
        assert!(!input.is_active());
    }

    #[test]
    fn test_set_expression() {
        let mut input = FilterInput::new();
        input.set_expression("status:error".to_string());

        assert_eq!(input.get_expression(), "status:error");
        assert_eq!(input.cursor_position, 12);
    }

    #[test]
    fn test_validate_valid() {
        let mut input = FilterInput::new();
        input.set_expression("status=error".to_string());

        let result = input.validate();
        assert!(result.is_ok());
        assert!(input.error.is_none());
    }

    #[test]
    fn test_validate_invalid() {
        let mut input = FilterInput::new();
        input.set_expression("(unclosed".to_string());

        let result = input.validate();
        assert!(result.is_err());
        assert!(input.error.is_some());
    }

    #[test]
    fn test_handle_key_char() {
        let mut input = FilterInput::new();
        input.activate();

        let key = KeyEvent::from(KeyCode::Char('a'));
        input.handle_key(key);

        assert_eq!(input.get_expression(), "a");
        assert_eq!(input.cursor_position, 1);
    }

    #[test]
    fn test_handle_key_backspace() {
        let mut input = FilterInput::new();
        input.activate();
        input.set_expression("test".to_string());

        let key = KeyEvent::from(KeyCode::Backspace);
        input.handle_key(key);

        assert_eq!(input.get_expression(), "tes");
        assert_eq!(input.cursor_position, 3);
    }

    #[test]
    fn test_handle_key_enter() {
        let mut input = FilterInput::new();
        input.activate();
        input.set_expression("error".to_string());

        let key = KeyEvent::from(KeyCode::Enter);
        let action = input.handle_key(key);

        assert_eq!(action, FilterAction::Apply("error".to_string()));
    }

    #[test]
    fn test_handle_key_esc() {
        let mut input = FilterInput::new();
        input.activate();
        input.set_expression("test".to_string());

        let key = KeyEvent::from(KeyCode::Esc);
        let action = input.handle_key(key);

        assert_eq!(action, FilterAction::Clear);
        assert_eq!(input.get_expression(), "");
    }

    #[test]
    fn test_set_matches() {
        let mut input = FilterInput::new();
        input.set_matches(42, 100);

        assert_eq!(input.matches_count, Some(42));
        assert_eq!(input.total_count, Some(100));
    }
}
