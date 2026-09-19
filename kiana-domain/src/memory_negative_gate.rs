//! Negative gates for model-originated Memory writes.
//!
//! The model may propose text and a collection intent, but server code derives origin, actor,
//! classification, admission and lifecycle. User-private writes require a separate operator path.

use crate::{
    json_digest, MemoryAdmission, MemoryClassification, MemoryCollection, MemoryOrigin,
    MemoryState, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MEMORY_MODEL_WRITE_GATE_SCHEMA: &str = "kiana.memory-model-write-gate.v1";
pub const MEMORY_MODEL_WRITE_GATE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

const PROTECTED_MODEL_FIELDS: &[&str] = &[
    "actor",
    "origin",
    "classification",
    "admission_state",
    "state",
    "reviewed_by",
    "reviewed_at_ms",
    "operator_approval_ref",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryModelWriteGate {
    pub schema: String,
    pub version: SchemaVersion,
    pub collection: MemoryCollection,
    pub session_id: String,
    pub origin: MemoryOrigin,
    pub admission_state: MemoryAdmission,
    pub state: MemoryState,
    pub classification: MemoryClassification,
    pub purpose_id: String,
    pub operator_approval_required: bool,
    pub gate_digest: String,
}

impl MemoryModelWriteGate {
    pub fn derive(
        collection: MemoryCollection,
        session_id: impl Into<String>,
    ) -> Result<Self, String> {
        let session_id = session_id.into();
        if session_id.trim().is_empty() || session_id.len() > 256 || session_id.contains('\0') {
            return Err("memory_model_session_invalid".to_owned());
        }
        if collection.layer == crate::MEMORY_LAYER_USER {
            return Err("memory_user_private_requires_operator".to_owned());
        }
        let scratch = collection.layer == crate::MEMORY_LAYER_INSTANCE_SCRATCH;
        let mut gate = Self {
            schema: MEMORY_MODEL_WRITE_GATE_SCHEMA.to_owned(),
            version: MEMORY_MODEL_WRITE_GATE_VERSION,
            classification: MemoryClassification::for_collection(&collection),
            collection,
            session_id,
            origin: MemoryOrigin::Model,
            admission_state: if scratch {
                MemoryAdmission::Ephemeral
            } else {
                MemoryAdmission::Candidate
            },
            state: if scratch {
                MemoryState::Active
            } else {
                MemoryState::Draft
            },
            purpose_id: if scratch {
                "memory.scratch".to_owned()
            } else {
                "memory.candidate".to_owned()
            },
            operator_approval_required: !scratch,
            gate_digest: String::new(),
        };
        gate.gate_digest = gate.digest();
        gate.validate()?;
        Ok(gate)
    }

    pub fn reject_model_overrides(payload: &Value) -> Result<(), String> {
        if let Some(object) = payload.as_object() {
            if let Some(field) = PROTECTED_MODEL_FIELDS
                .iter()
                .find(|field| object.contains_key(**field))
            {
                return Err(format!("memory_model_field_server_owned:{field}"));
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_MODEL_WRITE_GATE_SCHEMA
            || !self
                .version
                .is_compatible_with(&MEMORY_MODEL_WRITE_GATE_VERSION)
            || self.session_id.trim().is_empty()
            || self.origin != MemoryOrigin::Model
            || self.gate_digest != self.digest()
        {
            return Err("memory_model_write_gate_invalid".to_owned());
        }
        if self.collection.layer == crate::MEMORY_LAYER_USER {
            return Err("memory_user_private_requires_operator".to_owned());
        }
        let scratch = self.collection.layer == crate::MEMORY_LAYER_INSTANCE_SCRATCH;
        if scratch {
            if self.admission_state != MemoryAdmission::Ephemeral
                || self.state != MemoryState::Active
                || self.operator_approval_required
            {
                return Err("memory_scratch_gate_invalid".to_owned());
            }
        } else if self.admission_state != MemoryAdmission::Candidate
            || self.state != MemoryState::Draft
            || !self.operator_approval_required
        {
            return Err("memory_candidate_gate_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "collection": self.collection,
            "session_id": self.session_id,
            "origin": self.origin,
            "admission_state": self.admission_state,
            "state": self.state,
            "classification": self.classification,
            "purpose_id": self.purpose_id,
            "operator_approval_required": self.operator_approval_required,
        }))
    }
}
