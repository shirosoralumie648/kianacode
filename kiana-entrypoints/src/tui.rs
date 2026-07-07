use anyhow::{anyhow, Result};
use kiana_commands::{
    create_default_command_registry, Command, CommandContext, CommandRegistry, CommandResult,
    CommandType,
};
use kiana_screens::{
    history::{normalize_history_entries, HistoryEntry},
    repl::{ConversationMessage, MessageRole, ReplPermissionPanel},
    resume_conversation::SessionEntry,
    settings::{SettingsRow, SettingsSection},
    App, AppAction, AppScreen,
};
use kiana_services::api::streaming::{ContentBlock, Delta, StreamEvent};
use kiana_tools::tool_execution::{
    PermissionPromptDecision, PermissionPromptHandler, PermissionPromptRequest,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot, watch,
};

pub(crate) const TUI_PROMPT_HISTORY_LIMIT: usize = 200;
pub(crate) const TUI_PROMPT_HISTORY_FILE: &str = "tui-history.jsonl";

pub async fn run_tui() -> Result<()> {
    ensure_tui_terminal(
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )?;
    let cwd = std::env::current_dir()?;
    let runtime = Rc::new(RefCell::new(TuiRuntime::new(cwd).await?));
    let action_runtime = Rc::clone(&runtime);
    let tick_runtime = Rc::clone(&runtime);

    kiana_screens::run_app_with_handlers(
        move |action, app| action_runtime.borrow_mut().handle_action(action, app),
        move |app| tick_runtime.borrow_mut().drain_events(app),
    )
}

fn ensure_tui_terminal(stdin_is_terminal: bool, stdout_is_terminal: bool) -> Result<()> {
    match (stdin_is_terminal, stdout_is_terminal) {
        (true, true) => Ok(()),
        (false, false) => Err(anyhow!(
            "kiana tui requires an interactive terminal on stdin and stdout; run it directly from a terminal instead of a pipe or background process"
        )),
        (false, true) => Err(anyhow!(
            "kiana tui requires interactive stdin; run it directly from a terminal instead of piping input"
        )),
        (true, false) => Err(anyhow!(
            "kiana tui requires interactive stdout; run it directly from a terminal instead of redirecting output"
        )),
    }
}

struct TuiRuntime {
    cwd: PathBuf,
    session_id: String,
    app_state: HashMap<String, Value>,
    command_registry: CommandRegistry,
    active_prompt_message_index: Option<usize>,
    active_prompt_tool_workbenches: HashMap<String, String>,
    active_prompt_abort: Option<watch::Sender<bool>>,
    queued_prompts: VecDeque<String>,
    pending_resume_session_id: Option<String>,
    pending_permission: Option<PendingPermission>,
    pending_permission_queue: VecDeque<PendingPermission>,
    onboarding_shown: bool,
    prompt_history_path: PathBuf,
    prompt_history_entries: Vec<HistoryEntry>,
    prompt_history_synced: bool,
    events_tx: UnboundedSender<TuiEvent>,
    events_rx: UnboundedReceiver<TuiEvent>,
}

enum TuiEvent {
    DoctorLoaded(Result<String, String>),
    ResumeEntriesLoaded(Result<Vec<SessionEntry>, String>),
    SettingsLoaded(Result<Vec<SettingsSection>, String>),
    SlashCommandCompleted {
        name: String,
        command_type: CommandType,
        result: Result<CommandResult, String>,
        session_sync: Option<Result<ResumedSession, String>>,
    },
    PromptStreamDelta(String),
    PromptStreamUsage {
        model: Option<String>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    PromptToolUseStarted {
        id: String,
        name: String,
        input: Value,
    },
    PromptToolResult {
        id: String,
        is_error: bool,
        content: String,
    },
    PromptCompleted(Result<String, String>),
    PromptSessionRefreshed {
        session_id: String,
        result: Result<ResumedSession, String>,
    },
    PermissionRequested {
        request: PermissionPromptRequest,
        respond_to: oneshot::Sender<PermissionPromptDecision>,
    },
    SessionResumed {
        requested_session_id: String,
        result: Result<ResumedSession, String>,
    },
    SessionCleared(Result<String, String>),
}

struct PendingPermission {
    request: PermissionPromptRequest,
    respond_to: oneshot::Sender<PermissionPromptDecision>,
}

struct ResumedSession {
    session_id: String,
    title: String,
    messages: Vec<ConversationMessage>,
}

impl TuiRuntime {
    async fn new(cwd: PathBuf) -> Result<Self> {
        let session_id = create_tui_session(&cwd).await?;
        let prompt_history_path = prompt_history_path();
        let prompt_history_entries =
            load_prompt_history_entries_from_path(&prompt_history_path).unwrap_or_default();
        let (events_tx, events_rx) = unbounded_channel();
        Ok(Self {
            cwd,
            session_id,
            app_state: HashMap::new(),
            command_registry: create_default_command_registry(),
            active_prompt_message_index: None,
            active_prompt_tool_workbenches: HashMap::new(),
            active_prompt_abort: None,
            queued_prompts: VecDeque::new(),
            pending_resume_session_id: None,
            pending_permission: None,
            pending_permission_queue: VecDeque::new(),
            onboarding_shown: false,
            prompt_history_path,
            prompt_history_entries,
            prompt_history_synced: false,
            events_tx,
            events_rx,
        })
    }

    fn handle_action(&mut self, action: AppAction, app: &mut App) -> Result<()> {
        match action {
            AppAction::LoadDoctor => {
                self.load_doctor(app);
            }
            AppAction::LoadResumeSessions => {
                app.resume.loading = true;
                let events_tx = self.events_tx.clone();
                let cwd = self.cwd.clone();
                tokio::spawn(async move {
                    let result = load_resume_entries(&cwd)
                        .await
                        .map_err(|error| error.to_string());
                    let _ = events_tx.send(TuiEvent::ResumeEntriesLoaded(result));
                });
            }
            AppAction::LoadPromptHistory => {
                self.load_prompt_history(app);
            }
            AppAction::LoadSettings => {
                self.load_settings(app);
            }
            AppAction::SubmitPrompt(prompt) => {
                self.record_prompt_history(&prompt, app);
                self.start_prompt(prompt, app);
            }
            AppAction::QueuePrompt(prompt) => {
                self.record_prompt_history(&prompt, app);
                self.queue_prompt(prompt, app);
            }
            AppAction::CancelPrompt => {
                self.cancel_active_prompt(app);
            }
            AppAction::RunSlashCommand { name, args } => {
                self.handle_slash_command(name, args, app);
            }
            AppAction::ResumeSession(session_id) => {
                app.repl.is_loading = true;
                self.pending_resume_session_id = Some(session_id.clone());
                let events_tx = self.events_tx.clone();
                tokio::spawn(async move {
                    let requested_session_id = session_id.clone();
                    let result = load_resumed_session(session_id)
                        .await
                        .map_err(|error| error.to_string());
                    let _ = events_tx.send(TuiEvent::SessionResumed {
                        requested_session_id,
                        result,
                    });
                });
            }
        }
        Ok(())
    }

    fn load_prompt_history(&mut self, app: &mut App) {
        app.history.loading = false;
        match load_prompt_history_entries_from_path(&self.prompt_history_path) {
            Ok(entries) => {
                self.prompt_history_entries = entries.clone();
                self.prompt_history_synced = true;
                app.history.load_entries(entries.clone());
                app.repl.load_persisted_history(&entries);
            }
            Err(error) => {
                app.screen = AppScreen::Repl;
                app.repl.push_message(
                    MessageRole::System,
                    format!("Failed to load prompt history: {error}"),
                );
            }
        }
    }

    fn record_prompt_history(&mut self, prompt: &str, app: &mut App) {
        match record_prompt_history_at_path(&self.prompt_history_path, prompt) {
            Ok(entries) => {
                self.prompt_history_entries = entries.clone();
                self.prompt_history_synced = true;
                app.history.load_entries(entries);
            }
            Err(error) => app.repl.push_message(
                MessageRole::System,
                format!("Failed to save prompt history: {error}"),
            ),
        }
    }

    fn load_doctor(&mut self, app: &mut App) {
        app.doctor.set_loading();
        let Some(command) = self.command_registry.get("doctor").cloned() else {
            app.doctor
                .set_error("doctor command is not registered".to_string());
            return;
        };

        let app_state = self.command_app_state(app);
        let events_tx = self.events_tx.clone();
        tokio::spawn(async move {
            let result = command
                .execute(CommandContext {
                    args: String::new(),
                    app_state,
                })
                .await
                .map(|result| result.value)
                .map_err(|error| error.to_string());
            let _ = events_tx.send(TuiEvent::DoctorLoaded(result));
        });
    }

    fn load_settings(&mut self, app: &mut App) {
        app.settings.set_loading();
        let auth_command = self.command_registry.get("auth").cloned();
        let model_command = self.command_registry.get("model").cloned();
        let permissions_command = self.command_registry.get("permissions").cloned();
        let mcp_command = self.command_registry.get("mcp").cloned();
        let doctor_command = self.command_registry.get("doctor").cloned();
        let app_state = self.command_app_state(app);
        let events_tx = self.events_tx.clone();
        tokio::spawn(async move {
            let result = load_settings_sections(
                auth_command,
                model_command,
                permissions_command,
                mcp_command,
                doctor_command,
                app_state,
            )
            .await;
            let _ = events_tx.send(TuiEvent::SettingsLoaded(result));
        });
    }

    fn start_prompt(&mut self, prompt: String, app: &mut App) {
        if app.repl.is_loading {
            app.repl.push_message(
                MessageRole::System,
                "A prompt is already running.".to_string(),
            );
            return;
        }
        app.repl.is_loading = true;
        self.active_prompt_message_index = None;
        self.active_prompt_tool_workbenches.clear();
        let mut options = repl_prompt_options(&self.session_id, &self.cwd);
        options.insert("execute".to_string(), Value::Bool(true));
        options.insert(
            "permission_prompt_tool".to_string(),
            Value::String("stdio".to_string()),
        );
        let (abort_tx, abort_rx) = watch::channel(false);
        self.active_prompt_abort = Some(abort_tx);
        let events_tx = self.events_tx.clone();
        tokio::spawn(async move {
            let stream_events_tx = events_tx.clone();
            let permission_handler = TuiPermissionPromptHandler {
                events_tx: events_tx.clone(),
            };
            let result = crate::sdk::unstable_v2_prompt_streaming_with_local_events_and_permission_handler_and_abort_signal(
                    prompt,
                    options,
                    move |event| {
                        match event {
                            crate::sdk::SdkPromptStreamEvent::Model(event) => {
                                if let Some(delta) = prompt_text_delta_from_stream_event(&event) {
                                    let _ = stream_events_tx.send(TuiEvent::PromptStreamDelta(delta));
                                }
                                if let Some(event) = prompt_usage_from_stream_event(&event) {
                                    let _ = stream_events_tx.send(event);
                                }
                                if let Some(event) = prompt_tool_use_from_stream_event(&event) {
                                    let _ = stream_events_tx.send(event);
                                }
                            }
                            crate::sdk::SdkPromptStreamEvent::ToolResult {
                                id,
                                is_error,
                                content,
                                ..
                            } => {
                                let _ = stream_events_tx.send(TuiEvent::PromptToolResult {
                                    id,
                                    is_error,
                                    content,
                                });
                            }
                        }
                        Ok(())
                    },
                    &permission_handler,
                    abort_rx,
                )
                .await
                .map(|result| assistant_text_from_result(&result).to_string())
                .map_err(|error| error.to_string());
            let _ = events_tx.send(TuiEvent::PromptCompleted(result));
        });
    }

    fn queue_prompt(&mut self, prompt: String, app: &mut App) {
        if prompt.trim().is_empty() {
            return;
        }
        self.queued_prompts.push_back(prompt);
        app.repl.push_message(
            MessageRole::System,
            "Queued prompt; it will run after the current response.".to_string(),
        );
    }

    fn handle_slash_command(&mut self, name: String, args: String, app: &mut App) {
        let name = if name == "quit" {
            "exit".to_string()
        } else {
            name
        };

        if self.handle_permission_response_command(&name, &args, app) {
            return;
        }

        if matches!(name.as_str(), "cancel" | "stop") {
            self.cancel_active_prompt(app);
            return;
        }

        if app.repl.is_loading {
            app.repl.push_message(
                MessageRole::System,
                "A prompt or command is already running.".to_string(),
            );
            return;
        }

        if is_clear_session_command(&name, &args) {
            self.start_clear_session(app);
            return;
        }

        let Some(command) = self.command_registry.get(&name).cloned() else {
            app.repl.push_message(
                MessageRole::System,
                format!("Unknown command: /{name}. Try /help."),
            );
            return;
        };

        app.repl.is_loading = true;
        let command_type = command.command_type();
        let app_state = self.command_app_state(app);
        let current_session_id = self.session_id.clone();
        let events_tx = self.events_tx.clone();
        tokio::spawn(async move {
            let result = command
                .execute(CommandContext { args, app_state })
                .await
                .map_err(|error| error.to_string());
            let session_sync = match &result {
                Ok(result) => match session_sync_request(&name, result, &current_session_id) {
                    Some(session_id) => Some(
                        load_resumed_session(session_id)
                            .await
                            .map_err(|error| error.to_string()),
                    ),
                    None => None,
                },
                Err(_) => None,
            };
            let _ = events_tx.send(TuiEvent::SlashCommandCompleted {
                name,
                command_type,
                result,
                session_sync,
            });
        });
    }

    fn start_clear_session(&mut self, app: &mut App) {
        app.repl.is_loading = true;
        let cwd = self.cwd.clone();
        let events_tx = self.events_tx.clone();
        tokio::spawn(async move {
            let result = create_tui_session(&cwd)
                .await
                .map_err(|error| error.to_string());
            let _ = events_tx.send(TuiEvent::SessionCleared(result));
        });
    }

    fn cancel_active_prompt(&mut self, app: &mut App) {
        let mut cancelled = false;
        if let Some(abort_tx) = self.active_prompt_abort.take() {
            let _ = abort_tx.send(true);
            cancelled = true;
        }
        if let Some(pending) = self.pending_permission.take() {
            let _ = pending.respond_to.send(PermissionPromptDecision::Deny(
                "Prompt cancelled by user.".to_string(),
            ));
            cancelled = true;
        }
        if !self.pending_permission_queue.is_empty() {
            for pending in self.pending_permission_queue.drain(..) {
                let _ = pending.respond_to.send(PermissionPromptDecision::Deny(
                    "Prompt cancelled by user.".to_string(),
                ));
            }
            cancelled = true;
        }

        if cancelled {
            self.active_prompt_message_index = None;
            app.repl.clear_permission_request();
            self.queued_prompts.clear();
            app.repl.push_message(
                MessageRole::System,
                "Cancelling current prompt and clearing queued prompt.".to_string(),
            );
        } else {
            app.repl
                .push_message(MessageRole::System, "No prompt is running.".to_string());
        }
    }

    fn command_app_state(&self, app: &App) -> HashMap<String, Value> {
        let mut state = self.app_state.clone();
        state.insert(
            "session_id".to_string(),
            Value::String(self.session_id.clone()),
        );
        state.insert(
            "cwd".to_string(),
            Value::String(self.cwd.to_string_lossy().to_string()),
        );
        state.insert(
            "tui_permission_request_active".to_string(),
            Value::Bool(app.repl.permission_request_active),
        );
        state.insert(
            "tui_permission_request_queue_len".to_string(),
            Value::from(app.repl.permission_request_queue_len),
        );
        state
    }

    fn drain_events(&mut self, app: &mut App) -> Result<()> {
        self.sync_prompt_history(app);
        self.maybe_show_onboarding(app);
        while let Ok(event) = self.events_rx.try_recv() {
            self.apply_event(app, event);
        }
        Ok(())
    }

    fn sync_prompt_history(&mut self, app: &mut App) {
        if self.prompt_history_synced {
            return;
        }
        self.prompt_history_synced = true;
        app.history
            .load_entries(self.prompt_history_entries.clone());
        app.repl
            .load_persisted_history(&self.prompt_history_entries);
    }

    fn maybe_show_onboarding(&mut self, app: &mut App) {
        if self.onboarding_shown {
            return;
        }
        self.onboarding_shown = true;
        if kiana_services::auth::get_api_key().is_some()
            || kiana_services::auth::check_oauth_tokens()
        {
            return;
        }
        app.repl.push_message(
            MessageRole::System,
            "Onboarding: authentication is not configured. Run `kiana auth login <api-key>` or set `ANTHROPIC_API_KEY`, then run `/doctor` to verify readiness."
                .to_string(),
        );
    }

    fn apply_event(&mut self, app: &mut App, event: TuiEvent) {
        match event {
            TuiEvent::DoctorLoaded(result) => match result {
                Ok(output) => app.doctor.set_output(output),
                Err(error) => app.doctor.set_error(error),
            },
            TuiEvent::ResumeEntriesLoaded(result) => {
                app.resume.loading = false;
                match result {
                    Ok(entries) => app.resume.load_sessions(entries),
                    Err(error) => {
                        app.screen = AppScreen::Repl;
                        app.repl.push_message(
                            MessageRole::System,
                            format!("Failed to load sessions: {error}"),
                        );
                    }
                }
            }
            TuiEvent::SettingsLoaded(result) => match result {
                Ok(sections) => app.settings.set_sections(sections),
                Err(error) => app.settings.set_error(error),
            },
            TuiEvent::SlashCommandCompleted {
                name,
                command_type,
                result,
                session_sync,
            } => {
                app.repl.is_loading = false;
                match result {
                    Ok(result) if result.output_type == "exit" => {
                        if !result.value.trim().is_empty() {
                            app.repl.push_message(MessageRole::System, result.value);
                        }
                        app.should_quit = true;
                    }
                    Ok(result) => match command_type {
                        CommandType::Prompt => {
                            if result.value.trim().is_empty() {
                                app.repl.push_message(
                                    MessageRole::System,
                                    format!("/{name} produced an empty prompt."),
                                );
                            } else {
                                app.repl.push_message(
                                    MessageRole::System,
                                    format!("Expanded /{name} and sent it to the assistant."),
                                );
                                self.start_prompt(result.value, app);
                            }
                        }
                        CommandType::Local | CommandType::LocalJsx => {
                            self.apply_local_command_result(app, &name, result, session_sync)
                        }
                    },
                    Err(error) => app
                        .repl
                        .push_message(MessageRole::System, format!("/{name} failed: {error}")),
                }
            }
            TuiEvent::PromptStreamDelta(delta) => {
                self.apply_prompt_stream_delta(app, delta);
            }
            TuiEvent::PromptStreamUsage {
                model,
                input_tokens,
                output_tokens,
            } => {
                if let Some(model) = model {
                    app.repl.model_name = model;
                }
                if let Some(input_tokens) = input_tokens {
                    app.repl.input_tokens = input_tokens;
                }
                if let Some(output_tokens) = output_tokens {
                    app.repl.output_tokens = output_tokens;
                }
            }
            TuiEvent::PromptToolUseStarted { id, name, input } => {
                self.active_prompt_message_index = None;
                let workbench = crate::runner::default_tool_workbench(&name);
                if let Some(workbench) = &workbench {
                    self.active_prompt_tool_workbenches
                        .insert(id.clone(), workbench.clone());
                }
                app.repl.push_message(
                    MessageRole::Tool,
                    format_tool_use_message(&id, &name, workbench.as_deref(), &input),
                );
            }
            TuiEvent::PromptToolResult {
                id,
                is_error,
                content,
            } => {
                let workbench = self.active_prompt_tool_workbenches.remove(&id);
                self.apply_prompt_tool_result(app, id, is_error, workbench.as_deref(), content);
            }
            TuiEvent::PromptCompleted(result) => {
                let should_wait_for_refresh = result.is_ok();
                app.repl.is_loading = false;
                app.repl.clear_permission_request();
                self.active_prompt_abort = None;
                self.apply_prompt_completed(app, result);
                if !should_wait_for_refresh {
                    self.start_queued_prompt_if_idle(app);
                }
            }
            TuiEvent::PromptSessionRefreshed { session_id, result } => {
                self.apply_prompt_session_refreshed(app, session_id, result);
                self.start_queued_prompt_if_idle(app);
            }
            TuiEvent::PermissionRequested {
                request,
                respond_to,
            } => {
                self.apply_permission_requested(app, request, respond_to);
            }
            TuiEvent::SessionResumed {
                requested_session_id,
                result,
            } => {
                if self.pending_resume_session_id.as_deref() != Some(&requested_session_id) {
                    return;
                }
                self.pending_resume_session_id = None;
                app.repl.is_loading = false;
                self.active_prompt_message_index = None;
                match result {
                    Ok(resumed) => {
                        self.session_id = resumed.session_id.clone();
                        app.repl.messages = resumed.messages;
                        app.repl.push_message(
                            MessageRole::System,
                            format!("Resumed session {} ({})", resumed.session_id, resumed.title),
                        );
                    }
                    Err(error) => app
                        .repl
                        .push_message(MessageRole::System, format!("Resume failed: {error}")),
                }
            }
            TuiEvent::SessionCleared(result) => {
                app.repl.is_loading = false;
                app.repl.clear_permission_request();
                self.active_prompt_message_index = None;
                self.pending_resume_session_id = None;
                self.queued_prompts.clear();
                match result {
                    Ok(session_id) => {
                        self.session_id = session_id.clone();
                        self.app_state.clear();
                        app.repl.messages.clear();
                        app.repl.scroll_offset = 0;
                        app.repl.push_message(
                            MessageRole::System,
                            format!("New session: {session_id}"),
                        );
                    }
                    Err(error) => app
                        .repl
                        .push_message(MessageRole::System, format!("Clear failed: {error}")),
                }
            }
        }
    }

    fn apply_local_command_result(
        &mut self,
        app: &mut App,
        command_name: &str,
        result: CommandResult,
        session_sync: Option<Result<ResumedSession, String>>,
    ) {
        if let Some(sync) = session_sync {
            match sync {
                Ok(resumed) => {
                    self.session_id = resumed.session_id.clone();
                    let message_count = resumed.messages.len();
                    let status =
                        session_sync_status_message(&result, &self.session_id, message_count);
                    app.repl.messages = resumed.messages;
                    app.repl.push_message(MessageRole::System, status);
                    return;
                }
                Err(error) => {
                    app.repl.push_message(
                        MessageRole::System,
                        format!("Session command completed but transcript refresh failed: {error}"),
                    );
                    return;
                }
            }
        }

        if let Some(preview) = format_tui_diff_preview(command_name, &result.value) {
            app.repl.push_message(MessageRole::System, preview);
            return;
        }

        if !result.value.trim().is_empty() {
            app.repl.push_message(MessageRole::System, result.value);
        }
    }

    fn start_queued_prompt_if_idle(&mut self, app: &mut App) {
        if app.repl.is_loading {
            return;
        }
        let Some(prompt) = self.queued_prompts.pop_front() else {
            return;
        };
        app.repl.push_message(MessageRole::User, prompt.clone());
        self.start_prompt(prompt, app);
    }

    fn apply_prompt_stream_delta(&mut self, app: &mut App, delta: String) {
        if delta.is_empty() {
            return;
        }

        if let Some(index) = self.active_prompt_message_index {
            if let Some(message) = app.repl.messages.get_mut(index) {
                if message.role == MessageRole::Assistant {
                    message.content.push_str(&delta);
                    app.repl.scroll_offset = 0;
                    return;
                }
            }
        }

        app.repl.push_message(MessageRole::Assistant, delta);
        self.active_prompt_message_index = app.repl.messages.len().checked_sub(1);
    }

    fn apply_prompt_tool_result(
        &mut self,
        app: &mut App,
        id: String,
        is_error: bool,
        workbench: Option<&str>,
        content: String,
    ) {
        let result_message = format_tool_result_message(is_error, workbench, &content, None);
        for message in app.repl.messages.iter_mut().rev() {
            if message.role == MessageRole::Tool && tool_message_matches_id(&message.content, &id) {
                message.content = append_or_replace_tool_result(&message.content, &result_message);
                app.repl.scroll_offset = 0;
                return;
            }
        }

        app.repl.push_message(
            MessageRole::Tool,
            format!("tool_use_id: {id}\n{result_message}"),
        );
    }

    fn apply_prompt_completed(&mut self, app: &mut App, result: Result<String, String>) {
        match result {
            Ok(text) if text.trim().is_empty() => {
                self.active_prompt_message_index = None;
                self.refresh_completed_prompt_session();
            }
            Ok(text) => {
                let mut completed_text = Some(text);
                if let Some(index) = self.active_prompt_message_index {
                    if let Some(message) = app.repl.messages.get_mut(index) {
                        if message.role == MessageRole::Assistant {
                            message.content = completed_text.take().unwrap_or_default();
                            app.repl.scroll_offset = 0;
                        }
                    }
                }
                if let Some(text) = completed_text {
                    app.repl.push_message(MessageRole::Assistant, text);
                }
                self.active_prompt_message_index = None;
                self.refresh_completed_prompt_session();
            }
            Err(error) if error.contains("assistant turn cancelled") => app
                .repl
                .push_message(MessageRole::System, "Prompt cancelled.".to_string()),
            Err(error) => app
                .repl
                .push_message(MessageRole::System, format!("Prompt failed: {error}")),
        }
    }

    fn refresh_completed_prompt_session(&self) {
        let session_id = self.session_id.clone();
        let events_tx = self.events_tx.clone();
        tokio::spawn(async move {
            let result = load_resumed_session(session_id.clone())
                .await
                .map_err(|error| error.to_string());
            let _ = events_tx.send(TuiEvent::PromptSessionRefreshed { session_id, result });
        });
    }

    fn apply_prompt_session_refreshed(
        &mut self,
        app: &mut App,
        session_id: String,
        result: Result<ResumedSession, String>,
    ) {
        if session_id != self.session_id {
            return;
        }

        match result {
            Ok(resumed) => {
                app.repl.messages = resumed.messages;
                app.repl.scroll_offset = 0;
            }
            Err(error) => app.repl.push_message(
                MessageRole::System,
                format!("Failed to refresh prompt session: {error}"),
            ),
        }
    }

    fn apply_permission_requested(
        &mut self,
        app: &mut App,
        request: PermissionPromptRequest,
        respond_to: oneshot::Sender<PermissionPromptDecision>,
    ) {
        self.active_prompt_message_index = None;
        let pending = PendingPermission {
            request,
            respond_to,
        };
        if self.pending_permission.is_none() {
            self.show_pending_permission(app, pending);
        } else {
            let tool_name = pending.request.tool_name.clone();
            self.pending_permission_queue.push_back(pending);
            app.repl.permission_request_queue_len = self.pending_permission_queue.len();
            app.repl.push_message(
                MessageRole::System,
                format!("Queued permission request for {tool_name}."),
            );
        }
    }

    fn handle_permission_response_command(
        &mut self,
        name: &str,
        args: &str,
        app: &mut App,
    ) -> bool {
        let decision = match name {
            "allow" | "approve" => Some(PermissionPromptDecision::Allow),
            "deny" | "reject" => Some(PermissionPromptDecision::Deny(if args.trim().is_empty() {
                "Permission denied in TUI.".to_string()
            } else {
                args.trim().to_string()
            })),
            _ => None,
        };
        let Some(decision) = decision else {
            return false;
        };

        let Some(pending) = self.pending_permission.take() else {
            app.repl.push_message(
                MessageRole::System,
                "No pending permission request.".to_string(),
            );
            return true;
        };

        let tool_name = pending.request.tool_name.clone();
        let status = match &decision {
            PermissionPromptDecision::Allow => "Approved",
            PermissionPromptDecision::Deny(_) => "Denied",
        };
        match pending.respond_to.send(decision) {
            Ok(()) => app
                .repl
                .push_message(MessageRole::System, format!("{status} {tool_name}.")),
            Err(_) => app.repl.push_message(
                MessageRole::System,
                format!("Permission request for {tool_name} is no longer active."),
            ),
        }
        self.activate_next_pending_permission(app);
        true
    }

    fn show_pending_permission(&mut self, app: &mut App, pending: PendingPermission) {
        let panel = permission_panel_from_request(&pending.request);
        app.repl
            .set_permission_request(panel, self.pending_permission_queue.len());
        app.repl.push_message(
            MessageRole::System,
            format_permission_request_message(&pending.request),
        );
        self.pending_permission = Some(pending);
    }

    fn activate_next_pending_permission(&mut self, app: &mut App) {
        if self.pending_permission.is_some() {
            return;
        }
        if let Some(pending) = self.pending_permission_queue.pop_front() {
            self.show_pending_permission(app, pending);
        } else {
            app.repl.clear_permission_request();
        }
    }
}

pub(crate) async fn load_settings_sections(
    auth_command: Option<Arc<dyn Command>>,
    model_command: Option<Arc<dyn Command>>,
    permissions_command: Option<Arc<dyn Command>>,
    mcp_command: Option<Arc<dyn Command>>,
    doctor_command: Option<Arc<dyn Command>>,
    app_state: HashMap<String, Value>,
) -> std::result::Result<Vec<SettingsSection>, String> {
    let auth = execute_settings_command("auth", auth_command, "status --json", &app_state).await;
    let model = execute_settings_command("model", model_command, "list --json", &app_state).await;
    let permissions =
        execute_settings_command("permissions", permissions_command, "status", &app_state).await;
    let mcp = execute_settings_command("mcp", mcp_command, "status", &app_state).await;
    let doctor = execute_settings_command("doctor", doctor_command, "--json", &app_state).await;

    Ok(vec![
        auth_settings_section(auth),
        model_settings_section(model),
        permissions_settings_section(permissions),
        mcp_settings_section(mcp),
        doctor_settings_section(doctor),
    ])
}

async fn execute_settings_command(
    name: &'static str,
    command: Option<Arc<dyn Command>>,
    args: &'static str,
    app_state: &HashMap<String, Value>,
) -> std::result::Result<String, String> {
    let Some(command) = command else {
        return Err(format!("{name} command is not registered"));
    };
    command
        .execute(CommandContext {
            args: args.to_string(),
            app_state: app_state.clone(),
        })
        .await
        .map(|result| result.value)
        .map_err(|error| error.to_string())
}

fn auth_settings_section(result: std::result::Result<String, String>) -> SettingsSection {
    let title = "Account/Auth";
    let output = match result {
        Ok(output) => output,
        Err(error) => return command_error_section(title, error),
    };
    let value: Value = match serde_json::from_str(&output) {
        Ok(value) => value,
        Err(error) => return command_error_section(title, format!("invalid auth JSON: {error}")),
    };

    let mut rows = vec![
        SettingsRow::new("api_key", json_path_display(&value, &["api_key"])),
        SettingsRow::new("source", json_path_display(&value, &["source"])),
        SettingsRow::new("oauth", json_path_display(&value, &["oauth", "status"])),
        SettingsRow::new(
            "oauth_refreshable",
            json_path_display(&value, &["oauth", "refreshable"]),
        ),
    ];
    if let Some(providers) = value.get("providers").and_then(Value::as_array) {
        rows.push(SettingsRow::new("providers", providers.len().to_string()));
        for provider in providers {
            let provider_id = json_path_display(provider, &["provider_id"]);
            let status = json_path_display(provider, &["status"]);
            let model = json_path_display(provider, &["model_id"]);
            let auth = json_path_display(provider, &["auth"]);
            let auth_source = json_path_display(provider, &["auth_source"]);
            let issues = provider
                .get("issues")
                .and_then(Value::as_array)
                .map(|issues| issues.len())
                .unwrap_or_default();
            let issue_suffix = if issues == 0 {
                "ready".to_string()
            } else {
                format!("issues={issues}")
            };
            rows.push(SettingsRow::new(
                provider_id,
                format!("{status} model={model} auth={auth}/{auth_source} {issue_suffix}"),
            ));
        }
    }

    SettingsSection::new(title, rows)
}

fn model_settings_section(result: std::result::Result<String, String>) -> SettingsSection {
    let title = "Provider/Model";
    let output = match result {
        Ok(output) => output,
        Err(error) => return command_error_section(title, error),
    };
    let profiles: Value = match serde_json::from_str(&output) {
        Ok(value) => value,
        Err(error) => return command_error_section(title, format!("invalid model JSON: {error}")),
    };
    let Some(profiles) = profiles.as_array() else {
        return command_error_section(title, "model list JSON was not an array".to_string());
    };

    let tool_capable = profiles
        .iter()
        .filter(|profile| json_path_bool(profile, &["supports_tools"]))
        .count();
    let streaming_capable = profiles
        .iter()
        .filter(|profile| json_path_bool(profile, &["supports_streaming"]))
        .count();
    let mut rows = vec![
        SettingsRow::new("profiles", profiles.len().to_string()),
        SettingsRow::new("tool_capable", tool_capable.to_string()),
        SettingsRow::new("streaming_capable", streaming_capable.to_string()),
    ];
    for profile in profiles {
        let label = format!(
            "{}/{}",
            json_path_display(profile, &["provider_id"]),
            json_path_display(profile, &["model_id"])
        );
        rows.push(SettingsRow::new(
            label,
            format!(
                "tools={} streaming={} vision={} structured={} context={}",
                json_path_display(profile, &["supports_tools"]),
                json_path_display(profile, &["supports_streaming"]),
                json_path_display(profile, &["supports_vision"]),
                json_path_display(profile, &["supports_structured_output"]),
                json_path_display(profile, &["context_window"])
            ),
        ));
    }

    SettingsSection::new(title, rows)
}

fn permissions_settings_section(result: std::result::Result<String, String>) -> SettingsSection {
    let title = "Permissions";
    let output = match result {
        Ok(output) => output,
        Err(error) => return command_error_section(title, error),
    };
    SettingsSection::new(
        title,
        text_status_rows(
            &output,
            &[
                "profile",
                "mode",
                "managed_policy_status",
                "sources",
                "allowed_tools",
                "disallowed_tools",
                "managed_disallowed_tools",
                "managed_ask_tools",
            ],
        ),
    )
}

fn mcp_settings_section(result: std::result::Result<String, String>) -> SettingsSection {
    let title = "MCP";
    let output = match result {
        Ok(output) => output,
        Err(error) => return command_error_section(title, error),
    };
    SettingsSection::new(
        title,
        text_status_rows(
            &output,
            &[
                "client_stdio",
                "client_http",
                "client_sse",
                "client_ws",
                "server_transport",
                "protocol_surfaces",
                "error_states",
                "registered_session_invocations",
            ],
        ),
    )
}

fn doctor_settings_section(result: std::result::Result<String, String>) -> SettingsSection {
    let title = "Remote/Diagnostics";
    let output = match result {
        Ok(output) => output,
        Err(error) => return command_error_section(title, error),
    };
    let value: Value = match serde_json::from_str(&output) {
        Ok(value) => value,
        Err(error) => return command_error_section(title, format!("invalid doctor JSON: {error}")),
    };

    let remote_bridge = format!(
        "start_command={} token={}",
        json_path_display(&value, &["remote_bridge", "start_command_wired"]),
        json_path_display(&value, &["remote_bridge", "token_configured"])
    );
    let remote_session = format!(
        "configured={} token={} source={}",
        json_path_display(&value, &["remote_code_session", "configured"]),
        json_path_display(&value, &["remote_code_session", "live_smoke_token"]),
        json_path_display(&value, &["remote_code_session", "source"])
    );
    let warnings = value
        .get("warnings")
        .and_then(Value::as_array)
        .map(|warnings| warnings.len())
        .unwrap_or_default();

    SettingsSection::new(
        title,
        vec![
            SettingsRow::new("status", json_path_display(&value, &["status"])),
            SettingsRow::new("model", json_path_display(&value, &["model"])),
            SettingsRow::new("remote_bridge", remote_bridge),
            SettingsRow::new("remote_code_session", remote_session),
            SettingsRow::new(
                "oauth_token_file",
                json_path_display(&value, &["oauth_token_file", "status"]),
            ),
            SettingsRow::new(
                "bash_sandbox",
                json_path_display(&value, &["bash_sandbox", "status"]),
            ),
            SettingsRow::new(
                "commercial_security",
                json_path_display(&value, &["commercial_security", "status"]),
            ),
            SettingsRow::new("warnings", warnings.to_string()),
        ],
    )
}

fn command_error_section(title: impl Into<String>, error: String) -> SettingsSection {
    SettingsSection::new(
        title,
        vec![
            SettingsRow::new("status", "error"),
            SettingsRow::new("error", error),
        ],
    )
}

fn text_status_rows(output: &str, keys: &[&str]) -> Vec<SettingsRow> {
    keys.iter()
        .filter_map(|key| text_status_value(output, key).map(|value| SettingsRow::new(*key, value)))
        .collect()
}

fn text_status_value(output: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}: ");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::to_string))
}

fn json_path_display(value: &Value, path: &[&str]) -> String {
    let Some(value) = json_path(value, path) else {
        return "unknown".to_string();
    };
    match value {
        Value::Null => "none".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) if value.trim().is_empty() => "empty".to_string(),
        Value::String(value) => value.clone(),
        Value::Array(value) => value.len().to_string(),
        Value::Object(_) => "object".to_string(),
    }
}

fn json_path_bool(value: &Value, path: &[&str]) -> bool {
    json_path(value, path)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn json_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

#[derive(Clone)]
struct TuiPermissionPromptHandler {
    events_tx: UnboundedSender<TuiEvent>,
}

#[async_trait::async_trait]
impl PermissionPromptHandler for TuiPermissionPromptHandler {
    async fn prompt(
        &self,
        request: PermissionPromptRequest,
    ) -> Result<PermissionPromptDecision, String> {
        let (respond_to, response_rx) = oneshot::channel();
        self.events_tx
            .send(TuiEvent::PermissionRequested {
                request,
                respond_to,
            })
            .map_err(|_| "TUI permission prompt is unavailable".to_string())?;
        response_rx
            .await
            .map_err(|_| "TUI permission prompt was dismissed".to_string())
    }
}

async fn create_tui_session(cwd: &Path) -> Result<String> {
    let mut options = HashMap::new();
    options.insert(
        "title".to_string(),
        Value::String(tui_session_title(cwd).to_string()),
    );
    options.insert("tag".to_string(), Value::String("tui".to_string()));
    options.insert(
        "cwd".to_string(),
        Value::String(cwd.to_string_lossy().to_string()),
    );
    let session = crate::sdk::unstable_v2_create_session(options).await?;
    Ok(session.session_id)
}

async fn load_resume_entries(cwd: &Path) -> Result<Vec<SessionEntry>> {
    let sessions = crate::sdk::list_sessions().await?;
    Ok(filter_resume_sessions_for_cwd(sessions, cwd)
        .into_iter()
        .map(session_info_to_entry)
        .collect())
}

pub(crate) fn prompt_history_path() -> PathBuf {
    tui_kiana_home_dir().join(TUI_PROMPT_HISTORY_FILE)
}

fn tui_kiana_home_dir() -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_HOME") {
        return PathBuf::from(path);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kiana");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".kiana");
    }
    PathBuf::from(".kiana")
}

#[cfg(test)]
fn load_prompt_history_entries() -> Result<Vec<HistoryEntry>> {
    load_prompt_history_entries_from_path(&prompt_history_path())
}

pub(crate) fn load_prompt_history_entries_from_path(path: &Path) -> Result<Vec<HistoryEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    let entries = contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            serde_json::from_str::<HistoryEntry>(line).ok()
        })
        .collect::<Vec<_>>();
    let mut entries = normalize_history_entries(entries);
    entries.truncate(TUI_PROMPT_HISTORY_LIMIT);
    Ok(entries)
}

#[cfg(test)]
fn record_prompt_history_entry(prompt: &str) -> Result<Vec<HistoryEntry>> {
    record_prompt_history_at_path(&prompt_history_path(), prompt)
}

fn record_prompt_history_at_path(path: &Path, prompt: &str) -> Result<Vec<HistoryEntry>> {
    let prompt = prompt.trim();
    let mut entries = load_prompt_history_entries_from_path(path)?;
    if prompt.is_empty() {
        return Ok(entries);
    }
    entries.retain(|entry| entry.prompt != prompt);
    entries.insert(
        0,
        HistoryEntry::new(prompt.to_string(), current_history_timestamp()),
    );
    entries.truncate(TUI_PROMPT_HISTORY_LIMIT);
    write_prompt_history_entries(path, &entries)?;
    Ok(entries)
}

fn write_prompt_history_entries(path: &Path, entries: &[HistoryEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut contents = String::new();
    for entry in entries {
        contents.push_str(&serde_json::to_string(entry)?);
        contents.push('\n');
    }
    fs::write(path, contents)?;
    Ok(())
}

fn current_history_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn filter_resume_sessions_for_cwd(
    sessions: Vec<crate::sdk::SdkSessionInfo>,
    cwd: &Path,
) -> Vec<crate::sdk::SdkSessionInfo> {
    let cwd = cwd.to_string_lossy();
    let has_cwd_metadata = sessions.iter().any(|session| session.cwd.is_some());
    let current_cwd_sessions = sessions
        .iter()
        .filter(|session| session.cwd.as_deref() == Some(cwd.as_ref()))
        .cloned()
        .collect::<Vec<_>>();
    if !current_cwd_sessions.is_empty() || has_cwd_metadata {
        return current_cwd_sessions;
    }
    sessions
}

fn session_info_to_entry(session: crate::sdk::SdkSessionInfo) -> SessionEntry {
    SessionEntry {
        session_id: session.session_id,
        title: session.title.unwrap_or_else(|| "Untitled".to_string()),
        timestamp: session.updated_at.to_string(),
        message_count: session.message_count,
    }
}

async fn load_resumed_session(session_id: String) -> Result<ResumedSession> {
    let Some(info) = crate::sdk::get_session_info(session_id.clone()).await? else {
        return Err(anyhow!("session '{}' was not found", session_id));
    };
    let messages = crate::sdk::get_session_messages(session_id.clone()).await?;
    Ok(ResumedSession {
        session_id: session_id.clone(),
        title: info.title.unwrap_or_else(|| "untitled".to_string()),
        messages: sdk_messages_to_conversation(&session_id, messages),
    })
}

fn session_sync_request(
    command_name: &str,
    result: &CommandResult,
    current_session_id: &str,
) -> Option<String> {
    if command_name != "session" {
        return None;
    }
    if let Some(metadata) = &result.metadata {
        if metadata
            .get("session_switch_reason")
            .is_some_and(|value| value == "fork")
            && metadata
                .get("session_source_id")
                .is_some_and(|value| value == current_session_id)
        {
            return metadata.get("session_switch_to").cloned();
        }
        if metadata
            .get("session_transcript_mutated")
            .is_some_and(|value| value == "true")
        {
            return metadata
                .get("session_id")
                .filter(|session_id| session_id.as_str() == current_session_id)
                .cloned();
        }
    }
    let value = serde_json::from_str::<Value>(&result.value).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("sdk_prompt_recorded") {
        return None;
    }
    value
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|session_id| !session_id.trim().is_empty())
        .filter(|session_id| *session_id == current_session_id)
        .map(str::to_string)
}

fn is_clear_session_command(name: &str, args: &str) -> bool {
    name == "clear" && args.trim().is_empty()
}

fn session_sync_status_message(
    result: &CommandResult,
    session_id: &str,
    message_count: usize,
) -> String {
    if result
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("session_switch_reason"))
        .is_some_and(|value| value == "fork")
    {
        return result.value.replacen("Session forked", "Forked session", 1);
    }
    format!("Session updated\nid: {session_id}\nmessages: {message_count}")
}

fn repl_prompt_options(session_id: &str, cwd: &Path) -> HashMap<String, Value> {
    HashMap::from([
        (
            "session_id".to_string(),
            Value::String(session_id.to_string()),
        ),
        (
            "cwd".to_string(),
            Value::String(cwd.to_string_lossy().to_string()),
        ),
    ])
}

fn tui_session_title(cwd: &Path) -> String {
    let name = cwd
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(".");
    format!("TUI {}", name)
}

fn assistant_text_from_result(result: &Value) -> &str {
    result
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn prompt_text_delta_from_stream_event(event: &StreamEvent) -> Option<String> {
    match event {
        StreamEvent::ContentBlockStart {
            content_block: ContentBlock::Text { text },
            ..
        }
        | StreamEvent::ContentBlockDelta {
            delta: Delta::TextDelta { text },
            ..
        } if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn prompt_usage_from_stream_event(event: &StreamEvent) -> Option<TuiEvent> {
    match event {
        StreamEvent::MessageStart { message } => Some(TuiEvent::PromptStreamUsage {
            model: Some(message.model.clone()),
            input_tokens: Some(u64::from(message.usage.input_tokens)),
            output_tokens: Some(u64::from(message.usage.output_tokens)),
        }),
        StreamEvent::MessageDelta { usage, .. } => Some(TuiEvent::PromptStreamUsage {
            model: None,
            input_tokens: None,
            output_tokens: Some(u64::from(usage.output_tokens)),
        }),
        _ => None,
    }
}

fn prompt_tool_use_from_stream_event(event: &StreamEvent) -> Option<TuiEvent> {
    match event {
        StreamEvent::ContentBlockStart {
            content_block: ContentBlock::ToolUse(tool_use),
            ..
        } => Some(TuiEvent::PromptToolUseStarted {
            id: tool_use.id.clone(),
            name: tool_use.name.clone(),
            input: tool_use.input.clone(),
        }),
        _ => None,
    }
}

fn format_tool_use_message(id: &str, name: &str, workbench: Option<&str>, input: &Value) -> String {
    let mut lines = vec![
        format!("Tool requested: {name}"),
        format!("tool_use_id: {id}"),
    ];
    if let Some(workbench) = non_empty_workbench(workbench) {
        lines.push(format!("workbench: {workbench}"));
    }
    if let Some(summary) = format_tool_input_summary(input) {
        lines.push(format!("input:\n{summary}"));
    }
    lines.join("\n")
}

fn format_tool_result_message(
    is_error: bool,
    workbench: Option<&str>,
    content: &str,
    error: Option<&Value>,
) -> String {
    let status = if is_error { "error" } else { "success" };
    let mut lines = vec![format!("result: {status}")];
    if let Some(workbench) = non_empty_workbench(workbench) {
        lines.push(format!("workbench: {workbench}"));
    }
    let summary = truncate_chars(content.trim().to_string(), 2000);
    if !summary.is_empty() {
        lines.push(summary);
    }
    if let Some(error) = error.filter(|value| !value.is_null()) {
        lines.push("tool error metadata:".to_string());
        lines.push(truncate_for_tui(
            serde_json::to_string_pretty(error)
                .unwrap_or_else(|_| error.to_string())
                .as_str(),
            2000,
        ));
    }
    lines.join("\n")
}

fn non_empty_workbench(workbench: Option<&str>) -> Option<&str> {
    workbench.filter(|value| !value.trim().is_empty())
}

fn tool_message_matches_id(content: &str, id: &str) -> bool {
    let needle = format!("tool_use_id: {id}");
    content.lines().any(|line| line.trim() == needle)
}

fn append_or_replace_tool_result(content: &str, result_message: &str) -> String {
    if let Some(index) = content.find("\nresult: ") {
        return format!("{}\n{}", &content[..index], result_message);
    }
    format!("{content}\n{result_message}")
}

fn format_tool_input_summary(input: &Value) -> Option<String> {
    if input.is_null() || input.as_object().is_some_and(|object| object.is_empty()) {
        return None;
    }
    let text = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
    Some(truncate_chars(text, 1200))
}

fn format_tui_diff_preview(command_name: &str, output: &str) -> Option<String> {
    if command_name != "diff" {
        return None;
    }
    let value: Value = serde_json::from_str(output).ok()?;
    if value.get("schema").and_then(Value::as_str) == Some("kiana.diff.from_checkpoint.v1") {
        return Some(format_checkpoint_diff_preview(&value));
    }
    if value.get("schema").and_then(Value::as_str) == Some("kiana.diff.session_changes.v1") {
        return Some(format_session_changes_diff_preview(&value));
    }
    if value
        .get("inside_git_repo")
        .and_then(Value::as_bool)
        .is_some()
    {
        return Some(format_git_diff_preview(&value));
    }
    None
}

fn format_git_diff_preview(value: &Value) -> String {
    if !value
        .get("inside_git_repo")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return "Diff preview\nNot inside a git repository.".to_string();
    }

    let dirty = value.get("dirty").and_then(Value::as_bool).unwrap_or(false);
    let mut lines = vec![format!(
        "Diff preview\nstatus: {}",
        if dirty { "changes present" } else { "clean" }
    )];
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if files.is_empty() {
        lines.push("files: none".to_string());
    } else {
        lines.push(format!("files: {}", files.len()));
        for file in files.iter().take(20) {
            let path = file
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let index = file.get("index").and_then(Value::as_str).unwrap_or(" ");
            let worktree = file.get("worktree").and_then(Value::as_str).unwrap_or(" ");
            lines.push(format!("- {path} index={index} worktree={worktree}"));
        }
        if files.len() > 20 {
            lines.push(format!("- ... {} more files", files.len() - 20));
        }
    }
    push_diff_stat(&mut lines, "staged", value.get("staged"));
    push_diff_stat(&mut lines, "unstaged", value.get("unstaged"));
    lines.join("\n")
}

fn format_session_changes_diff_preview(value: &Value) -> String {
    let changed = value
        .get("changed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut lines = vec![format!(
        "File changes preview\nstatus: {}",
        if changed { "changes present" } else { "clean" }
    )];
    if let Some(session_id) = value.get("session_id").and_then(Value::as_str) {
        lines.push(format!("session: {session_id}"));
    }
    let file_count = value.get("file_count").and_then(Value::as_u64).unwrap_or(0);
    let change_count = value
        .get("change_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    lines.push(format!("files: {file_count}"));
    lines.push(format!("changes: {change_count}"));

    let files = value
        .get("files")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if files.is_empty() {
        lines.push("file list: none".to_string());
    } else {
        for file in files.iter().take(20) {
            let path = file
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            lines.push(format!("- {path}"));
            if let Some(operations) = json_string_list(file.get("operations")) {
                lines.push(format!("  operations: {operations}"));
            }
            if let Some(sources) = json_string_list(file.get("sources")) {
                lines.push(format!("  sources: {sources}"));
            }
        }
        if files.len() > 20 {
            lines.push(format!("- ... {} more files", files.len() - 20));
        }
    }

    lines.join("\n")
}

fn format_checkpoint_diff_preview(value: &Value) -> String {
    let changed = value
        .get("changed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut lines = vec![format!(
        "Assistant diff preview\nstatus: {}",
        if changed { "changes present" } else { "clean" }
    )];
    if let Some(checkpoint_id) = value
        .get("checkpoint")
        .and_then(|checkpoint| checkpoint.get("id"))
        .and_then(Value::as_str)
    {
        lines.push(format!("checkpoint: {checkpoint_id}"));
    }
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if files.is_empty() {
        lines.push("files: none".to_string());
    } else {
        lines.push(format!("files: {}", files.len()));
        for file in files.iter().take(20) {
            let path = file
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            lines.push(format!("- {path}"));
        }
        if files.len() > 20 {
            lines.push(format!("- ... {} more files", files.len() - 20));
        }
    }
    if let Some(patch) = value
        .get("patch")
        .and_then(Value::as_str)
        .filter(|patch| !patch.trim().is_empty())
    {
        lines.push(String::new());
        lines.push("patch preview:".to_string());
        lines.push(truncate_for_tui(patch, 4000));
    }
    lines.join("\n")
}

fn push_diff_stat(lines: &mut Vec<String>, label: &str, section: Option<&Value>) {
    let Some(stat) = section
        .and_then(|section| section.get("stat"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|stat| !stat.is_empty())
    else {
        return;
    };
    lines.push(String::new());
    lines.push(format!("{label} stat:"));
    lines.push(stat.to_string());
}

fn json_string_list(value: Option<&Value>) -> Option<String> {
    let items = value?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if items.is_empty() {
        None
    } else {
        Some(items.join(", "))
    }
}

fn truncate_chars(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n...");
    truncated
}

fn format_permission_request_message(request: &PermissionPromptRequest) -> String {
    let mut lines = vec![
        format!("Permission requested for {}.", request.tool_name),
        format!("tool_use_id: {}", request.tool_use_id),
    ];
    if let Some(reason) = request
        .decision_reason
        .get("reason")
        .and_then(Value::as_str)
    {
        lines.push(format!("reason: {reason}"));
    }
    if let Some(path) = &request.blocked_path {
        lines.push(format!("path: {path}"));
    }
    lines.push(format!(
        "input: {}",
        truncate_for_tui(
            serde_json::to_string_pretty(&request.input)
                .unwrap_or_else(|_| request.input.to_string())
                .as_str(),
            1200,
        )
    ));
    lines.push("Respond with /allow, /approve, /deny, or /reject.".to_string());
    lines.join("\n")
}

fn permission_panel_from_request(request: &PermissionPromptRequest) -> ReplPermissionPanel {
    ReplPermissionPanel {
        tool_name: request.tool_name.clone(),
        tool_use_id: request.tool_use_id.clone(),
        reason: request
            .decision_reason
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        blocked_path: request.blocked_path.clone(),
        input_preview: permission_value_preview(&request.input),
        suggestions_preview: permission_value_preview(&request.permission_suggestions),
    }
}

fn permission_value_preview(value: &Value) -> Option<String> {
    if value.is_null() || value.as_object().is_some_and(|object| object.is_empty()) {
        return None;
    }
    Some(truncate_for_tui(
        serde_json::to_string_pretty(value)
            .unwrap_or_else(|_| value.to_string())
            .as_str(),
        1200,
    ))
}

fn truncate_for_tui(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n...");
    truncated
}

fn sdk_messages_to_conversation(
    session_id: &str,
    messages: Vec<Value>,
) -> Vec<ConversationMessage> {
    runtime_events_to_conversation(sdk_messages_to_runtime_events(session_id, messages))
}

fn sdk_messages_to_runtime_events(
    session_id: &str,
    messages: Vec<Value>,
) -> Vec<kiana_types::RuntimeEvent> {
    messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            let timestamp = message_timestamp(&message);
            kiana_types::sdk_message_to_runtime_event(
                session_id,
                &format!("turn-{index}"),
                index.checked_sub(1).map(|parent| format!("turn-{parent}")),
                index as u64,
                &timestamp,
                message,
            )
        })
        .collect()
}

fn runtime_events_to_conversation(
    events: Vec<kiana_types::RuntimeEvent>,
) -> Vec<ConversationMessage> {
    events
        .into_iter()
        .flat_map(runtime_event_to_conversation_messages)
        .collect()
}

fn runtime_event_to_conversation_messages(
    event: kiana_types::RuntimeEvent,
) -> Vec<ConversationMessage> {
    let timestamp = event.timestamp;
    match event.payload {
        kiana_types::RuntimeEventPayload::UserMessage(message)
        | kiana_types::RuntimeEventPayload::AssistantMessage(message) => {
            runtime_message_to_conversation_messages(message.message, timestamp)
        }
        kiana_types::RuntimeEventPayload::ToolCall(tool_call) => vec![ConversationMessage {
            role: MessageRole::Tool,
            content: format_tool_use_message(
                &tool_call.tool_call_id,
                &tool_call.name,
                tool_call.workbench.as_deref(),
                &tool_call.input,
            ),
            timestamp,
        }],
        kiana_types::RuntimeEventPayload::ToolResult(tool_result) => vec![ConversationMessage {
            role: MessageRole::Tool,
            content: format!(
                "tool_use_id: {}\n{}",
                tool_result.tool_call_id,
                format_tool_result_message(
                    tool_result.is_error,
                    tool_result.workbench.as_deref(),
                    &message_content_text(&tool_result.content),
                    tool_result.error.as_ref(),
                )
            ),
            timestamp,
        }],
        kiana_types::RuntimeEventPayload::StreamDelta(delta) => {
            let text = runtime_delta_text(&delta.delta);
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![ConversationMessage {
                    role: MessageRole::Assistant,
                    content: text,
                    timestamp,
                }]
            }
        }
        kiana_types::RuntimeEventPayload::PermissionRequest(permission) => {
            vec![ConversationMessage {
                role: MessageRole::System,
                content: format!(
                    "Permission requested for {}.\nrequest_id: {}\naction: {}\ninput: {}",
                    permission.tool_name,
                    permission.request_id,
                    permission.action,
                    serde_json::to_string_pretty(&permission.input)
                        .unwrap_or_else(|_| permission.input.to_string())
                ),
                timestamp,
            }]
        }
        kiana_types::RuntimeEventPayload::SessionEvent(session_event) => {
            if session_event.subtype == "sdk_message" {
                if let Some(message) = session_event.metadata.get("message").cloned() {
                    return runtime_message_to_conversation_messages(message, timestamp);
                }
            }
            vec![ConversationMessage {
                role: MessageRole::System,
                content: session_event
                    .message
                    .unwrap_or_else(|| format!("Session event: {}", session_event.subtype)),
                timestamp,
            }]
        }
        kiana_types::RuntimeEventPayload::Error(error) => vec![ConversationMessage {
            role: MessageRole::System,
            content: format!("Error: {}", error.message),
            timestamp,
        }],
        kiana_types::RuntimeEventPayload::Result(result) => {
            if let Some(content) = result
                .assistant_text
                .as_ref()
                .filter(|text| !text.trim().is_empty())
            {
                return vec![ConversationMessage {
                    role: MessageRole::Assistant,
                    content: content.clone(),
                    timestamp,
                }];
            }
            vec![ConversationMessage {
                role: MessageRole::System,
                content: format_runtime_result_status_message(&result),
                timestamp,
            }]
        }
    }
}

fn format_runtime_result_status_message(result: &kiana_types::RuntimeResultEvent) -> String {
    let mut lines = vec![
        format!("Run result: {}", result.status),
        format!("stop_reason: {}", result.stop_reason),
    ];
    if !result.metadata.is_null()
        && !result
            .metadata
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
    {
        lines.push("metadata:".to_string());
        lines.push(
            serde_json::to_string_pretty(&result.metadata)
                .unwrap_or_else(|_| result.metadata.to_string()),
        );
    }
    lines.join("\n")
}

fn runtime_delta_text(delta: &Value) -> String {
    delta
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| message_content_text(delta))
}

fn runtime_message_to_conversation_messages(
    message: Value,
    event_timestamp: String,
) -> Vec<ConversationMessage> {
    let timestamp = if event_timestamp.is_empty() {
        message_timestamp(&message)
    } else {
        event_timestamp
    };
    let role = message_role(&message);
    let content = message.get("content").unwrap_or(&Value::Null);
    let Some(blocks) = content.as_array() else {
        return vec![ConversationMessage {
            role,
            content: message_content_text(content),
            timestamp,
        }];
    };

    let mut messages = Vec::new();
    for block in blocks {
        let block_role = content_block_role(block).unwrap_or_else(|| role.clone());
        let block_content = conversation_content_block_text(block);
        if block_content.trim().is_empty() {
            continue;
        }
        messages.push(ConversationMessage {
            role: block_role,
            content: block_content,
            timestamp: timestamp.clone(),
        });
    }

    if messages.is_empty() {
        messages.push(ConversationMessage {
            role,
            content: message_content_text(content),
            timestamp,
        });
    }
    messages
}

fn message_timestamp(message: &Value) -> String {
    message
        .get("created_at")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn message_role(message: &Value) -> MessageRole {
    match message.get("role").and_then(Value::as_str) {
        Some("user") if message_contains_tool_result(message) => MessageRole::Tool,
        Some("user") => MessageRole::User,
        Some("assistant") => MessageRole::Assistant,
        Some("tool") => MessageRole::Tool,
        _ => MessageRole::System,
    }
}

fn message_contains_tool_result(message: &Value) -> bool {
    message
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| {
            blocks
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        })
}

fn content_block_role(value: &Value) -> Option<MessageRole> {
    match value.get("type").and_then(Value::as_str) {
        Some("tool_use") | Some("tool_result") => Some(MessageRole::Tool),
        _ => None,
    }
}

fn message_content_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(blocks) = value.as_array() {
        return blocks
            .iter()
            .map(content_block_text)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
    }
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn conversation_content_block_text(value: &Value) -> String {
    match value.get("type").and_then(Value::as_str) {
        Some("tool_use") => format_tool_use_message(
            value.get("id").and_then(Value::as_str).unwrap_or_default(),
            value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            value.get("workbench").and_then(Value::as_str),
            value.get("input").unwrap_or(&Value::Null),
        ),
        Some("tool_result") => {
            let result_message = format_tool_result_message(
                value
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                value.get("workbench").and_then(Value::as_str),
                &message_content_text(value.get("content").unwrap_or(&Value::Null)),
                value.get("error"),
            );
            match value
                .get("tool_use_id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
            {
                Some(id) => format!("tool_use_id: {id}\n{result_message}"),
                None => result_message,
            }
        }
        _ => content_block_text(value),
    }
}

fn content_block_text(value: &Value) -> String {
    match value.get("type").and_then(Value::as_str) {
        Some("text") => value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Some("tool_use") => format!(
            "[tool_use:{}]",
            value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        Some("tool_result") => format!(
            "[tool_result] {}",
            message_content_text(value.get("content").unwrap_or(&Value::Null))
        ),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    };

    fn runtime_for_test(session_id: &str) -> TuiRuntime {
        static HISTORY_COUNTER: AtomicU64 = AtomicU64::new(0);
        let history_id = HISTORY_COUNTER.fetch_add(1, Ordering::SeqCst);
        let (events_tx, events_rx) = unbounded_channel();
        TuiRuntime {
            cwd: PathBuf::from("/tmp/work"),
            session_id: session_id.to_string(),
            app_state: HashMap::new(),
            command_registry: create_default_command_registry(),
            active_prompt_message_index: None,
            active_prompt_tool_workbenches: HashMap::new(),
            active_prompt_abort: None,
            queued_prompts: VecDeque::new(),
            pending_resume_session_id: None,
            pending_permission: None,
            pending_permission_queue: VecDeque::new(),
            onboarding_shown: true,
            prompt_history_path: std::env::temp_dir().join(format!(
                "kiana-tui-test-history-{}-{history_id}.jsonl",
                std::process::id()
            )),
            prompt_history_entries: Vec::new(),
            prompt_history_synced: true,
            events_tx,
            events_rx,
        }
    }

    fn tui_env_lock() -> &'static Mutex<()> {
        crate::test_support::env_lock()
    }

    struct EnvSnapshot {
        values: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvSnapshot {
        fn take(keys: &[&'static str]) -> Self {
            let values = keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect();
            Self { values }
        }
    }

    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    struct ScopedKianaHome {
        root: PathBuf,
        previous: Option<std::ffi::OsString>,
    }

    impl ScopedKianaHome {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "kiana-tui-session-command-{}-{unique}",
                std::process::id()
            ));
            let previous = std::env::var_os("KIANA_HOME");
            std::env::set_var("KIANA_HOME", &root);
            Self { root, previous }
        }

        fn write_session(&self, session_id: &str) {
            self.write_session_with_cwd(session_id, None);
        }

        fn write_session_with_cwd(&self, session_id: &str, cwd: Option<&Path>) {
            let dir = self.root.join("sdk-sessions");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{session_id}.json")),
                serde_json::to_string_pretty(&json!({
                    "session_id": session_id,
                    "title": "TUI Test",
                    "tag": "tui",
                    "parent_session_id": null,
                    "created_at": 1,
                    "updated_at": 2,
                    "cwd": cwd.map(|path| path.to_string_lossy().to_string()),
                    "messages": [{"role": "user", "content": "hello"}]
                }))
                .unwrap(),
            )
            .unwrap();
        }

        fn write_compactable_session(&self, session_id: &str) {
            let dir = self.root.join("sdk-sessions");
            std::fs::create_dir_all(&dir).unwrap();
            let messages = vec![
                json!({"role": "user", "content": "first message ".repeat(80)}),
                json!({"role": "assistant", "content": "middle one ".repeat(80)}),
                json!({"role": "user", "content": "middle two ".repeat(80)}),
                json!({"role": "assistant", "content": "middle three ".repeat(80)}),
                json!({"role": "user", "content": "recent tail"}),
            ];
            std::fs::write(
                dir.join(format!("{session_id}.json")),
                serde_json::to_string_pretty(&json!({
                    "session_id": session_id,
                    "title": "TUI Compact Test",
                    "tag": "tui",
                    "parent_session_id": null,
                    "created_at": 1,
                    "updated_at": 2,
                    "messages": messages
                }))
                .unwrap(),
            )
            .unwrap();
        }
    }

    impl Drop for ScopedKianaHome {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                std::env::set_var("KIANA_HOME", previous);
            } else {
                std::env::remove_var("KIANA_HOME");
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn prompt_history_path_uses_kiana_home() {
        let _guard = tui_env_lock().lock().unwrap();
        let home = ScopedKianaHome::new();

        assert_eq!(
            prompt_history_path(),
            home.root.join(TUI_PROMPT_HISTORY_FILE)
        );
    }

    #[test]
    fn prompt_history_file_records_newest_first_and_dedupes() {
        let _guard = tui_env_lock().lock().unwrap();
        let home = ScopedKianaHome::new();

        record_prompt_history_entry("first prompt").unwrap();
        record_prompt_history_entry("second prompt").unwrap();
        record_prompt_history_entry("first prompt").unwrap();

        let entries = load_prompt_history_entries().unwrap();
        let prompts = entries
            .iter()
            .map(|entry| entry.prompt.as_str())
            .collect::<Vec<_>>();

        assert_eq!(prompts, vec!["first prompt", "second prompt"]);
        assert!(entries.iter().all(|entry| !entry.timestamp.is_empty()));
        assert!(home.root.join(TUI_PROMPT_HISTORY_FILE).exists());
    }

    #[test]
    fn prompt_history_loader_skips_invalid_lines_and_blank_prompts() {
        let _guard = tui_env_lock().lock().unwrap();
        let home = ScopedKianaHome::new();
        let path = home.root.join(TUI_PROMPT_HISTORY_FILE);
        let newest =
            serde_json::to_string(&HistoryEntry::new("keep this".to_string(), "2".to_string()))
                .unwrap();
        let duplicate =
            serde_json::to_string(&HistoryEntry::new("keep this".to_string(), "1".to_string()))
                .unwrap();
        let blank =
            serde_json::to_string(&HistoryEntry::new("   ".to_string(), "0".to_string())).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("not json\n{newest}\n{duplicate}\n{blank}\n")).unwrap();

        let entries = load_prompt_history_entries_from_path(&path).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].prompt, "keep this");
        assert_eq!(entries[0].timestamp, "2");
    }

    #[test]
    fn load_prompt_history_action_populates_picker_and_repl_recall() {
        let _guard = tui_env_lock().lock().unwrap();
        let _home = ScopedKianaHome::new();
        record_prompt_history_entry("older prompt").unwrap();
        record_prompt_history_entry("newer prompt").unwrap();
        let mut runtime = runtime_for_test("session-1");
        runtime.prompt_history_path = prompt_history_path();
        let mut app = App::new();
        app.screen = AppScreen::History;
        app.history.loading = true;

        runtime
            .handle_action(AppAction::LoadPromptHistory, &mut app)
            .unwrap();

        let prompts = app
            .history
            .entries
            .iter()
            .map(|entry| entry.prompt.as_str())
            .collect::<Vec<_>>();
        assert_eq!(prompts, vec!["newer prompt", "older prompt"]);
        assert!(!app.history.loading);
        assert_eq!(app.repl.history, vec!["older prompt", "newer prompt"]);
    }

    #[test]
    fn queue_prompt_action_records_prompt_history() {
        let _guard = tui_env_lock().lock().unwrap();
        let _home = ScopedKianaHome::new();
        let mut runtime = runtime_for_test("session-1");
        runtime.prompt_history_path = prompt_history_path();
        let mut app = App::new();

        runtime
            .handle_action(
                AppAction::QueuePrompt("queued history prompt".to_string()),
                &mut app,
            )
            .unwrap();

        let entries = load_prompt_history_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].prompt, "queued history prompt");
        assert_eq!(
            app.history
                .selected_entry()
                .map(|entry| entry.prompt.as_str()),
            Some("queued history prompt")
        );
    }

    fn permission_request_for_test() -> PermissionPromptRequest {
        PermissionPromptRequest {
            request_id: "perm-1".to_string(),
            tool_name: "TodoWrite".to_string(),
            input: json!({
                "todos": [{
                    "content": "verify TUI permissions",
                    "status": "in_progress",
                    "activeForm": "Verifying TUI permissions"
                }]
            }),
            tool_use_id: "toolu_todo".to_string(),
            permission_suggestions: Value::Null,
            blocked_path: None,
            decision_reason: json!({
                "type": "other",
                "reason": "Tool TodoWrite requires permission in ask mode."
            }),
            agent_id: None,
        }
    }

    async fn drain_until_idle(runtime: &mut TuiRuntime, app: &mut App) {
        for _ in 0..50 {
            runtime.drain_events(app).unwrap();
            if !app.repl.is_loading {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("TUI runtime did not become idle");
    }

    async fn drain_until_doctor_loaded(runtime: &mut TuiRuntime, app: &mut App) {
        for _ in 0..50 {
            runtime.drain_events(app).unwrap();
            if !app.doctor.loading
                && app
                    .doctor
                    .diagnostic_lines
                    .iter()
                    .any(|line| line == "Doctor")
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("TUI doctor diagnostics did not load");
    }

    async fn drain_until_settings_loaded(runtime: &mut TuiRuntime, app: &mut App) {
        for _ in 0..50 {
            runtime.drain_events(app).unwrap();
            if !app.settings.loading
                && app
                    .settings
                    .sections
                    .iter()
                    .any(|section| section.title == "Account/Auth")
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("TUI settings did not load");
    }

    async fn drain_until_resume_entries_loaded(runtime: &mut TuiRuntime, app: &mut App) {
        for _ in 0..50 {
            runtime.drain_events(app).unwrap();
            if !app.resume.loading {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("TUI resume entries did not load");
    }

    async fn assert_prompt_session_refresh_queued(
        runtime: &mut TuiRuntime,
        expected_session_id: &str,
    ) {
        let event =
            tokio::time::timeout(std::time::Duration::from_secs(1), runtime.events_rx.recv())
                .await
                .expect("prompt session refresh was not queued")
                .expect("TUI event channel closed");
        match event {
            TuiEvent::PromptSessionRefreshed { session_id, .. } => {
                assert_eq!(session_id, expected_session_id);
            }
            _ => panic!("expected prompt session refresh event"),
        }
    }

    #[test]
    fn formats_tui_session_title_from_cwd() {
        assert_eq!(
            tui_session_title(Path::new("/tmp/kianacode")),
            "TUI kianacode"
        );
        assert_eq!(tui_session_title(Path::new("/")), "TUI .");
    }

    #[test]
    fn tui_terminal_guard_accepts_interactive_stdio() {
        ensure_tui_terminal(true, true).unwrap();
    }

    #[test]
    fn tui_terminal_guard_rejects_non_interactive_stdio_before_session_setup() {
        let error = ensure_tui_terminal(false, false).unwrap_err().to_string();

        assert!(error.contains("requires an interactive terminal"));
        assert!(error.contains("stdin and stdout"));
    }

    #[test]
    fn tui_terminal_guard_names_non_interactive_stdin() {
        let error = ensure_tui_terminal(false, true).unwrap_err().to_string();

        assert!(error.contains("interactive stdin"));
    }

    #[test]
    fn tui_terminal_guard_names_non_interactive_stdout() {
        let error = ensure_tui_terminal(true, false).unwrap_err().to_string();

        assert!(error.contains("interactive stdout"));
    }

    #[test]
    fn tui_shows_onboarding_when_auth_or_config_missing() {
        let _guard = tui_env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "ANTHROPIC_API_KEY",
            "KIANA_CONFIG_FILE",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_MANAGED_SETTINGS_FILE",
            "KIANA_MANAGED_POLICY_FILE",
            "KIANA_OAUTH_TOKENS_FILE",
            "CLAUDE_CODE_OAUTH_TOKENS_FILE",
            "KIANA_HOME",
        ]);
        let root = std::env::temp_dir().join(format!(
            "kiana-tui-onboarding-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_SETTINGS_JSON");
        std::env::remove_var("KIANA_REMOTE_SETTINGS_FILE");
        std::env::remove_var("KIANA_SETTINGS_FILE");
        std::env::remove_var("KIANA_MANAGED_SETTINGS_FILE");
        std::env::remove_var("KIANA_MANAGED_POLICY_FILE");
        std::env::set_var("KIANA_CONFIG_FILE", root.join("missing-config.toml"));
        std::env::set_var("KIANA_HOME", &root);
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", root.join("missing-oauth.json"));
        std::env::remove_var("CLAUDE_CODE_OAUTH_TOKENS_FILE");
        let mut runtime = runtime_for_test("session-1");
        runtime.onboarding_shown = false;
        let mut app = App::new();

        runtime.drain_events(&mut app).unwrap();
        runtime.drain_events(&mut app).unwrap();

        let onboarding_messages = app
            .repl
            .messages
            .iter()
            .filter(|message| message.content.contains("Onboarding: authentication"))
            .count();
        assert_eq!(onboarding_messages, 1);
        assert!(app.repl.messages[0].content.contains("kiana auth login"));
        assert!(app.repl.messages[0].content.contains("/doctor"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tui_diff_json_result_renders_file_preview() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime.apply_event(
            &mut app,
            TuiEvent::SlashCommandCompleted {
                name: "diff".to_string(),
                command_type: CommandType::Local,
                result: Ok(CommandResult::text(
                    json!({
                        "root": "/repo",
                        "inside_git_repo": true,
                        "dirty": true,
                        "files": [
                            {"path": "src/main.rs", "index": " ", "worktree": "M"}
                        ],
                        "staged": {"changed": false, "stat": ""},
                        "unstaged": {"changed": true, "stat": " src/main.rs | 2 +-"}
                    })
                    .to_string(),
                )),
                session_sync: None,
            },
        );

        let message = &app.repl.messages.last().unwrap().content;
        assert!(message.contains("Diff preview"));
        assert!(message.contains("changes present"));
        assert!(message.contains("src/main.rs"));
        assert!(message.contains("unstaged stat:"));
        assert!(!message.contains("\"inside_git_repo\""));
    }

    #[test]
    fn tui_last_assistant_diff_json_result_renders_patch_preview() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime.apply_event(
            &mut app,
            TuiEvent::SlashCommandCompleted {
                name: "diff".to_string(),
                command_type: CommandType::Local,
                result: Ok(CommandResult::text(
                    json!({
                        "schema": "kiana.diff.from_checkpoint.v1",
                        "changed": true,
                        "checkpoint": {"id": "checkpoint-1"},
                        "files": [{"path": "src/lib.rs"}],
                        "patch": "diff --git a/src/lib.rs b/src/lib.rs\n+new line"
                    })
                    .to_string(),
                )),
                session_sync: None,
            },
        );

        let message = &app.repl.messages.last().unwrap().content;
        assert!(message.contains("Assistant diff preview"));
        assert!(message.contains("checkpoint-1"));
        assert!(message.contains("src/lib.rs"));
        assert!(message.contains("patch preview:"));
        assert!(message.contains("+new line"));
        assert!(!message.contains("\"schema\""));
    }

    #[test]
    fn tui_session_changes_diff_json_result_renders_file_changes_preview() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime.apply_event(
            &mut app,
            TuiEvent::SlashCommandCompleted {
                name: "diff".to_string(),
                command_type: CommandType::Local,
                result: Ok(CommandResult::text(
                    json!({
                        "schema": "kiana.diff.session_changes.v1",
                        "session_id": "session-1",
                        "changed": true,
                        "file_count": 2,
                        "change_count": 3,
                        "files": [
                            {
                                "path": "README.md",
                                "operations": ["update"],
                                "sources": ["Edit"]
                            },
                            {
                                "path": "src/lib.rs",
                                "operations": ["create", "update"],
                                "sources": ["Edit", "Write"]
                            }
                        ]
                    })
                    .to_string(),
                )),
                session_sync: None,
            },
        );

        let message = &app.repl.messages.last().unwrap().content;
        assert!(message.contains("File changes preview"));
        assert!(message.contains("session: session-1"));
        assert!(message.contains("files: 2"));
        assert!(message.contains("changes: 3"));
        assert!(message.contains("README.md"));
        assert!(message.contains("operations: update"));
        assert!(message.contains("src/lib.rs"));
        assert!(message.contains("sources: Edit, Write"));
        assert!(!message.contains("\"schema\""));
    }

    #[test]
    fn extracts_text_from_sdk_content_blocks() {
        let content = json!([
            {"type": "text", "text": "hello"},
            {"type": "tool_use", "name": "Read"},
            {"type": "tool_result", "content": "done"}
        ]);

        assert_eq!(
            message_content_text(&content),
            "hello\n[tool_use:Read]\n[tool_result] done"
        );
    }

    #[test]
    fn extracts_tui_prompt_text_from_stream_events() {
        let start = StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: "hel".to_string(),
            },
        };
        let delta = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta {
                text: "lo".to_string(),
            },
        };
        let tool_delta = StreamEvent::ContentBlockDelta {
            index: 1,
            delta: Delta::InputJsonDelta {
                partial_json: "{\"cmd\"".to_string(),
            },
        };

        assert_eq!(
            prompt_text_delta_from_stream_event(&start),
            Some("hel".to_string())
        );
        assert_eq!(
            prompt_text_delta_from_stream_event(&delta),
            Some("lo".to_string())
        );
        assert_eq!(prompt_text_delta_from_stream_event(&tool_delta), None);
    }

    #[test]
    fn extracts_tui_usage_and_tool_use_from_stream_events() {
        let start = StreamEvent::MessageStart {
            message: kiana_services::api::streaming::MessageStart {
                id: "msg_1".to_string(),
                model: "mock-model".to_string(),
                role: "assistant".to_string(),
                usage: kiana_services::api::streaming::DeltaUsage {
                    input_tokens: 11,
                    output_tokens: 0,
                },
            },
        };
        let delta = StreamEvent::MessageDelta {
            delta: kiana_services::api::streaming::MessageDelta { stop_reason: None },
            usage: kiana_services::api::streaming::DeltaUsage {
                input_tokens: 0,
                output_tokens: 7,
            },
        };
        let tool = StreamEvent::ContentBlockStart {
            index: 1,
            content_block: ContentBlock::ToolUse(kiana_services::api::streaming::ToolUse {
                id: "toolu_1".to_string(),
                name: "Read".to_string(),
                input: json!({}),
            }),
        };

        match prompt_usage_from_stream_event(&start).unwrap() {
            TuiEvent::PromptStreamUsage {
                model,
                input_tokens,
                output_tokens,
            } => {
                assert_eq!(model.as_deref(), Some("mock-model"));
                assert_eq!(input_tokens, Some(11));
                assert_eq!(output_tokens, Some(0));
            }
            _ => panic!("expected prompt usage event"),
        }
        match prompt_usage_from_stream_event(&delta).unwrap() {
            TuiEvent::PromptStreamUsage {
                model,
                input_tokens,
                output_tokens,
            } => {
                assert_eq!(model, None);
                assert_eq!(input_tokens, None);
                assert_eq!(output_tokens, Some(7));
            }
            _ => panic!("expected prompt usage event"),
        }
        match prompt_tool_use_from_stream_event(&tool).unwrap() {
            TuiEvent::PromptToolUseStarted { id, name, input } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "Read");
                assert_eq!(input, json!({}));
            }
            _ => panic!("expected tool use event"),
        }
    }

    #[test]
    fn formats_tui_permission_request_with_tool_context() {
        let message = format_permission_request_message(&permission_request_for_test());

        assert!(message.contains("Permission requested for TodoWrite."));
        assert!(message.contains("tool_use_id: toolu_todo"));
        assert!(message.contains("ask mode"));
        assert!(message.contains("verify TUI permissions"));
        assert!(message.contains("Respond with /allow"));
    }

    #[test]
    fn maps_permission_request_to_structured_tui_panel() {
        let mut request = permission_request_for_test();
        request.blocked_path = Some("src/main.rs".to_string());
        request.permission_suggestions = json!([
            {"decision": "allow", "scope": "once"}
        ]);

        let panel = permission_panel_from_request(&request);

        assert_eq!(panel.tool_name, "TodoWrite");
        assert_eq!(panel.tool_use_id, "toolu_todo");
        assert_eq!(
            panel.reason.as_deref(),
            Some("Tool TodoWrite requires permission in ask mode.")
        );
        assert_eq!(panel.blocked_path.as_deref(), Some("src/main.rs"));
        assert!(panel
            .input_preview
            .as_deref()
            .unwrap()
            .contains("verify TUI permissions"));
        assert!(panel
            .suggestions_preview
            .as_deref()
            .unwrap()
            .contains("allow"));
    }

    #[test]
    fn maps_sdk_messages_to_conversation_messages() {
        let messages = vec![json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "answer"}],
            "created_at": 123
        })];

        let conversation = sdk_messages_to_conversation("session-1", messages);
        assert_eq!(conversation.len(), 1);
        assert_eq!(conversation[0].role, MessageRole::Assistant);
        assert_eq!(conversation[0].content, "answer");
        assert_eq!(conversation[0].timestamp, "123");
    }

    #[test]
    fn maps_runtime_events_to_conversation_messages() {
        let events = vec![
            kiana_types::RuntimeEvent::new(
                "evt-1",
                "session-1",
                "turn-0",
                None,
                0,
                "123",
                kiana_types::RuntimeEventPayload::AssistantMessage(
                    kiana_types::MessageRuntimeEvent {
                        message: json!({
                            "role": "assistant",
                            "content": [{"type": "text", "text": "answer"}],
                            "created_at": 123
                        }),
                    },
                ),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-2",
                "session-1",
                "turn-0",
                None,
                1,
                "124",
                kiana_types::RuntimeEventPayload::ToolCall(kiana_types::RuntimeToolCallEvent {
                    tool_call_id: "toolu_read".to_string(),
                    name: "MCP".to_string(),
                    workbench: Some("mcp".to_string()),
                    input: json!({"file_path": "src/lib.rs"}),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-3",
                "session-1",
                "turn-0",
                None,
                2,
                "125",
                kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                    tool_call_id: "toolu_read".to_string(),
                    name: Some("MCP".to_string()),
                    workbench: Some("mcp".to_string()),
                    is_error: false,
                    content: json!("pub fn main() {}"),
                    changed_files: None,
                    error: None,
                }),
            ),
        ];

        let conversation = runtime_events_to_conversation(events);

        assert_eq!(conversation.len(), 3);
        assert_eq!(conversation[0].role, MessageRole::Assistant);
        assert_eq!(conversation[0].content, "answer");
        assert_eq!(conversation[0].timestamp, "123");
        assert_eq!(conversation[1].role, MessageRole::Tool);
        assert!(conversation[1].content.contains("Tool requested: MCP"));
        assert!(conversation[1].content.contains("tool_use_id: toolu_read"));
        assert!(conversation[1].content.contains("src/lib.rs"));
        assert_eq!(conversation[1].timestamp, "124");
        assert_eq!(conversation[2].role, MessageRole::Tool);
        assert!(conversation[2].content.contains("tool_use_id: toolu_read"));
        assert!(conversation[1].content.contains("workbench: mcp"));
        assert!(conversation[2].content.contains("workbench: mcp"));
        assert!(conversation[2].content.contains("result: success"));
        assert!(conversation[2].content.contains("pub fn main()"));
        assert_eq!(conversation[2].timestamp, "125");
    }

    #[test]
    fn maps_tool_result_error_metadata_to_conversation_messages() {
        let events = vec![kiana_types::RuntimeEvent::new(
            "evt-tool-error",
            "session-1",
            "turn-0",
            None,
            0,
            "126",
            kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                tool_call_id: "toolu_validation".to_string(),
                name: Some("TestValidation".to_string()),
                workbench: Some("local".to_string()),
                is_error: true,
                content: json!("path is required\n\nRepair hint: Provide the required input fields for TestValidation and retry the tool call."),
                changed_files: None,
                error: Some(json!({
                    "type": "tool_error",
                    "code": "tool_validation_error",
                    "tool_name": "TestValidation",
                    "tool_use_id": "toolu_validation",
                    "message": "path is required",
                    "validation_error_code": 42,
                    "repair_hint": "Provide the required input fields for TestValidation and retry the tool call."
                })),
            }),
        )];

        let conversation = runtime_events_to_conversation(events);

        assert_eq!(conversation.len(), 1);
        assert_eq!(conversation[0].role, MessageRole::Tool);
        assert_eq!(conversation[0].timestamp, "126");
        assert!(conversation[0]
            .content
            .contains("tool_use_id: toolu_validation"));
        assert!(conversation[0].content.contains("result: error"));
        assert!(conversation[0].content.contains("workbench: local"));
        assert!(conversation[0].content.contains("tool error metadata:"));
        assert!(conversation[0]
            .content
            .contains("\"code\": \"tool_validation_error\""));
        assert!(conversation[0]
            .content
            .contains("\"validation_error_code\": 42"));
        assert!(conversation[0].content.contains("\"repair_hint\": \"Provide the required input fields for TestValidation and retry the tool call.\""));
    }

    #[test]
    fn maps_full_runtime_event_fixture_to_stable_conversation_view() {
        let events = vec![
            kiana_types::RuntimeEvent::new(
                "evt-1",
                "session-1",
                "turn-0",
                None,
                0,
                "100",
                kiana_types::RuntimeEventPayload::UserMessage(kiana_types::MessageRuntimeEvent {
                    message: json!({
                        "role": "user",
                        "content": "start"
                    }),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-2",
                "session-1",
                "turn-0",
                None,
                1,
                "101",
                kiana_types::RuntimeEventPayload::AssistantMessage(
                    kiana_types::MessageRuntimeEvent {
                        message: json!({
                            "role": "assistant",
                            "content": [{"type": "text", "text": "answer"}]
                        }),
                    },
                ),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-3",
                "session-1",
                "turn-0",
                None,
                2,
                "102",
                kiana_types::RuntimeEventPayload::StreamDelta(
                    kiana_types::RuntimeStreamDeltaEvent {
                        delta: json!({"text": "streaming chunk"}),
                    },
                ),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-4",
                "session-1",
                "turn-0",
                None,
                3,
                "103",
                kiana_types::RuntimeEventPayload::ToolCall(kiana_types::RuntimeToolCallEvent {
                    tool_call_id: "toolu_read".to_string(),
                    name: "Read".to_string(),
                    workbench: Some("local".to_string()),
                    input: json!({"file_path": "src/lib.rs"}),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-5",
                "session-1",
                "turn-0",
                None,
                4,
                "104",
                kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                    tool_call_id: "toolu_read".to_string(),
                    name: Some("Read".to_string()),
                    workbench: Some("local".to_string()),
                    is_error: true,
                    content: json!("permission denied"),
                    changed_files: None,
                    error: None,
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-6",
                "session-1",
                "turn-0",
                None,
                5,
                "105",
                kiana_types::RuntimeEventPayload::PermissionRequest(
                    kiana_types::RuntimePermissionRequestEvent {
                        request_id: "req-1".to_string(),
                        tool_name: "Write".to_string(),
                        action: "ask".to_string(),
                        input: json!({"file_path": "src/main.rs"}),
                        reason: Some("workspace policy".to_string()),
                    },
                ),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-7",
                "session-1",
                "turn-0",
                None,
                6,
                "106",
                kiana_types::RuntimeEventPayload::SessionEvent(kiana_types::RuntimeSessionEvent {
                    subtype: "started".to_string(),
                    message: Some("session started".to_string()),
                    metadata: json!({}),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-8",
                "session-1",
                "turn-0",
                None,
                7,
                "107",
                kiana_types::RuntimeEventPayload::Error(kiana_types::RuntimeErrorEvent {
                    code: Some("provider_error".to_string()),
                    message: "provider failed".to_string(),
                    details: json!({"retryable": false}),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "evt-9",
                "session-1",
                "turn-0",
                None,
                8,
                "108",
                kiana_types::RuntimeEventPayload::Result(kiana_types::RuntimeResultEvent {
                    status: "completed".to_string(),
                    stop_reason: "model_stop".to_string(),
                    assistant_text: Some("final answer".to_string()),
                    metadata: json!({"duration_ms": 12}),
                }),
            ),
        ];

        let conversation = runtime_events_to_conversation(events);

        let expected = vec![
            (MessageRole::User, "start", "100"),
            (MessageRole::Assistant, "answer", "101"),
            (MessageRole::Assistant, "streaming chunk", "102"),
            (
                MessageRole::Tool,
                concat!(
                    "Tool requested: Read\n",
                    "tool_use_id: toolu_read\n",
                    "workbench: local\n",
                    "input:\n",
                    "{\n",
                    "  \"file_path\": \"src/lib.rs\"\n",
                    "}"
                ),
                "103",
            ),
            (
                MessageRole::Tool,
                "tool_use_id: toolu_read\nresult: error\nworkbench: local\npermission denied",
                "104",
            ),
            (
                MessageRole::System,
                concat!(
                    "Permission requested for Write.\n",
                    "request_id: req-1\n",
                    "action: ask\n",
                    "input: {\n",
                    "  \"file_path\": \"src/main.rs\"\n",
                    "}"
                ),
                "105",
            ),
            (MessageRole::System, "session started", "106"),
            (MessageRole::System, "Error: provider failed", "107"),
            (MessageRole::Assistant, "final answer", "108"),
        ];

        assert_eq!(conversation.len(), expected.len());
        for (message, (role, content, timestamp)) in conversation.iter().zip(expected) {
            assert_eq!(message.role, role);
            assert_eq!(message.content, content);
            assert_eq!(message.timestamp, timestamp);
        }
    }

    #[test]
    fn maps_result_event_without_assistant_text_to_terminal_status_message() {
        let events = vec![kiana_types::RuntimeEvent::new(
            "evt-result",
            "session-1",
            "turn-0",
            None,
            0,
            "200",
            kiana_types::RuntimeEventPayload::Result(kiana_types::RuntimeResultEvent {
                status: "max_turns".to_string(),
                stop_reason: "max_turns".to_string(),
                assistant_text: None,
                metadata: json!({"turns": 3}),
            }),
        )];

        let conversation = runtime_events_to_conversation(events);

        assert_eq!(conversation.len(), 1);
        assert_eq!(conversation[0].role, MessageRole::System);
        assert_eq!(conversation[0].timestamp, "200");
        assert!(conversation[0].content.contains("Run result: max_turns"));
        assert!(conversation[0].content.contains("stop_reason: max_turns"));
        assert!(conversation[0].content.contains("\"turns\": 3"));
    }

    #[test]
    fn maps_persisted_tool_result_messages_to_tool_role() {
        let messages = vec![
            json!({
                "role": "user",
                "content": "ordinary prompt",
                "created_at": 122
            }),
            json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_read",
                    "content": "file content"
                }],
                "created_at": 123
            }),
            json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_bash",
                    "is_error": true,
                    "content": "command failed"
                }],
                "created_at": 124
            }),
        ];

        let conversation = sdk_messages_to_conversation("session-1", messages);

        assert_eq!(conversation.len(), 3);
        assert_eq!(conversation[0].role, MessageRole::User);
        assert_eq!(conversation[0].content, "ordinary prompt");
        assert_eq!(conversation[1].role, MessageRole::Tool);
        assert!(conversation[1].content.contains("tool_use_id: toolu_read"));
        assert!(conversation[1].content.contains("result: success"));
        assert!(conversation[1].content.contains("file content"));
        assert_eq!(conversation[2].role, MessageRole::Tool);
        assert!(conversation[2].content.contains("tool_use_id: toolu_bash"));
        assert!(conversation[2].content.contains("result: error"));
        assert!(conversation[2].content.contains("command failed"));
    }

    #[test]
    fn maps_persisted_tool_use_blocks_to_separate_tool_messages() {
        let messages = vec![json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "checking file"},
                {
                    "type": "tool_use",
                    "id": "toolu_read",
                    "name": "Read",
                    "input": {"file_path": "src/lib.rs", "limit": 20}
                }
            ],
            "created_at": 123
        })];

        let conversation = sdk_messages_to_conversation("session-1", messages);

        assert_eq!(conversation.len(), 2);
        assert_eq!(conversation[0].role, MessageRole::Assistant);
        assert_eq!(conversation[0].content, "checking file");
        assert_eq!(conversation[1].role, MessageRole::Tool);
        assert!(conversation[1].content.contains("Tool requested: Read"));
        assert!(conversation[1].content.contains("tool_use_id: toolu_read"));
        assert!(conversation[1].content.contains("file_path"));
        assert!(conversation[1].content.contains("src/lib.rs"));
    }

    #[test]
    fn maps_session_info_to_resume_entry_without_loading_messages() {
        let entry = session_info_to_entry(crate::sdk::SdkSessionInfo {
            session_id: "session-1".to_string(),
            title: None,
            tag: Some("tui".to_string()),
            parent_session_id: None,
            cwd: None,
            created_at: 10,
            updated_at: 20,
            message_count: 42,
            assistant_message_count: 9,
            last_role: Some("assistant".to_string()),
        });

        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.title, "Untitled");
        assert_eq!(entry.timestamp, "20");
        assert_eq!(entry.message_count, 42);
    }

    #[tokio::test]
    async fn load_doctor_action_populates_screen_from_command_output() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.permission_request_active = true;
        app.repl.permission_request_queue_len = 2;

        runtime
            .handle_action(AppAction::LoadDoctor, &mut app)
            .unwrap();

        assert!(app.doctor.loading);

        drain_until_doctor_loaded(&mut runtime, &mut app).await;

        assert!(!app.doctor.loading);
        assert!(app
            .doctor
            .diagnostic_lines
            .iter()
            .any(|line| line.starts_with("cwd: ")));
        assert!(app
            .doctor
            .diagnostic_lines
            .iter()
            .any(|line| line.starts_with("mcp_transport: ")));
        assert!(app
            .doctor
            .diagnostic_lines
            .iter()
            .any(|line| line == "tui_permission_request: active=yes queued=2"));
    }

    #[tokio::test]
    async fn load_settings_action_populates_readiness_hub() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.permission_request_active = true;
        app.repl.permission_request_queue_len = 1;

        runtime
            .handle_action(AppAction::LoadSettings, &mut app)
            .unwrap();

        assert!(app.settings.loading);

        drain_until_settings_loaded(&mut runtime, &mut app).await;

        assert!(!app.settings.loading);
        let section_titles = app
            .settings
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect::<Vec<_>>();
        assert!(section_titles.contains(&"Account/Auth"));
        assert!(section_titles.contains(&"Provider/Model"));
        assert!(section_titles.contains(&"Permissions"));
        assert!(section_titles.contains(&"MCP"));
        assert!(section_titles.contains(&"Remote/Diagnostics"));
        assert!(app
            .settings
            .sections
            .iter()
            .flat_map(|section| section.rows.iter())
            .any(|row| row.label == "anthropic"));
        assert!(app
            .settings
            .sections
            .iter()
            .flat_map(|section| section.rows.iter())
            .any(|row| row.label == "commercial_security"));
    }

    #[test]
    fn unknown_slash_command_reports_locally_without_model_submit() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "missing".to_string(),
                    args: String::new(),
                },
                &mut app,
            )
            .unwrap();

        assert!(!app.repl.is_loading);
        assert_eq!(app.repl.messages.len(), 1);
        assert_eq!(app.repl.messages[0].role, MessageRole::System);
        assert!(app.repl.messages[0]
            .content
            .contains("Unknown command: /missing"));
    }

    #[tokio::test]
    async fn help_slash_command_renders_registry_output_in_tui() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "help".to_string(),
                    args: String::new(),
                },
                &mut app,
            )
            .unwrap();

        drain_until_idle(&mut runtime, &mut app).await;

        assert_eq!(app.repl.messages.len(), 1);
        assert_eq!(app.repl.messages[0].role, MessageRole::System);
        assert!(app.repl.messages[0]
            .content
            .contains("Kiana local commands"));
        assert!(app.repl.messages[0].content.contains("/session"));
        assert!(app.repl.messages[0].content.contains("kiana --help"));
        assert!(!app.repl.messages[0].content.contains("Unknown command"));
    }

    #[tokio::test]
    async fn exit_slash_command_marks_tui_for_shutdown() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "quit".to_string(),
                    args: String::new(),
                },
                &mut app,
            )
            .unwrap();

        drain_until_idle(&mut runtime, &mut app).await;

        assert!(app.should_quit);
        assert_eq!(app.repl.messages[0].content, "Goodbye!");
    }

    #[tokio::test]
    async fn session_reply_record_only_refreshes_tui_transcript_for_current_session() {
        let _guard = tui_env_lock().lock().unwrap();
        let home = ScopedKianaHome::new();
        home.write_session("session-tui-reply");
        let mut runtime = runtime_for_test("session-tui-reply");
        let mut app = App::new();

        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "session".to_string(),
                    args: "reply current --record-only follow up from tui".to_string(),
                },
                &mut app,
            )
            .unwrap();

        drain_until_idle(&mut runtime, &mut app).await;

        assert!(app.repl.messages.iter().any(|message| {
            message.role == MessageRole::User && message.content == "follow up from tui"
        }));
        assert!(!app
            .repl
            .messages
            .iter()
            .any(|message| message.content.contains("sdk_prompt_recorded")));
    }

    #[tokio::test]
    async fn session_compact_refreshes_tui_transcript_for_current_session() {
        let _guard = tui_env_lock().lock().unwrap();
        let home = ScopedKianaHome::new();
        home.write_compactable_session("session-tui-compact");
        let mut runtime = runtime_for_test("session-tui-compact");
        let mut app = App::new();

        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "session".to_string(),
                    args: "compact current --threshold-tokens 10 --target-tokens 80".to_string(),
                },
                &mut app,
            )
            .unwrap();

        drain_until_idle(&mut runtime, &mut app).await;

        assert!(app.repl.messages.len() < 5);
        assert!(app
            .repl
            .messages
            .iter()
            .any(|message| message.content.contains("[Compacted conversation summary]")));
        assert!(app
            .repl
            .messages
            .iter()
            .any(|message| message.role == MessageRole::User && message.content == "recent tail"));
        assert!(!app
            .repl
            .messages
            .iter()
            .any(|message| message.content.contains("Session compacted")));
    }

    #[tokio::test]
    async fn session_fork_switches_tui_to_forked_session() {
        let _guard = tui_env_lock().lock().unwrap();
        let home = ScopedKianaHome::new();
        home.write_session("session-tui-fork");
        let mut runtime = runtime_for_test("session-tui-fork");
        let mut app = App::new();

        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "session".to_string(),
                    args: "fork current".to_string(),
                },
                &mut app,
            )
            .unwrap();

        drain_until_idle(&mut runtime, &mut app).await;

        assert_ne!(runtime.session_id, "session-tui-fork");
        assert!(app
            .repl
            .messages
            .iter()
            .any(|message| message.role == MessageRole::User && message.content == "hello"));
        assert!(app.repl.messages.iter().any(|message| {
            message.role == MessageRole::System
                && message.content.contains("Forked session")
                && message.content.contains("source: session-tui-fork")
                && message.content.contains(&runtime.session_id)
        }));
    }

    #[tokio::test]
    async fn applies_prompt_completed_event_without_blocking_ui_state() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;

        runtime.apply_event(
            &mut app,
            TuiEvent::PromptCompleted(Ok("answer".to_string())),
        );

        assert!(!app.repl.is_loading);
        assert_eq!(app.repl.messages.len(), 1);
        assert_eq!(app.repl.messages[0].role, MessageRole::Assistant);
        assert_eq!(app.repl.messages[0].content, "answer");
        assert_prompt_session_refresh_queued(&mut runtime, "session-1").await;
    }

    #[test]
    fn prompt_stream_delta_creates_and_updates_active_assistant_message() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl
            .push_message(MessageRole::User, "question".to_string());
        app.repl.is_loading = true;

        runtime.apply_event(&mut app, TuiEvent::PromptStreamDelta("hel".to_string()));
        runtime.apply_event(&mut app, TuiEvent::PromptStreamDelta("lo".to_string()));

        assert_eq!(app.repl.messages.len(), 2);
        assert_eq!(app.repl.messages[1].role, MessageRole::Assistant);
        assert_eq!(app.repl.messages[1].content, "hello");
        assert_eq!(runtime.active_prompt_message_index, Some(1));
    }

    #[tokio::test]
    async fn prompt_completed_updates_streamed_message_without_duplication() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl
            .push_message(MessageRole::User, "question".to_string());
        app.repl.is_loading = true;

        runtime.apply_event(&mut app, TuiEvent::PromptStreamDelta("draft".to_string()));
        runtime.apply_event(
            &mut app,
            TuiEvent::PromptCompleted(Ok("final answer".to_string())),
        );

        assert!(!app.repl.is_loading);
        assert_eq!(app.repl.messages.len(), 2);
        assert_eq!(app.repl.messages[1].role, MessageRole::Assistant);
        assert_eq!(app.repl.messages[1].content, "final answer");
        assert_eq!(runtime.active_prompt_message_index, None);
        assert_prompt_session_refresh_queued(&mut runtime, "session-1").await;
    }

    #[test]
    fn prompt_session_refresh_replaces_transcript_without_sync_noise() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl
            .push_message(MessageRole::Tool, "Tool requested: Read".to_string());

        runtime.apply_event(
            &mut app,
            TuiEvent::PromptSessionRefreshed {
                session_id: "session-1".to_string(),
                result: Ok(ResumedSession {
                    session_id: "session-1".to_string(),
                    title: "Recovered".to_string(),
                    messages: vec![
                        ConversationMessage {
                            role: MessageRole::User,
                            content: "question".to_string(),
                            timestamp: "1".to_string(),
                        },
                        ConversationMessage {
                            role: MessageRole::Assistant,
                            content: "answer".to_string(),
                            timestamp: "2".to_string(),
                        },
                    ],
                }),
            },
        );

        assert_eq!(app.repl.messages.len(), 2);
        assert_eq!(app.repl.messages[0].role, MessageRole::User);
        assert_eq!(app.repl.messages[1].role, MessageRole::Assistant);
        assert!(!app
            .repl
            .messages
            .iter()
            .any(|message| message.content.contains("Synced session")));
    }

    #[test]
    fn prompt_stream_usage_updates_repl_status_and_tool_message() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;
        runtime.apply_event(
            &mut app,
            TuiEvent::PromptStreamUsage {
                model: Some("mock-model".to_string()),
                input_tokens: Some(12),
                output_tokens: Some(3),
            },
        );
        runtime.apply_event(
            &mut app,
            TuiEvent::PromptToolUseStarted {
                id: "toolu_read".to_string(),
                name: "Read".to_string(),
                input: json!({}),
            },
        );

        assert_eq!(app.repl.model_name, "mock-model");
        assert_eq!(app.repl.input_tokens, 12);
        assert_eq!(app.repl.output_tokens, 3);
        assert_eq!(app.repl.messages.len(), 1);
        assert_eq!(app.repl.messages[0].role, MessageRole::Tool);
        assert!(app.repl.messages[0]
            .content
            .contains("Tool requested: Read"));
        assert!(app.repl.messages[0].content.contains("toolu_read"));
    }

    #[test]
    fn prompt_tool_use_message_includes_input_summary() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        let event = StreamEvent::ContentBlockStart {
            index: 1,
            content_block: ContentBlock::ToolUse(kiana_services::api::streaming::ToolUse {
                id: "toolu_read".to_string(),
                name: "Read".to_string(),
                input: json!({
                    "file_path": "src/lib.rs",
                    "limit": 20
                }),
            }),
        };

        runtime.apply_event(&mut app, prompt_tool_use_from_stream_event(&event).unwrap());

        assert_eq!(app.repl.messages.len(), 1);
        assert_eq!(app.repl.messages[0].role, MessageRole::Tool);
        assert!(app.repl.messages[0]
            .content
            .contains("Tool requested: Read"));
        assert!(app.repl.messages[0].content.contains("toolu_read"));
        assert!(app.repl.messages[0].content.contains("file_path"));
        assert!(app.repl.messages[0].content.contains("src/lib.rs"));
        assert!(app.repl.messages[0].content.contains("limit"));
        assert!(app.repl.messages[0].content.contains("20"));
    }

    #[test]
    fn prompt_tool_result_updates_matching_tool_message() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime.apply_event(
            &mut app,
            TuiEvent::PromptToolUseStarted {
                id: "toolu_read".to_string(),
                name: "Read".to_string(),
                input: json!({ "file_path": "src/lib.rs" }),
            },
        );
        runtime.apply_event(
            &mut app,
            TuiEvent::PromptToolResult {
                id: "toolu_read".to_string(),
                is_error: false,
                content: "pub fn main() {}".to_string(),
            },
        );

        assert_eq!(app.repl.messages.len(), 1);
        assert_eq!(app.repl.messages[0].role, MessageRole::Tool);
        assert!(app.repl.messages[0]
            .content
            .contains("Tool requested: Read"));
        assert!(app.repl.messages[0].content.contains("toolu_read"));
        assert!(app.repl.messages[0].content.contains("result: success"));
        assert!(app.repl.messages[0].content.contains("pub fn main()"));
    }

    #[test]
    fn prompt_tool_lifecycle_messages_include_live_workbench_metadata() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        let event = StreamEvent::ContentBlockStart {
            index: 1,
            content_block: ContentBlock::ToolUse(kiana_services::api::streaming::ToolUse {
                id: "toolu_mcp".to_string(),
                name: "MCP".to_string(),
                input: json!({
                    "server_name": "filesystem",
                    "tool_name": "read_file"
                }),
            }),
        };

        runtime.apply_event(&mut app, prompt_tool_use_from_stream_event(&event).unwrap());
        runtime.apply_event(
            &mut app,
            TuiEvent::PromptToolResult {
                id: "toolu_mcp".to_string(),
                is_error: false,
                content: "read ok".to_string(),
            },
        );

        let message = &app.repl.messages[0].content;
        assert!(message.contains("Tool requested: MCP"));
        assert!(message.contains("result: success"));
        assert_eq!(message.matches("workbench: mcp").count(), 2);
    }

    #[tokio::test]
    async fn permission_request_can_be_approved_from_tui_slash_command() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;
        runtime.active_prompt_message_index = Some(0);
        let (respond_to, response_rx) = oneshot::channel();

        runtime.apply_event(
            &mut app,
            TuiEvent::PermissionRequested {
                request: permission_request_for_test(),
                respond_to,
            },
        );

        assert_eq!(runtime.active_prompt_message_index, None);
        assert!(runtime.pending_permission.is_some());
        assert!(app.repl.permission_request_active);
        assert_eq!(
            app.repl
                .permission_panel
                .as_ref()
                .map(|panel| panel.tool_name.as_str()),
            Some("TodoWrite")
        );
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("Permission requested for TodoWrite."));

        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "allow".to_string(),
                    args: String::new(),
                },
                &mut app,
            )
            .unwrap();

        assert!(runtime.pending_permission.is_none());
        assert_eq!(response_rx.await.unwrap(), PermissionPromptDecision::Allow);
        assert!(!app.repl.permission_request_active);
        assert!(app.repl.permission_panel.is_none());
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("Approved TodoWrite."));
    }

    #[tokio::test]
    async fn permission_request_can_be_denied_from_tui_slash_command() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;
        let (respond_to, response_rx) = oneshot::channel();

        runtime.apply_event(
            &mut app,
            TuiEvent::PermissionRequested {
                request: permission_request_for_test(),
                respond_to,
            },
        );
        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "deny".to_string(),
                    args: "not now".to_string(),
                },
                &mut app,
            )
            .unwrap();

        assert_eq!(
            response_rx.await.unwrap(),
            PermissionPromptDecision::Deny("not now".to_string())
        );
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("Denied TodoWrite."));
    }

    #[tokio::test]
    async fn permission_requests_are_processed_fifo_instead_of_replacing_active_prompt() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;
        let (first_respond_to, mut first_rx) = oneshot::channel();
        let (second_respond_to, second_rx) = oneshot::channel();
        let mut second_request = permission_request_for_test();
        second_request.request_id = "perm-2".to_string();
        second_request.tool_name = "Bash".to_string();
        second_request.tool_use_id = "toolu_bash".to_string();
        second_request.input = json!({"command": "git status"});
        second_request.decision_reason = json!({
            "type": "other",
            "reason": "Tool Bash requires permission in ask mode."
        });

        runtime.apply_event(
            &mut app,
            TuiEvent::PermissionRequested {
                request: permission_request_for_test(),
                respond_to: first_respond_to,
            },
        );
        runtime.apply_event(
            &mut app,
            TuiEvent::PermissionRequested {
                request: second_request,
                respond_to: second_respond_to,
            },
        );

        assert!(first_rx.try_recv().is_err());
        assert_eq!(app.repl.permission_request_queue_len, 1);
        assert_eq!(
            app.repl
                .permission_panel
                .as_ref()
                .map(|panel| panel.tool_name.as_str()),
            Some("TodoWrite")
        );
        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "allow".to_string(),
                    args: String::new(),
                },
                &mut app,
            )
            .unwrap();

        assert_eq!(first_rx.await.unwrap(), PermissionPromptDecision::Allow);
        assert_eq!(app.repl.permission_request_queue_len, 0);
        assert_eq!(
            app.repl
                .permission_panel
                .as_ref()
                .map(|panel| panel.tool_name.as_str()),
            Some("Bash")
        );
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("Permission requested for Bash."));

        runtime
            .handle_action(
                AppAction::RunSlashCommand {
                    name: "deny".to_string(),
                    args: "not now".to_string(),
                },
                &mut app,
            )
            .unwrap();

        assert_eq!(
            second_rx.await.unwrap(),
            PermissionPromptDecision::Deny("not now".to_string())
        );
        assert!(runtime.pending_permission.is_none());
        assert!(app.repl.permission_panel.is_none());
    }

    #[test]
    fn queue_prompt_stores_prompts_in_fifo_order_and_reports_status() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime
            .handle_action(AppAction::QueuePrompt("first queued".to_string()), &mut app)
            .unwrap();
        runtime
            .handle_action(
                AppAction::QueuePrompt("second queued".to_string()),
                &mut app,
            )
            .unwrap();

        let queued = runtime
            .queued_prompts
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(queued, vec!["first queued", "second queued"]);
        assert_eq!(app.repl.messages.len(), 2);
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("Queued prompt"));
    }

    #[test]
    fn command_app_state_includes_tui_permission_queue_state() {
        let runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.permission_request_active = true;
        app.repl.permission_request_queue_len = 2;

        let app_state = runtime.command_app_state(&app);

        assert_eq!(
            app_state.get("tui_permission_request_active"),
            Some(&json!(true))
        );
        assert_eq!(
            app_state.get("tui_permission_request_queue_len"),
            Some(&json!(2))
        );
    }

    #[tokio::test]
    async fn prompt_session_refresh_starts_queued_prompt_after_transcript_sync() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        runtime.queued_prompts.push_back("follow up".to_string());

        runtime.apply_event(
            &mut app,
            TuiEvent::PromptSessionRefreshed {
                session_id: "session-1".to_string(),
                result: Ok(ResumedSession {
                    session_id: "session-1".to_string(),
                    title: "Recovered".to_string(),
                    messages: vec![ConversationMessage {
                        role: MessageRole::Assistant,
                        content: "answer".to_string(),
                        timestamp: "1".to_string(),
                    }],
                }),
            },
        );

        assert!(runtime.queued_prompts.is_empty());
        assert!(runtime.active_prompt_abort.is_some());
        assert!(app.repl.is_loading);
        assert_eq!(app.repl.messages.len(), 2);
        assert_eq!(app.repl.messages[0].content, "answer");
        assert_eq!(app.repl.messages[1].role, MessageRole::User);
        assert_eq!(app.repl.messages[1].content, "follow up");
    }

    #[tokio::test]
    async fn prompt_queue_preserves_multiple_prompts_fifo() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();

        runtime
            .handle_action(
                AppAction::QueuePrompt("first follow up".to_string()),
                &mut app,
            )
            .unwrap();
        runtime
            .handle_action(
                AppAction::QueuePrompt("second follow up".to_string()),
                &mut app,
            )
            .unwrap();

        runtime.apply_event(
            &mut app,
            TuiEvent::PromptSessionRefreshed {
                session_id: "session-1".to_string(),
                result: Ok(ResumedSession {
                    session_id: "session-1".to_string(),
                    title: "Recovered".to_string(),
                    messages: vec![ConversationMessage {
                        role: MessageRole::Assistant,
                        content: "answer".to_string(),
                        timestamp: "1".to_string(),
                    }],
                }),
            },
        );

        assert_eq!(app.repl.messages.len(), 2);
        assert_eq!(app.repl.messages[1].role, MessageRole::User);
        assert_eq!(app.repl.messages[1].content, "first follow up");
        assert_eq!(
            runtime.queued_prompts.front().map(String::as_str),
            Some("second follow up")
        );
    }

    #[tokio::test]
    async fn cancel_prompt_sends_abort_signal_and_denies_pending_permission() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;
        runtime.active_prompt_message_index = Some(0);
        runtime
            .queued_prompts
            .push_back("queued followup".to_string());
        let (abort_tx, abort_rx) = watch::channel(false);
        runtime.active_prompt_abort = Some(abort_tx);
        let (respond_to, response_rx) = oneshot::channel();
        runtime.pending_permission = Some(PendingPermission {
            request: permission_request_for_test(),
            respond_to,
        });
        let (queued_respond_to, queued_response_rx) = oneshot::channel();
        runtime
            .pending_permission_queue
            .push_back(PendingPermission {
                request: permission_request_for_test(),
                respond_to: queued_respond_to,
            });

        runtime
            .handle_action(AppAction::CancelPrompt, &mut app)
            .unwrap();

        assert!(*abort_rx.borrow());
        assert!(runtime.active_prompt_abort.is_none());
        assert_eq!(runtime.active_prompt_message_index, None);
        assert!(runtime.queued_prompts.is_empty());
        assert!(runtime.pending_permission.is_none());
        assert!(runtime.pending_permission_queue.is_empty());
        assert!(app.repl.permission_panel.is_none());
        assert_eq!(
            response_rx.await.unwrap(),
            PermissionPromptDecision::Deny("Prompt cancelled by user.".to_string())
        );
        assert_eq!(
            queued_response_rx.await.unwrap(),
            PermissionPromptDecision::Deny("Prompt cancelled by user.".to_string())
        );
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("clearing queued prompt."));
    }

    #[test]
    fn cancelled_prompt_completion_reports_cancelled_instead_of_failed() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;

        runtime.apply_event(
            &mut app,
            TuiEvent::PromptCompleted(Err("assistant turn cancelled".to_string())),
        );

        assert!(!app.repl.is_loading);
        assert!(runtime.active_prompt_abort.is_none());
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("Prompt cancelled."));
    }

    #[test]
    fn tui_clear_session_command_requires_no_args() {
        assert!(is_clear_session_command("clear", ""));
        assert!(is_clear_session_command("clear", "   "));
        assert!(!is_clear_session_command("clear", "status"));
        assert!(!is_clear_session_command("clear", "--help"));
        assert!(!is_clear_session_command("status", ""));
    }

    #[test]
    fn applies_resume_entries_event_to_resume_screen() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.screen = AppScreen::ResumeConversation;
        app.resume.loading = true;

        runtime.apply_event(
            &mut app,
            TuiEvent::ResumeEntriesLoaded(Ok(vec![SessionEntry {
                session_id: "session-2".to_string(),
                title: "Existing session".to_string(),
                timestamp: "10".to_string(),
                message_count: 2,
            }])),
        );

        assert!(!app.resume.loading);
        assert_eq!(app.resume.sessions.len(), 1);
        assert_eq!(app.resume.sessions[0].session_id, "session-2");
    }

    #[tokio::test]
    async fn load_resume_sessions_filters_to_tui_cwd() {
        let _guard = tui_env_lock().lock().unwrap();
        let home = ScopedKianaHome::new();
        let current_project = home.root.join("current-project");
        let other_project = home.root.join("other-project");
        std::fs::create_dir_all(&current_project).unwrap();
        std::fs::create_dir_all(&other_project).unwrap();
        home.write_session_with_cwd("session-current-project", Some(&current_project));
        home.write_session_with_cwd("session-other-project", Some(&other_project));
        let mut runtime = runtime_for_test("session-current-project");
        runtime.cwd = current_project;
        let mut app = App::new();
        app.screen = AppScreen::ResumeConversation;

        runtime
            .handle_action(AppAction::LoadResumeSessions, &mut app)
            .unwrap();

        drain_until_resume_entries_loaded(&mut runtime, &mut app).await;

        let session_ids = app
            .resume
            .sessions
            .iter()
            .map(|entry| entry.session_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(session_ids, vec!["session-current-project"]);
    }

    #[test]
    fn resume_entries_error_returns_to_repl_with_visible_message() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.screen = AppScreen::ResumeConversation;
        app.resume.loading = true;

        runtime.apply_event(
            &mut app,
            TuiEvent::ResumeEntriesLoaded(Err("session store unavailable".to_string())),
        );

        assert!(!app.resume.loading);
        assert_eq!(app.screen, AppScreen::Repl);
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("Failed to load sessions: session store unavailable"));
    }

    #[test]
    fn applies_session_resumed_event_replaces_runtime_session() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;
        runtime.pending_resume_session_id = Some("session-2".to_string());
        app.repl.push_message(MessageRole::User, "old".to_string());

        runtime.apply_event(
            &mut app,
            TuiEvent::SessionResumed {
                requested_session_id: "session-2".to_string(),
                result: Ok(ResumedSession {
                    session_id: "session-2".to_string(),
                    title: "Recovered".to_string(),
                    messages: vec![ConversationMessage {
                        role: MessageRole::Assistant,
                        content: "existing answer".to_string(),
                        timestamp: "123".to_string(),
                    }],
                }),
            },
        );

        assert!(!app.repl.is_loading);
        assert!(runtime.pending_resume_session_id.is_none());
        assert_eq!(runtime.session_id, "session-2");
        assert_eq!(app.repl.messages[0].content, "existing answer");
        assert_eq!(app.repl.messages.last().unwrap().role, MessageRole::System);
        assert!(app
            .repl
            .messages
            .last()
            .unwrap()
            .content
            .contains("Resumed session session-2"));
    }

    #[test]
    fn stale_session_resumed_event_is_ignored() {
        let mut runtime = runtime_for_test("session-1");
        let mut app = App::new();
        app.repl.is_loading = true;
        runtime.pending_resume_session_id = Some("session-3".to_string());
        app.repl
            .push_message(MessageRole::User, "current".to_string());

        runtime.apply_event(
            &mut app,
            TuiEvent::SessionResumed {
                requested_session_id: "session-2".to_string(),
                result: Ok(ResumedSession {
                    session_id: "session-2".to_string(),
                    title: "Stale".to_string(),
                    messages: vec![ConversationMessage {
                        role: MessageRole::Assistant,
                        content: "stale answer".to_string(),
                        timestamp: "123".to_string(),
                    }],
                }),
            },
        );

        assert!(app.repl.is_loading);
        assert_eq!(runtime.session_id, "session-1");
        assert_eq!(
            runtime.pending_resume_session_id.as_deref(),
            Some("session-3")
        );
        assert_eq!(app.repl.messages[0].content, "current");
    }
}
