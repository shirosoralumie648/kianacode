use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph},
    Frame,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsRow {
    pub label: String,
    pub value: String,
}

impl SettingsRow {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSection {
    pub title: String,
    pub rows: Vec<SettingsRow>,
}

impl SettingsSection {
    pub fn new(title: impl Into<String>, rows: Vec<SettingsRow>) -> Self {
        Self {
            title: title.into(),
            rows,
        }
    }
}

#[derive(Debug, Default)]
pub struct SettingsState {
    pub loading: bool,
    pub sections: Vec<SettingsSection>,
    pub errors: Vec<String>,
    pub scroll_offset: usize,
}

impl SettingsState {
    pub fn set_loading(&mut self) {
        self.loading = true;
        self.errors.clear();
        self.sections = vec![SettingsSection::new(
            "Readiness",
            vec![SettingsRow::new("status", "loading settings...")],
        )];
        self.scroll_offset = 0;
    }

    pub fn set_sections(&mut self, sections: Vec<SettingsSection>) {
        self.loading = false;
        self.errors.clear();
        self.sections = sections;
        self.scroll_offset = 0;
    }

    pub fn set_error(&mut self, error: String) {
        self.loading = false;
        self.errors = vec![error];
        self.sections = vec![SettingsSection::new(
            "Readiness",
            vec![SettingsRow::new("status", "failed")],
        )];
        self.scroll_offset = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => true,
            KeyCode::PageUp | KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                false
            }
            KeyCode::PageDown | KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                false
            }
            _ => false,
        }
    }
}

pub struct SettingsScreen;

impl SettingsScreen {
    pub fn draw(frame: &mut Frame, state: &mut SettingsState) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        let mut items = Vec::new();
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Settings and Readiness",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])));
        items.push(ListItem::new(Line::from("")));

        for error in &state.errors {
            items.push(ListItem::new(Line::from(vec![
                Span::styled("error: ", Style::default().fg(Color::Red)),
                Span::raw(error.clone()),
            ])));
        }

        for section in &state.sections {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                section.title.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )])));
            if section.rows.is_empty() {
                items.push(ListItem::new(Line::from("  none")));
            } else {
                for row in &section.rows {
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  {}: ", row.label),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(row.value.clone()),
                    ])));
                }
            }
            items.push(ListItem::new(Line::from("")));
        }

        let height = chunks[0].height.saturating_sub(2) as usize;
        let max_offset = items.len().saturating_sub(height.max(1));
        if state.scroll_offset > max_offset {
            state.scroll_offset = max_offset;
        }
        let visible = items
            .into_iter()
            .skip(state.scroll_offset)
            .take(height.max(1))
            .collect::<Vec<_>>();

        let title = if state.loading {
            " Settings (loading) "
        } else {
            " Settings "
        };
        let list = List::new(visible).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        );
        frame.render_widget(list, chunks[0]);

        let prompt = Paragraph::new("Press Enter or Esc to return to REPL")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .padding(Padding::horizontal(1)),
            );
        frame.render_widget(prompt, chunks[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render_settings_to_text(state: &mut SettingsState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| SettingsScreen::draw(frame, state))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .fold(String::new(), |mut output, cell| {
                output.push_str(cell.symbol());
                output
            })
    }

    #[test]
    fn settings_state_tracks_loading_success_and_error() {
        let mut state = SettingsState::default();
        state.set_loading();
        assert!(state.loading);
        assert!(state.sections[0].rows[0].value.contains("loading"));

        state.set_sections(vec![SettingsSection::new(
            "Account",
            vec![SettingsRow::new("api_key", "missing")],
        )]);
        assert!(!state.loading);
        assert_eq!(state.sections[0].title, "Account");

        state.set_error("auth status failed".to_string());
        assert!(!state.loading);
        assert_eq!(state.errors, vec!["auth status failed"]);
    }

    #[test]
    fn settings_render_shows_readiness_sections() {
        let mut state = SettingsState::default();
        state.set_sections(vec![
            SettingsSection::new(
                "Account/Auth",
                vec![
                    SettingsRow::new("api_key", "missing"),
                    SettingsRow::new("oauth", "missing"),
                ],
            ),
            SettingsSection::new(
                "Provider/Model",
                vec![SettingsRow::new("anthropic", "missing claude-sonnet-4-6")],
            ),
        ]);

        let rendered = render_settings_to_text(&mut state, 90, 18);

        assert!(rendered.contains("Settings and Readiness"));
        assert!(rendered.contains("Account/Auth"));
        assert!(rendered.contains("api_key"));
        assert!(rendered.contains("Provider/Model"));
        assert!(rendered.contains("anthropic"));
        assert!(rendered.contains("Press Enter or Esc"));
    }
}
