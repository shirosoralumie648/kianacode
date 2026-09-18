//! Read-only JSONL integrity scan and fail-closed health gate.

use crate::JsonlEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityScanStatus {
    Empty,
    Ready,
    Corrupt,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrityScanReport {
    pub schema: String,
    pub path: PathBuf,
    pub status: IntegrityScanStatus,
    pub source_cursor: u64,
    pub quarantine_required: bool,
    pub incident_code: Option<String>,
    pub recovery_action: String,
}

impl IntegrityScanReport {
    pub const SCHEMA: &'static str = "kiana.eventlog-integrity-report.v1";

    pub fn health_gate(&self) -> Result<(), PortError> {
        match self.status {
            IntegrityScanStatus::Ready | IntegrityScanStatus::Empty => Ok(()),
            IntegrityScanStatus::Corrupt => Err(PortError::Conflict(
                "eventlog_integrity_quarantine_required".to_owned(),
            )),
            IntegrityScanStatus::Unknown => Err(PortError::Unavailable(
                "eventlog_integrity_unknown".to_owned(),
            )),
        }
    }
}

fn classify_error(error: &PortError) -> (IntegrityScanStatus, String) {
    let text = error.to_string();
    if text.contains("corrupt")
        || text.contains("checksum")
        || text.contains("integrity")
        || text.contains("frame_invalid")
    {
        (
            IntegrityScanStatus::Corrupt,
            "quarantine_and_reconcile".to_owned(),
        )
    } else {
        (
            IntegrityScanStatus::Unknown,
            "pause_and_reconcile".to_owned(),
        )
    }
}

pub async fn scan_jsonl(path: impl AsRef<Path>) -> IntegrityScanReport {
    let path = path.as_ref().to_path_buf();
    match JsonlEventLog::open_async(&path).await {
        Ok(store) => match store.read_all().await {
            Ok(events) => IntegrityScanReport {
                schema: IntegrityScanReport::SCHEMA.to_owned(),
                path,
                status: if events.is_empty() {
                    IntegrityScanStatus::Empty
                } else {
                    IntegrityScanStatus::Ready
                },
                source_cursor: events.len() as u64,
                quarantine_required: false,
                incident_code: None,
                recovery_action: "continue".to_owned(),
            },
            Err(error) => {
                let (status, action) = classify_error(&error);
                IntegrityScanReport {
                    schema: IntegrityScanReport::SCHEMA.to_owned(),
                    path,
                    status,
                    source_cursor: 0,
                    quarantine_required: status == IntegrityScanStatus::Corrupt,
                    incident_code: Some(error.to_string()),
                    recovery_action: action,
                }
            }
        },
        Err(error) => {
            let (status, action) = classify_error(&error);
            IntegrityScanReport {
                schema: IntegrityScanReport::SCHEMA.to_owned(),
                path,
                status,
                source_cursor: 0,
                quarantine_required: status == IntegrityScanStatus::Corrupt,
                incident_code: Some(error.to_string()),
                recovery_action: action,
            }
        }
    }
}
