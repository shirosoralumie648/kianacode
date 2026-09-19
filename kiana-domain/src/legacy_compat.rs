//! Explicit compatibility/migration decisions for legacy wire and checkpoint material.
//!
//! Compatibility is additive and evidence-bound. Unknown checkpoint versions are never resumed;
//! callers receive a migration/read-only disposition instead of a guessed identity or random ID.

use crate::{json_digest, RunId, SchemaVersion, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const LEGACY_COMPATIBILITY_SCHEMA: &str = "kiana.legacy-compatibility.v1";
pub const LEGACY_CHECKPOINT_DECISION_SCHEMA: &str = "kiana.legacy-checkpoint-decision.v1";
pub const LEGACY_COMPATIBILITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyDisposition {
    Compatible,
    MigrateReadOnly,
    NotSupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyWireKind {
    RunV1,
    ContinueV1,
    EventV1,
    CheckpointV1,
    CassetteV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCompatibilityDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: LegacyWireKind,
    pub input_schema: String,
    pub output_schema: String,
    pub disposition: LegacyDisposition,
    pub preserves_server_ids: bool,
    pub same_daemon_host: bool,
    pub reason: String,
    pub decision_digest: String,
}

impl LegacyCompatibilityDecision {
    pub fn new(
        kind: LegacyWireKind,
        input_schema: impl Into<String>,
        output_schema: impl Into<String>,
        disposition: LegacyDisposition,
        preserves_server_ids: bool,
        same_daemon_host: bool,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut decision = Self {
            schema: LEGACY_COMPATIBILITY_SCHEMA.to_owned(),
            version: LEGACY_COMPATIBILITY_VERSION,
            kind,
            input_schema: input_schema.into(),
            output_schema: output_schema.into(),
            disposition,
            preserves_server_ids,
            same_daemon_host,
            reason: reason.into(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEGACY_COMPATIBILITY_SCHEMA
            || self.version != LEGACY_COMPATIBILITY_VERSION
            || !bounded(&self.input_schema, 256)
            || !bounded(&self.output_schema, 256)
            || !bounded(&self.reason, 1_024)
            || !digest(&self.decision_digest)
            || self.decision_digest != self.digest()
            || self.disposition == LegacyDisposition::Compatible
                && (!self.preserves_server_ids || !self.same_daemon_host)
        {
            return Err("legacy_compatibility_decision_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "kind": self.kind,
            "input_schema": self.input_schema,
            "output_schema": self.output_schema,
            "disposition": self.disposition,
            "preserves_server_ids": self.preserves_server_ids,
            "same_daemon_host": self.same_daemon_host,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCheckpointDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub checkpoint_schema: String,
    pub checkpoint_version: u32,
    pub run_id: Option<RunId>,
    pub turn_id: Option<TurnId>,
    pub disposition: LegacyDisposition,
    pub resume_allowed: bool,
    pub reason: String,
    pub decision_digest: String,
}

impl LegacyCheckpointDecision {
    pub fn assess(
        checkpoint_schema: impl Into<String>,
        checkpoint_version: u32,
        run_id: Option<RunId>,
        turn_id: Option<TurnId>,
    ) -> Result<Self, String> {
        let checkpoint_schema = checkpoint_schema.into();
        let (disposition, resume_allowed, reason) =
            if checkpoint_schema != "kiana.harness-checkpoint.v1" {
                (
                    LegacyDisposition::NotSupported,
                    false,
                    "unknown_checkpoint_schema",
                )
            } else if checkpoint_version != 1 {
                (
                    LegacyDisposition::NotSupported,
                    false,
                    "unknown_checkpoint_version",
                )
            } else if run_id.is_none() || turn_id.is_none() {
                (
                    LegacyDisposition::MigrateReadOnly,
                    false,
                    "checkpoint_identity_incomplete",
                )
            } else {
                (
                    LegacyDisposition::Compatible,
                    true,
                    "checkpoint_identity_bound",
                )
            };
        let mut decision = Self {
            schema: LEGACY_CHECKPOINT_DECISION_SCHEMA.to_owned(),
            version: LEGACY_COMPATIBILITY_VERSION,
            checkpoint_schema,
            checkpoint_version,
            run_id,
            turn_id,
            disposition,
            resume_allowed,
            reason: reason.to_owned(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEGACY_CHECKPOINT_DECISION_SCHEMA
            || self.version != LEGACY_COMPATIBILITY_VERSION
            || !bounded(&self.checkpoint_schema, 256)
            || !bounded(&self.reason, 256)
            || !digest(&self.decision_digest)
            || self.decision_digest != self.digest()
            || (self.resume_allowed
                && (self.disposition != LegacyDisposition::Compatible
                    || self.run_id.is_none()
                    || self.turn_id.is_none()))
            || (self.disposition == LegacyDisposition::NotSupported && self.resume_allowed)
        {
            return Err("legacy_checkpoint_decision_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "checkpoint_schema": self.checkpoint_schema,
            "checkpoint_version": self.checkpoint_version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "disposition": self.disposition,
            "resume_allowed": self.resume_allowed,
            "reason": self.reason,
        }))
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
