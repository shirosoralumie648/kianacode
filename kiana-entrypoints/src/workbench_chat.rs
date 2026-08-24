//! Conversation surface for the folder workbench.
//!
//! Layout is Codex/pi-shaped: transcript, input, status line. The spine is
//! still `DaemonHost`. `kiana tui` stays parked on the legacy SDK stream.
//! Token streaming is not claimed: the status line shows running/idle.

use anyhow::{anyhow, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use kiana_daemon::DaemonHost;
use kiana_protocol::{ExecutionStatus, RunId};
use kiana_types::{write_project_trust, ProjectTrust};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Terminal;
use serde_json::Value;
use std::collections::HashMap;
use std::io::stdout;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::harness_run;
use crate::workbench::WORKBENCH_USAGE;

const SLASH_HELP: &str =
    "Slash: /trust  /sandbox read-only|workspace-write  /receipt  /cancel  /quit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatAction {
    None,
    Submit(String),
    Cancel,
    Quit,
    Trust,
    SetSandbox(String),
    ShowSandbox,
    Receipt,
    Help,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchView {
    pub folder: PathBuf,
    pub trusted: bool,
    pub sandbox: String,
    pub session_id: String,
    pub running: bool,
    pub input: String,
    pub messages: Vec<ChatMessage>,
}

impl WorkbenchView {
    pub fn new(folder: PathBuf, trusted: bool, sandbox: String, session_id: String) -> Self {
        let mut view = Self {
            folder,
            trusted,
            sandbox,
            session_id,
            running: false,
            input: String::new(),
            messages: Vec::new(),
        };
        view.push_system(format!(
            "Conversation surface on DaemonHost. {SLASH_HELP}. Esc/Ctrl-C cancels a running turn."
        ));
        view
    }

    pub fn status_line(&self) -> String {
        format!(
            "kiana  {}  trusted:{}  sandbox:{}  {}  session:{}",
            self.folder.display(),
            if self.trusted { "yes" } else { "no" },
            self.sandbox,
            if self.running { "running" } else { "idle" },
            short_id(&self.session_id)
        )
    }

    pub fn interpret_line(&self, raw: &str) -> ChatAction {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return ChatAction::None;
        }
        if let Some(rest) = trimmed.strip_prefix('/') {
            let (name, args) = split_slash(rest);
            return match name {
                "quit" | "exit" | "q" => ChatAction::Quit,
                "help" | "h" => ChatAction::Help,
                "trust" => ChatAction::Trust,
                "sandbox" if args.is_empty() => ChatAction::ShowSandbox,
                "sandbox" => interpret_sandbox(args),
                "receipt" => ChatAction::Receipt,
                "cancel" => ChatAction::Cancel,
                _ => ChatAction::Help,
            };
        }
        ChatAction::Submit(trimmed.to_owned())
    }

    pub fn push_system(&mut self, text: impl Into<String>) {
        self.messages.push(ChatMessage {
            role: ChatRole::System,
            text: text.into(),
        });
    }

    pub fn push_user(&mut self, text: impl Into<String>) {
        self.messages.push(ChatMessage {
            role: ChatRole::User,
            text: text.into(),
        });
    }

    pub fn apply_response(&mut self, response: &kiana_protocol::ResponseEnvelope) {
        if response.status != ExecutionStatus::Completed {
            self.push_system(format!(
                "blocked: {}",
                response.error.as_deref().unwrap_or("kiana_harness_failed")
            ));
            self.running = false;
            return;
        }
        if let Some(text) = response.output["output"]["text"].as_str() {
            if !text.trim().is_empty() {
                self.messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    text: text.to_owned(),
                });
            }
        }
        if let Some(files) = response.output["files_changed"].as_array() {
            let changed: Vec<String> = files
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            if !changed.is_empty() {
                self.messages.push(ChatMessage {
                    role: ChatRole::Changed,
                    text: changed.join(", "),
                });
            }
        }
        self.running = false;
    }
}

fn interpret_sandbox(args: &str) -> ChatAction {
    match normalize_sandbox(args) {
        Ok(sandbox) => ChatAction::SetSandbox(sandbox),
        Err(error) => ChatAction::Error(error.to_string()),
    }
}

pub fn normalize_sandbox(value: &str) -> Result<String> {
    match value.trim().replace('_', "-").to_ascii_lowercase().as_str() {
        "read-only" | "readonly" => Ok("read-only".to_owned()),
        "workspace-write" | "workspacewrite" | "workspace" => Ok("workspace-write".to_owned()),
        "danger-full-access" | "dangerfullaccess" | "full" => {
            Err(anyhow!("danger_full_access_rejected"))
        }
        other => Err(anyhow!("sandbox_unsupported:{other}")),
    }
}

fn split_slash(rest: &str) -> (&str, &str) {
    match rest.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args.trim()),
        None => (rest, ""),
    }
}

fn short_id(value: &str) -> &str {
    if value.len() > 8 {
        &value[..8]
    } else {
        value
    }
}

pub fn action_from_key(key: KeyEvent, running: bool) -> Option<KeyCommand> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') if running => Some(KeyCommand::Cancel),
            KeyCode::Char('c') | KeyCode::Char('d') => Some(KeyCommand::Quit),
            KeyCode::Char('j') => Some(KeyCommand::Newline),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc if running => Some(KeyCommand::Cancel),
        KeyCode::Esc => None,
        KeyCode::Enter => Some(KeyCommand::Submit),
        KeyCode::Backspace => Some(KeyCommand::Backspace),
        KeyCode::Char(ch) => Some(KeyCommand::Insert(ch)),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCommand {
    Insert(char),
    Backspace,
    Newline,
    Submit,
    Cancel,
    Quit,
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
    }
}

pub async fn run(
    host: Arc<DaemonHost>,
    session_id: String,
    workdir: PathBuf,
    sandbox: String,
    mut options: HashMap<String, Value>,
    initial_prompt: Option<String>,
) -> Result<()> {
    let trusted = harness_run::project_trusted(&workdir.to_string_lossy())?;
    let mut view = WorkbenchView::new(workdir.clone(), trusted, sandbox, session_id.clone());
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut events = EventStream::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<Result<kiana_protocol::ResponseEnvelope>>();
    let mut started = false;
    let mut last_run_id: Option<RunId> = None;

    if let Some(prompt) = initial_prompt {
        submit_turn(
            &mut view,
            &host,
            &session_id,
            &options,
            started,
            last_run_id.clone(),
            prompt,
            &tx,
        );
        started = true;
    }

    loop {
        terminal.draw(|frame| draw(frame, &view))?;
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Some(Ok(response)) => {
                        last_run_id = run_id_from(&response).or(last_run_id);
                        started = true;
                        view.apply_response(&response);
                    }
                    Some(Err(error)) => {
                        view.running = false;
                        view.push_system(error.to_string());
                    }
                    None => view.running = false,
                }
            }
            event = events.next() => {
                let Some(Ok(Event::Key(key))) = event else { continue };
                match action_from_key(key, view.running) {
                    Some(KeyCommand::Quit) => break,
                    Some(KeyCommand::Cancel) => {
                        spawn_cancel(&host, &session_id, last_run_id.clone(), &options, &tx);
                    }
                    Some(KeyCommand::Submit) => {
                        let action = view.interpret_line(&view.input.clone());
                        view.input.clear();
                        match handle_action(
                            action,
                            &mut view,
                            &host,
                            &session_id,
                            &mut options,
                            started,
                            last_run_id.clone(),
                            &tx,
                        )
                        .await?
                        {
                            LoopControl::Quit => break,
                            LoopControl::Submitted => started = true,
                            LoopControl::Continue => {}
                        }
                    }
                    Some(KeyCommand::Backspace) => {
                        view.input.pop();
                    }
                    Some(KeyCommand::Newline) => view.input.push('\n'),
                    Some(KeyCommand::Insert(ch)) => view.input.push(ch),
                    None => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(80)) => {}
        }
    }
    Ok(())
}

enum LoopControl {
    Continue,
    Submitted,
    Quit,
}

async fn handle_action(
    action: ChatAction,
    view: &mut WorkbenchView,
    host: &Arc<DaemonHost>,
    session_id: &str,
    options: &mut HashMap<String, Value>,
    started: bool,
    last_run_id: Option<RunId>,
    tx: &mpsc::UnboundedSender<Result<kiana_protocol::ResponseEnvelope>>,
) -> Result<LoopControl> {
    match action {
        ChatAction::None => Ok(LoopControl::Continue),
        ChatAction::Quit => Ok(LoopControl::Quit),
        ChatAction::Help => {
            view.push_system(format!("{WORKBENCH_USAGE}\n{SLASH_HELP}"));
            Ok(LoopControl::Continue)
        }
        ChatAction::Error(error) => {
            view.push_system(error);
            Ok(LoopControl::Continue)
        }
        ChatAction::ShowSandbox => {
            view.push_system(format!("sandbox: {}", view.sandbox));
            Ok(LoopControl::Continue)
        }
        ChatAction::Trust => {
            write_project_trust(&view.folder, ProjectTrust::Trusted).map_err(anyhow::Error::msg)?;
            view.trusted = true;
            view.push_system(format!("trusted {}", view.folder.display()));
            Ok(LoopControl::Continue)
        }
        ChatAction::SetSandbox(sandbox) => {
            view.sandbox = sandbox.clone();
            options.insert("sandbox".to_string(), Value::String(sandbox.clone()));
            view.push_system(format!("sandbox: {sandbox}"));
            Ok(LoopControl::Continue)
        }
        ChatAction::Receipt => {
            let response = harness_run::receipt_envelope_on_host(
                Arc::clone(host),
                session_id.to_owned(),
                last_run_id,
                options,
            )
            .await?;
            view.apply_response(&response);
            Ok(LoopControl::Continue)
        }
        ChatAction::Cancel => {
            if !view.running {
                view.push_system("nothing to cancel");
                return Ok(LoopControl::Continue);
            }
            spawn_cancel(host, session_id, last_run_id, options, tx);
            Ok(LoopControl::Continue)
        }
        ChatAction::Submit(prompt) => {
            if view.running {
                view.push_system("turn is running; Esc or /cancel first");
                return Ok(LoopControl::Continue);
            }
            submit_turn(
                view,
                host,
                session_id,
                options,
                started,
                last_run_id,
                prompt,
                tx,
            );
            Ok(LoopControl::Submitted)
        }
    }
}

fn spawn_cancel(
    host: &Arc<DaemonHost>,
    session_id: &str,
    last_run_id: Option<RunId>,
    options: &HashMap<String, Value>,
    tx: &mpsc::UnboundedSender<Result<kiana_protocol::ResponseEnvelope>>,
) {
    let host = Arc::clone(host);
    let session_id = session_id.to_owned();
    let options = options.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let result =
            harness_run::cancel_envelope_on_host(host, session_id, last_run_id, "user", &options)
                .await;
        let _ = tx.send(result);
    });
}

fn submit_turn(
    view: &mut WorkbenchView,
    host: &Arc<DaemonHost>,
    session_id: &str,
    options: &HashMap<String, Value>,
    started: bool,
    last_run_id: Option<RunId>,
    prompt: String,
    tx: &mpsc::UnboundedSender<Result<kiana_protocol::ResponseEnvelope>>,
) {
    view.push_user(prompt.clone());
    view.running = true;
    let host = Arc::clone(host);
    let session_id = session_id.to_owned();
    let options = options.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = if started {
            harness_run::continue_envelope_on_host(host, session_id, prompt, last_run_id, &options)
                .await
        } else {
            harness_run::run_envelope_on_host(host, session_id, prompt, &options).await
        };
        let _ = tx.send(result);
    });
}

fn run_id_from(response: &kiana_protocol::ResponseEnvelope) -> Option<RunId> {
    response
        .output
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(RunId::parse_str)
}

fn visible_messages(messages: &[ChatMessage], height: usize) -> &[ChatMessage] {
    let height = height.max(1);
    if messages.len() <= height {
        messages
    } else {
        &messages[messages.len() - height..]
    }
}

fn draw(frame: &mut ratatui::Frame, view: &WorkbenchView) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let list_height = chunks[0].height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = visible_messages(&view.messages, list_height)
        .iter()
        .map(|message| {
            let (label, color) = match message.role {
                ChatRole::User => ("You", Color::Cyan),
                ChatRole::Assistant => ("Kiana", Color::Green),
                ChatRole::System => ("system", Color::DarkGray),
                ChatRole::Changed => ("changed", Color::Yellow),
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{label}: "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(message.text.clone()),
            ]))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("conversation")),
        chunks[0],
    );

    let input = Paragraph::new(view.input.as_str())
        .block(Block::default().borders(Borders::ALL).title("input"));
    frame.render_widget(input, chunks[1]);

    let status = Paragraph::new(view.status_line()).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_protocol::{ExecutionStatus, RequestId, ResponseEnvelope, PROTOCOL_SCHEMA};

    fn completed_response(output: Value) -> ResponseEnvelope {
        ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: RequestId::new(),
            status: ExecutionStatus::Completed,
            output,
            error: None,
        }
    }

    #[test]
    fn status_line_names_folder_trust_and_sandbox() {
        let mut view = WorkbenchView::new(
            PathBuf::from("/tmp/demo"),
            true,
            "workspace-write".to_owned(),
            "abcdef12-session".to_owned(),
        );
        let status = view.status_line();
        assert!(status.contains("/tmp/demo"), "{status}");
        assert!(status.contains("trusted:yes"), "{status}");
        assert!(status.contains("sandbox:workspace-write"), "{status}");
        assert!(status.contains("idle"), "{status}");
        view.running = true;
        let running = view.status_line();
        assert!(running.contains("running"), "{running}");
    }

    #[test]
    fn sandbox_slash_actually_switches() {
        let view = WorkbenchView::new(
            PathBuf::from("/tmp/demo"),
            true,
            "workspace-write".to_owned(),
            "s".to_owned(),
        );
        assert_eq!(
            view.interpret_line("/sandbox read-only"),
            ChatAction::SetSandbox("read-only".to_owned())
        );
        assert_eq!(
            view.interpret_line("/sandbox workspace-write"),
            ChatAction::SetSandbox("workspace-write".to_owned())
        );
        assert_eq!(view.interpret_line("/sandbox"), ChatAction::ShowSandbox);
        assert_eq!(
            view.interpret_line("/sandbox danger-full-access"),
            ChatAction::Error("danger_full_access_rejected".to_owned())
        );
        assert_eq!(view.interpret_line("/quit"), ChatAction::Quit);
        assert_eq!(
            view.interpret_line("write GOLDEN_PATH.txt"),
            ChatAction::Submit("write GOLDEN_PATH.txt".to_owned())
        );
    }

    #[test]
    fn esc_cancels_only_while_running() {
        let running = KeyEvent::from(KeyCode::Esc);
        assert_eq!(action_from_key(running, true), Some(KeyCommand::Cancel));
        assert_eq!(action_from_key(running, false), None);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(action_from_key(ctrl_c, true), Some(KeyCommand::Cancel));
        assert_eq!(action_from_key(ctrl_c, false), Some(KeyCommand::Quit));
    }

    #[test]
    fn apply_response_appends_assistant_and_files() {
        let mut view = WorkbenchView::new(
            PathBuf::from("/tmp/demo"),
            true,
            "workspace-write".to_owned(),
            "s".to_owned(),
        );
        view.running = true;
        let response = completed_response(serde_json::json!({
            "output": {"text": "created GOLDEN_PATH.txt"},
            "files_changed": ["GOLDEN_PATH.txt"]
        }));
        view.apply_response(&response);
        assert!(!view.running);
        assert!(view
            .messages
            .iter()
            .any(|message| message.role == ChatRole::Assistant
                && message.text.contains("created GOLDEN_PATH.txt")));
        assert!(view
            .messages
            .iter()
            .any(|message| message.role == ChatRole::Changed
                && message.text.contains("GOLDEN_PATH.txt")));
    }

    #[test]
    fn danger_sandbox_is_rejected() {
        let error = normalize_sandbox("danger-full-access")
            .unwrap_err()
            .to_string();
        assert_eq!(error, "danger_full_access_rejected");
        assert_eq!(
            normalize_sandbox("read_only").unwrap(),
            "read-only".to_owned()
        );
    }

    #[test]
    fn visible_messages_keep_the_tail() {
        let messages: Vec<ChatMessage> = (0..5)
            .map(|index| ChatMessage {
                role: ChatRole::System,
                text: index.to_string(),
            })
            .collect();
        let visible = visible_messages(&messages, 2);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].text, "3");
        assert_eq!(visible[1].text, "4");
    }
}
