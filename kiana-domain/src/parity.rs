//! Cross-entrypoint projection parity contract.

use crate::{canonical_journal_bytes, json_digest, EventId, ExecutionStatus, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const ENTRYPOINT_PARITY_SCHEMA: &str = "kiana.entrypoint-parity.v1";
pub const ENTRYPOINT_PARITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_PARITY_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryPointKind {
    Cli,
    Tty,
    Web,
    Workbench,
    Desktop,
    Scheduler,
    Swarm,
    Connector,
}

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPointParitySnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub entrypoint: EntryPointKind,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub projection_version: u64,
    pub status: ExecutionStatus,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    #[serde(default)]
    pub audit_digest: Option<String>,
    #[serde(default)]
    pub health_digest: Option<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub snapshot_digest: String,
}

impl EntryPointParitySnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        entrypoint: EntryPointKind,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        projection_version: u64,
        status: ExecutionStatus,
        receipt_digest: Option<String>,
        audit_digest: Option<String>,
        health_digest: Option<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: ENTRYPOINT_PARITY_SCHEMA.to_owned(),
            version: ENTRYPOINT_PARITY_SCHEMA_VERSION,
            entrypoint,
            source_cursor,
            source_event_ids,
            projection_version,
            status,
            receipt_digest,
            audit_digest,
            health_digest,
            limitations,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ENTRYPOINT_PARITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&ENTRYPOINT_PARITY_SCHEMA_VERSION)
        {
            return Err("entrypoint_parity_schema_invalid".to_owned());
        }
        if self.source_cursor == 0 || self.projection_version == 0 {
            return Err("entrypoint_parity_cursor_required".to_owned());
        }
        if self.source_event_ids.is_empty() || self.source_event_ids.len() > 256 {
            return Err("entrypoint_parity_source_ids_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for event_id in &self.source_event_ids {
            if !ids.insert(event_id.to_string()) {
                return Err("entrypoint_parity_source_duplicate".to_owned());
            }
        }
        for (value, field) in [
            (
                self.receipt_digest.as_deref(),
                "entrypoint_parity_receipt_digest",
            ),
            (
                self.audit_digest.as_deref(),
                "entrypoint_parity_audit_digest",
            ),
            (
                self.health_digest.as_deref(),
                "entrypoint_parity_health_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        if self.status == ExecutionStatus::Completed && self.receipt_digest.is_none() {
            return Err("entrypoint_parity_completed_receipt_required".to_owned());
        }
        if self.limitations.len() > MAX_PARITY_LIMITATIONS {
            return Err("entrypoint_parity_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            nonempty(limitation, "entrypoint_parity_limitation", 256)?;
        }
        digest(&self.snapshot_digest, "entrypoint_parity_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("entrypoint_parity_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "snapshot_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
