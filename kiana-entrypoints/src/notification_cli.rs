//! CLI/TTY notification inbox and run-status presenter.
//!
//! This module is a bounded view adapter. It accepts server-produced page/frame DTOs, strips
//! payload/detail authority, and returns display rows. It never submits an action, changes read or
//! HumanTask state, retries a run, or starts another execution loop.

use kiana_protocol::{UiFeedFrameKind, UiFeedFrameV1};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CLI_NOTIFICATION_VIEW_SCHEMA: &str = "kiana.cli-notification-view.v1";
pub const CLI_RUN_STATUS_VIEW_SCHEMA: &str = "kiana.cli-run-status-view.v1";
pub const CLI_NOTIFICATION_MAX_ITEMS: usize = 30;
pub const CLI_NOTIFICATION_MAX_ACTIONS: usize = 16;

fn required(value: &str, field: &'static str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliNotificationAction {
    pub action_id: String,
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliNotificationRow {
    pub item_id: String,
    pub kind: String,
    pub title: String,
    pub urgency: String,
    pub due_at_unix_ms: u64,
    pub read: bool,
    pub acknowledged: bool,
    pub snoozed: bool,
    pub actions: Vec<CliNotificationAction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliNotificationView {
    pub schema: String,
    pub source_cursor: u64,
    pub rows: Vec<CliNotificationRow>,
    pub next_after_item_id: Option<String>,
    pub unknown_visible: bool,
    pub limitations: Vec<String>,
}

pub fn present_notification_page(page: &Value) -> Result<CliNotificationView, String> {
    if page.get("schema").and_then(Value::as_str) != Some("kiana.notification-page.v1") {
        return Err("cli_notification_page_schema_invalid".to_owned());
    }
    let source_cursor = page
        .get("source_cursor")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "cli_notification_page_cursor_invalid".to_owned())?;
    let values = page
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "cli_notification_page_items_invalid".to_owned())?;
    if values.len() > CLI_NOTIFICATION_MAX_ITEMS {
        return Err("cli_notification_page_limit".to_owned());
    }
    let mut rows = Vec::with_capacity(values.len());
    for value in values {
        let item = value
            .get("item")
            .ok_or_else(|| "cli_notification_item_missing".to_owned())?;
        let item_id = item
            .get("item_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "cli_notification_item_id_missing".to_owned())?;
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| "cli_notification_kind_missing".to_owned())?;
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| "cli_notification_title_missing".to_owned())?;
        required(item_id, "cli_notification_item_id", 512)?;
        required(kind, "cli_notification_kind", 64)?;
        required(title, "cli_notification_title", 512)?;
        let urgency = value
            .get("urgency")
            .and_then(Value::as_str)
            .ok_or_else(|| "cli_notification_urgency_missing".to_owned())?;
        required(urgency, "cli_notification_urgency", 32)?;
        let due_at_unix_ms = value
            .get("due_at_unix_ms")
            .and_then(Value::as_u64)
            .ok_or_else(|| "cli_notification_due_missing".to_owned())?;
        let actions = item
            .get("actions")
            .and_then(Value::as_array)
            .ok_or_else(|| "cli_notification_actions_missing".to_owned())?;
        if actions.len() > CLI_NOTIFICATION_MAX_ACTIONS {
            return Err("cli_notification_actions_limit".to_owned());
        }
        let mut safe_actions = Vec::with_capacity(actions.len());
        for action in actions {
            let action_id = action
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "cli_notification_action_id_missing".to_owned())?;
            let command = action
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| "cli_notification_action_command_missing".to_owned())?;
            required(action_id, "cli_notification_action_id", 256)?;
            required(command, "cli_notification_action_command", 128)?;
            safe_actions.push(CliNotificationAction {
                action_id: action_id.to_owned(),
                command: command.to_owned(),
            });
        }
        rows.push(CliNotificationRow {
            item_id: item_id.to_owned(),
            kind: kind.to_owned(),
            title: title.to_owned(),
            urgency: urgency.to_owned(),
            due_at_unix_ms,
            read: value.get("read").and_then(Value::as_bool).unwrap_or(false),
            acknowledged: value
                .get("acknowledged")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            snoozed: value
                .get("snoozed_until_unix_ms")
                .and_then(Value::as_u64)
                .is_some(),
            actions: safe_actions,
        });
    }
    Ok(CliNotificationView {
        schema: CLI_NOTIFICATION_VIEW_SCHEMA.to_owned(),
        source_cursor,
        rows,
        next_after_item_id: page
            .get("next_after_item_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        unknown_visible: true,
        limitations: vec![
            "actions_are_display_only_until_re_admitted_by_control_plane".to_owned(),
            "presenter_does_not_prove_durable_or_live_delivery".to_owned(),
        ],
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliRunStatus {
    Running,
    Heartbeat,
    Unknown,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliRunStatusView {
    pub schema: String,
    pub status: CliRunStatus,
    pub instance_id: String,
    pub authority_epoch: String,
    pub feed_sequence: u64,
    pub source_cursor: u64,
    pub snapshot_required: bool,
    pub retry: String,
}

pub fn present_run_status(frame: &UiFeedFrameV1) -> Result<CliRunStatusView, String> {
    frame.validate()?;
    let (status, snapshot_required, retry) = match frame.kind.clone() {
        UiFeedFrameKind::Gap => (CliRunStatus::Unknown, true, "query_original"),
        UiFeedFrameKind::Heartbeat => (CliRunStatus::Heartbeat, false, "do_not_retry"),
        UiFeedFrameKind::Terminal => (CliRunStatus::Terminal, false, "do_not_retry"),
        UiFeedFrameKind::Unknown => (CliRunStatus::Unknown, true, "query_original"),
        UiFeedFrameKind::SnapshotBoundary | UiFeedFrameKind::Delta => {
            (CliRunStatus::Running, false, "do_not_retry")
        }
    };
    Ok(CliRunStatusView {
        schema: CLI_RUN_STATUS_VIEW_SCHEMA.to_owned(),
        status,
        instance_id: frame.cursor.instance_id.clone(),
        authority_epoch: frame.cursor.authority_epoch.clone(),
        feed_sequence: frame.cursor.feed_sequence,
        source_cursor: frame.cursor.snapshot_cursor.sequence,
        snapshot_required,
        retry: retry.to_owned(),
    })
}
