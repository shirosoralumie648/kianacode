//! Web Human Inbox boundary.
//!
//! The loopback server accepts a versioned, deny-first action intent and forwards it through the
//! existing `human.resolve` command path.  This module performs shape validation only; actor,
//! approval, scope, expiry and policy remain server-owned in ControlPlane/Core.

use kiana_protocol::{
    json_digest, UiHumanActionIntentV1, UI_HUMAN_ACTION_CARD_SCHEMA, UI_HUMAN_ACTION_INTENT_SCHEMA,
    UI_HUMAN_INBOX_SCHEMA,
};
use serde_json::{json, Value};

pub const WEB_HUMAN_INBOX_ROUTE_SCHEMA: &str = "kiana.web-human-inbox-route.v1";
pub const WEB_HUMAN_RESOLVE_COMMAND: &str = "human.resolve";

/// Validate a Web human action before it reaches the existing command facade.  A normal read
/// command is left untouched; `human.resolve` must carry the typed intent and cannot carry actor,
/// scope or approval-grant fields supplied by the browser.
pub(crate) fn validate_human_action_command(
    command: &str,
    arguments: &Value,
) -> Result<(), String> {
    if command != WEB_HUMAN_RESOLVE_COMMAND {
        return Ok(());
    }
    let intent: UiHumanActionIntentV1 = serde_json::from_value(arguments.clone())
        .map_err(|_| "web_human_action_intent_invalid".to_owned())?;
    if intent.schema != UI_HUMAN_ACTION_INTENT_SCHEMA {
        return Err("web_human_action_intent_schema_invalid".to_owned());
    }
    intent.validate()
}

/// Return a stable display classification for a command response.  `unknown` is kept distinct
/// from rejection so the browser can query the original command instead of retrying blindly.
#[allow(dead_code)]
pub(crate) fn human_result_class(response: &Value) -> &'static str {
    match response.get("status").and_then(Value::as_str) {
        Some("completed") => "applied",
        Some("accepted") | Some("running") | Some("awaiting_approval") => "accepted",
        Some("unknown") | Some("result_unknown") => "unknown",
        Some("blocked") | Some("rejected") | Some("failed") => "rejected",
        _ => "unknown",
    }
}

/// Additive server-owned metadata to the existing `human.inbox` response.  The original item and
/// action arguments remain available to the ControlPlane command path; the Web card contains only
/// redacted display fields and a digest, so the browser cannot treat its copy as authority.
pub(crate) fn annotate_human_inbox_response(response: &mut Value) {
    if response.get("status").and_then(Value::as_str) != Some("completed") {
        return;
    }
    let Some(output) = response.get_mut("output").and_then(Value::as_object_mut) else {
        return;
    };
    output.insert("schema".to_owned(), json!(UI_HUMAN_INBOX_SCHEMA));
    output.insert(
        "action_card_schema".to_owned(),
        json!(UI_HUMAN_ACTION_CARD_SCHEMA),
    );
    let Some(items) = output.get_mut("items").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        let item_id = item
            .get("item_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let source_ref = item
            .get("source_ref")
            .and_then(Value::as_str)
            .unwrap_or(&item_id)
            .to_owned();
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Human action")
            .to_owned();
        let detail = item.get("detail").cloned().unwrap_or(Value::Null);
        let expected_revision = detail
            .get("expected_version")
            .and_then(Value::as_u64)
            .or_else(|| detail.get("company_revision").and_then(Value::as_u64));
        let expires_at_unix_ms = detail
            .get("challenge")
            .and_then(|challenge| challenge.get("expires_at_unix_ms"))
            .and_then(Value::as_u64);
        let reason = detail
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or(&title)
            .to_owned();
        let scope_summary = detail
            .get("scope")
            .and_then(|scope| scope.get("summary"))
            .and_then(Value::as_str)
            .unwrap_or(&source_ref)
            .to_owned();
        let scope_digest = json_digest(&json!({"item_id": &item_id, "source_ref": &source_ref}));
        let Some(actions) = item.get_mut("actions").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut cards = Vec::with_capacity(actions.len());
        for action in actions.iter() {
            let action_id = action
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let command = action
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("human.resolve")
                .to_owned();
            let arguments = action.get("arguments").cloned().unwrap_or(Value::Null);
            let payload_digest = json_digest(&arguments);
            let fields = action
                .get("required_fields")
                .and_then(Value::as_array)
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|name| {
                            json!({
                                "name": name,
                                "label": name,
                                "field_type": "json",
                                "required": true,
                                "allowed_values": []
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            cards.push(json!({
                "schema": UI_HUMAN_ACTION_CARD_SCHEMA,
                "item_id": &item_id,
                "action_id": &action_id,
                "command": &command,
                "target_id": &source_ref,
                "reason": &reason,
                "scope": {"summary": &scope_summary, "digest": &scope_digest, "paths": []},
                "expires_at_unix_ms": expires_at_unix_ms,
                "revoked": false,
                "expected_revision": expected_revision,
                "fields": fields,
                "allowed_decisions": [&action_id],
                "payload_digest": &payload_digest
            }));
        }
        if let Some(item_object) = item.as_object_mut() {
            item_object.insert("action_cards".to_owned(), Value::Array(cards));
        }
    }
}
