/// Doctor screen — diagnostics and installation info.
///
/// TypeScript reference: `reference/src/screens/Doctor.tsx`
///
/// Displays:
///   - installation type, version, path
///   - auto-update status
///   - tool/agent counts
///   - config warnings
///   - press Enter/Esc to return to REPL
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph},
    Frame,
};

/// Doctor screen state.
#[derive(Debug, Default)]
pub struct DoctorState {
    pub loading: bool,
    pub diagnostic_lines: Vec<String>,
    pub version: String,
    pub install_type: String,
    pub install_path: String,
    pub package_manager: Option<String>,
    pub auto_updates: String,
    pub ripgrep_working: bool,
    pub active_tools: usize,
    pub active_agents: usize,
    pub warnings: Vec<String>,
}

impl DoctorState {
    pub fn set_loading(&mut self) {
        self.loading = true;
        self.diagnostic_lines = vec!["Loading diagnostics...".to_string()];
    }

    pub fn set_output(&mut self, output: String) {
        self.loading = false;
        self.diagnostic_lines =
            diagnostic_lines_or_fallback(output, "Doctor completed with no output.");
    }

    pub fn set_error(&mut self, error: String) {
        self.loading = false;
        self.diagnostic_lines = vec![format!("Doctor failed: {error}")];
    }

    /// Returns `true` if the user pressed Enter/Esc to exit the doctor screen.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        matches!(key.code, KeyCode::Enter | KeyCode::Esc)
    }
}

fn diagnostic_lines_or_fallback(output: String, fallback: &str) -> Vec<String> {
    let lines = output
        .lines()
        .map(str::trim_end)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if lines.iter().any(|line| !line.trim().is_empty()) {
        lines
    } else {
        vec![fallback.to_string()]
    }
}

pub struct DoctorScreen;

impl DoctorScreen {
    pub fn draw(frame: &mut Frame, state: &DoctorState) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        let info_area = chunks[0];
        let prompt_area = chunks[1];

        // Build diagnostic lines
        let mut items = Vec::new();

        // Title
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Diagnostics",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])));

        items.push(ListItem::new(Line::from("")));

        if !state.diagnostic_lines.is_empty() {
            for line in &state.diagnostic_lines {
                items.push(ListItem::new(Line::from(line.clone())));
            }
            if !state.warnings.is_empty() {
                items.push(ListItem::new(Line::from("")));
                items.push(ListItem::new(Line::from(vec![Span::styled(
                    "Warnings",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )])));
                for warning in &state.warnings {
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled("└ ", Style::default().fg(Color::Yellow)),
                        Span::raw(warning.clone()),
                    ])));
                }
            }

            let list = List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Doctor ")
                    .title_style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
            );

            frame.render_widget(list, info_area);

            let prompt = Paragraph::new("Press Enter or Esc to return to REPL")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .padding(Padding::horizontal(1)),
                );
            frame.render_widget(prompt, prompt_area);
            return;
        }

        // Installation info
        items.push(ListItem::new(Line::from(vec![
            Span::raw("└ Currently running: "),
            Span::styled(
                format!("{} ({})", state.install_type, state.version),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ])));

        if let Some(ref pm) = state.package_manager {
            items.push(ListItem::new(Line::from(format!(
                "└ Package manager: {}",
                pm
            ))));
        }

        items.push(ListItem::new(Line::from(format!(
            "└ Path: {}",
            state.install_path
        ))));

        items.push(ListItem::new(Line::from(format!(
            "└ Search: {}",
            if state.ripgrep_working {
                "OK"
            } else {
                "Not working"
            }
        ))));

        items.push(ListItem::new(Line::from("")));

        // Updates
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Updates",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])));
        items.push(ListItem::new(Line::from(format!(
            "└ Auto-updates: {}",
            state.auto_updates
        ))));

        items.push(ListItem::new(Line::from("")));

        // Tools & Agents
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Environment",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])));
        items.push(ListItem::new(Line::from(format!(
            "└ Active tools: {}",
            state.active_tools
        ))));
        items.push(ListItem::new(Line::from(format!(
            "└ Active agents: {}",
            state.active_agents
        ))));

        // Warnings
        if !state.warnings.is_empty() {
            items.push(ListItem::new(Line::from("")));
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "Warnings",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )])));
            for warning in &state.warnings {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("└ ", Style::default().fg(Color::Yellow)),
                    Span::raw(warning.clone()),
                ])));
            }
        }

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Doctor ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        );

        frame.render_widget(list, info_area);

        // Prompt at the bottom
        let prompt = Paragraph::new("Press Enter or Esc to return to REPL")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .padding(Padding::horizontal(1)),
            );
        frame.render_widget(prompt, prompt_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_state_uses_cli_output_lines() {
        let mut state = DoctorState::default();

        state.set_output("Doctor\ncwd: /tmp/work\napi_key_set: yes\n".to_string());

        assert!(!state.loading);
        assert_eq!(
            state.diagnostic_lines,
            vec![
                "Doctor".to_string(),
                "cwd: /tmp/work".to_string(),
                "api_key_set: yes".to_string()
            ]
        );
    }

    #[test]
    fn doctor_state_reports_empty_output_and_errors() {
        let mut state = DoctorState::default();
        state.set_output(String::new());
        assert_eq!(
            state.diagnostic_lines,
            vec!["Doctor completed with no output."]
        );

        state.set_error("boom".to_string());
        assert_eq!(state.diagnostic_lines, vec!["Doctor failed: boom"]);
    }
}
