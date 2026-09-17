//! Rebuildable projection checkpoint contracts.
//!
//! A checkpoint is an optimization for a read model, never an authority fact. It carries the
//! serialized projection state plus the exact source cursor/event identity needed to decide
//! whether a projector may apply a tail or must rebuild from the EventLog.

use crate::{json_digest, EventCursor, EventId, SchemaVersion, MAX_SOURCE_EVENT_IDS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PROJECTION_CHECKPOINT_SCHEMA: &str = "kiana.projection-checkpoint.v1";
pub const PROJECTION_CHECKPOINT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionCheckpoint {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub source_cursor: EventCursor,
    #[serde(default)]
    pub source_event_ids: Vec<EventId>,
    pub state: Value,
    pub state_digest: String,
    pub checkpoint_digest: String,
}

impl ProjectionCheckpoint {
    pub fn new(
        projector: &str,
        source_cursor: EventCursor,
        mut source_event_ids: Vec<EventId>,
        state: Value,
    ) -> Result<Self, String> {
        if projector.trim().is_empty() || projector.len() > 256 {
            return Err("projection_checkpoint_projector_invalid".to_owned());
        }
        source_event_ids.sort();
        source_event_ids.dedup();
        let state_digest = json_digest(&state);
        let mut checkpoint = Self {
            schema: PROJECTION_CHECKPOINT_SCHEMA.to_owned(),
            version: PROJECTION_CHECKPOINT_VERSION,
            projector: projector.to_owned(),
            source_cursor,
            source_event_ids,
            state,
            state_digest,
            checkpoint_digest: String::new(),
        };
        checkpoint.checkpoint_digest = checkpoint.digest();
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let checkpoint: Self = serde_json::from_value(value.clone())
            .map_err(|_| "projection_checkpoint_decode_failed".to_owned())?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "projection_checkpoint_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECTION_CHECKPOINT_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECTION_CHECKPOINT_VERSION)
            || self.projector.trim().is_empty()
            || self.projector.len() > 256
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || self
                .source_event_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.source_event_ids.iter().any(|id| id.as_uuid().is_nil())
            || !valid_digest(&self.state_digest)
            || !valid_digest(&self.checkpoint_digest)
        {
            return Err("projection_checkpoint_header_invalid".to_owned());
        }
        if self.state_digest != json_digest(&self.state) {
            return Err("projection_checkpoint_state_digest_mismatch".to_owned());
        }
        if self.checkpoint_digest != self.digest() {
            return Err("projection_checkpoint_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "state": self.state,
            "state_digest": self.state_digest,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
