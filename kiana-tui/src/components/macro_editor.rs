use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::macro_system::{Macro, MacroAction};

/// Action returned by macro editor
#[derive(Debug, Clone, PartialEq)]
pub enum MacroEditorAction {
    None,
    Close,
    DeleteMacro(char),
    DeleteAction(char, usize),
    SetDescription(char, String),
}

/// Visual macro editor widget
pub struct MacroEditor {
    /// All macros as a sorted list
    macros: Vec<Macro>,
    /// Selected macro index
    selected_macro: usize,
    /// Selected action index within macro
    selected_action: usize,
    /// List state for macro list
    macro_list_state: ListState,
    /// Scroll offset for actions
    action_scroll: usize,
    /// Whether in edit mode
    editing: bool,
}

impl MacroEditor {
    /// Create a new macro editor
    pub fn new(macros: Vec<Macro>) -> Self {
        let mut sorted = macros;
        sorted.sort_by_key(|m| m.register);

        let mut macro_list_state = ListState::default();
        if !sorted.is_empty() {
            macro_list_state.select(Some(0));
        }

        Self {
            macros: sorted,
            selected_macro: 0,
            selected_action: 0,
            macro_list_state,
            action_scroll: 0,
            editing: false,
        }
    }

    /// Update macros and refresh view
    pub fn update_macros(&mut self, macros: Vec<Macro>) {
        let mut sorted = macros;
        sorted.sort_by_key(|m| m.register);

        // Preserve selection if possible
        let old_register = self.get_selected_macro().map(|m| m.register);

        self.macros = sorted;

        if let Some(old_reg) = old_register {
            if let Some(idx) = self.macros.iter().position(|m| m.register == old_reg) {
                self.selected_macro = idx;
                self.macro_list_state.select(Some(idx));
            } else {
                self.selected_macro = 0;
                self.macro_list_state.select(if self.macros.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
        }
    }

    /// Handle keyboard input
    pub fn handle_key(&mut self, key: KeyEvent) -> MacroEditorAction {
        if self.macros.is_empty() {
            return match key.code {
                KeyCode::Char('q') | KeyCode::Esc => MacroEditorAction::Close,
                _ => MacroEditorAction::None,
            };
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => MacroEditorAction::Close,

            KeyCode::Up | KeyCode::Char('k') if !self.editing => {
                self.move_up();
                MacroEditorAction::None
            }

            KeyCode::Down | KeyCode::Char('j') if !self.editing => {
                self.move_down();
                MacroEditorAction::None
            }

            KeyCode::Left | KeyCode::Char('h') if !self.editing => {
                self.move_to_macro_list();
                MacroEditorAction::None
            }

            KeyCode::Right | KeyCode::Char('l') if !self.editing => {
                self.move_to_action_list();
                MacroEditorAction::None
            }

            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_selected_macro()
            }

            KeyCode::Delete => self.delete_selected_action(),

            _ => MacroEditorAction::None,
        }
    }

    fn move_up(&mut self) {
        if self.selected_macro > 0 {
            self.selected_macro -= 1;
            self.macro_list_state.select(Some(self.selected_macro));
            self.selected_action = 0;
            self.action_scroll = 0;
        }
    }

    fn move_down(&mut self) {
        if self.selected_macro + 1 < self.macros.len() {
            self.selected_macro += 1;
            self.macro_list_state.select(Some(self.selected_macro));
            self.selected_action = 0;
            self.action_scroll = 0;
        }
    }

    fn move_to_macro_list(&mut self) {
        // Switch focus to macro list (left panel)
        self.editing = false;
    }

    fn move_to_action_list(&mut self) {
        // Switch focus to action list (right panel)
        if let Some(macro_def) = self.get_selected_macro() {
            if !macro_def.actions.is_empty() {
                self.editing = true;
            }
        }
    }

    fn delete_selected_macro(&mut self) -> MacroEditorAction {
        if let Some(macro_def) = self.get_selected_macro() {
            let register = macro_def.register;
            MacroEditorAction::DeleteMacro(register)
        } else {
            MacroEditorAction::None
        }
    }

    fn delete_selected_action(&mut self) -> MacroEditorAction {
        if !self.editing {
            return MacroEditorAction::None;
        }

        if let Some(macro_def) = self.get_selected_macro() {
            if self.selected_action < macro_def.actions.len() {
                return MacroEditorAction::DeleteAction(macro_def.register, self.selected_action);
            }
        }
        MacroEditorAction::None
    }

    fn get_selected_macro(&self) -> Option<&Macro> {
        self.macros.get(self.selected_macro)
    }

    /// Render the macro editor
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(area);

        self.render_macro_list(frame, chunks[0]);
        self.render_action_details(frame, chunks[1]);
    }

    fn render_macro_list(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .macros
            .iter()
            .map(|m| {
                let action_count = m.actions.len();
                let desc = m.description.as_deref().unwrap_or("(no description)");
                let content = format!("{}: {} [{}]", m.register, desc, action_count);
                ListItem::new(content)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title("Macros (a-z)")
                    .borders(Borders::ALL)
                    .style(if !self.editing {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default()
                    }),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, area, &mut self.macro_list_state);
    }

    fn render_action_details(&self, frame: &mut Frame, area: Rect) {
        if let Some(macro_def) = self.get_selected_macro() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(10), Constraint::Length(3)])
                .split(area);

            // Actions list
            let actions_text: Vec<Line> = macro_def
                .actions
                .iter()
                .enumerate()
                .map(|(idx, action)| {
                    let prefix = if idx == self.selected_action && self.editing {
                        "> "
                    } else {
                        "  "
                    };
                    let action_str = format_action(action);
                    Line::from(vec![
                        Span::styled(
                            format!("{}{:3}. ", prefix, idx + 1),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(action_str),
                    ])
                })
                .collect();

            let actions_paragraph = Paragraph::new(actions_text)
                .block(
                    Block::default()
                        .title(format!("Actions (@{})", macro_def.register))
                        .borders(Borders::ALL)
                        .style(if self.editing {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default()
                        }),
                )
                .wrap(Wrap { trim: false });

            frame.render_widget(actions_paragraph, chunks[0]);

            // Metadata
            let metadata_text = vec![Line::from(vec![
                Span::styled("Created: ", Style::default().fg(Color::Gray)),
                Span::raw(macro_def.created_at.format("%Y-%m-%d %H:%M").to_string()),
                Span::raw("  "),
                Span::styled("Modified: ", Style::default().fg(Color::Gray)),
                Span::raw(macro_def.modified_at.format("%Y-%m-%d %H:%M").to_string()),
            ])];

            let metadata =
                Paragraph::new(metadata_text).block(Block::default().borders(Borders::ALL));

            frame.render_widget(metadata, chunks[1]);
        } else {
            let empty = Paragraph::new("No macro selected")
                .block(Block::default().title("Actions").borders(Borders::ALL))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, area);
        }
    }

    /// Get total macro count
    pub fn macro_count(&self) -> usize {
        self.macros.len()
    }
}

/// Format a macro action for display
fn format_action(action: &MacroAction) -> String {
    match action {
        MacroAction::KeyPress { code, modifiers } => {
            let mut parts = Vec::new();

            if modifiers.contains(KeyModifiers::CONTROL) {
                parts.push("Ctrl");
            }
            if modifiers.contains(KeyModifiers::ALT) {
                parts.push("Alt");
            }
            if modifiers.contains(KeyModifiers::SHIFT) {
                parts.push("Shift");
            }

            let key_str = match code {
                KeyCode::Char(c) => format!("'{}'", c),
                KeyCode::F(n) => format!("F{}", n),
                KeyCode::Backspace => "Backspace".to_string(),
                KeyCode::Enter => "Enter".to_string(),
                KeyCode::Left => "←".to_string(),
                KeyCode::Right => "→".to_string(),
                KeyCode::Up => "↑".to_string(),
                KeyCode::Down => "↓".to_string(),
                KeyCode::Home => "Home".to_string(),
                KeyCode::End => "End".to_string(),
                KeyCode::PageUp => "PgUp".to_string(),
                KeyCode::PageDown => "PgDn".to_string(),
                KeyCode::Tab => "Tab".to_string(),
                KeyCode::BackTab => "BackTab".to_string(),
                KeyCode::Delete => "Del".to_string(),
                KeyCode::Insert => "Ins".to_string(),
                KeyCode::Esc => "Esc".to_string(),
                _ => "?".to_string(),
            };

            parts.push(&key_str);
            format!("KeyPress({})", parts.join("+"))
        }
        MacroAction::Delay { ms } => format!("Delay({}ms)", ms),
    }
}
