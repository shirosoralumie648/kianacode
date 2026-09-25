//! Web notification page and SSE presentation contracts.
//!
//! This module accepts server-produced page/feed DTOs and keeps only bounded, redaction-safe
//! display fields.  It never submits an action, copies HumanTask status, retries a run, or turns
//! a feed gap into completion.

use kiana_protocol::{
    UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiFeedGapV1, UI_FEED_FRAME_SCHEMA,
};
use serde_json::{json, Value};

pub const WEB_NOTIFICATION_PAGE_SCHEMA: &str = "kiana.web-notification-page.v1";
pub const WEB_NOTIFICATION_SSE_SCHEMA: &str = "kiana.web-notification-sse.v1";
pub const WEB_NOTIFICATION_MAX_ITEMS: usize = 30;
pub const WEB_NOTIFICATION_MAX_ACTIONS: usize = 16;
pub const MAX_WEB_NOTIFICATION_CURSOR_BYTES: usize = 2_048;
pub const MAX_WEB_NOTIFICATION_SSE_EVENT_BYTES: usize = 256 * 1024;
const MAX_TEXT_BYTES: usize = 512;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn required_cursor_context(
    session_id: &str,
    tab_id: &str,
    instance_id: &str,
    authority_epoch: &str,
) -> Result<(), String> {
    required(session_id, "web_notification_session", 256)?;
    required(tab_id, "web_notification_tab", 256)?;
    required(instance_id, "web_notification_instance", 256)?;
    required(authority_epoch, "web_notification_epoch", 256)
}

fn bounded_payload(payload: Value) -> Result<Value, String> {
    let bytes = serde_json::to_vec(&payload)
        .map_err(|_| "web_notification_payload_encode_failed".to_owned())?;
    if bytes.len() > MAX_WEB_NOTIFICATION_SSE_EVENT_BYTES {
        return Err("web_notification_payload_too_large".to_owned());
    }
    Ok(payload)
}

/// Keep only the fields a browser inbox needs.  `detail`, `source_ref`, action arguments and
/// other payload-shaped values never cross this presentation boundary.
pub fn present_notification_page(
    page: &Value,
    session_id: &str,
    tab_id: &str,
    instance_id: &str,
    authority_epoch: &str,
) -> Result<Value, String> {
    required_cursor_context(session_id, tab_id, instance_id, authority_epoch)?;
    if page.get("schema").and_then(Value::as_str) != Some("kiana.notification-page.v1") {
        return Err("web_notification_page_schema_invalid".to_owned());
    }
    let source_cursor = page
        .get("source_cursor")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "web_notification_page_cursor_invalid".to_owned())?;
    let values = page
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "web_notification_page_items_invalid".to_owned())?;
    if values.len() > WEB_NOTIFICATION_MAX_ITEMS {
        return Err("web_notification_page_limit".to_owned());
    }
    let mut items = Vec::with_capacity(values.len());
    for value in values {
        let item = value
            .get("item")
            .ok_or_else(|| "web_notification_item_missing".to_owned())?;
        let item_id = item
            .get("item_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "web_notification_item_id_missing".to_owned())?;
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| "web_notification_kind_missing".to_owned())?;
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| "web_notification_title_missing".to_owned())?;
        let urgency = value
            .get("urgency")
            .and_then(Value::as_str)
            .ok_or_else(|| "web_notification_urgency_missing".to_owned())?;
        let due_at_unix_ms = value
            .get("due_at_unix_ms")
            .and_then(Value::as_u64)
            .ok_or_else(|| "web_notification_due_missing".to_owned())?;
        required(item_id, "web_notification_item_id", MAX_TEXT_BYTES)?;
        required(kind, "web_notification_kind", 64)?;
        required(title, "web_notification_title", MAX_TEXT_BYTES)?;
        required(urgency, "web_notification_urgency", 32)?;
        let actions = item
            .get("actions")
            .and_then(Value::as_array)
            .ok_or_else(|| "web_notification_actions_missing".to_owned())?;
        if actions.len() > WEB_NOTIFICATION_MAX_ACTIONS {
            return Err("web_notification_actions_limit".to_owned());
        }
        let mut safe_actions = Vec::with_capacity(actions.len());
        for action in actions {
            let action_id = action
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "web_notification_action_id_missing".to_owned())?;
            let label = action
                .get("label")
                .and_then(Value::as_str)
                .ok_or_else(|| "web_notification_action_label_missing".to_owned())?;
            let command = action
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| "web_notification_action_command_missing".to_owned())?;
            required(action_id, "web_notification_action_id", 256)?;
            required(label, "web_notification_action_label", MAX_TEXT_BYTES)?;
            required(command, "web_notification_action_command", 256)?;
            safe_actions.push(json!({
                "id": action_id,
                "label": label,
                "command": command,
            }));
        }
        items.push(json!({
            "item_id": item_id,
            "kind": kind,
            "title": title,
            "urgency": urgency,
            "due_at_unix_ms": due_at_unix_ms,
            "read": value.get("read").and_then(Value::as_bool).unwrap_or(false),
            "acknowledged": value
                .get("acknowledged")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "snoozed": value
                .get("snoozed_until_unix_ms")
                .and_then(Value::as_u64)
                .is_some(),
            "actions": safe_actions,
        }));
    }
    let next_after_item_id = page
        .get("next_after_item_id")
        .and_then(Value::as_str)
        .map(|value| {
            required(value, "web_notification_next_after", MAX_TEXT_BYTES)?;
            Ok::<_, String>(value)
        })
        .transpose()?;
    let page_digest = page
        .get("page_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| "web_notification_page_digest_missing".to_owned())?;
    required(page_digest, "web_notification_page_digest", 128)?;
    bounded_payload(json!({
        "schema": WEB_NOTIFICATION_PAGE_SCHEMA,
        "session_id": session_id,
        "tab_id": tab_id,
        "instance_id": instance_id,
        "authority_epoch": authority_epoch,
        "source_cursor": source_cursor,
        "items": items,
        "next_after_item_id": next_after_item_id,
        "page_digest": page_digest,
        "unknown_visible": true,
        "limitations": [
            "actions_are_display_only_until_re_admitted_by_control_plane",
            "page_is_a_rebuildable_projection_not_a_durable_read_receipt",
            "sse_gap_or_auth_error_requires_snapshot_hydration"
        ]
    }))
}

pub fn notification_frame_event_name(frame: &UiFeedFrameV1) -> &'static str {
    match &frame.kind {
        UiFeedFrameKind::SnapshotBoundary => "snapshot_boundary",
        UiFeedFrameKind::Delta => "delta",
        UiFeedFrameKind::Heartbeat => "heartbeat",
        UiFeedFrameKind::Gap => "stream_gap",
        UiFeedFrameKind::Terminal => "terminal",
        UiFeedFrameKind::Unknown => "unknown",
    }
}

/// Present a feed frame without copying the potentially sensitive RunStream event payload.
pub fn present_notification_frame(
    frame: &UiFeedFrameV1,
    session_id: &str,
    tab_id: &str,
) -> Result<Value, String> {
    frame.validate()?;
    required(session_id, "web_notification_session", 256)?;
    required(tab_id, "web_notification_tab", 256)?;
    let mut payload = json!({
        "schema": WEB_NOTIFICATION_SSE_SCHEMA,
        "session_id": session_id,
        "tab_id": tab_id,
        "kind": serde_json::to_value(&frame.kind)
            .map_err(|_| "web_notification_frame_kind_encode_failed")?,
        "event_id": frame.event_id,
        "cursor": frame.cursor,
        "replay": frame.replay,
        "terminal": frame.terminal,
        "snapshot_required": matches!(&frame.kind, &UiFeedFrameKind::Gap | &UiFeedFrameKind::Unknown),
        "retry": if matches!(&frame.kind, &UiFeedFrameKind::Gap | &UiFeedFrameKind::Unknown) {
            "query_original"
        } else {
            "do_not_retry"
        },
        "limitations": ["feed_payload_redacted; query_snapshot_for_authoritative_state"]
    });
    if let Some(gap) = &frame.gap {
        payload["gap"] = json!({
            "reason": serde_json::to_value(&gap.reason)
                .map_err(|_| "web_notification_gap_encode_failed")?,
            "snapshot_required": gap.snapshot_required,
        });
    }
    bounded_payload(payload)
}

pub fn notification_gap_frame(gap: UiFeedGapV1) -> Result<UiFeedFrameV1, String> {
    gap.validate()?;
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: UiFeedFrameKind::Gap,
        cursor: gap.to.clone(),
        event_id: format!("notification-gap:{}", gap.to.cursor_digest),
        replay: false,
        terminal: false,
        event: None,
        gap: Some(gap),
    };
    frame.validate()?;
    Ok(frame)
}

pub fn present_notification_stream_error(
    cursor: &UiFeedCursorV1,
    session_id: &str,
    tab_id: &str,
    error: &str,
) -> Result<Value, String> {
    cursor.validate()?;
    required(session_id, "web_notification_session", 256)?;
    required(tab_id, "web_notification_tab", 256)?;
    required(error, "web_notification_stream_error", 512)?;
    bounded_payload(json!({
        "schema": WEB_NOTIFICATION_SSE_SCHEMA,
        "session_id": session_id,
        "tab_id": tab_id,
        "kind": "stream_error",
        "cursor": cursor,
        "error": error,
        "snapshot_required": true,
        "retry": "query_original",
        "limitations": ["connection_or_auth_state_unknown; query notification snapshot"]
    }))
}

pub fn encode_notification_cursor(cursor: &UiFeedCursorV1) -> Result<String, String> {
    let encoded = cursor.encode()?;
    if encoded.len() > MAX_WEB_NOTIFICATION_CURSOR_BYTES {
        return Err("web_notification_cursor_too_large".to_owned());
    }
    Ok(encoded)
}

pub fn decode_notification_cursor(value: &str) -> Result<UiFeedCursorV1, String> {
    if value.trim().is_empty() || value.len() > MAX_WEB_NOTIFICATION_CURSOR_BYTES {
        return Err("web_notification_cursor_invalid".to_owned());
    }
    UiFeedCursorV1::decode(value)
}
