//! Bounded TTY editor state for the folder Workbench.
//!
//! Crossterm decodes terminal byte chunks into complete events before this module sees them.
//! This layer only edits a draft and emits an immutable committed command on Enter; pasted text
//! is never interpreted as key input. It has no execution, shell, or daemon authority.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub const MAX_TTY_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_TTY_INPUT_CHARS: usize = 16 * 1024;
pub const MAX_TTY_HISTORY_ENTRIES: usize = 64;
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// Input mode is explicit so callers cannot silently turn a pipe into an interactive editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Tty,
    NonTty,
}

/// Small deterministic decoder for PTY byte chunks. Incomplete escape and UTF-8 sequences are
/// retained until the next chunk; neither can emit a command on its own. Bracketed paste is
/// emitted as one `Event::Paste` after its closing marker, so payload controls are data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PtyChunkDecoder {
    pending: Vec<u8>,
    paste: Vec<u8>,
    in_paste: bool,
}

impl PtyChunkDecoder {
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Event>, &'static str> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        loop {
            if self.in_paste {
                self.paste.extend_from_slice(&self.pending);
                self.pending.clear();
                let Some(index) = find_bytes(&self.paste, PASTE_END) else {
                    if self.paste.len() > MAX_TTY_INPUT_BYTES + PASTE_END.len() {
                        self.pending.clear();
                        self.paste.clear();
                        self.in_paste = false;
                        return Err("tty_input_limit_exceeded");
                    }
                    return Ok(events);
                };
                let payload = self.paste[..index].to_vec();
                let rest = self.paste[index + PASTE_END.len()..].to_vec();
                self.paste.clear();
                self.pending = rest;
                self.in_paste = false;
                let text = String::from_utf8(payload).map_err(|_| "tty_input_invalid_utf8")?;
                events.push(Event::Paste(text));
                continue;
            }

            if self.pending.starts_with(PASTE_START) {
                self.pending.drain(..PASTE_START.len());
                self.in_paste = true;
                continue;
            }
            if self.pending.is_empty() {
                return Ok(events);
            }

            if self.pending[0] == 0x1b {
                let known = [
                    (&b"\x1b[A"[..], KeyCode::Up),
                    (&b"\x1b[B"[..], KeyCode::Down),
                    (&b"\x1b[C"[..], KeyCode::Right),
                    (&b"\x1b[D"[..], KeyCode::Left),
                    (&b"\x1b[3~"[..], KeyCode::Delete),
                ];
                let mut prefix = PASTE_START.starts_with(&self.pending);
                for (sequence, code) in known {
                    if self.pending.starts_with(sequence) {
                        self.pending.drain(..sequence.len());
                        events.push(Event::Key(KeyEvent::from(code)));
                        prefix = false;
                        break;
                    }
                    if sequence.starts_with(&self.pending) {
                        prefix = true;
                    }
                }
                if prefix {
                    return Ok(events);
                }
                if self.pending.len() == 1 {
                    return Ok(events);
                }
                self.pending.clear();
                return Err("tty_input_partial_escape_rejected");
            }

            let event = match self.pending[0] {
                0x03 => {
                    self.pending.drain(..1);
                    Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
                }
                0x04 => {
                    self.pending.drain(..1);
                    Event::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL))
                }
                0x08 | 0x7f => {
                    self.pending.drain(..1);
                    Event::Key(KeyEvent::from(KeyCode::Backspace))
                }
                b'\r' | b'\n' => {
                    self.pending.drain(..1);
                    Event::Key(KeyEvent::from(KeyCode::Enter))
                }
                first => {
                    let width = utf8_width(first).ok_or("tty_input_invalid_utf8")?;
                    if self.pending.len() < width {
                        return Ok(events);
                    }
                    let text = std::str::from_utf8(&self.pending[..width])
                        .map_err(|_| "tty_input_invalid_utf8")?;
                    let ch = text.chars().next().ok_or("tty_input_invalid_utf8")?;
                    self.pending.drain(..width);
                    Event::Key(KeyEvent::from(KeyCode::Char(ch)))
                }
            };
            events.push(event);
        }
    }

    /// End-of-input is a hard boundary: a dangling escape or UTF-8 prefix is rejected, never
    /// interpreted as Enter or a shell command.
    pub fn finish(&mut self) -> Result<(), &'static str> {
        if self.in_paste || !self.pending.is_empty() || !self.paste.is_empty() {
            self.pending.clear();
            self.paste.clear();
            self.in_paste = false;
            return Err("tty_input_partial_escape_rejected");
        }
        Ok(())
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn utf8_width(first: u8) -> Option<usize> {
    match first {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedInput {
    /// Exact bounded text present at the Enter boundary.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputTransition {
    Changed,
    Committed(CommittedInput),
    Cancel,
    Quit,
    Resized(TerminalSize),
    Rejected(&'static str),
    Ignored,
}

/// The Workbench editor owns only an editable draft and bounded presentation history.
/// `CommittedInput` is transferred to the caller and is not modified by later key events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtyInputState {
    mode: InputMode,
    draft: String,
    composition: String,
    composing: bool,
    history: Vec<String>,
    history_cursor: Option<usize>,
    saved_draft: String,
    terminal_size: Option<TerminalSize>,
    last_committed: Option<CommittedInput>,
}

impl Default for TtyInputState {
    fn default() -> Self {
        Self {
            mode: InputMode::Tty,
            draft: String::new(),
            composition: String::new(),
            composing: false,
            history: Vec::new(),
            history_cursor: None,
            saved_draft: String::new(),
            terminal_size: None,
            last_committed: None,
        }
    }
}

impl TtyInputState {
    pub fn new(mode: InputMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn is_composing(&self) -> bool {
        self.composing
    }

    pub fn terminal_size(&self) -> Option<TerminalSize> {
        self.terminal_size
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn last_committed(&self) -> Option<&CommittedInput> {
        self.last_committed.as_ref()
    }

    /// Apply one PTY byte chunk through the same event/state boundary used by Workbench. A
    /// decoder error returns before any decoded transition is applied, so a malformed chunk
    /// cannot partially submit a command.
    pub fn apply_chunk(
        &mut self,
        decoder: &mut PtyChunkDecoder,
        chunk: &[u8],
        running: bool,
    ) -> Result<Vec<InputTransition>, &'static str> {
        let events = decoder.feed(chunk)?;
        Ok(events
            .into_iter()
            .map(|event| self.apply_event(event, running))
            .collect())
    }

    /// Handle an actual input-stream EOF. A nonempty draft is retained and rejected rather than
    /// silently submitting or dropping it; an empty idle editor can exit cleanly.
    pub fn eof(&mut self) -> InputTransition {
        if self.mode == InputMode::NonTty {
            return InputTransition::Rejected("tty_input_requires_tty");
        }
        if self.composing {
            return self.cancel_ime();
        }
        if self.draft.is_empty() {
            InputTransition::Quit
        } else {
            InputTransition::Rejected("tty_input_eof_with_draft")
        }
    }

    /// Consume a complete Crossterm event. Paste payload is inserted as text in one bounded
    /// operation; it cannot synthesize Enter, Escape, slash commands, or shell actions.
    pub fn apply_event(&mut self, event: Event, running: bool) -> InputTransition {
        if self.mode == InputMode::NonTty {
            return InputTransition::Rejected("tty_input_requires_tty");
        }
        match event {
            Event::Key(key) => self.apply_key(key, running),
            Event::Paste(text) if self.composing => {
                let _ = text;
                InputTransition::Rejected("tty_input_ime_active")
            }
            Event::Paste(text) => self.insert_text(&text),
            Event::Resize(width, height) => {
                let size = TerminalSize { width, height };
                self.terminal_size = Some(size);
                InputTransition::Resized(size)
            }
            Event::FocusGained | Event::FocusLost | Event::Mouse(_) => InputTransition::Ignored,
        }
    }

    fn apply_key(&mut self, key: KeyEvent, running: bool) -> InputTransition {
        if key.kind == KeyEventKind::Release {
            return InputTransition::Ignored;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.composing {
                return self.cancel_ime();
            }
            return match key.code {
                KeyCode::Char('c') if running => InputTransition::Cancel,
                KeyCode::Char('c') => InputTransition::Quit,
                KeyCode::Char('d') if self.draft.is_empty() => InputTransition::Quit,
                KeyCode::Char('d') => InputTransition::Ignored,
                KeyCode::Char('j') => self.insert_text("\n"),
                _ => InputTransition::Ignored,
            };
        }

        match key.code {
            KeyCode::Esc if self.composing => self.cancel_ime(),
            KeyCode::Esc if running => InputTransition::Cancel,
            KeyCode::Esc => InputTransition::Ignored,
            KeyCode::Enter if self.composing => InputTransition::Rejected("tty_input_ime_active"),
            KeyCode::Enter => self.commit(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Up => self.history_previous(),
            KeyCode::Down => self.history_next(),
            KeyCode::Char(ch) => self.insert_text(&ch.to_string()),
            _ => InputTransition::Ignored,
        }
    }

    fn insert_text(&mut self, text: &str) -> InputTransition {
        if self.composing {
            return InputTransition::Rejected("tty_input_ime_active");
        }
        if text
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        {
            return InputTransition::Rejected("tty_input_control_character_rejected");
        }
        let next_bytes = self.draft.len().saturating_add(text.len());
        let next_chars = self
            .draft
            .chars()
            .count()
            .saturating_add(text.chars().count());
        if next_bytes > MAX_TTY_INPUT_BYTES || next_chars > MAX_TTY_INPUT_CHARS {
            return InputTransition::Rejected("tty_input_limit_exceeded");
        }
        if text.is_empty() {
            return InputTransition::Ignored;
        }
        self.draft.push_str(text);
        self.history_cursor = None;
        self.saved_draft.clear();
        InputTransition::Changed
    }

    fn backspace(&mut self) -> InputTransition {
        if self.draft.pop().is_some() {
            self.history_cursor = None;
            self.saved_draft.clear();
            InputTransition::Changed
        } else {
            InputTransition::Ignored
        }
    }

    fn commit(&mut self) -> InputTransition {
        if self.composing {
            return InputTransition::Rejected("tty_input_ime_active");
        }
        let text = std::mem::take(&mut self.draft);
        self.history_cursor = None;
        self.saved_draft.clear();
        if text.trim().is_empty() {
            self.draft = text;
            return InputTransition::Ignored;
        }
        if !text.trim().is_empty() {
            if self.history.last() != Some(&text) {
                self.history.push(text.clone());
                if self.history.len() > MAX_TTY_HISTORY_ENTRIES {
                    self.history.remove(0);
                }
            }
        }
        let committed = CommittedInput { text };
        self.last_committed = Some(committed.clone());
        InputTransition::Committed(committed)
    }

    fn history_previous(&mut self) -> InputTransition {
        if self.history.is_empty() {
            return InputTransition::Ignored;
        }
        let next = match self.history_cursor {
            Some(0) => 0,
            Some(index) => index - 1,
            None => {
                self.saved_draft.clone_from(&self.draft);
                self.history.len() - 1
            }
        };
        self.history_cursor = Some(next);
        self.draft.clone_from(&self.history[next]);
        InputTransition::Changed
    }

    fn history_next(&mut self) -> InputTransition {
        let Some(index) = self.history_cursor else {
            return InputTransition::Ignored;
        };
        if index + 1 < self.history.len() {
            let next = index + 1;
            self.history_cursor = Some(next);
            self.draft.clone_from(&self.history[next]);
        } else {
            self.history_cursor = None;
            self.draft.clone_from(&self.saved_draft);
            self.saved_draft.clear();
        }
        InputTransition::Changed
    }

    /// Start an IME composition. Composition text is held separately from the submitted draft.
    pub fn begin_ime(&mut self) -> InputTransition {
        if self.composing {
            return InputTransition::Ignored;
        }
        self.composing = true;
        self.composition.clear();
        InputTransition::Changed
    }

    /// Replace the uncommitted IME candidate without exposing it as a command or key sequence.
    pub fn update_ime(&mut self, text: &str) -> InputTransition {
        if !self.composing {
            return InputTransition::Rejected("tty_input_ime_not_active");
        }
        if text
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        {
            return InputTransition::Rejected("tty_input_control_character_rejected");
        }
        if self.draft.len().saturating_add(text.len()) > MAX_TTY_INPUT_BYTES
            || self
                .draft
                .chars()
                .count()
                .saturating_add(text.chars().count())
                > MAX_TTY_INPUT_CHARS
        {
            return InputTransition::Rejected("tty_input_limit_exceeded");
        }
        self.composition.clear();
        self.composition.push_str(text);
        InputTransition::Changed
    }

    /// Commit the IME candidate into the draft; this still does not submit a command.
    pub fn commit_ime(&mut self) -> InputTransition {
        if !self.composing {
            return InputTransition::Ignored;
        }
        let composition = std::mem::take(&mut self.composition);
        self.composing = false;
        self.insert_text(&composition)
    }

    pub fn cancel_ime(&mut self) -> InputTransition {
        if !self.composing {
            return InputTransition::Ignored;
        }
        self.composition.clear();
        self.composing = false;
        InputTransition::Changed
    }
}
