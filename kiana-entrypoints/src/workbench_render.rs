//! Bounded Workbench timeline projection.
//!
//! This module turns the additive run stream into disposable, plain-text timeline items. It does
//! not decide run state, execute tool calls, parse markdown, or replace EventLog/Receipt facts.
//! Cursor/epoch checks are retained at this boundary so a display gap cannot look like success.

use kiana_protocol::{
    redact_text, scan_secret_sentinels, ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent,
    SecretScanChannel, UiConnectorHealthProjection, UiCursor, PROTOCOL_SCHEMA,
};
use serde_json::Value;

pub const MAX_TIMELINE_ITEMS: usize = 512;
pub const MAX_TIMELINE_BYTES: usize = 512 * 1024;
pub const MAX_TIMELINE_ITEM_BYTES: usize = 32 * 1024;
pub const MAX_TIMELINE_JSON_BYTES: usize = 16 * 1024;
const TIMELINE_LIMIT_BODY: &str = "render limit reached; additional stream content is hidden";
const MAX_TIMELINE_SCAN_BYTES: usize = MAX_TIMELINE_ITEM_BYTES + 1024;

/// Bounded display row for a connector probe. UI code receives only typed, redacted projection
/// data; it never formats provider response bodies, paths or credential references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorHealthRenderRow {
    pub binding_id: String,
    pub connector_id: String,
    pub status: String,
    pub stale: bool,
    pub limitations: Vec<String>,
}

pub fn render_connector_health(value: &Value) -> Result<Vec<ConnectorHealthRenderRow>, String> {
    let projection: UiConnectorHealthProjection = serde_json::from_value(value.clone())
        .map_err(|error| format!("connector_health_ui_decode:{error}"))?;
    projection.validate()?;
    if serde_json::to_vec(&projection)
        .map_err(|_| "connector_health_ui_encode".to_owned())?
        .len()
        > 256 * 1024
    {
        return Err("connector_health_ui_too_large".to_owned());
    }
    Ok(projection
        .entries
        .into_iter()
        .map(|entry| ConnectorHealthRenderRow {
            binding_id: entry.health.binding_id,
            connector_id: entry.health.connector_id,
            status: entry.health.status.as_str().to_owned(),
            stale: entry.stale,
            limitations: entry.limitations,
        })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineItemKind {
    Delta,
    Terminal,
    Usage,
    ToolCall,
    Approval,
    Artifact,
    Error,
    Unknown,
    Gap,
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineItem {
    pub id: String,
    pub kind: TimelineItemKind,
    /// Plain text only; markdown/OSC/ANSI are not interpreted by the renderer.
    pub body: String,
    pub collapsed: bool,
    pub loading: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineApply {
    Applied,
    IgnoredDuplicate,
    IgnoredAfterTerminal,
    GapRequiresSnapshot,
}

/// A bounded and cursor-aware display projection for one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRenderer {
    run_id: RunId,
    cursor: UiCursor,
    items: Vec<TimelineItem>,
    total_bytes: usize,
    legacy_sequence: u64,
    terminal: bool,
    gap_detected: bool,
    limited: bool,
}

impl TimelineRenderer {
    pub fn new(run_id: RunId) -> Self {
        Self {
            run_id,
            cursor: UiCursor::default(),
            items: Vec::new(),
            total_bytes: 0,
            legacy_sequence: 0,
            terminal: false,
            gap_detected: false,
            limited: false,
        }
    }

    pub fn reset(&mut self, run_id: RunId) {
        *self = Self::new(run_id);
    }

    pub fn run_id(&self) -> RunId {
        self.run_id
    }

    pub fn cursor(&self) -> &UiCursor {
        &self.cursor
    }

    pub fn items(&self) -> &[TimelineItem] {
        &self.items
    }

    pub fn terminal(&self) -> bool {
        self.terminal
    }

    pub fn gap_detected(&self) -> bool {
        self.gap_detected
    }

    pub fn limited(&self) -> bool {
        self.limited
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Apply one versioned stream envelope. Duplicates are harmless; a gap requests snapshot
    /// hydration and never advances the display to a completed state.
    pub fn apply(&mut self, envelope: &RunStreamEnvelope) -> Result<TimelineApply, String> {
        if self.terminal {
            return Ok(TimelineApply::IgnoredAfterTerminal);
        }
        if envelope.schema != PROTOCOL_SCHEMA {
            return Err("timeline_schema_mismatch".to_owned());
        }
        self.ensure_run_id(&envelope.event)?;
        if self.gap_detected {
            return Ok(TimelineApply::GapRequiresSnapshot);
        }
        let cursor_result = envelope.advance_cursor(&mut self.cursor);
        let legacy = envelope.epoch.is_empty() && envelope.sequence == 0;
        if let Err(error) = cursor_result {
            if error == "stream_sequence_gap" {
                self.gap_detected = true;
                self.add_item(
                    self.item_id("gap", envelope.sequence),
                    TimelineItemKind::Gap,
                    "stream gap detected; hydrate a snapshot before rendering more events",
                    false,
                )?;
                return Ok(TimelineApply::GapRequiresSnapshot);
            }
            return Err(error.to_owned());
        }
        if !legacy && cursor_result == Ok(false) {
            return Ok(TimelineApply::IgnoredDuplicate);
        }
        let event = &envelope.event;
        self.legacy_sequence = self.legacy_sequence.saturating_add(1);
        let item_sequence = if legacy {
            self.legacy_sequence
        } else {
            envelope.sequence
        };
        self.append_event(event, self.item_id(event_kind_name(event), item_sequence))?;
        Ok(TimelineApply::Applied)
    }

    /// Legacy stream helper used by the current Workbench adapter, whose cursor has already been
    /// advanced by the daemon subscription path.
    pub fn apply_event(&mut self, event: RunStreamEvent) -> Result<TimelineApply, String> {
        if self.terminal {
            return Ok(TimelineApply::IgnoredAfterTerminal);
        }
        self.ensure_run_id(&event)?;
        self.legacy_sequence = self.legacy_sequence.saturating_add(1);
        let id = self.item_id(event_kind_name(&event), self.legacy_sequence);
        self.append_event(&event, id)?;
        Ok(TimelineApply::Applied)
    }

    fn append_event(&mut self, event: &RunStreamEvent, id: String) -> Result<(), String> {
        match event {
            RunStreamEvent::Delta { text, .. } => {
                self.add_item(id, TimelineItemKind::Delta, text, false)?;
            }
            RunStreamEvent::Terminal { response, .. } => {
                let body = terminal_text(response);
                self.add_item(id.clone(), TimelineItemKind::Terminal, &body, false)?;
                self.add_artifacts(response, &id)?;
                self.terminal = true;
            }
            RunStreamEvent::Usage { data, .. } => {
                self.add_item(id, TimelineItemKind::Usage, &json_text(data), true)?;
            }
            RunStreamEvent::ToolCall { data, .. } => {
                self.add_item(id, TimelineItemKind::ToolCall, &json_text(data), true)?;
            }
            RunStreamEvent::ApprovalRequested { data, .. } => {
                self.add_item(id, TimelineItemKind::Approval, &json_text(data), true)?;
            }
            RunStreamEvent::Error { data, .. } => {
                self.add_item(id, TimelineItemKind::Error, &json_text(data), false)?;
            }
            RunStreamEvent::Unknown => {
                self.add_item(
                    id,
                    TimelineItemKind::Unknown,
                    "unknown stream event; refresh the snapshot",
                    true,
                )?;
            }
        }
        Ok(())
    }

    fn add_artifacts(&mut self, response: &ResponseEnvelope, parent: &str) -> Result<(), String> {
        for key in ["files_changed", "artifacts"] {
            let Some(values) = response.output.get(key).and_then(Value::as_array) else {
                continue;
            };
            for (index, value) in values.iter().take(64).enumerate() {
                let body = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| json_text(value));
                self.add_item(
                    format!("{parent}:{key}:artifact:{index}"),
                    TimelineItemKind::Artifact,
                    &body,
                    true,
                )?;
            }
        }
        Ok(())
    }

    fn add_item(
        &mut self,
        id: String,
        kind: TimelineItemKind,
        body: &str,
        collapsed: bool,
    ) -> Result<(), String> {
        if self.items.iter().any(|item| item.id == id) {
            return Ok(());
        }
        let (mut body, mut truncated) = bounded_plain_text(body, MAX_TIMELINE_ITEM_BYTES);
        if matches!(
            kind,
            TimelineItemKind::Usage
                | TimelineItemKind::ToolCall
                | TimelineItemKind::Approval
                | TimelineItemKind::Error
        ) {
            let (bounded_json, json_truncated) = bounded_plain_text(&body, MAX_TIMELINE_JSON_BYTES);
            body = bounded_json;
            truncated |= json_truncated;
        }
        let reserved_limit = TIMELINE_LIMIT_BODY.len();
        if self.items.len() >= MAX_TIMELINE_ITEMS.saturating_sub(1)
            || self
                .total_bytes
                .saturating_add(body.len())
                .saturating_add(reserved_limit)
                > MAX_TIMELINE_BYTES
        {
            self.add_limit_marker();
            return Ok(());
        }
        self.total_bytes = self.total_bytes.saturating_add(body.len());
        self.items.push(TimelineItem {
            id,
            kind,
            body,
            collapsed,
            loading: false,
            truncated,
        });
        Ok(())
    }

    fn add_limit_marker(&mut self) {
        if self.limited {
            return;
        }
        self.limited = true;
        if self.items.len() < MAX_TIMELINE_ITEMS {
            let body = TIMELINE_LIMIT_BODY.to_owned();
            self.total_bytes = self.total_bytes.saturating_add(body.len());
            self.items.push(TimelineItem {
                id: "timeline:limit".to_owned(),
                kind: TimelineItemKind::Limit,
                body,
                collapsed: false,
                loading: false,
                truncated: true,
            });
        }
    }

    fn item_id(&self, kind: &str, sequence: u64) -> String {
        if self.cursor.epoch.is_empty() {
            format!("legacy:{kind}:{sequence}")
        } else {
            format!(
                "stream:{}:{sequence}:{kind}",
                bounded_id(&self.cursor.epoch)
            )
        }
    }

    fn ensure_run_id(&self, event: &RunStreamEvent) -> Result<(), String> {
        let actual = match event {
            RunStreamEvent::Delta { run_id, .. }
            | RunStreamEvent::Terminal { run_id, .. }
            | RunStreamEvent::Usage { run_id, .. }
            | RunStreamEvent::ToolCall { run_id, .. }
            | RunStreamEvent::ApprovalRequested { run_id, .. }
            | RunStreamEvent::Error { run_id, .. } => Some(*run_id),
            RunStreamEvent::Unknown => None,
        };
        if actual.is_some_and(|run_id| run_id != self.run_id) {
            return Err("timeline_event_run_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn toggle_collapsed(&mut self, id: &str) -> Result<(), String> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| "timeline_item_not_found".to_owned())?;
        item.collapsed = !item.collapsed;
        Ok(())
    }

    pub fn set_loading(&mut self, id: &str, loading: bool) -> Result<(), String> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| "timeline_item_not_found".to_owned())?;
        item.loading = loading;
        Ok(())
    }
}

fn event_kind_name(event: &RunStreamEvent) -> &'static str {
    match event {
        RunStreamEvent::Delta { .. } => "delta",
        RunStreamEvent::Terminal { .. } => "terminal",
        RunStreamEvent::Usage { .. } => "usage",
        RunStreamEvent::ToolCall { .. } => "tool_call",
        RunStreamEvent::ApprovalRequested { .. } => "approval",
        RunStreamEvent::Error { .. } => "error",
        RunStreamEvent::Unknown => "unknown",
    }
}

fn terminal_text(response: &ResponseEnvelope) -> String {
    let mut parts = vec![response.status.as_str().to_owned()];
    if let Some(error) = response.error.as_deref() {
        parts.push(error.to_owned());
    }
    if let Some(text) = response.output["output"]["text"].as_str() {
        parts.push(text.to_owned());
    }
    parts.join(": ")
}

fn json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[unserializable]".to_owned())
}

fn bounded_id(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':'))
        .take(64)
        .collect()
}

fn bounded_plain_text(value: &str, limit: usize) -> (String, bool) {
    let clean = redact_and_strip_controls(value);
    if clean.len() <= limit {
        return (clean, false);
    }
    let suffix = " … [truncated]";
    let end_limit = limit.saturating_sub(suffix.len());
    let mut end = end_limit.min(clean.len());
    while end > 0 && !clean.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}{}", &clean[..end], suffix), true)
}

fn redact_and_strip_controls(value: &str) -> String {
    let mut clean = String::with_capacity(value.len().min(MAX_TIMELINE_ITEM_BYTES));
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
            continue;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            clean.push('�');
        } else {
            clean.push(ch);
        }
        if clean.len() >= MAX_TIMELINE_SCAN_BYTES {
            break;
        }
    }

    const SECRET_MARKERS: [&str; 5] =
        ["authorization:", "bearer ", "token=", "api_key=", "secret="];
    let lower = clean.to_ascii_lowercase();
    let mut redacted = String::with_capacity(clean.len());
    let mut cursor = 0;
    while cursor < clean.len() {
        let Some((marker_index, marker)) = SECRET_MARKERS
            .iter()
            .filter_map(|marker| {
                lower[cursor..]
                    .find(marker)
                    .map(|offset| (cursor + offset, *marker))
            })
            .min_by_key(|(index, _)| *index)
        else {
            redacted.push_str(&clean[cursor..]);
            break;
        };
        redacted.push_str(&clean[cursor..marker_index]);
        let value_start = marker_index + marker.len();
        redacted.push_str(&clean[marker_index..value_start]);
        let mut value_end = value_start;
        if marker == "authorization:" {
            // An Authorization header is a scheme plus credentials. Consuming only the first
            // token would turn `Authorization: Bearer secret` into a leaked trailing `secret`.
            while value_end < clean.len() {
                let ch = clean[value_end..].chars().next().unwrap_or_default();
                if matches!(ch, '\r' | '\n') {
                    break;
                }
                value_end += ch.len_utf8();
            }
        } else {
            while value_end < clean.len() {
                let ch = clean[value_end..].chars().next().unwrap_or_default();
                if ch.is_whitespace() {
                    break;
                }
                value_end += ch.len_utf8();
            }
        }
        redacted.push_str("[REDACTED]");
        cursor = value_end;
    }
    let redacted = redact_text(&redacted);
    if scan_secret_sentinels(SecretScanChannel::Transcript, &redacted).is_err() {
        return "[REDACTED]".to_owned();
    }
    redacted
}
