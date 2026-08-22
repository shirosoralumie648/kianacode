use chrono::{DateTime, Utc};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

use super::error::{MacroError, Result};

/// A single recorded action in a macro
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum MacroAction {
    /// Raw key press event
    #[serde(rename = "key")]
    KeyPress {
        #[serde(with = "key_code_serde")]
        code: KeyCode,
        #[serde(with = "key_modifiers_serde")]
        modifiers: KeyModifiers,
    },
    /// Optional: delay between actions (milliseconds)
    #[serde(rename = "delay")]
    Delay { ms: u64 },
}

/// Macro definition stored in a register
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Macro {
    /// Register identifier (a-z)
    pub register: char,
    /// Sequence of actions
    pub actions: Vec<MacroAction>,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last modified timestamp
    pub modified_at: DateTime<Utc>,
}

/// Playback state for a macro
#[derive(Debug)]
struct PlaybackState {
    /// Register being played
    register: char,
    /// Current action index
    current_index: usize,
    /// Remaining repetitions
    remaining_count: usize,
    /// Start time of playback
    start_time: Instant,
}

/// Core macro recorder and playback engine
#[derive(Debug)]
pub struct MacroRecorder {
    /// Currently recording to this register
    recording_register: Option<char>,
    /// Actions being recorded
    recording_buffer: Vec<MacroAction>,
    /// Start time of current recording
    recording_start: Option<Instant>,
    /// Last action timestamp for delay calculation
    last_action_time: Option<Instant>,
    /// All stored macros (a-z registers)
    macros: HashMap<char, Macro>,
    /// Last played macro register
    last_played: Option<char>,
    /// Playback state
    playback_state: Option<PlaybackState>,
    /// Max actions per macro (safety limit)
    max_actions: usize,
    /// Whether to record delays between actions
    record_delays: bool,
}

impl MacroRecorder {
    /// Create a new macro recorder with default settings
    pub fn new() -> Self {
        Self::with_max_actions(1000)
    }

    /// Create a macro recorder with custom max actions limit
    pub fn with_max_actions(max_actions: usize) -> Self {
        Self {
            recording_register: None,
            recording_buffer: Vec::new(),
            recording_start: None,
            last_action_time: None,
            macros: HashMap::new(),
            last_played: None,
            playback_state: None,
            max_actions,
            record_delays: false,
        }
    }

    /// Start recording to a register
    pub fn start_recording(&mut self, register: char) -> Result<()> {
        if !register.is_ascii_lowercase() {
            return Err(MacroError::InvalidRegister(register));
        }

        if let Some(current) = self.recording_register {
            return Err(MacroError::AlreadyRecording(current));
        }

        self.recording_register = Some(register);
        self.recording_buffer.clear();
        self.recording_start = Some(Instant::now());
        self.last_action_time = Some(Instant::now());

        Ok(())
    }

    /// Stop recording and save to register
    pub fn stop_recording(&mut self) -> Result<char> {
        let register = self.recording_register.ok_or(MacroError::NotRecording)?;

        let actions = std::mem::take(&mut self.recording_buffer);
        let now = Utc::now();

        let macro_def = Macro {
            register,
            actions,
            description: None,
            created_at: now,
            modified_at: now,
        };

        self.macros.insert(register, macro_def);
        self.recording_register = None;
        self.recording_start = None;
        self.last_action_time = None;

        Ok(register)
    }

    /// Record a key event during macro recording
    pub fn record_event(&mut self, event: KeyEvent) -> Result<()> {
        if self.recording_register.is_none() {
            return Ok(()); // Not recording, silently ignore
        }

        if self.recording_buffer.len() >= self.max_actions {
            return Err(MacroError::MaxActionsExceeded(self.max_actions));
        }

        // Prevent recording macro commands to avoid recursion
        if self.is_macro_command(&event) {
            return Err(MacroError::RecursiveMacro);
        }

        // Record delay if enabled
        if self.record_delays {
            if let Some(last_time) = self.last_action_time {
                let delay_ms = last_time.elapsed().as_millis() as u64;
                if delay_ms > 50 {
                    // Only record significant delays
                    self.recording_buffer
                        .push(MacroAction::Delay { ms: delay_ms });
                }
            }
        }

        self.recording_buffer.push(MacroAction::KeyPress {
            code: event.code,
            modifiers: event.modifiers,
        });

        self.last_action_time = Some(Instant::now());

        Ok(())
    }

    /// Check if a key event is a macro command (q, @)
    fn is_macro_command(&self, event: &KeyEvent) -> bool {
        matches!(
            event.code,
            KeyCode::Char('q') | KeyCode::Char('@') if event.modifiers.is_empty()
        )
    }

    /// Start playing a macro from a register
    pub fn play_macro(&mut self, register: char, count: usize) -> Result<()> {
        let macro_def = self
            .macros
            .get(&register)
            .ok_or(MacroError::MacroNotFound(register))?;

        if macro_def.actions.is_empty() {
            return Err(MacroError::EmptyMacro(register));
        }

        if count == 0 {
            return Ok(()); // Play zero times = no-op
        }

        self.playback_state = Some(PlaybackState {
            register,
            current_index: 0,
            remaining_count: count,
            start_time: Instant::now(),
        });

        self.last_played = Some(register);

        Ok(())
    }

    /// Get next action to execute during playback
    pub fn next_playback_action(&mut self) -> Option<MacroAction> {
        let state = self.playback_state.as_mut()?;
        let macro_def = self.macros.get(&state.register)?;

        if state.current_index >= macro_def.actions.len() {
            // End of macro sequence
            state.remaining_count = state.remaining_count.saturating_sub(1);

            if state.remaining_count == 0 {
                // All repetitions complete
                self.playback_state = None;
                return None;
            }

            // Loop back to start for next repetition
            state.current_index = 0;
        }

        let action = macro_def.actions[state.current_index].clone();
        state.current_index += 1;

        Some(action)
    }

    /// Repeat last played macro
    pub fn repeat_last(&mut self, count: usize) -> Result<()> {
        let register = self.last_played.ok_or(MacroError::NoLastMacro)?;
        self.play_macro(register, count)
    }

    /// Stop current playback
    pub fn stop_playback(&mut self) {
        self.playback_state = None;
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.recording_register.is_some()
    }

    /// Get current recording register
    pub fn recording_register(&self) -> Option<char> {
        self.recording_register
    }

    /// Check if currently playing back
    pub fn is_playing(&self) -> bool {
        self.playback_state.is_some()
    }

    /// Get current playback register
    pub fn playback_register(&self) -> Option<char> {
        self.playback_state.as_ref().map(|s| s.register)
    }

    /// Get reference to all macros
    pub fn macros(&self) -> &HashMap<char, Macro> {
        &self.macros
    }

    /// Get mutable reference to all macros
    pub fn macros_mut(&mut self) -> &mut HashMap<char, Macro> {
        &mut self.macros
    }

    /// Load macros from external source
    pub fn load_macros(&mut self, macros: HashMap<char, Macro>) {
        self.macros = macros;
    }

    /// Get a specific macro by register
    pub fn get_macro(&self, register: char) -> Option<&Macro> {
        self.macros.get(&register)
    }

    /// Delete a macro
    pub fn delete_macro(&mut self, register: char) -> Result<()> {
        self.macros
            .remove(&register)
            .ok_or(MacroError::MacroNotFound(register))?;
        Ok(())
    }

    /// Update macro description
    pub fn set_description(&mut self, register: char, description: String) -> Result<()> {
        let macro_def = self
            .macros
            .get_mut(&register)
            .ok_or(MacroError::MacroNotFound(register))?;
        macro_def.description = Some(description);
        macro_def.modified_at = Utc::now();
        Ok(())
    }

    /// Get recording statistics
    pub fn recording_stats(&self) -> Option<RecordingStats> {
        if let Some(register) = self.recording_register {
            let elapsed = self.recording_start?.elapsed();
            Some(RecordingStats {
                register,
                action_count: self.recording_buffer.len(),
                duration_secs: elapsed.as_secs(),
            })
        } else {
            None
        }
    }
}

impl Default for MacroRecorder {
    fn default() -> Self {
        Self::new()
    }
}

/// Recording statistics
#[derive(Debug, Clone)]
pub struct RecordingStats {
    pub register: char,
    pub action_count: usize,
    pub duration_secs: u64,
}

// Serialization helpers for crossterm types
mod key_code_serde {
    use ratatui::crossterm::event::KeyCode;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "type")]
    enum KeyCodeDef {
        Char { c: char },
        F { n: u8 },
        Backspace,
        Enter,
        Left,
        Right,
        Up,
        Down,
        Home,
        End,
        PageUp,
        PageDown,
        Tab,
        BackTab,
        Delete,
        Insert,
        Esc,
        Null,
    }

    pub fn serialize<S>(code: &KeyCode, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let def = match code {
            KeyCode::Char(c) => KeyCodeDef::Char { c: *c },
            KeyCode::F(n) => KeyCodeDef::F { n: *n },
            KeyCode::Backspace => KeyCodeDef::Backspace,
            KeyCode::Enter => KeyCodeDef::Enter,
            KeyCode::Left => KeyCodeDef::Left,
            KeyCode::Right => KeyCodeDef::Right,
            KeyCode::Up => KeyCodeDef::Up,
            KeyCode::Down => KeyCodeDef::Down,
            KeyCode::Home => KeyCodeDef::Home,
            KeyCode::End => KeyCodeDef::End,
            KeyCode::PageUp => KeyCodeDef::PageUp,
            KeyCode::PageDown => KeyCodeDef::PageDown,
            KeyCode::Tab => KeyCodeDef::Tab,
            KeyCode::BackTab => KeyCodeDef::BackTab,
            KeyCode::Delete => KeyCodeDef::Delete,
            KeyCode::Insert => KeyCodeDef::Insert,
            KeyCode::Esc => KeyCodeDef::Esc,
            KeyCode::Null => KeyCodeDef::Null,
            _ => KeyCodeDef::Null, // Fallback for unsupported variants
        };
        def.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<KeyCode, D::Error>
    where
        D: Deserializer<'de>,
    {
        let def = KeyCodeDef::deserialize(deserializer)?;
        Ok(match def {
            KeyCodeDef::Char { c } => KeyCode::Char(c),
            KeyCodeDef::F { n } => KeyCode::F(n),
            KeyCodeDef::Backspace => KeyCode::Backspace,
            KeyCodeDef::Enter => KeyCode::Enter,
            KeyCodeDef::Left => KeyCode::Left,
            KeyCodeDef::Right => KeyCode::Right,
            KeyCodeDef::Up => KeyCode::Up,
            KeyCodeDef::Down => KeyCode::Down,
            KeyCodeDef::Home => KeyCode::Home,
            KeyCodeDef::End => KeyCode::End,
            KeyCodeDef::PageUp => KeyCode::PageUp,
            KeyCodeDef::PageDown => KeyCode::PageDown,
            KeyCodeDef::Tab => KeyCode::Tab,
            KeyCodeDef::BackTab => KeyCode::BackTab,
            KeyCodeDef::Delete => KeyCode::Delete,
            KeyCodeDef::Insert => KeyCode::Insert,
            KeyCodeDef::Esc => KeyCode::Esc,
            KeyCodeDef::Null => KeyCode::Null,
        })
    }
}

mod key_modifiers_serde {
    use ratatui::crossterm::event::KeyModifiers;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct KeyModifiersDef {
        shift: bool,
        control: bool,
        alt: bool,
    }

    pub fn serialize<S>(mods: &KeyModifiers, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let def = KeyModifiersDef {
            shift: mods.contains(KeyModifiers::SHIFT),
            control: mods.contains(KeyModifiers::CONTROL),
            alt: mods.contains(KeyModifiers::ALT),
        };
        def.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<KeyModifiers, D::Error>
    where
        D: Deserializer<'de>,
    {
        let def = KeyModifiersDef::deserialize(deserializer)?;
        let mut mods = KeyModifiers::empty();
        if def.shift {
            mods |= KeyModifiers::SHIFT;
        }
        if def.control {
            mods |= KeyModifiers::CONTROL;
        }
        if def.alt {
            mods |= KeyModifiers::ALT;
        }
        Ok(mods)
    }
}
