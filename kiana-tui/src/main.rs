mod acp;
mod commands;
mod completion;
mod markdown;
mod sessions;
mod config;

use acp::{AcpClient, AcpMessage};
use anyhow::Result;
use config::Config;
use completion::CompletionEngine;
use kiana_tui::overlay::{Overlay, OverlayAction};
use markdown::render_markdown;
use sessions::SessionManager;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Terminal,
};
use std::io;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use std::time::{Duration, Instant};
use std::collections::VecDeque;

// Performance configuration
const MAX_MESSAGES: usize = 1000; // FIFO limit for memory management
const DEBOUNCE_MS: u64 = 100; // Debounce for high-frequency events

// Security configuration
const MAX_INPUT_LENGTH: usize = 10000; // Maximum input length in characters

#[derive(Clone, Debug)]
struct Message {
    role: String,
    content: String,
}

impl Message {
    fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

struct App {
    input_buffer: String,
    cursor_pos: usize,
    messages: VecDeque<Message>, // Changed to VecDeque for efficient FIFO
    scroll_offset: usize,
    session_id: Option<String>,
    acp_client: Option<AcpClient>,
    status_message: Option<String>,
    last_render: Instant,
    needs_render: bool,
    pending_events: Vec<(String, serde_json::Value)>, // Batched events
    reconnect_attempts: u32,
    show_retry_button: bool,
    completion_engine: CompletionEngine,
    completions: Vec<String>,
    completion_selected: usize,
    config: Config,
    config_mode: bool, // Whether we're in config editing mode
    config_field: usize, // Which field is selected in config editor
    config_temp_buffer: String, // Temporary buffer for editing config values
    session_manager: SessionManager,
    show_session_list: bool,
    session_list_selected: usize,
    // Overlay management
    /// Currently active overlay (e.g., search history)
    active_overlay: Option<Box<dyn Overlay>>,
    /// Result returned from overlay (waiting to be processed)
    overlay_result: Option<String>,
}

impl App {
    fn new() -> Self {
        let config = Config::load().unwrap_or_else(|e| {
            eprintln!("Warning: Failed to load config: {}. Using defaults.", e);
            Config::default()
        });

        let session_manager = SessionManager::new().unwrap_or_else(|e| {
            eprintln!("Warning: Failed to initialize session manager: {}. Using minimal state.", e);
            // This shouldn't fail in practice, but we handle it gracefully
            SessionManager::new().unwrap()
        });

        Self {
            input_buffer: String::new(),
            cursor_pos: 0,
            messages: VecDeque::new(),
            scroll_offset: 0,
            session_id: None,
            acp_client: None,
            status_message: None,
            last_render: Instant::now(),
            needs_render: true,
            pending_events: Vec::new(),
            reconnect_attempts: 0,
            show_retry_button: false,
            completion_engine: CompletionEngine::new(),
            completions: Vec::new(),
            completion_selected: 0,
            config,
            config_mode: false,
            config_field: 0,
            config_temp_buffer: String::new(),
            session_manager,
            show_session_list: false,
            session_list_selected: 0,
            active_overlay: None,
            overlay_result: None,
        }
    }

    fn add_message(&mut self, message: Message) {
        self.messages.push_back(message);

        // FIFO eviction: keep only last MAX_MESSAGES
        while self.messages.len() > MAX_MESSAGES {
            self.messages.pop_front();
        }

        self.needs_render = true;
    }

    /// Check if there is an active overlay
    fn has_active_overlay(&self) -> bool {
        self.active_overlay.is_some()
    }

    /// Take and consume the overlay result
    fn take_overlay_result(&mut self) -> Option<String> {
        self.overlay_result.take()
    }

    /// Close the current overlay
    fn close_overlay(&mut self) {
        self.active_overlay = None;
    }

    fn init_acp(&mut self) -> Result<()> {
        let start = Instant::now();
        tracing::info!("Attempting to spawn ACP server...");

        let mut client = match AcpClient::new() {
            Ok(client) => client,
            Err(e) => {
                tracing::error!("Failed to spawn ACP server: {}", e);
                self.show_retry_button = true;
                return Err(e);
            }
        };

        let spawn_time = start.elapsed();
        tracing::info!("ACP server spawned in {:?}", spawn_time);

        let init_start = Instant::now();
        let project_root = std::env::current_dir()?
            .to_str()
            .unwrap_or(".")
            .to_string();

        let session_id = match client.initialize_session(&project_root) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to initialize session: {}", e);
                self.show_retry_button = true;
                return Err(e);
            }
        };

        let init_time = init_start.elapsed();
        tracing::info!("Session initialized in {:?}", init_time);

        self.add_message(Message::new(
            "system",
            format!("Session initialized: {} (spawn: {:?}, init: {:?})",
                session_id, spawn_time, init_time),
        ));

        self.session_id = Some(session_id);
        self.acp_client = Some(client);
        self.show_retry_button = false;
        self.reconnect_attempts = 0;

        Ok(())
    }

    fn retry_connection(&mut self) {
        self.reconnect_attempts += 1;
        if self.reconnect_attempts > 3 {
            self.add_message(Message::new(
                "error",
                "Max reconnection attempts (3) reached. Please restart the application.",
            ));
            self.show_retry_button = false;
            return;
        }

        self.add_message(Message::new(
            "system",
            format!("Reconnecting... (attempt {}/3)", self.reconnect_attempts),
        ));

        if let Err(e) = self.init_acp() {
            self.add_message(Message::new(
                "error",
                format!("Reconnection failed: {}. Press 'r' to retry.", e),
            ));
        }
    }

    fn check_connection(&mut self) {
        if let Some(client) = &mut self.acp_client {
            if let Err(e) = client.check_connection() {
                tracing::warn!("Connection check failed: {}", e);
                self.add_message(Message::new(
                    "error",
                    format!("Connection lost: {}. Press 'r' to reconnect.", e),
                ));
                self.acp_client = None;
                self.session_id = None;
                self.show_retry_button = true;
            }
        }
    }

    fn handle_input(&mut self) {
        if self.input_buffer.is_empty() {
            return;
        }

        // Security: Validate input length
        if self.input_buffer.len() > MAX_INPUT_LENGTH {
            self.add_message(Message::new(
                "error",
                format!("Input exceeds maximum length of {} characters", MAX_INPUT_LENGTH),
            ));
            self.input_buffer.clear();
            return;
        }

        let input = self.input_buffer.clone();

        // Check if this is a slash command
        let (command_name, arguments) = if input.trim().starts_with('/') {
            let parts: Vec<&str> = input.trim()[1..].splitn(2, ' ').collect();
            let cmd_name = parts[0];
            let args = if parts.len() > 1 { parts[1] } else { "" };
            (Some(cmd_name), args)
        } else {
            (None, input.as_str())
        };

        // Handle special commands locally
        if let Some(cmd) = command_name {
            match cmd {
                "clear" => {
                    self.messages.clear();
                    self.scroll_offset = 0;
                    self.input_buffer.clear();
                    self.add_message(Message::new("system", "Conversation cleared."));
                    return;
                }
                "help" => {
                    let commands = commands::register_commands();
                    let mut help_text = String::from("Available commands:\n\n");
                    for cmd in commands {
                        help_text.push_str(&format!("/{} - {}\n", cmd.name, cmd.description));
                    }
                    self.add_message(Message::new("user", input.clone()));
                    self.add_message(Message::new("assistant", help_text));
                    self.input_buffer.clear();
                    self.scroll_to_bottom();
                    return;
                }
                "config" => {
                    // Enter config mode
                    self.config_mode = true;
                    self.config_field = 0;
                    self.config_temp_buffer.clear();
                    self.input_buffer.clear();
                    self.needs_render = true;
                    return;
                }
                "sessions" => {
                    self.show_session_list = true;
                    self.session_list_selected = 0;
                    self.input_buffer.clear();
                    self.needs_render = true;
                    return;
                }
                "new" => {
                    match self.session_manager.new_session() {
                        Ok(new_id) => {
                            self.messages.clear();
                            self.add_message(Message::new("system", format!("New session created: {}", new_id)));
                            self.acp_client = None;
                            self.session_id = None;
                        }
                        Err(e) => {
                            self.add_message(Message::new("error", format!("Failed to create new session: {}", e)));
                        }
                    }
                    self.input_buffer.clear();
                    return;
                }
                "fork" => {
                    match self.session_manager.fork_session() {
                        Ok(new_id) => {
                            self.add_message(Message::new("system", format!("Session forked: {}", new_id)));
                            self.acp_client = None;
                            self.session_id = None;
                        }
                        Err(e) => {
                            self.add_message(Message::new("error", format!("Failed to fork session: {}", e)));
                        }
                    }
                    self.input_buffer.clear();
                    return;
                }
                "switch" => {
                    let session_id = arguments.trim();
                    if session_id.is_empty() {
                        self.add_message(Message::new("error", "Usage: /switch <session_id>"));
                    } else {
                        match self.session_manager.switch_session(session_id) {
                            Ok(()) => {
                                self.messages.clear();
                                self.add_message(Message::new("system", format!("Switched to session: {}", session_id)));
                                self.acp_client = None;
                                self.session_id = None;
                            }
                            Err(e) => {
                                self.add_message(Message::new("error", format!("Failed to switch session: {}", e)));
                            }
                        }
                    }
                    self.input_buffer.clear();
                    return;
                }
                _ => {}
            }
        }

        // Add user message
        self.add_message(Message::new("user", input.clone()));
        self.scroll_to_bottom();

        // Show status message
        self.status_message = Some("Executing command...".to_string());
        self.needs_render = true;

        // Determine ACP command and arguments
        let (acp_command, acp_arguments) = if let Some(cmd_name) = command_name {
            if let Some(slash_cmd) = commands::find_command(cmd_name) {
                (slash_cmd.acp_command.to_string(), slash_cmd.build_arguments(arguments))
            } else {
                // Unknown command, treat as regular query
                ("system.architecture".to_string(), serde_json::json!({"query": input}))
            }
        } else {
            // Regular input, use default command
            ("system.architecture".to_string(), serde_json::json!({"query": input}))
        };

        // Try to execute command via ACP
        if let (Some(client), Some(session_id)) = (&mut self.acp_client, &self.session_id) {
            let execute_start = Instant::now();
            match client.execute_command(
                session_id,
                &acp_command,
                acp_arguments,
            ) {
                Ok(result) => {
                    let execute_time = execute_start.elapsed();
                    self.status_message = None;

                    // Extract content from response
                    let mut content = if let Some(content) = result.get("content") {
                        content.as_str().unwrap_or("No content in response").to_string()
                    } else {
                        format!("{}", result)
                    };

                    // Append timing info
                    content.push_str(&format!("\n\n_Execution time: {:?}_", execute_time));

                    self.add_message(Message::new("assistant", content));
                }
                Err(e) => {
                    self.status_message = None;
                    let error_msg = e.to_string();
                    tracing::error!("Command execution failed: {}", error_msg);

                    if error_msg.contains("timeout") {
                        self.add_message(Message::new("error", "Request timeout after 30 seconds. Please retry."));
                    } else if error_msg.contains("connection closed") || error_msg.contains("connection lost") {
                        self.add_message(Message::new("error", "Connection lost. Press 'r' to reconnect."));
                        self.acp_client = None;
                        self.session_id = None;
                        self.show_retry_button = true;
                    } else {
                        self.add_message(Message::new("error", format!("Command execution failed: {}", error_msg)));
                    }
                }
            }
        } else {
            self.status_message = None;
            self.add_message(Message::new("error", "ACP not initialized. Connection to server failed."));
        }

        self.scroll_to_bottom();
        self.input_buffer.clear();
        self.cursor_pos = 0;
        self.completions.clear();
    }

    fn trigger_completion(&mut self) {
        self.completions = self.completion_engine.get_completions(&self.input_buffer, self.cursor_pos);
        self.completion_selected = 0;

        if self.completions.len() == 1 {
            // Single match: auto-complete
            let completion = self.completions[0].clone();
            let (new_input, new_cursor) = CompletionEngine::apply_completion(
                &self.input_buffer,
                self.cursor_pos,
                &completion,
            );
            self.input_buffer = new_input;
            self.cursor_pos = new_cursor;
            self.completions.clear();
        }

        self.needs_render = true;
    }

    fn apply_selected_completion(&mut self) {
        if self.completions.is_empty() {
            return;
        }

        let completion = self.completions[self.completion_selected].clone();
        let (new_input, new_cursor) = CompletionEngine::apply_completion(
            &self.input_buffer,
            self.cursor_pos,
            &completion,
        );
        self.input_buffer = new_input;
        self.cursor_pos = new_cursor;
        self.completions.clear();
        self.needs_render = true;
    }

    fn completion_next(&mut self) {
        if !self.completions.is_empty() {
            self.completion_selected = (self.completion_selected + 1) % self.completions.len();
            self.needs_render = true;
        }
    }

    fn completion_prev(&mut self) {
        if !self.completions.is_empty() {
            if self.completion_selected == 0 {
                self.completion_selected = self.completions.len() - 1;
            } else {
                self.completion_selected -= 1;
            }
            self.needs_render = true;
        }
    }

    fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            self.needs_render = true;
        }
    }

    fn scroll_down(&mut self) {
        self.scroll_offset += 1;
        self.needs_render = true;
    }

    fn scroll_page_up(&mut self, page_size: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
        self.needs_render = true;
    }

    fn scroll_page_down(&mut self, page_size: usize) {
        self.scroll_offset += page_size;
        self.needs_render = true;
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.messages.len();
        self.needs_render = true;
    }

    fn get_visible_messages(&self, height: usize) -> Vec<&Message> {
        let total = self.messages.len();
        if total == 0 {
            return Vec::new();
        }

        // Calculate the effective scroll position
        let max_scroll = total.saturating_sub(1);
        let effective_scroll = self.scroll_offset.min(max_scroll);

        // Calculate start position (showing messages up to scroll position)
        let start = effective_scroll.saturating_sub(height.saturating_sub(1));
        let end = (effective_scroll + 1).min(total);

        self.messages.range(start..end).collect()
    }

    fn handle_notification(&mut self, method: String, params: serde_json::Value) {
        // Batch events for debouncing
        self.pending_events.push((method, params));
    }

    fn flush_pending_events(&mut self) {
        if self.pending_events.is_empty() {
            return;
        }

        // Process batched events
        for (method, params) in self.pending_events.drain(..) {
            if method == "runtime_event" {
                if let Some(event_type) = params.get("event_type").and_then(|v| v.as_str()) {
                    let message = match event_type {
                        "tool_use" => {
                            let tool_name = params.get("tool_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            format!("Using tool: {}", tool_name)
                        }
                        "file_read" => {
                            let file_path = params.get("file_path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            format!("Reading file: {}", file_path)
                        }
                        "thinking" => "Thinking...".to_string(),
                        _ => format!("Event: {}", event_type),
                    };
                    self.status_message = Some(message);
                }
            }
        }

        self.needs_render = true;
    }

    fn should_render(&self) -> bool {
        if !self.needs_render {
            return false;
        }

        // Debounce: only render if enough time has passed
        self.last_render.elapsed() >= Duration::from_millis(DEBOUNCE_MS)
    }

    fn mark_rendered(&mut self) {
        self.needs_render = false;
        self.last_render = Instant::now();
    }

    fn handle_config_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                // Exit config mode without saving
                self.config_mode = false;
                self.config_temp_buffer.clear();
                self.needs_render = true;
            }
            KeyCode::Up => {
                // Move to previous field
                if self.config_field > 0 {
                    self.config_field -= 1;
                    self.config_temp_buffer.clear();
                    self.needs_render = true;
                }
            }
            KeyCode::Down => {
                // Move to next field
                if self.config_field < 4 {  // 5 fields total (0-4)
                    self.config_field += 1;
                    self.config_temp_buffer.clear();
                    self.needs_render = true;
                }
            }
            KeyCode::Enter => {
                // Save config
                if self.save_config() {
                    self.config_mode = false;
                    self.config_temp_buffer.clear();
                    self.add_message(Message::new("system", "Configuration saved successfully."));
                }
                self.needs_render = true;
            }
            KeyCode::Char(c) => {
                // Edit current field
                self.config_temp_buffer.push(c);
                self.update_config_field();
                self.needs_render = true;
            }
            KeyCode::Backspace => {
                self.config_temp_buffer.pop();
                self.update_config_field();
                self.needs_render = true;
            }
            _ => {}
        }
    }

    fn update_config_field(&mut self) {
        if self.config_temp_buffer.is_empty() {
            return;
        }

        match self.config_field {
            0 => {
                // Provider name
                self.config.provider.name = self.config_temp_buffer.clone();
            }
            1 => {
                // Model name
                self.config.provider.model = self.config_temp_buffer.clone();
            }
            2 => {
                // Temperature
                if let Ok(temp) = self.config_temp_buffer.parse::<f32>() {
                    self.config.parameters.temperature = temp;
                }
            }
            3 => {
                // Max tokens
                if let Ok(tokens) = self.config_temp_buffer.parse::<u32>() {
                    self.config.parameters.max_tokens = tokens;
                }
            }
            _ => {}
        }
    }

    fn save_config(&mut self) -> bool {
        // Validate before saving
        if let Err(e) = self.config.validate() {
            self.add_message(Message::new("error", format!("Invalid configuration: {}", e)));
            return false;
        }

        // Save to file
        if let Err(e) = self.config.save() {
            self.add_message(Message::new("error", format!("Failed to save config: {}", e)));
            return false;
        }

        true
    }

    fn render_config_editor(&self, f: &mut ratatui::Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),   // Title
                Constraint::Length(3),   // Provider
                Constraint::Length(3),   // Model
                Constraint::Length(3),   // Temperature
                Constraint::Length(3),   // Max tokens
                Constraint::Length(3),   // Buttons
                Constraint::Min(1),      // Remaining space
            ])
            .split(area);

        // Clear background
        f.render_widget(Clear, area);

        // Title
        let title = Paragraph::new("Configuration Editor")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        // Provider field
        let provider_style = if self.config_field == 0 {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let provider_text = if self.config_field == 0 && !self.config_temp_buffer.is_empty() {
            &self.config_temp_buffer
        } else {
            &self.config.provider.name
        };
        let provider = Paragraph::new(format!("Provider: {}", provider_text))
            .style(provider_style)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(provider, chunks[1]);

        // Model field
        let model_style = if self.config_field == 1 {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let model_text = if self.config_field == 1 && !self.config_temp_buffer.is_empty() {
            &self.config_temp_buffer
        } else {
            &self.config.provider.model
        };
        let model = Paragraph::new(format!("Model: {}", model_text))
            .style(model_style)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(model, chunks[2]);

        // Temperature field
        let temp_style = if self.config_field == 2 {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let temp_text = if self.config_field == 2 && !self.config_temp_buffer.is_empty() {
            self.config_temp_buffer.clone()
        } else {
            format!("{}", self.config.parameters.temperature)
        };
        let temperature = Paragraph::new(format!("Temperature: {}", temp_text))
            .style(temp_style)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(temperature, chunks[3]);

        // Max tokens field
        let tokens_style = if self.config_field == 3 {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let tokens_text = if self.config_field == 3 && !self.config_temp_buffer.is_empty() {
            self.config_temp_buffer.clone()
        } else {
            format!("{}", self.config.parameters.max_tokens)
        };
        let max_tokens = Paragraph::new(format!("Max Tokens: {}", tokens_text))
            .style(tokens_style)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(max_tokens, chunks[4]);

        // Buttons
        let button_style = if self.config_field == 4 {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        let buttons = Paragraph::new("[Enter] Save  [Esc] Cancel  [Up/Down] Navigate")
            .style(button_style)
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);
        f.render_widget(buttons, chunks[5]);
    }
}

fn run_app() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and render initial screen
    let mut app = App::new();

    // Render first screen immediately (startup optimization)
    terminal.draw(|f| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(f.area());

        let title = Paragraph::new("Kiana Chat - Initializing...")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);
    })?;

    // Initialize ACP after first render (delayed spawn)
    if let Err(e) = app.init_acp() {
        app.add_message(Message::new("error", format!("Failed to init ACP: {}", e)));
    }

    // Main loop
    loop {
        // Only render if needed and debounce period elapsed
        if app.should_render() {
            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),      // Title bar
                        Constraint::Min(1),         // Conversation area
                        Constraint::Length(3),      // Input box
                    ])
                    .split(f.area());

                // Title bar
                let title = Paragraph::new("Kiana Chat")
                    .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .block(Block::default().borders(Borders::ALL));
                f.render_widget(title, chunks[0]);

                // Conversation area - render messages with markdown support
                let conversation_height = chunks[1].height.saturating_sub(2) as usize;
                let visible_messages = app.get_visible_messages(conversation_height);

                let mut message_lines: Vec<Line> = Vec::new();

                for msg in visible_messages {
                    let (prefix, style) = match msg.role.as_str() {
                        "user" => ("You: ", Style::default().fg(Color::Green)),
                        "assistant" => ("Assistant: ", Style::default().fg(Color::Blue)),
                        "system" => ("System: ", Style::default().fg(Color::Cyan)),
                        "error" => ("Error: ", Style::default().fg(Color::Red)),
                        _ => ("", Style::default()),
                    };

                    // Add role prefix
                    message_lines.push(Line::from(vec![
                        Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                    ]));

                    // Render content with markdown for assistant messages
                    if msg.role == "assistant" {
                        let rendered = render_markdown(&msg.content);
                        message_lines.extend(rendered);
                    } else {
                        // For other roles, render as plain text
                        for line in msg.content.lines() {
                            message_lines.push(Line::from(line.to_string()));
                        }
                    }

                    // Add spacing between messages
                    message_lines.push(Line::from(""));
                }

                // Add status message if present
                if let Some(status) = &app.status_message {
                    message_lines.push(Line::from(vec![
                        Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
                        Span::styled(status, Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC)),
                    ]));
                }

                let scroll_info = if app.messages.len() > conversation_height {
                    format!(" [{}/{}]", app.scroll_offset.min(app.messages.len()), app.messages.len())
                } else {
                    String::new()
                };

                let conversation = Paragraph::new(message_lines)
                    .block(Block::default()
                        .borders(Borders::ALL)
                        .title(format!("Messages{}", scroll_info)));
                f.render_widget(conversation, chunks[1]);

                // Input box
                let input_text = vec![Line::from(vec![
                    Span::raw("> "),
                    Span::styled(&app.input_buffer, Style::default().fg(Color::Yellow)),
                    Span::styled("█", Style::default().fg(Color::Gray)),
                ])];

                let input = Paragraph::new(input_text)
                    .block(Block::default().borders(Borders::ALL).title("Input (Ctrl-C to exit, Tab to complete)"));
                f.render_widget(input, chunks[2]);

                // Completion popup
                if app.completions.len() > 1 {
                    let completion_items: Vec<ListItem> = app.completions
                        .iter()
                        .enumerate()
                        .map(|(idx, item)| {
                            let style = if idx == app.completion_selected {
                                Style::default().bg(Color::Blue).fg(Color::White)
                            } else {
                                Style::default()
                            };
                            ListItem::new(item.clone()).style(style)
                        })
                        .collect();

                    let list = List::new(completion_items)
                        .block(Block::default().borders(Borders::ALL).title("Completions (Tab/Shift+Tab to navigate, Enter to select)"));

                    // Position popup above input box
                    let popup_height = (app.completions.len() + 2).min(10) as u16;
                    let popup_area = Rect {
                        x: chunks[2].x + 2,
                        y: chunks[2].y.saturating_sub(popup_height),
                        width: chunks[2].width.saturating_sub(4).min(60),
                        height: popup_height,
                    };

                    f.render_widget(Clear, popup_area);
                    f.render_widget(list, popup_area);
                }
            })?;

            app.mark_rendered();
        }

        // Handle input events
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key {
                    KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    } => break,
                    KeyEvent {
                        code: KeyCode::Enter,
                        ..
                    } => {
                        if !app.completions.is_empty() {
                            app.apply_selected_completion();
                        } else {
                            app.handle_input();
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Tab,
                        modifiers: KeyModifiers::SHIFT,
                        ..
                    } => {
                        if !app.completions.is_empty() {
                            app.completion_prev();
                        }
                    }
                    KeyEvent {
                        code: KeyCode::BackTab,
                        ..
                    } => {
                        if !app.completions.is_empty() {
                            app.completion_prev();
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Tab,
                        ..
                    } => {
                        if !app.completions.is_empty() {
                            app.completion_next();
                        } else {
                            app.trigger_completion();
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Esc,
                        ..
                    } => {
                        if !app.completions.is_empty() {
                            app.completions.clear();
                            app.needs_render = true;
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Up,
                        ..
                    } => {
                        if app.completions.is_empty() {
                            app.scroll_up();
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Down,
                        ..
                    } => {
                        if app.completions.is_empty() {
                            app.scroll_down();
                        }
                    }
                    KeyEvent {
                        code: KeyCode::PageUp,
                        ..
                    } => {
                        app.scroll_page_up(10);
                    }
                    KeyEvent {
                        code: KeyCode::PageDown,
                        ..
                    } => {
                        app.scroll_page_down(10);
                    }
                    KeyEvent {
                        code: KeyCode::Char(c),
                        ..
                    } => {
                        // Handle retry with 'r' key
                        if c == 'r' && app.show_retry_button {
                            app.retry_connection();
                        } else {
                            app.input_buffer.insert(app.cursor_pos, c);
                            app.cursor_pos += 1;
                            app.completions.clear();
                        }
                        app.needs_render = true;
                    }
                    KeyEvent {
                        code: KeyCode::Backspace,
                        ..
                    } => {
                        if app.cursor_pos > 0 {
                            app.input_buffer.remove(app.cursor_pos - 1);
                            app.cursor_pos -= 1;
                            app.completions.clear();
                        }
                        app.needs_render = true;
                    }
                    KeyEvent {
                        code: KeyCode::Left,
                        ..
                    } => {
                        if app.cursor_pos > 0 {
                            app.cursor_pos -= 1;
                            app.completions.clear();
                            app.needs_render = true;
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Right,
                        ..
                    } => {
                        if app.cursor_pos < app.input_buffer.len() {
                            app.cursor_pos += 1;
                            app.completions.clear();
                            app.needs_render = true;
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Home,
                        ..
                    } => {
                        app.cursor_pos = 0;
                        app.completions.clear();
                        app.needs_render = true;
                    }
                    KeyEvent {
                        code: KeyCode::End,
                        ..
                    } => {
                        app.cursor_pos = app.input_buffer.len();
                        app.completions.clear();
                        app.needs_render = true;
                    }
                    _ => {}
                }
            }
        }

        // Poll for ACP notifications (RuntimeEvent) - collect but don't render immediately
        let mut notifications = Vec::new();
        if let Some(client) = &mut app.acp_client {
            while let Ok(Some(msg)) = client.try_read_message() {
                match msg {
                    AcpMessage::Notification(method, params) => {
                        notifications.push((method, params));
                    }
                    AcpMessage::Response(_) => {
                        // Responses are handled elsewhere
                    }
                }
            }
        }

        // Handle collected notifications
        for (method, params) in notifications {
            app.handle_notification(method, params);
        }

        // Flush pending events periodically (debounced)
        if app.last_render.elapsed() >= Duration::from_millis(DEBOUNCE_MS) {
            app.flush_pending_events();
        }

        // Periodic connection check (every 10 seconds)
        static mut LAST_CHECK: Option<Instant> = None;
        unsafe {
            let now = Instant::now();
            let should_check = match LAST_CHECK {
                None => true,
                Some(last) => now.duration_since(last) > Duration::from_secs(10),
            };
            if should_check {
                app.check_connection();
                LAST_CHECK = Some(now);
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn main() {
    // Initialize logging system
    let log_dir = dirs::home_dir()
        .map(|p| p.join(".kiana").join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from(".kiana/logs"));

    std::fs::create_dir_all(&log_dir).ok();

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("tui")
        .filename_suffix("log")
        .max_log_files(10)
        .build(&log_dir)
        .expect("Failed to initialize log file appender");

    tracing_subscriber::fmt()
        .with_writer(file_appender.with_max_level(tracing::Level::INFO))
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(false)
        .init();

    tracing::info!("Kiana TUI starting...");

    match run_app() {
        Ok(_) => {
            tracing::info!("Kiana TUI exited normally");
        }
        Err(e) => {
            tracing::error!("Fatal error: {}", e);
            eprintln!("Error: {}", e);
            eprintln!("Check logs at: {}", log_dir.join("tui.log").display());
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_overlay_state() {
        let mut app = App::new();
        assert!(!app.has_active_overlay());
        assert!(app.take_overlay_result().is_none());
    }

    #[test]
    fn test_overlay_result_lifecycle() {
        let mut app = App::new();
        app.overlay_result = Some("test result".to_string());

        assert_eq!(app.take_overlay_result(), Some("test result".to_string()));
        assert!(app.take_overlay_result().is_none()); // Already consumed
    }
}
