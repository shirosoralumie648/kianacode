//! Projection lag and Unknown mutation reconciliation contracts.
//!
//! A read model is usable only when its cursor/generation is explicit.  An uncertain mutation is
//! reconciled by its original idempotency key and protected request digest; callers cannot turn an
//! uncertain commit into a fresh mutation by changing the ID.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const PROJECTION_LAG_VIEW_SCHEMA: &str = "kiana.projection-lag-view.v1";
pub const UNKNOWN_MUTATION_RECONCILIATION_SCHEMA: &str = "kiana.unknown-mutation-reconciliation.v1";
pub const PROJECTION_RECOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionLagStatus {
    CaughtUp,
    Pending,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionLagView {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_cursor: u64,
    #[serde(default)]
    pub projection_cursor: Option<u64>,
    pub projection_generation: u64,
    pub data_epoch: u64,
    pub status: ProjectionLagStatus,
    pub projection_pending: bool,
    pub reason: Option<String>,
    pub view_digest: String,
}

impl ProjectionLagView {
    pub fn new(
        source_cursor: u64,
        projection_cursor: Option<u64>,
        projection_generation: u64,
        data_epoch: u64,
        reason: Option<String>,
    ) -> Result<Self, String> {
        if source_cursor == 0
            || projection_cursor == Some(0)
            || projection_cursor.is_some_and(|cursor| cursor > source_cursor)
            || projection_generation == 0
            || data_epoch == 0
        {
            return Err("projection_lag_cursor_invalid".to_owned());
        }
        let projection_pending = projection_cursor.is_none_or(|cursor| cursor < source_cursor);
        let status = if projection_cursor.is_none() {
            ProjectionLagStatus::Unknown
        } else if projection_pending {
            ProjectionLagStatus::Pending
        } else {
            ProjectionLagStatus::CaughtUp
        };
        if projection_pending && reason.as_deref().is_none_or(str::is_empty) {
            return Err("projection_lag_reason_required".to_owned());
        }
        if !projection_pending && reason.is_some() {
            return Err("projection_lag_caught_up_reason_forbidden".to_owned());
        }
        let mut view = Self {
            schema: PROJECTION_LAG_VIEW_SCHEMA.to_owned(),
            version: PROJECTION_RECOVERY_VERSION,
            source_cursor,
            projection_cursor,
            projection_generation,
            data_epoch,
            status,
            projection_pending,
            reason,
            view_digest: String::new(),
        };
        view.view_digest = view.digest();
        view.validate()?;
        Ok(view)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECTION_LAG_VIEW_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECTION_RECOVERY_VERSION)
            || self.source_cursor == 0
            || self.projection_cursor == Some(0)
            || self
                .projection_cursor
                .is_some_and(|cursor| cursor > self.source_cursor)
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.projection_pending
                != self
                    .projection_cursor
                    .is_none_or(|cursor| cursor < self.source_cursor)
        {
            return Err("projection_lag_view_invalid".to_owned());
        }
        if self.projection_pending && self.reason.as_deref().is_none_or(str::is_empty) {
            return Err("projection_lag_reason_required".to_owned());
        }
        if !self.projection_pending && self.reason.is_some() {
            return Err("projection_lag_caught_up_reason_forbidden".to_owned());
        }
        if self.status
            != if self.projection_cursor.is_none() {
                ProjectionLagStatus::Unknown
            } else if self.projection_pending {
                ProjectionLagStatus::Pending
            } else {
                ProjectionLagStatus::CaughtUp
            }
        {
            return Err("projection_lag_status_mismatch".to_owned());
        }
        if let Some(reason) = &self.reason {
            required(reason, "projection_lag_reason", 256)?;
        }
        digest(&self.view_digest, "projection_lag_view_digest")?;
        if self.view_digest != self.digest() {
            return Err("projection_lag_view_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn read_consistent(&self) -> bool {
        self.validate().is_ok() && !self.projection_pending
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "status": self.status,
            "projection_pending": self.projection_pending,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownMutationState {
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownMutationReconciliation {
    pub schema: String,
    pub version: SchemaVersion,
    pub mutation_id: String,
    pub request_digest: String,
    pub state: UnknownMutationState,
    pub retry_with_new_id_allowed: bool,
    pub reconciliation_digest: String,
}

impl UnknownMutationReconciliation {
    pub fn new(
        mutation_id: impl Into<String>,
        request_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut reconciliation = Self {
            schema: UNKNOWN_MUTATION_RECONCILIATION_SCHEMA.to_owned(),
            version: PROJECTION_RECOVERY_VERSION,
            mutation_id: mutation_id.into(),
            request_digest: request_digest.into(),
            state: UnknownMutationState::ResultUnknown,
            retry_with_new_id_allowed: false,
            reconciliation_digest: String::new(),
        };
        reconciliation.reconciliation_digest = reconciliation.digest();
        reconciliation.validate()?;
        Ok(reconciliation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UNKNOWN_MUTATION_RECONCILIATION_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECTION_RECOVERY_VERSION)
            || self.state != UnknownMutationState::ResultUnknown
            || self.retry_with_new_id_allowed
        {
            return Err("unknown_mutation_reconciliation_invalid".to_owned());
        }
        required(&self.mutation_id, "unknown_mutation_id", 256)?;
        digest(&self.request_digest, "unknown_mutation_request_digest")?;
        digest(
            &self.reconciliation_digest,
            "unknown_mutation_reconciliation_digest",
        )?;
        if self.reconciliation_digest != self.digest() {
            return Err("unknown_mutation_reconciliation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn reconcile(&self, mutation_id: &str, request_digest: &str) -> Result<(), String> {
        self.validate()?;
        if mutation_id != self.mutation_id {
            return Err("unknown_mutation_new_id_forbidden".to_owned());
        }
        if request_digest != self.request_digest {
            return Err("unknown_mutation_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "mutation_id": self.mutation_id,
            "request_digest": self.request_digest,
            "state": self.state,
            "retry_with_new_id_allowed": self.retry_with_new_id_allowed,
        }))
    }
}
