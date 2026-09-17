//! Server-owned cancellation facts and stop evidence.
//!
//! A cancellation request is a durable intent separate from the in-process watch signal. The
//! fact records only redacted reason digests and target identities; a terminal `Cancelled` fact
//! requires confirmed stopping, while `ResultUnknown` keeps resources fenced.

use crate::{json_digest, RequestId, RunCancellationState, RunId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const RUN_CANCELLATION_SCHEMA: &str = "kiana.run-cancellation-fact.v1";
pub const RUN_CANCELLATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCancellationFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub command_id: RequestId,
    pub state: RunCancellationState,
    pub reason_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default)]
    pub target_invocation_ids: Vec<RequestId>,
    pub expected_version: u64,
    pub stop_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_confirmed: Option<bool>,
    pub at_unix_ms: u64,
    pub fact_digest: String,
}

impl RunCancellationFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        command_id: RequestId,
        state: RunCancellationState,
        reason: &str,
        actor_id: Option<String>,
        mut target_invocation_ids: Vec<RequestId>,
        expected_version: u64,
        stop_requested: bool,
        stop_confirmed: Option<bool>,
        at_unix_ms: u64,
    ) -> Result<Self, String> {
        target_invocation_ids.sort();
        target_invocation_ids.dedup();
        let mut fact = Self {
            schema: RUN_CANCELLATION_SCHEMA.to_owned(),
            version: RUN_CANCELLATION_VERSION,
            run_id,
            command_id,
            state,
            reason_digest: json_digest(&json!({"reason": reason})),
            actor_id,
            target_invocation_ids,
            expected_version,
            stop_requested,
            stop_confirmed,
            at_unix_ms,
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let fact: Self = serde_json::from_value(value.clone())
            .map_err(|_| "run_cancellation_fact_decode_failed".to_owned())?;
        fact.validate()?;
        Ok(fact)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "run_cancellation_fact_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RUN_CANCELLATION_SCHEMA
            || !self.version.is_compatible_with(&RUN_CANCELLATION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.at_unix_ms == 0
            || !self.stop_requested
            || self.actor_id.as_ref().is_some_and(|actor| {
                actor.trim().is_empty() || actor.len() > 256 || actor.contains('\0')
            })
        {
            return Err("run_cancellation_fact_header_invalid".to_owned());
        }
        if self
            .target_invocation_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
            || self
                .target_invocation_ids
                .iter()
                .any(|id| id.as_uuid().is_nil())
        {
            return Err("run_cancellation_fact_targets_invalid".to_owned());
        }
        if !valid_digest(&self.reason_digest) || !valid_digest(&self.fact_digest) {
            return Err("run_cancellation_fact_digest_invalid".to_owned());
        }
        match self.state {
            RunCancellationState::Requested | RunCancellationState::Stopping => {
                if self.stop_confirmed == Some(true) {
                    return Err("run_cancellation_fact_stop_conflict".to_owned());
                }
            }
            RunCancellationState::Cancelled => {
                if self.stop_confirmed != Some(true) {
                    return Err("run_cancellation_fact_stop_confirmation_required".to_owned());
                }
            }
            RunCancellationState::ResultUnknown => {
                if self.stop_confirmed == Some(true) {
                    return Err("run_cancellation_fact_unknown_stop_conflict".to_owned());
                }
            }
            RunCancellationState::Active => {
                return Err("run_cancellation_fact_state_invalid".to_owned())
            }
        }
        if self.fact_digest != self.digest() {
            return Err("run_cancellation_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "command_id": self.command_id,
            "state": self.state,
            "reason_digest": self.reason_digest,
            "actor_id": self.actor_id,
            "target_invocation_ids": self.target_invocation_ids,
            "expected_version": self.expected_version,
            "stop_requested": self.stop_requested,
            "stop_confirmed": self.stop_confirmed,
            "at_unix_ms": self.at_unix_ms,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
