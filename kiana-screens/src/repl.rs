/// REPL screen — the main conversation loop.
///
/// TypeScript reference: `reference/src/screens/REPL.tsx`
///
/// The TS component is enormous (the full file is ~3 MB compiled), so this
/// Rust translation keeps the screen-state and rendering layer focused:
///   - scrollable message list (virtual)
///   - multi-line prompt input with history
///   - status bar (model, cost, turn spinner)
///   - basic keybindings (Enter to send, ↑/↓ history)
///
/// The entrypoint crate wires submitted prompts to SDK sessions and the
/// assistant runner through the AppAction channel. Slash commands are parsed
/// here so local commands never leak into the model prompt path.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::AppScreen;

// ─── Message entry ───────────────────────────────────────────────────────────

/// A single entry in the conversation transcript.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub content: String,
    /// ISO-8601 timestamp for display.
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplPermissionPanel {
    pub tool_name: String,
    pub tool_use_id: String,
    pub reason: Option<String>,
    pub blocked_path: Option<String>,
    pub input_preview: Option<String>,
    pub suggestions_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplEvent {
    SwitchScreen(AppScreen),
    SubmitPrompt(String),
    QueuePrompt(String),
    RunSlashCommand { name: String, args: String },
}

impl MessageRole {
    fn label(&self) -> &'static str {
        match self {
            MessageRole::User => "You",
            MessageRole::Assistant => "Kiana",
            MessageRole::System => "System",
            MessageRole::Tool => "Tool",
        }
    }

    fn color(&self) -> Color {
        match self {
            MessageRole::User => Color::Cyan,
            MessageRole::Assistant => Color::Green,
            MessageRole::System => Color::DarkGray,
            MessageRole::Tool => Color::Yellow,
        }
    }
}

// ─── Spinner state ───────────────────────────────────────────────────────────

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ─── REPL state ──────────────────────────────────────────────────────────────

/// All mutable state owned by the REPL screen.
#[derive(Debug)]
pub struct ReplState {
    /// The conversation transcript.
    pub messages: Vec<ConversationMessage>,
    /// Current prompt text.
    pub input: String,
    /// Cursor position within `input` (byte offset).
    pub cursor: usize,
    /// Command history for ↑/↓ navigation.
    pub history: Vec<String>,
    /// Index into history; `None` = not navigating.
    pub history_idx: Option<usize>,
    /// Saved draft while navigating history.
    pub history_draft: String,
    /// Is the assistant currently responding?
    pub is_loading: bool,
    /// Is there an active permission request waiting for user response?
    pub permission_request_active: bool,
    /// Number of permission requests waiting behind the active request.
    pub permission_request_queue_len: usize,
    /// Structured details for the active permission request.
    pub permission_panel: Option<ReplPermissionPanel>,
    /// Spinner tick counter.
    pub spinner_tick: usize,
    /// Scroll offset in the message list (items from bottom).
    pub scroll_offset: usize,
    /// Ratatui list state for the message list.
    pub list_state: ListState,
    /// Token / cost info for the status bar.
    pub session_cost: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Current model name.
    pub model_name: String,
}

impl Default for ReplState {
    fn default() -> Self {
        let mut list_state = ListState::default();
        list_state.select(None);
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_idx: None,
            history_draft: String::new(),
            is_loading: false,
            permission_request_active: false,
            permission_request_queue_len: 0,
            permission_panel: None,
            spinner_tick: 0,
            scroll_offset: 0,
            list_state,
            session_cost: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            model_name: "claude-sonnet-4-5".to_owned(),
        }
    }
}

impl ReplState {
    /// Handle a key event.  Returns the next screen to switch to (if any).
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ReplEvent> {
        match key.code {
            // ── Submit ────────────────────────────────────────────────────
            KeyCode::Enter => {
                if is_newline_key(key) {
                    self.insert_char('\n');
                    return None;
                }
                let text = self.input.trim().to_owned();
                if text.is_empty() {
                    return None;
                }
                if self.is_loading {
                    if let Some((name, args)) = split_slash_command(&text) {
                        if is_loading_slash_command(&name) {
                            self.history_idx = None;
                            self.history_draft.clear();
                            self.input.clear();
                            self.cursor = 0;
                            return Some(ReplEvent::RunSlashCommand { name, args });
                        }
                        return None;
                    }
                    if self.history.last().map(|h| h != &text).unwrap_or(true) {
                        self.history.push(text.clone());
                    }
                    self.history_idx = None;
                    self.history_draft.clear();
                    self.input.clear();
                    self.cursor = 0;
                    return Some(ReplEvent::QueuePrompt(text));
                }
                if self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text.clone());
                }
                self.history_idx = None;
                self.history_draft.clear();
                self.input.clear();
                self.cursor = 0;

                if let Some((name, args)) = split_slash_command(&text) {
                    return match name.as_str() {
                        "doctor" => Some(ReplEvent::SwitchScreen(AppScreen::Doctor)),
                        "resume" => Some(ReplEvent::SwitchScreen(AppScreen::ResumeConversation)),
                        _ => Some(ReplEvent::RunSlashCommand { name, args }),
                    };
                }

                // Add user message to transcript
                self.push_message(MessageRole::User, text);
                return Some(ReplEvent::SubmitPrompt(
                    self.messages
                        .last()
                        .map(|message| message.content.clone())
                        .unwrap_or_default(),
                ));
            }

            // ── Editing ───────────────────────────────────────────────────
            KeyCode::Char(c) => {
                // Handle Ctrl-U (clear line)
                if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'j' {
                    self.insert_char('\n');
                } else if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' {
                    let tail = self.input[self.cursor..].to_owned();
                    self.input = tail;
                    self.cursor = 0;
                } else if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'w' {
                    // Ctrl-W: delete word before cursor
                    let head = &self.input[..self.cursor];
                    let new_end = head
                        .trim_end_matches(|c: char| !c.is_whitespace())
                        .trim_end_matches(char::is_whitespace)
                        .len();
                    self.input =
                        format!("{}{}", &self.input[..new_end], &self.input[self.cursor..]);
                    self.cursor = new_end;
                } else if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'a' {
                    self.cursor = 0;
                } else if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'e' {
                    self.cursor = self.input.len();
                } else {
                    self.insert_char(c);
                }
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    // Walk back one Unicode char
                    let prev = self.input[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.remove(prev);
                    self.cursor = prev;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = self.input[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                if self.cursor < self.input.len() {
                    let ch = self.input[self.cursor..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    self.cursor += ch;
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),

            // ── History navigation ────────────────────────────────────────
            KeyCode::Up => {
                if self.history.is_empty() {
                    return None;
                }
                if self.history_idx.is_none() {
                    self.history_draft = self.input.clone();
                }
                let new_idx = self
                    .history_idx
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(self.history.len() - 1);
                self.history_idx = Some(new_idx);
                self.input = self.history[new_idx].clone();
                self.cursor = self.input.len();
            }
            KeyCode::Down => {
                if let Some(idx) = self.history_idx {
                    if idx + 1 >= self.history.len() {
                        self.history_idx = None;
                        self.input = self.history_draft.clone();
                    } else {
                        let new_idx = idx + 1;
                        self.history_idx = Some(new_idx);
                        self.input = self.history[new_idx].clone();
                    }
                    self.cursor = self.input.len();
                }
            }

            // ── Scroll ────────────────────────────────────────────────────
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(5);
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
            }

            _ => {}
        }
        None
    }

    pub fn push_message(&mut self, role: MessageRole, content: String) {
        self.messages.push(ConversationMessage {
            role,
            content,
            timestamp: chrono_now(),
        });
        // Auto-scroll to bottom
        self.scroll_offset = 0;
    }

    pub fn tick_spinner(&mut self) {
        self.spinner_tick = (self.spinner_tick + 1) % SPINNER_FRAMES.len();
    }

    pub fn set_permission_request(&mut self, panel: ReplPermissionPanel, queued_count: usize) {
        self.permission_request_active = true;
        self.permission_request_queue_len = queued_count;
        self.permission_panel = Some(panel);
    }

    pub fn clear_permission_request(&mut self) {
        self.permission_request_active = false;
        self.permission_request_queue_len = 0;
        self.permission_panel = None;
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }
}

fn is_newline_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && (key.modifiers.contains(KeyModifiers::SHIFT)
            || key.modifiers.contains(KeyModifiers::ALT))
}

fn split_slash_command(input: &str) -> Option<(String, String)> {
    let command_line = input.strip_prefix('/')?;
    let mut parts = command_line.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let args = parts.next().unwrap_or_default().trim().to_string();
    Some((name, args))
}

fn is_loading_slash_command(name: &str) -> bool {
    matches!(
        name,
        "allow" | "approve" | "deny" | "reject" | "cancel" | "stop"
    )
}

// ─── Rendering ───────────────────────────────────────────────────────────────

pub struct ReplScreen;

impl ReplScreen {
    pub fn draw(frame: &mut Frame, state: &mut ReplState) {
        let area = frame.area();
        let permission_height = permission_panel_height(state, area.width, area.height);
        let input_height = input_panel_height(
            state,
            area.width,
            area.height.saturating_sub(permission_height),
        );

        if state.permission_panel.is_some() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(permission_height),
                    Constraint::Length(1),
                    Constraint::Length(input_height),
                ])
                .split(area);

            Self::draw_messages(frame, state, chunks[0]);
            Self::draw_permission_panel(frame, state, chunks[1]);
            Self::draw_status(frame, state, chunks[2]);
            Self::draw_input(frame, state, chunks[3]);
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),    // message list
                    Constraint::Length(1), // status line
                    Constraint::Length(input_height),
                ])
                .split(area);

            Self::draw_messages(frame, state, chunks[0]);
            Self::draw_status(frame, state, chunks[1]);
            Self::draw_input(frame, state, chunks[2]);
        }
    }

    fn draw_messages(frame: &mut Frame, state: &mut ReplState, area: Rect) {
        let height = area.height as usize;

        // Build list items from messages, newest at bottom
        let items: Vec<ListItem> = state
            .messages
            .iter()
            .flat_map(|msg| message_to_lines(msg, area.width as usize))
            .collect();

        let total = items.len();
        // Clamp scroll
        let max_offset = total.saturating_sub(height);
        if state.scroll_offset > max_offset {
            state.scroll_offset = max_offset;
        }

        // Select the window of items that fills the viewport
        let visible: Vec<ListItem> = if total <= height {
            items
        } else {
            let start = total - height - state.scroll_offset;
            items.into_iter().skip(start).take(height).collect()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Kiana ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );

        let list = List::new(visible).block(block);
        frame.render_stateful_widget(list, area, &mut state.list_state);
    }

    fn draw_status(frame: &mut Frame, state: &ReplState, area: Rect) {
        let para = Paragraph::new(status_text(state)).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(para, area);
    }

    fn draw_permission_panel(frame: &mut Frame, state: &ReplState, area: Rect) {
        let para = Paragraph::new(permission_panel_text(state))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Approval ")
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::White));
        frame.render_widget(para, area);
    }

    fn draw_input(frame: &mut Frame, state: &ReplState, area: Rect) {
        // Show cursor indicator
        let display_text = input_display_text(&state.input);

        let para = Paragraph::new(display_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Prompt ")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(para, area);

        // Position cursor
        let content_width = area.width.saturating_sub(2) as usize;
        let (cursor_row, cursor_column) =
            input_cursor_position(&state.input, state.cursor, content_width);
        let cursor_x = area.x + 1 + cursor_column as u16;
        let cursor_y = area.y + 1 + cursor_row as u16;
        if cursor_x < area.x + area.width.saturating_sub(1)
            && cursor_y < area.y + area.height.saturating_sub(1)
        {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Wrap a `ConversationMessage` into display `ListItem`s, one per visual line.
fn message_to_lines<'a>(msg: &'a ConversationMessage, width: usize) -> Vec<ListItem<'a>> {
    let header = Line::from(vec![
        Span::styled(
            format!("[{}] ", msg.role.label()),
            Style::default()
                .fg(msg.role.color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(msg.timestamp.clone(), Style::default().fg(Color::DarkGray)),
    ]);

    // Word-wrap the body
    let body_width = width.saturating_sub(2);
    let lines = wrap_text(&msg.content, body_width);

    let mut items = vec![ListItem::new(header)];
    for line in lines {
        items.push(ListItem::new(Line::from(Span::raw(format!("  {}", line)))));
    }
    // blank separator
    items.push(ListItem::new(Line::from("")));
    items
}

/// Simple word-wrap (no hyphenation).
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        let mut current_width = 0usize;
        for word in paragraph.split_whitespace() {
            let word_width = word.width();
            if current.is_empty() {
                current.push_str(word);
                current_width = word_width;
            } else if current_width + 1 + word_width <= width {
                current.push(' ');
                current.push_str(word);
                current_width += 1 + word_width;
            } else {
                lines.push(current.clone());
                current = word.to_owned();
                current_width = word_width;
            }
        }
        if !current.is_empty() || paragraph.is_empty() {
            lines.push(current);
        }
    }
    lines
}

#[cfg(test)]
fn input_display_width_to_cursor(input: &str, cursor: usize) -> usize {
    input
        .get(..cursor)
        .unwrap_or(input)
        .chars()
        .map(|ch| ch.width().unwrap_or(0))
        .sum()
}

fn input_display_text(input: &str) -> String {
    let mut output = String::new();
    for (index, line) in input.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
            output.push_str("  ");
        } else {
            output.push_str("> ");
        }
        output.push_str(line);
    }
    output
}

fn status_text(state: &ReplState) -> String {
    let spinner = if state.is_loading {
        format!("{} ", SPINNER_FRAMES[state.spinner_tick])
    } else {
        String::new()
    };

    let key_hint = if state.permission_request_active {
        let mut hint = "[Ctrl-Y allow] [Ctrl-N deny] [Ctrl-C cancel]".to_string();
        if state.permission_request_queue_len > 0 {
            hint.push_str(&format!(
                " [queued permissions: {}]",
                state.permission_request_queue_len
            ));
        }
        hint
    } else if state.is_loading {
        "[Ctrl-C cancel]".to_string()
    } else {
        "[Shift+Enter newline] [Ctrl-C exit]".to_string()
    };

    format!(
        "{}model: {}  in: {}  out: {}  cost: ${:.4}  [/doctor] [/resume] {}",
        spinner,
        state.model_name,
        state.input_tokens,
        state.output_tokens,
        state.session_cost,
        key_hint,
    )
}

fn permission_panel_text(state: &ReplState) -> String {
    let Some(panel) = &state.permission_panel else {
        return String::new();
    };
    let mut lines = vec![
        format!("Tool: {}", panel.tool_name),
        format!("tool_use_id: {}", panel.tool_use_id),
    ];
    if let Some(reason) = panel.reason.as_deref().filter(|value| !value.is_empty()) {
        lines.push(format!("Reason: {reason}"));
    }
    if let Some(path) = panel
        .blocked_path
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Path: {path}"));
    }
    if let Some(input) = panel
        .input_preview
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Input: {input}"));
    }
    if let Some(suggestions) = panel
        .suggestions_preview
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Suggestions: {suggestions}"));
    }
    lines.push("[Ctrl-Y allow] [Ctrl-N deny] [Ctrl-C cancel]".to_string());
    if state.permission_request_queue_len > 0 {
        lines.push(format!("Queued: {}", state.permission_request_queue_len));
    }
    lines.join("\n")
}

fn permission_panel_height(state: &ReplState, area_width: u16, area_height: u16) -> u16 {
    if state.permission_panel.is_none() {
        return 0;
    }
    let content_width = area_width.saturating_sub(2) as usize;
    let content_rows = permission_panel_text(state)
        .lines()
        .map(|line| wrap_text(line, content_width).len().max(1))
        .sum::<usize>();
    let preferred = (content_rows as u16).saturating_add(2).clamp(4, 12);
    let available = area_height.saturating_sub(5).max(4);
    preferred.min(available)
}

fn input_panel_height(state: &ReplState, area_width: u16, area_height: u16) -> u16 {
    let content_width = area_width.saturating_sub(2) as usize;
    let content_rows = input_cursor_position(&state.input, state.input.len(), content_width).0 + 1;
    let preferred = (content_rows as u16).saturating_add(2).clamp(3, 8);
    let available = area_height.saturating_sub(4).max(3);
    preferred.min(available)
}

fn input_cursor_position(input: &str, cursor: usize, content_width: usize) -> (usize, usize) {
    let before_cursor = input.get(..cursor).unwrap_or(input);
    let mut row = 0usize;
    let mut column = 0usize;

    for (line_index, line) in before_cursor.split('\n').enumerate() {
        if line_index > 0 {
            row += 1;
        }
        column = if line_index == 0 {
            "> ".width()
        } else {
            "  ".width()
        };
        for ch in line.chars() {
            advance_wrapped_position(
                &mut row,
                &mut column,
                ch.width().unwrap_or(0),
                content_width,
            );
        }
    }

    (row, column)
}

fn advance_wrapped_position(
    row: &mut usize,
    column: &mut usize,
    width: usize,
    content_width: usize,
) {
    if width == 0 {
        return;
    }
    if content_width == 0 {
        *column += width;
        return;
    }
    if *column > 0 && column.saturating_add(width) > content_width {
        *row += 1;
        *column = 0;
    }
    *column += width;
    if *column >= content_width {
        *row += 1;
        *column = 0;
    }
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render_repl_to_text(state: &mut ReplState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| ReplScreen::draw(frame, state))
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
    fn cursor_display_width_handles_cjk_input() {
        assert_eq!(input_display_width_to_cursor("abc", 3), 3);
        assert_eq!(input_display_width_to_cursor("你", "你".len()), 2);
        assert_eq!(input_display_width_to_cursor("a你b", "a你".len()), 3);
    }

    #[test]
    fn wrap_text_uses_terminal_display_width() {
        assert_eq!(wrap_text("你好 世界", 5), vec!["你好", "世界"]);
        assert_eq!(wrap_text("hello world", 8), vec!["hello", "world"]);
    }

    #[test]
    fn newline_keys_insert_multiline_prompt_without_submitting() {
        let mut state = ReplState::default();
        state.input = "first".to_string();
        state.cursor = state.input.len();

        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            None
        );
        state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));

        assert_eq!(state.input, "first\ns");
        assert_eq!(state.cursor, state.input.len());
    }

    #[test]
    fn ctrl_j_inserts_newline() {
        let mut state = ReplState::default();
        state.input = "first".to_string();
        state.cursor = state.input.len();

        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));

        assert_eq!(state.input, "first\n");
        assert_eq!(state.cursor, state.input.len());
    }

    #[test]
    fn enter_submits_multiline_prompt() {
        let mut state = ReplState::default();
        state.input = "first\nsecond".to_string();
        state.cursor = state.input.len();

        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(ReplEvent::SubmitPrompt("first\nsecond".to_string()))
        );
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].content, "first\nsecond");
        assert!(state.input.is_empty());
    }

    #[test]
    fn input_cursor_position_handles_multiline_and_wide_chars() {
        assert_eq!(input_cursor_position("hi", 2, 20), (0, 4));
        assert_eq!(input_cursor_position("hi\n你", "hi\n你".len(), 20), (1, 4));
    }

    #[test]
    fn input_panel_height_expands_for_multiline_prompt() {
        let mut state = ReplState::default();
        state.input = "one\ntwo\nthree".to_string();
        state.cursor = state.input.len();

        assert_eq!(input_panel_height(&state, 80, 24), 5);
    }

    #[test]
    fn status_text_reports_queued_permission_requests() {
        let mut state = ReplState::default();
        state.is_loading = true;
        state.permission_request_active = true;
        state.permission_request_queue_len = 2;

        let status = status_text(&state);

        assert!(status.contains("[Ctrl-Y allow]"));
        assert!(status.contains("queued permissions: 2"));
    }

    #[test]
    fn permission_panel_text_renders_structured_request_context() {
        let mut state = ReplState::default();
        state.set_permission_request(
            ReplPermissionPanel {
                tool_name: "Bash".to_string(),
                tool_use_id: "toolu_bash".to_string(),
                reason: Some("Command requires approval in ask mode.".to_string()),
                blocked_path: Some("src/main.rs".to_string()),
                input_preview: Some("{\"command\":\"git status\"}".to_string()),
                suggestions_preview: Some("[\"allow once\"]".to_string()),
            },
            2,
        );

        let text = permission_panel_text(&state);

        assert!(text.contains("Tool: Bash"));
        assert!(text.contains("tool_use_id: toolu_bash"));
        assert!(text.contains("Reason: Command requires approval"));
        assert!(text.contains("Path: src/main.rs"));
        assert!(text.contains("Input: {\"command\":\"git status\"}"));
        assert!(text.contains("Suggestions: [\"allow once\"]"));
        assert!(text.contains("Queued: 2"));
        assert!(text.contains("Ctrl-Y allow"));
        assert!(permission_panel_height(&state, 80, 24) >= 4);
    }

    #[test]
    fn repl_render_keeps_approval_status_and_prompt_visible() {
        let mut state = ReplState::default();
        state.is_loading = true;
        state.model_name = "llama3.1".to_string();
        state.input = "follow up".to_string();
        state.cursor = state.input.len();
        state.push_message(MessageRole::Assistant, "Previous answer".to_string());
        state.set_permission_request(
            ReplPermissionPanel {
                tool_name: "Bash".to_string(),
                tool_use_id: "toolu_bash".to_string(),
                reason: Some("Command requires approval in ask mode.".to_string()),
                blocked_path: Some("src/main.rs".to_string()),
                input_preview: Some("{\"command\":\"git status\"}".to_string()),
                suggestions_preview: Some("[\"allow once\"]".to_string()),
            },
            1,
        );

        let rendered = render_repl_to_text(&mut state, 120, 24);

        assert!(rendered.contains("Kiana"));
        assert!(rendered.contains("Previous answer"));
        assert!(rendered.contains("Approval"));
        assert!(rendered.contains("Tool: Bash"));
        assert!(rendered.contains("Path: src/main.rs"));
        assert!(rendered.contains("Queued: 1"));
        assert!(rendered.contains("[Ctrl-Y allow]"));
        assert!(rendered.contains("Prompt"));
        assert!(rendered.contains("> follow up"));
    }

    #[test]
    fn clear_permission_request_removes_panel_state() {
        let mut state = ReplState::default();
        state.set_permission_request(
            ReplPermissionPanel {
                tool_name: "Read".to_string(),
                tool_use_id: "toolu_read".to_string(),
                reason: None,
                blocked_path: None,
                input_preview: None,
                suggestions_preview: None,
            },
            0,
        );

        state.clear_permission_request();

        assert!(!state.permission_request_active);
        assert_eq!(state.permission_request_queue_len, 0);
        assert!(state.permission_panel.is_none());
        assert_eq!(permission_panel_height(&state, 80, 24), 0);
    }
}
