/// Top-level App state and screen router.
///
/// Mirrors the React AppState / screen-switching in the TypeScript reference.
/// Each variant holds the state needed by that screen.
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

use crate::doctor::{DoctorScreen, DoctorState};
use crate::history::{HistoryScreen, HistoryState};
use crate::repl::{ReplScreen, ReplState};
use crate::resume_conversation::{ResumeScreen, ResumeState};
use crate::settings::{SettingsScreen, SettingsState};

/// Which screen is currently active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppScreen {
    /// Interactive REPL (main conversation loop).
    Repl,
    /// Doctor/diagnostics screen (`/doctor`).
    Doctor,
    /// Session-resume picker screen.
    ResumeConversation,
    /// Prompt history picker screen (`/history`).
    History,
    /// Read-only settings and readiness hub (`/settings`).
    Settings,
}

/// Actions emitted by the UI that must be handled by the application shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    LoadDoctor,
    LoadResumeSessions,
    LoadPromptHistory,
    LoadSettings,
    SubmitPrompt(String),
    QueuePrompt(String),
    CancelPrompt,
    RunSlashCommand { name: String, args: String },
    ResumeSession(String),
}

/// Root application state.  Holds the active screen and its data.
pub struct App {
    pub screen: AppScreen,
    pub repl: ReplState,
    pub doctor: DoctorState,
    pub resume: ResumeState,
    pub history: HistoryState,
    pub settings: SettingsState,
    pub should_quit: bool,
    actions: Vec<AppAction>,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: AppScreen::Repl,
            repl: ReplState::default(),
            doctor: DoctorState::default(),
            resume: ResumeState::default(),
            history: HistoryState::default(),
            settings: SettingsState::default(),
            should_quit: false,
            actions: Vec::new(),
        }
    }

    /// Route a key event to the active screen.
    pub fn handle_key(&mut self, key: event::KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.screen == AppScreen::Repl && self.repl.permission_request_active {
                match key.code {
                    KeyCode::Char('y') => {
                        self.actions.push(AppAction::RunSlashCommand {
                            name: "allow".to_string(),
                            args: String::new(),
                        });
                        return;
                    }
                    KeyCode::Char('n') => {
                        self.actions.push(AppAction::RunSlashCommand {
                            name: "deny".to_string(),
                            args: String::new(),
                        });
                        return;
                    }
                    _ => {}
                }
            }
            if key.code == KeyCode::Char('c')
                && self.screen == AppScreen::Repl
                && self.repl.is_loading
            {
                self.actions.push(AppAction::CancelPrompt);
                return;
            }
            if key.code == KeyCode::Char('c') || key.code == KeyCode::Char('d') {
                self.should_quit = true;
                return;
            }
        }

        match self.screen {
            AppScreen::Repl => {
                if let Some(event) = self.repl.handle_key(key) {
                    match event {
                        crate::repl::ReplEvent::SwitchScreen(next) => {
                            if next == AppScreen::Doctor {
                                self.actions.push(AppAction::LoadDoctor);
                            }
                            if next == AppScreen::ResumeConversation {
                                self.actions.push(AppAction::LoadResumeSessions);
                            }
                            if next == AppScreen::History {
                                self.history.reset_search();
                                self.history.loading = true;
                                self.actions.push(AppAction::LoadPromptHistory);
                            }
                            if next == AppScreen::Settings {
                                self.settings.set_loading();
                                self.actions.push(AppAction::LoadSettings);
                            }
                            self.screen = next;
                        }
                        crate::repl::ReplEvent::SubmitPrompt(prompt) => {
                            self.actions.push(AppAction::SubmitPrompt(prompt));
                        }
                        crate::repl::ReplEvent::QueuePrompt(prompt) => {
                            self.actions.push(AppAction::QueuePrompt(prompt));
                        }
                        crate::repl::ReplEvent::RunSlashCommand { name, args } => {
                            self.actions.push(AppAction::RunSlashCommand { name, args });
                        }
                    }
                }
            }
            AppScreen::Doctor => {
                if self.doctor.handle_key(key) {
                    // Enter / Esc → back to REPL
                    self.screen = AppScreen::Repl;
                }
            }
            AppScreen::ResumeConversation => {
                if let Some(event) = self.resume.handle_key(key) {
                    match event {
                        crate::resume_conversation::ResumeEvent::SwitchScreen(next) => {
                            self.screen = next;
                        }
                        crate::resume_conversation::ResumeEvent::ResumeSession(session_id) => {
                            self.actions.push(AppAction::ResumeSession(session_id));
                            self.screen = AppScreen::Repl;
                        }
                    }
                }
            }
            AppScreen::History => {
                if let Some(event) = self.history.handle_key(key) {
                    match event {
                        crate::history::HistoryEvent::SwitchScreen(next) => {
                            self.screen = next;
                        }
                        crate::history::HistoryEvent::RestorePrompt(prompt) => {
                            self.repl.restore_draft(prompt);
                            self.screen = AppScreen::Repl;
                        }
                    }
                }
            }
            AppScreen::Settings => {
                if self.settings.handle_key(key) {
                    self.screen = AppScreen::Repl;
                }
            }
        }
    }

    pub fn take_actions(&mut self) -> Vec<AppAction> {
        std::mem::take(&mut self.actions)
    }

    /// Draw the current screen.
    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        match self.screen {
            AppScreen::Repl => ReplScreen::draw(frame, &mut self.repl),
            AppScreen::Doctor => DoctorScreen::draw(frame, &self.doctor),
            AppScreen::ResumeConversation => ResumeScreen::draw(frame, &mut self.resume),
            AppScreen::History => HistoryScreen::draw(frame, &mut self.history),
            AppScreen::Settings => SettingsScreen::draw(frame, &mut self.settings),
        }
    }

    pub fn tick(&mut self) {
        if self.repl.is_loading {
            self.repl.tick_spinner();
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialise the terminal, run the event loop, and restore the terminal on exit.
pub fn run_app() -> Result<()> {
    run_app_with_action_handler(|_, _| Ok(()))
}

/// Initialise the terminal, run the event loop, and pass emitted UI actions to
/// the application shell.
pub fn run_app_with_action_handler<H>(mut action_handler: H) -> Result<()>
where
    H: FnMut(AppAction, &mut App) -> Result<()>,
{
    run_app_with_handlers(|action, app| action_handler(action, app), |_| Ok(()))
}

pub fn run_app_with_handlers<H, T>(mut action_handler: H, mut tick_handler: T) -> Result<()>
where
    H: FnMut(AppAction, &mut App) -> Result<()>,
    T: FnMut(&mut App) -> Result<()>,
{
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut app = App::new();

    loop {
        app.tick();
        tick_handler(&mut app)?;
        terminal.draw(|f| app.draw(f))?;

        if event::poll(std::time::Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
                for action in app.take_actions() {
                    action_handler(action, &mut app)?;
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryEntry;
    use crate::resume_conversation::SessionEntry;

    fn key(code: KeyCode) -> event::KeyEvent {
        event::KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn repl_enter_emits_submit_prompt_action() {
        let mut app = App::new();
        app.repl.input = "inspect workspace".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::SubmitPrompt("inspect workspace".to_string())]
        );
        assert_eq!(app.repl.messages.len(), 1);
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_enter_while_loading_does_not_submit_again() {
        let mut app = App::new();
        app.repl.is_loading = true;
        app.repl.input = "second prompt".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::QueuePrompt("second prompt".to_string())]
        );
        assert_eq!(app.repl.messages.len(), 0);
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_enter_while_loading_allows_permission_response_command() {
        let mut app = App::new();
        app.repl.is_loading = true;
        app.repl.input = "/allow".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "allow".to_string(),
                args: String::new()
            }]
        );
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_enter_while_loading_allows_cancel_command() {
        let mut app = App::new();
        app.repl.is_loading = true;
        app.repl.input = "/cancel".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "cancel".to_string(),
                args: String::new()
            }]
        );
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_permission_shortcuts_emit_allow_and_deny_actions() {
        let mut app = App::new();
        app.repl.is_loading = true;
        app.repl.permission_request_active = true;

        app.handle_key(event::KeyEvent::new(
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        ));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "allow".to_string(),
                args: String::new()
            }]
        );

        app.handle_key(event::KeyEvent::new(
            KeyCode::Char('n'),
            KeyModifiers::CONTROL,
        ));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "deny".to_string(),
                args: String::new()
            }]
        );
    }

    #[test]
    fn ctrl_c_cancels_loading_repl_prompt_instead_of_quitting() {
        let mut app = App::new();
        app.repl.is_loading = true;

        app.handle_key(event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ));

        assert!(!app.should_quit);
        assert_eq!(app.take_actions(), vec![AppAction::CancelPrompt]);
    }

    #[test]
    fn ctrl_c_quits_when_repl_is_idle() {
        let mut app = App::new();

        app.handle_key(event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ));

        assert!(app.should_quit);
        assert!(app.take_actions().is_empty());
    }

    #[test]
    fn app_tick_advances_loading_spinner_only() {
        let mut app = App::new();
        app.tick();
        assert_eq!(app.repl.spinner_tick, 0);

        app.repl.is_loading = true;
        app.tick();

        assert_eq!(app.repl.spinner_tick, 1);
    }

    #[test]
    fn repl_resume_command_switches_screen_without_action() {
        let mut app = App::new();
        app.repl.input = "/resume".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::ResumeConversation);
        assert_eq!(app.take_actions(), vec![AppAction::LoadResumeSessions]);
    }

    #[test]
    fn repl_history_command_switches_screen_and_requests_history_load() {
        let mut app = App::new();
        app.history.search_active = true;
        app.history.search_query = "old filter".to_string();
        app.repl.input = "/history".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::History);
        assert!(app.history.loading);
        assert!(!app.history.search_active);
        assert!(app.history.search_query.is_empty());
        assert_eq!(app.take_actions(), vec![AppAction::LoadPromptHistory]);
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_settings_command_switches_screen_and_requests_load() {
        let mut app = App::new();
        app.repl.input = "/settings".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Settings);
        assert!(app.settings.loading);
        assert_eq!(app.take_actions(), vec![AppAction::LoadSettings]);
    }

    #[test]
    fn repl_doctor_command_switches_screen_and_requests_load() {
        let mut app = App::new();
        app.repl.input = "/doctor".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Doctor);
        assert_eq!(app.take_actions(), vec![AppAction::LoadDoctor]);
    }

    #[test]
    fn repl_help_command_emits_slash_action_without_model_submit() {
        let mut app = App::new();
        app.repl.input = "/help".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Repl);
        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "help".to_string(),
                args: String::new()
            }]
        );
        assert!(app.repl.messages.is_empty());
        assert!(app.repl.input.is_empty());
    }

    #[test]
    fn repl_slash_command_preserves_arguments_for_runtime_registry() {
        let mut app = App::new();
        app.repl.input = "/config set theme dark".to_string();
        app.repl.cursor = app.repl.input.len();

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.take_actions(),
            vec![AppAction::RunSlashCommand {
                name: "config".to_string(),
                args: "set theme dark".to_string()
            }]
        );
    }

    #[test]
    fn resume_enter_emits_resume_session_action() {
        let mut app = App::new();
        app.screen = AppScreen::ResumeConversation;
        app.resume.load_sessions(vec![SessionEntry {
            session_id: "session-1".to_string(),
            title: "Recent work".to_string(),
            timestamp: "2026-06-12 10:00:00".to_string(),
            message_count: 3,
        }]);

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Repl);
        assert_eq!(
            app.take_actions(),
            vec![AppAction::ResumeSession("session-1".to_string())]
        );
    }

    #[test]
    fn history_enter_restores_selected_prompt_as_repl_draft() {
        let mut app = App::new();
        app.screen = AppScreen::History;
        app.history.load_entries(vec![
            HistoryEntry::new("first prompt".to_string(), "2".to_string()),
            HistoryEntry::new("second prompt".to_string(), "1".to_string()),
        ]);
        app.history.selected = 1;
        app.history.list_state.select(Some(1));

        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.screen, AppScreen::Repl);
        assert_eq!(app.repl.input, "second prompt");
        assert_eq!(app.repl.cursor, app.repl.input.len());
        assert!(app.take_actions().is_empty());
    }
}
