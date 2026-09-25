//! Cross-entry notification/action/result parity contracts.
//!
//! CLI/TTY, Web, Desktop and Workbench are views of one server-owned projection. This comparator
//! only checks bounded IDs/cursors/digests; it does not compare prose, start a run, acknowledge an
//! item or create a second bus.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NOTIFICATION_ENTRYPOINT_SNAPSHOT_SCHEMA: &str =
    "kiana.notification-entrypoint-snapshot.v1";
pub const NOTIFICATION_ENTRYPOINT_PARITY_SCHEMA: &str = "kiana.notification-entrypoint-parity.v1";
pub const NOTIFICATION_PARITY_MAX_IDS: usize = 512;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn ids_valid(values: &[String], field: &str) -> Result<(), String> {
    if values.len() > NOTIFICATION_PARITY_MAX_IDS
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(format!("{field}_noncanonical"));
    }
    for value in values {
        required(value, field, 512)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEntrypoint {
    CliTty,
    Web,
    Desktop,
    Workbench,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationEntrypointSnapshot {
    pub schema: String,
    pub entrypoint: NotificationEntrypoint,
    pub instance_id: String,
    pub authority_epoch: String,
    pub source_cursor: u64,
    pub notification_ids: Vec<String>,
    pub action_ids: Vec<String>,
    pub terminal_result_ids: Vec<String>,
    pub pending_ids: Vec<String>,
    pub delivery_attempt_ids: Vec<String>,
    pub fresh_process_rebuilt: bool,
    pub projection_digest: String,
}

impl NotificationEntrypointSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_ENTRYPOINT_SNAPSHOT_SCHEMA || self.source_cursor == 0 {
            return Err("notification_entrypoint_snapshot_header_invalid".to_owned());
        }
        required(&self.instance_id, "notification_entrypoint_instance", 256)?;
        required(&self.authority_epoch, "notification_entrypoint_epoch", 256)?;
        ids_valid(
            &self.notification_ids,
            "notification_entrypoint_notification_ids",
        )?;
        ids_valid(&self.action_ids, "notification_entrypoint_action_ids")?;
        ids_valid(
            &self.terminal_result_ids,
            "notification_entrypoint_terminal_ids",
        )?;
        ids_valid(&self.pending_ids, "notification_entrypoint_pending_ids")?;
        ids_valid(
            &self.delivery_attempt_ids,
            "notification_entrypoint_delivery_ids",
        )?;
        digest(
            &self.projection_digest,
            "notification_entrypoint_projection_digest",
        )?;
        if self.projection_digest != self.digest() {
            return Err("notification_entrypoint_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "entrypoint": self.entrypoint,
            "instance_id": self.instance_id,
            "authority_epoch": self.authority_epoch,
            "source_cursor": self.source_cursor,
            "notification_ids": self.notification_ids,
            "action_ids": self.action_ids,
            "terminal_result_ids": self.terminal_result_ids,
            "pending_ids": self.pending_ids,
            "delivery_attempt_ids": self.delivery_attempt_ids,
            "fresh_process_rebuilt": self.fresh_process_rebuilt,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationParityDisposition {
    Consistent,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationEntrypointParity {
    pub schema: String,
    pub source_cursor: u64,
    pub authority_epoch: String,
    pub entrypoints: Vec<NotificationEntrypoint>,
    pub disposition: NotificationParityDisposition,
    pub mismatches: Vec<String>,
    pub next_action: String,
    pub report_digest: String,
}

impl NotificationEntrypointParity {
    pub fn compare(snapshots: Vec<NotificationEntrypointSnapshot>) -> Result<Self, String> {
        if snapshots.len() != 4 {
            return Err("notification_parity_four_entrypoints_required".to_owned());
        }
        for snapshot in &snapshots {
            snapshot.validate()?;
        }
        let mut entrypoints = snapshots
            .iter()
            .map(|snapshot| snapshot.entrypoint)
            .collect::<Vec<_>>();
        entrypoints.sort();
        entrypoints.dedup();
        if entrypoints.len() != 4 {
            return Err("notification_parity_duplicate_entrypoint".to_owned());
        }
        let first = &snapshots[0];
        let mut mismatches = Vec::new();
        for snapshot in snapshots.iter().skip(1) {
            if snapshot.source_cursor != first.source_cursor {
                mismatches.push(format!("source_cursor:{:?}", snapshot.entrypoint));
            }
            if snapshot.authority_epoch != first.authority_epoch {
                mismatches.push(format!("authority_epoch:{:?}", snapshot.entrypoint));
            }
            for (field, left, right) in [
                (
                    "notification_ids",
                    &first.notification_ids,
                    &snapshot.notification_ids,
                ),
                ("action_ids", &first.action_ids, &snapshot.action_ids),
                (
                    "terminal_result_ids",
                    &first.terminal_result_ids,
                    &snapshot.terminal_result_ids,
                ),
                ("pending_ids", &first.pending_ids, &snapshot.pending_ids),
                (
                    "delivery_attempt_ids",
                    &first.delivery_attempt_ids,
                    &snapshot.delivery_attempt_ids,
                ),
            ] {
                if left != right {
                    mismatches.push(format!("{field}:{:?}", snapshot.entrypoint));
                }
            }
        }
        if snapshots
            .iter()
            .any(|snapshot| !snapshot.fresh_process_rebuilt)
        {
            mismatches.push("fresh_process_rebuild_missing".to_owned());
        }
        mismatches.sort();
        mismatches.dedup();
        let disposition = if mismatches.is_empty() {
            NotificationParityDisposition::Consistent
        } else {
            NotificationParityDisposition::Unknown
        };
        let mut report = Self {
            schema: NOTIFICATION_ENTRYPOINT_PARITY_SCHEMA.to_owned(),
            source_cursor: first.source_cursor,
            authority_epoch: first.authority_epoch.clone(),
            entrypoints,
            disposition,
            mismatches,
            next_action: if disposition == NotificationParityDisposition::Consistent {
                "no_entrypoint_drift".to_owned()
            } else {
                "query_original_source_and_rebuild_snapshot".to_owned()
            },
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_ENTRYPOINT_PARITY_SCHEMA
            || self.source_cursor == 0
            || self.entrypoints.len() != 4
            || self.entrypoints.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("notification_parity_header_invalid".to_owned());
        }
        required(&self.authority_epoch, "notification_parity_epoch", 256)?;
        if self.mismatches.len() > 64 || self.mismatches.iter().any(|value| value.len() > 256) {
            return Err("notification_parity_mismatch_limit".to_owned());
        }
        required(&self.next_action, "notification_parity_next_action", 256)?;
        digest(&self.report_digest, "notification_parity_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("notification_parity_report_digest_mismatch".to_owned());
        }
        if self.disposition == NotificationParityDisposition::Consistent
            && !self.mismatches.is_empty()
        {
            return Err("notification_parity_consistent_with_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "entrypoints": self.entrypoints,
            "disposition": self.disposition,
            "mismatches": self.mismatches,
            "next_action": self.next_action,
        }))
    }
}
