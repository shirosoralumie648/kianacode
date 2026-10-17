//! Bounded Workbench timeline projection.
//!
//! This module turns the additive run stream into disposable, plain-text timeline items. It does
//! not decide run state, execute tool calls, parse markdown, or replace EventLog/Receipt facts.
//! Cursor/epoch checks are retained at this boundary so a display gap cannot look like success.
//!
//! The projection is append-only: an item is written once and later events can neither rewrite an
//! earlier fact (arrival order is not authority) nor clear a recorded error, tool result, usage row
//! or approval. Binary MIME artifacts are never inlined, and delta/JSON/diff bodies are bounded by
//! the per-kind truncation policy below. Display roles are owned here so an entrypoint cannot
//! re-label a tool result as assistant prose.

use kiana_protocol::{
    redact_text, scan_secret_sentinels, ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent,
    SecretScanChannel, UiConnectorHealthProjection, UiCursor, PROTOCOL_SCHEMA,
};
use serde_json::Value;

pub const MAX_TIMELINE_ITEMS: usize = 512;
pub const MAX_TIMELINE_BYTES: usize = 512 * 1024;
pub const MAX_TIMELINE_ITEM_BYTES: usize = 32 * 1024;
pub const MAX_TIMELINE_JSON_BYTES: usize = 16 * 1024;
/// Text/diff artifacts are capped separately: they are the largest single body the Workbench can
/// receive, and a diff must not be able to fill the timeline budget on its own.
pub const MAX_TIMELINE_DIFF_BYTES: usize = 16 * 1024;
/// One terminal response may reference many files; only the first slice is projected so a hostile
/// response cannot multiply one event into an unbounded item burst.
pub const MAX_TIMELINE_ARTIFACTS_PER_RESPONSE: usize = 64;
const TIMELINE_LIMIT_BODY: &str = "render limit reached; additional stream content is hidden";
const BINARY_ARTIFACT_MARKER: &str = "binary artifact not rendered inline";
const UNSPECIFIED_PAYLOAD_MARKER: &str = "unspecified artifact payload not rendered inline";
const MAX_TIMELINE_SCAN_BYTES: usize = MAX_TIMELINE_ITEM_BYTES + 1024;
const MAX_ARTIFACT_LABEL_CHARS: usize = 256;

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

/// 把连接器健康数据投影成一行行纯文本。
///
/// 【它是投影，不是判定】
/// 输入是一段 JSON，输出是给人看的行。**它不判断连接器是否健康**——
/// 上游说 degraded，这里就显示 degraded；上游说错，它就显示错。
///
/// ⚠ 所以它绝不能被当作健康结论的依据。要「这个连接器现在到底行不行」的答案，
///    得问控制面，不是问这块渲染代码。
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

/// Entry-point independent display role of a timeline item.
///
/// Only model prose and the terminal response may be rendered as assistant text. Tool results,
/// usage records, approvals, errors, gaps, unknown markers and limit markers are system rows:
/// rendering them as assistant prose would let recorded facts read as instructions or answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineDisplayRole {
    Assistant,
    System,
    Changed,
}

impl TimelineItemKind {
    /// The single authority for how an item kind is labelled by every entrypoint.
    pub fn display_role(self) -> TimelineDisplayRole {
        match self {
            Self::Delta | Self::Terminal => TimelineDisplayRole::Assistant,
            Self::Artifact => TimelineDisplayRole::Changed,
            Self::Usage
            | Self::ToolCall
            | Self::Approval
            | Self::Error
            | Self::Unknown
            | Self::Gap
            | Self::Limit => TimelineDisplayRole::System,
        }
    }
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
/// 把 run 流渲染成一条**有界**的时间线。
///
/// 【「有界」是这个类型存在的理由】
/// 一次长 run 可能产生成千上万条流事件。全部留着，界面会越来越慢、
/// 内存会一直涨。`TimelineRenderer` 在超限时丢弃最旧的项，
/// 从而保证渲染的代价与 run 的长度无关。
///
/// 【⚠ 丢弃是有代价的，而且这个代价必须是显式的】
/// 被丢掉的那部分**从界面上消失了**。所以时间线只能回答
/// 「最近发生了什么」，回答不了「从头到尾发生了什么」。
/// 要后者，去看 EventLog——那才是完整的。
///
/// 文件头那句「Cursor/epoch checks are retained at this boundary so a display gap
/// cannot look like success」说的就是这件事：即便有丢弃，游标与世代的检查仍然保留，
/// 免得「界面没显示」被读成「没发生」。
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
    /// advanced by the daemon subscription path. It keeps the same gap/terminal fences: once the
    /// projection has reported a gap, no further stream row is rendered until a snapshot hydrates.
    pub fn apply_event(&mut self, event: RunStreamEvent) -> Result<TimelineApply, String> {
        if self.terminal {
            return Ok(TimelineApply::IgnoredAfterTerminal);
        }
        self.ensure_run_id(&event)?;
        if self.gap_detected {
            return Ok(TimelineApply::GapRequiresSnapshot);
        }
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
        let mut projected = 0_usize;
        for key in ["files_changed", "artifacts"] {
            let Some(values) = response.output.get(key).and_then(Value::as_array) else {
                continue;
            };
            for (index, value) in values.iter().enumerate() {
                if projected >= MAX_TIMELINE_ARTIFACTS_PER_RESPONSE {
                    return Ok(());
                }
                projected += 1;
                self.add_item(
                    format!("{parent}:{key}:artifact:{index}"),
                    TimelineItemKind::Artifact,
                    &artifact_body(value),
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
        let (body, truncated) = bounded_item_body(kind, body);
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

/// 从一个响应信封里取出该给人看的终止文本。
///
/// 【⚠ 它不判断这次运行成功还是失败】
/// 它只取文本。成功、失败、被拒绝、结果未知，在这一层都只是「一段文字」。
/// 真正的结论在信封的 status 字段里，而那属于控制面。
///
/// 把两者混在一处，就会出现「界面上打着一句『已完成』，而 status 是
/// `result_unknown`」的情况——这正是本仓库最忌讳的那类显示与事实不一致。
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

/// Per-kind truncation policy. Delta/Terminal keep the item budget; structured JSON rows and
/// text/diff artifacts have tighter caps so one record cannot crowd out the rest of the timeline.
fn bounded_item_body(kind: TimelineItemKind, body: &str) -> (String, bool) {
    let limit = match kind {
        TimelineItemKind::Usage
        | TimelineItemKind::ToolCall
        | TimelineItemKind::Approval
        | TimelineItemKind::Error => MAX_TIMELINE_JSON_BYTES,
        TimelineItemKind::Artifact => MAX_TIMELINE_DIFF_BYTES,
        TimelineItemKind::Delta
        | TimelineItemKind::Terminal
        | TimelineItemKind::Unknown
        | TimelineItemKind::Gap
        | TimelineItemKind::Limit => MAX_TIMELINE_ITEM_BYTES,
    };
    bounded_plain_text(body, limit)
}

/// Render one `files_changed`/`artifacts` entry.
///
/// A binary MIME (`application/octet-stream`, `image/*`, any non-text type) or an explicitly
/// encoded payload (`encoding: base64|hex`) is summarized by its label and MIME only: the bytes are
/// never decoded, inlined or re-encoded. Text/diff artifacts are inlined and bounded by
/// [`MAX_TIMELINE_DIFF_BYTES`]. A payload with no declared text shape is treated as unspecified and
/// is not inlined either, so an attacker cannot smuggle a blob through a missing MIME.
fn artifact_body(value: &Value) -> String {
    if !value.is_object() {
        return match value {
            Value::String(text) => truncate_for_inspection(text),
            other => truncate_for_inspection(&json_text(other)),
        };
    }
    let label = artifact_label(value);
    if binary_artifact(value) {
        return match label {
            Some(label) => format!("{label}: {BINARY_ARTIFACT_MARKER}"),
            None => BINARY_ARTIFACT_MARKER.to_owned(),
        };
    }
    match artifact_inline_text(value) {
        Some(text) => match label {
            Some(label) => format!("{label}: {}", truncate_for_inspection(&text)),
            None => truncate_for_inspection(&text),
        },
        None => {
            if has_opaque_payload(value) {
                match label {
                    Some(label) => format!("{label}: {UNSPECIFIED_PAYLOAD_MARKER}"),
                    None => UNSPECIFIED_PAYLOAD_MARKER.to_owned(),
                }
            } else {
                truncate_for_inspection(&json_text(value))
            }
        }
    }
}

/// True when the artifact declares a non-text MIME or a non-plain encoding.
fn binary_artifact(value: &Value) -> bool {
    if let Some(encoding) = value.get("encoding").and_then(Value::as_str) {
        let encoding = encoding.trim().to_ascii_lowercase();
        if !matches!(encoding.as_str(), "" | "utf-8" | "utf8" | "plain" | "text") {
            return true;
        }
    }
    match text_mime(value) {
        Some(_) => false,
        None => value.get("mime").is_some() || value.get("mime_type").is_some(),
    }
}

/// The declared MIME, when it is a text shape the Workbench may render inline.
fn text_mime(value: &Value) -> Option<String> {
    let mime = value
        .get("mime")
        .or_else(|| value.get("mime_type"))
        .and_then(Value::as_str)?
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if mime.is_empty() {
        return None;
    }
    let text_like = mime.starts_with("text/")
        || matches!(
            mime.as_str(),
            "application/json"
                | "application/x-ndjson"
                | "application/jsonl"
                | "application/xml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/javascript"
                | "application/typescript"
                | "application/sql"
                | "application/x-diff"
                | "application/diff"
        );
    text_like.then_some(mime)
}

fn artifact_inline_text(value: &Value) -> Option<String> {
    let declared_text = text_mime(value).is_some();
    for key in ["diff", "patch", "text", "snippet", "preview"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text.to_owned());
        }
    }
    if declared_text {
        for key in ["content", "body", "data"] {
            if let Some(text) = value.get(key).and_then(Value::as_str) {
                return Some(text.to_owned());
            }
        }
    }
    None
}

/// A payload field that exists but has no declared text shape must not be inlined or echoed.
fn has_opaque_payload(value: &Value) -> bool {
    for key in ["data", "content", "body", "base64", "blob"] {
        if value.get(key).is_some() {
            return true;
        }
    }
    // A numeric `bytes` field is a size descriptor, not a payload.
    value.get("bytes").is_some_and(|bytes| !bytes.is_number())
}

fn artifact_label(value: &Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for key in ["id", "path", "name"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            let label = text
                .chars()
                .filter(|ch| !ch.is_control())
                .take(MAX_ARTIFACT_LABEL_CHARS)
                .collect::<String>();
            if !label.trim().is_empty() && !parts.contains(&label) {
                parts.push(label);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Cap a value before composing a body so a hostile multi-megabyte diff is inspected only up to a
/// bounded prefix (char-safe; at most 4 bytes per scalar).
fn truncate_for_inspection(value: &str) -> String {
    if value.len() <= MAX_TIMELINE_DIFF_BYTES {
        return value.to_owned();
    }
    value.chars().take(MAX_TIMELINE_DIFF_BYTES).collect()
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
        let mut value_start = marker_index + marker.len();
        if marker == "authorization:" {
            // Preserve horizontal header spacing while consuming both scheme and credentials.
            while value_start < clean.len() && matches!(clean.as_bytes()[value_start], b' ' | b'\t')
            {
                value_start += 1;
            }
        }
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
