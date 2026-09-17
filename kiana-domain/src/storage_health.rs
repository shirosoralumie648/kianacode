//! Storage error, capability and health contracts.
//!
//! These types keep empty/unavailable/corrupt/conflict/unknown/result-unknown distinct at the
//! boundary. A health projection never grants authority and an incident never silently repairs a
//! fact stream.
use crate::{
    json_digest, redact_text, StorageErrorId, StorageHealthId, StorageIntegrityIncidentId,
    StoreIdentityId,
};
use serde::{Deserialize, Serialize};

pub const STORAGE_ERROR_SCHEMA: &str = "kiana.storage-error.v1";
pub const STORAGE_CAPABILITIES_SCHEMA: &str = "kiana.storage-capabilities.v1";
pub const STORAGE_HEALTH_SCHEMA: &str = "kiana.storage-health.v1";
pub const STORAGE_INTEGRITY_INCIDENT_SCHEMA: &str = "kiana.storage-integrity-incident.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageErrorClass {
    Empty,
    Unavailable,
    Corrupt,
    Conflict,
    Unknown,
    ResultUnknown,
}

impl StorageErrorClass {
    pub const fn retryable(self) -> bool {
        matches!(self, Self::Unavailable | Self::Conflict)
    }

    pub const fn requires_reconciliation(self) -> bool {
        matches!(self, Self::Corrupt | Self::Unknown | Self::ResultUnknown)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageRetryDisposition {
    Never,
    RetryAfterRead,
    Reconcile,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageError {
    pub schema: String,
    pub error_id: StorageErrorId,
    pub class: StorageErrorClass,
    pub code: String,
    pub message: String,
    pub retry: StorageRetryDisposition,
    pub source_cursor: Option<u64>,
    pub error_digest: String,
}

impl StorageError {
    pub fn new(
        class: StorageErrorClass,
        code: impl Into<String>,
        message: impl Into<String>,
        source_cursor: Option<u64>,
    ) -> Result<Self, String> {
        let retry = if class.requires_reconciliation() {
            StorageRetryDisposition::Reconcile
        } else if class.retryable() {
            StorageRetryDisposition::RetryAfterRead
        } else {
            StorageRetryDisposition::Never
        };
        let mut error = Self {
            schema: STORAGE_ERROR_SCHEMA.to_owned(),
            error_id: StorageErrorId::new(),
            class,
            code: code.into(),
            message: message.into(),
            retry,
            source_cursor,
            error_digest: String::new(),
        };
        error.error_digest = error.digest();
        error.validate()?;
        Ok(error)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_ERROR_SCHEMA
            || self.error_id.as_uuid().is_nil()
            || self.code.trim().is_empty()
            || self.code.len() > 256
            || self.message.trim().is_empty()
            || self.message.len() > 4_096
            || self.code.contains('\0')
            || self.message.contains('\0')
            || redact_text(&self.message) != self.message
            || self.source_cursor == Some(0)
        {
            return Err("storage_error_invalid".to_owned());
        }
        let expected = if self.class.requires_reconciliation() {
            StorageRetryDisposition::Reconcile
        } else if self.class.retryable() {
            StorageRetryDisposition::RetryAfterRead
        } else {
            StorageRetryDisposition::Never
        };
        if self.retry != expected {
            return Err("storage_error_retry_mismatch".to_owned());
        }
        validate_digest(&self.error_digest, "storage_error_digest")?;
        if self.error_digest != self.digest() {
            return Err("storage_error_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "error_id": self.error_id,
            "class": self.class,
            "code": self.code,
            "message": self.message,
            "retry": self.retry,
            "source_cursor": self.source_cursor,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageCapabilities {
    pub schema: String,
    pub durable_commits: bool,
    pub atomic_transitions: bool,
    pub cursor_reads: bool,
    pub command_receipts: bool,
    pub fsync: bool,
    pub max_frame_bytes: usize,
    pub max_batch_events: usize,
    pub capabilities_digest: String,
}

impl StorageCapabilities {
    pub fn new(
        durable_commits: bool,
        atomic_transitions: bool,
        cursor_reads: bool,
        command_receipts: bool,
        fsync: bool,
        max_frame_bytes: usize,
        max_batch_events: usize,
    ) -> Result<Self, String> {
        let mut capabilities = Self {
            schema: STORAGE_CAPABILITIES_SCHEMA.to_owned(),
            durable_commits,
            atomic_transitions,
            cursor_reads,
            command_receipts,
            fsync,
            max_frame_bytes,
            max_batch_events,
            capabilities_digest: String::new(),
        };
        capabilities.capabilities_digest = capabilities.digest();
        capabilities.validate()?;
        Ok(capabilities)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_CAPABILITIES_SCHEMA
            || self.max_frame_bytes == 0
            || self.max_batch_events == 0
        {
            return Err("storage_capabilities_invalid".to_owned());
        }
        if (self.durable_commits || self.atomic_transitions) && !self.fsync {
            return Err("storage_capabilities_durability_conflict".to_owned());
        }
        validate_digest(&self.capabilities_digest, "storage_capabilities_digest")?;
        if self.capabilities_digest != self.digest() {
            return Err("storage_capabilities_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "durable_commits": self.durable_commits,
            "atomic_transitions": self.atomic_transitions,
            "cursor_reads": self.cursor_reads,
            "command_receipts": self.command_receipts,
            "fsync": self.fsync,
            "max_frame_bytes": self.max_frame_bytes,
            "max_batch_events": self.max_batch_events,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageHealthStatus {
    Empty,
    Ready,
    Degraded,
    Unavailable,
    Corrupt,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageHealth {
    pub schema: String,
    pub health_id: StorageHealthId,
    pub store_id: StoreIdentityId,
    pub status: StorageHealthStatus,
    pub capabilities: StorageCapabilities,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub observed_at_unix_ms: u64,
    pub limitations: Vec<String>,
    pub health_digest: String,
}

impl StorageHealth {
    pub fn new(
        store_id: StoreIdentityId,
        status: StorageHealthStatus,
        capabilities: StorageCapabilities,
        source_cursor: u64,
        authority_epoch: u64,
        observed_at_unix_ms: u64,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut health = Self {
            schema: STORAGE_HEALTH_SCHEMA.to_owned(),
            health_id: StorageHealthId::new(),
            store_id,
            status,
            capabilities,
            source_cursor,
            authority_epoch,
            observed_at_unix_ms,
            limitations,
            health_digest: String::new(),
        };
        health.health_digest = health.digest();
        health.validate()?;
        Ok(health)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_HEALTH_SCHEMA
            || self.health_id.as_uuid().is_nil()
            || self.store_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.source_cursor == 0
            || self.limitations.len() > 32
            || self
                .limitations
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 512 || value.contains('\0'))
        {
            return Err("storage_health_invalid".to_owned());
        }
        self.capabilities.validate()?;
        validate_digest(&self.health_digest, "storage_health_digest")?;
        if self.health_digest != self.digest() {
            return Err("storage_health_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "health_id": self.health_id,
            "store_id": self.store_id,
            "status": self.status,
            "capabilities": self.capabilities,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageIntegrityIncidentClass {
    Corrupt,
    Unknown,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageIntegrityIncident {
    pub schema: String,
    pub incident_id: StorageIntegrityIncidentId,
    pub store_id: StoreIdentityId,
    pub class: StorageIntegrityIncidentClass,
    pub code: String,
    pub detected_at_unix_ms: u64,
    pub source_cursor: Option<u64>,
    pub quarantine_required: bool,
    #[serde(default)]
    pub resolved_at_unix_ms: Option<u64>,
    pub incident_digest: String,
}

impl StorageIntegrityIncident {
    pub fn new(
        store_id: StoreIdentityId,
        class: StorageIntegrityIncidentClass,
        code: impl Into<String>,
        detected_at_unix_ms: u64,
        source_cursor: Option<u64>,
    ) -> Result<Self, String> {
        let mut incident = Self {
            schema: STORAGE_INTEGRITY_INCIDENT_SCHEMA.to_owned(),
            incident_id: StorageIntegrityIncidentId::new(),
            store_id,
            class,
            code: code.into(),
            detected_at_unix_ms,
            source_cursor,
            quarantine_required: true,
            resolved_at_unix_ms: None,
            incident_digest: String::new(),
        };
        incident.incident_digest = incident.digest();
        incident.validate()?;
        Ok(incident)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_INTEGRITY_INCIDENT_SCHEMA
            || self.incident_id.as_uuid().is_nil()
            || self.store_id.as_uuid().is_nil()
            || self.code.trim().is_empty()
            || self.code.len() > 256
            || self.code.contains('\0')
            || !self.quarantine_required
            || self.source_cursor == Some(0)
            || self
                .resolved_at_unix_ms
                .is_some_and(|resolved| resolved < self.detected_at_unix_ms)
        {
            return Err("storage_integrity_incident_invalid".to_owned());
        }
        validate_digest(&self.incident_digest, "storage_integrity_incident_digest")?;
        if self.incident_digest != self.digest() {
            return Err("storage_integrity_incident_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "incident_id": self.incident_id,
            "store_id": self.store_id,
            "class": self.class,
            "code": self.code,
            "detected_at_unix_ms": self.detected_at_unix_ms,
            "source_cursor": self.source_cursor,
            "quarantine_required": self.quarantine_required,
            "resolved_at_unix_ms": self.resolved_at_unix_ms,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
