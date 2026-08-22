use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

use crate::components::StatusBar;
use crate::macro_system::{MacroAction, MacroRecorder, MacroStorage};

/// Macro command state machine
#[derive(Debug, Clone, PartialEq)]
pub enum MacroCommand {
    None,
    StartRecording(char),
    StopRecording,
    PlayMacro(char, usize),
    RepeatLast(usize),
    WaitingForRegister(MacroRegisterAction),
}

/// Action to perform with a register
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MacroRegisterAction {
    Record,
    Play,
}

/// Macro input handler state
pub struct MacroInputHandler {
    /// Current command state
    command_state: MacroCommand,
    /// Pending count for commands (e.g., 5@a)
    pending_count: Option<usize>,
    /// Last key press time (for multi-key sequences)
    last_key_time: Instant,
}

impl MacroInputHandler {
    pub fn new() -> Self {
        Self {
            command_state: MacroCommand::None,
            pending_count: None,
            last_key_time: Instant::now(),
        }
    }

    /// Handle key input and return macro command if applicable
    pub fn handle_key(&mut self, key: KeyEvent, recorder: &MacroRecorder) -> MacroCommand {
        // Reset if timeout elapsed (1 second)
        if self.last_key_time.elapsed().as_secs() > 1 {
            self.command_state = MacroCommand::None;
            self.pending_count = None;
        }
        self.last_key_time = Instant::now();

        // Handle based on current state
        match &self.command_state {
            MacroCommand::WaitingForRegister(action) => {
                let action = *action;
                self.command_state = MacroCommand::None;

                match key.code {
                    KeyCode::Char(c) if c.is_ascii_lowercase() => match action {
                        MacroRegisterAction::Record => MacroCommand::StartRecording(c),
                        MacroRegisterAction::Play => {
                            let count = self.pending_count.take().unwrap_or(1);
                            MacroCommand::PlayMacro(c, count)
                        }
                    },
                    _ => MacroCommand::None,
                }
            }

            _ => {
                // Not waiting for register, parse new command
                match key.code {
                    KeyCode::Char('q') if key.modifiers.is_empty() => {
                        if recorder.is_recording() {
                            self.command_state = MacroCommand::None;
                            MacroCommand::StopRecording
                        } else {
                            self.command_state =
                                MacroCommand::WaitingForRegister(MacroRegisterAction::Record);
                            MacroCommand::None
                        }
                    }

                    KeyCode::Char('@') if key.modifiers.is_empty() => {
                        // Check for @@
                        if matches!(self.command_state, MacroCommand::None) {
                            self.command_state =
                                MacroCommand::WaitingForRegister(MacroRegisterAction::Play);
                            MacroCommand::None
                        } else {
                            // Repeat last
                            self.command_state = MacroCommand::None;
                            let count = self.pending_count.take().unwrap_or(1);
                            MacroCommand::RepeatLast(count)
                        }
                    }

                    KeyCode::Char(c) if c.is_ascii_digit() && key.modifiers.is_empty() => {
                        // Build count prefix
                        let digit = c.to_digit(10).unwrap() as usize;
                        let current = self.pending_count.unwrap_or(0);
                        self.pending_count = Some(current * 10 + digit);
                        MacroCommand::None
                    }

                    _ => {
                        // Other key, reset state
                        self.pending_count = None;
                        self.command_state = MacroCommand::None;
                        MacroCommand::None
                    }
                }
            }
        }
    }

    /// Reset the handler state
    pub fn reset(&mut self) {
        self.command_state = MacroCommand::None;
        self.pending_count = None;
    }
}

impl Default for MacroInputHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a macro action (convert to key event)
pub fn execute_macro_action(action: &MacroAction) -> Option<KeyEvent> {
    match action {
        MacroAction::KeyPress { code, modifiers } => Some(KeyEvent::new(*code, *modifiers)),
        MacroAction::Delay { .. } => None, // Delays are handled separately
    }
}

/// Integrate macro system into application event loop
pub struct MacroIntegration {
    pub recorder: MacroRecorder,
    pub storage: MacroStorage,
    pub input_handler: MacroInputHandler,
}

impl MacroIntegration {
    /// Create new macro integration
    pub fn new() -> Result<Self, crate::macro_system::MacroError> {
        let storage = MacroStorage::new()?;
        let mut recorder = MacroRecorder::new();

        // Load saved macros
        if let Ok(macros) = storage.load() {
            recorder.load_macros(macros);
        }

        Ok(Self {
            recorder,
            storage,
            input_handler: MacroInputHandler::new(),
        })
    }

    /// Handle a key event, returns true if event was consumed by macro system
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        status_bar: &mut StatusBar,
    ) -> Result<bool, crate::macro_system::MacroError> {
        // Check for macro commands
        let command = self.input_handler.handle_key(key, &self.recorder);

        match command {
            MacroCommand::StartRecording(register) => {
                self.recorder.start_recording(register)?;
                status_bar.set_macro_recording(Some(register));
                status_bar.set_status(Some(format!("Recording macro to @{}", register)));
                return Ok(true);
            }

            MacroCommand::StopRecording => {
                let register = self.recorder.stop_recording()?;
                status_bar.set_macro_recording(None);
                status_bar.set_status(Some(format!("Macro @{} saved", register)));

                // Auto-save
                self.storage.save(self.recorder.macros())?;
                return Ok(true);
            }

            MacroCommand::PlayMacro(register, count) => {
                self.recorder.play_macro(register, count)?;
                status_bar.set_status(Some(format!("Playing macro @{} ({}x)", register, count)));
                return Ok(true);
            }

            MacroCommand::RepeatLast(count) => {
                self.recorder.repeat_last(count)?;
                status_bar.set_status(Some(format!("Repeating last macro ({}x)", count)));
                return Ok(true);
            }

            MacroCommand::WaitingForRegister(_) => {
                // Waiting for next key
                return Ok(true);
            }

            MacroCommand::None => {
                // Not a macro command, continue processing
            }
        }

        // Record event if recording
        if self.recorder.is_recording() {
            self.recorder.record_event(key)?;
        }

        Ok(false)
    }

    /// Get next playback action
    pub fn next_playback_action(&mut self) -> Option<KeyEvent> {
        let action = self.recorder.next_playback_action()?;
        execute_macro_action(&action)
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> bool {
        self.recorder.is_playing()
    }

    /// Stop playback
    pub fn stop_playback(&mut self) {
        self.recorder.stop_playback();
    }

    /// Get reference to recorder
    pub fn recorder(&self) -> &MacroRecorder {
        &self.recorder
    }

    /// Get mutable reference to recorder
    pub fn recorder_mut(&mut self) -> &mut MacroRecorder {
        &mut self.recorder
    }
}

impl Default for MacroIntegration {
    fn default() -> Self {
        Self::new().expect("Failed to create macro integration")
    }
}
