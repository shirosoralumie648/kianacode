//! Versioned Run/Turn/Invocation/Execution identity contracts.
//!
//! `call_id` is model-provided correlation metadata. It is never a durable primary key; every
//! invocation has a server-derived InvocationId/ExecutionId and an explicit attempt.

use crate::{
    canonical_journal_bytes, json_digest, ExecutionId, InvocationId, ModelAttemptId, RequestId,
    RunId, SchemaVersion, SessionId, StepId, TurnId,
};
use serde::{Deserialize, Serialize};

pub const TURN_IDENTITY_SCHEMA: &str = "kiana.turn-identity.v1";
pub const INVOCATION_IDENTITY_SCHEMA: &str = "kiana.invocation-identity.v1";
pub const STEP_IDENTITY_SCHEMA: &str = "kiana.step-identity.v1";
pub const MODEL_ATTEMPT_IDENTITY_SCHEMA: &str = "kiana.model-attempt-identity.v1";
pub const EXECUTION_IDENTITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnSemantics {
    Start,
    NewTurn,
    LegacyContinue,
    Resume,
}

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
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
pub struct TurnIdentity {
    pub schema: String,
    pub version: SchemaVersion,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    #[serde(default)]
    pub previous_run_id: Option<RunId>,
    pub semantics: TurnSemantics,
    pub turn_number: u64,
    pub identity_digest: String,
}

impl TurnIdentity {
    pub fn new(
        session_id: impl Into<String>,
        run_id: RunId,
        turn_id: TurnId,
        previous_run_id: Option<RunId>,
        semantics: TurnSemantics,
        turn_number: u64,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: TURN_IDENTITY_SCHEMA.to_owned(),
            version: EXECUTION_IDENTITY_SCHEMA_VERSION,
            session_id: SessionId::new(session_id),
            run_id,
            turn_id,
            previous_run_id,
            semantics,
            turn_number,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TURN_IDENTITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_IDENTITY_SCHEMA_VERSION)
            || self.session_id.is_empty()
            || self.turn_number == 0
        {
            return Err("turn_identity_header_invalid".to_owned());
        }
        if matches!(self.semantics, TurnSemantics::NewTurn)
            && self
                .previous_run_id
                .is_none_or(|previous| previous == self.run_id)
        {
            return Err("turn_identity_predecessor_required".to_owned());
        }
        if matches!(self.semantics, TurnSemantics::Start | TurnSemantics::Resume)
            && self.previous_run_id.is_some()
        {
            return Err("turn_identity_predecessor_unexpected".to_owned());
        }
        digest(&self.identity_digest, "turn_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("turn_identity_digest_mismatch".to_owned());
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
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationIdentity {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub invocation_id: InvocationId,
    pub execution_id: ExecutionId,
    #[serde(default)]
    pub call_id: Option<String>,
    pub attempt: u32,
    pub identity_digest: String,
}

impl InvocationIdentity {
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        invocation_id: InvocationId,
        execution_id: ExecutionId,
        call_id: Option<String>,
        attempt: u32,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: INVOCATION_IDENTITY_SCHEMA.to_owned(),
            version: EXECUTION_IDENTITY_SCHEMA_VERSION,
            run_id,
            turn_id,
            invocation_id,
            execution_id,
            call_id,
            attempt,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INVOCATION_IDENTITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_IDENTITY_SCHEMA_VERSION)
            || self.attempt == 0
        {
            return Err("invocation_identity_header_invalid".to_owned());
        }
        if let Some(call_id) = &self.call_id {
            required(call_id, "invocation_call_id", 256)?;
        }
        digest(&self.identity_digest, "invocation_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("invocation_identity_digest_mismatch".to_owned());
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
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// Server-owned identity for one model/tool driver step inside a Turn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepIdentity {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub step_number: u32,
    pub identity_digest: String,
}

impl StepIdentity {
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        step_id: StepId,
        step_number: u32,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: STEP_IDENTITY_SCHEMA.to_owned(),
            version: EXECUTION_IDENTITY_SCHEMA_VERSION,
            run_id,
            turn_id,
            step_id,
            step_number,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STEP_IDENTITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_IDENTITY_SCHEMA_VERSION)
            || self.step_number == 0
        {
            return Err("step_identity_header_invalid".to_owned());
        }
        digest(&self.identity_digest, "step_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("step_identity_digest_mismatch".to_owned());
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
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// One provider/model attempt, linked to its server-owned Step identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttemptIdentity {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub model_attempt_id: ModelAttemptId,
    pub model_call_id: RequestId,
    pub attempt: u32,
    pub identity_digest: String,
}

impl ModelAttemptIdentity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        step_id: StepId,
        model_attempt_id: ModelAttemptId,
        model_call_id: RequestId,
        attempt: u32,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: MODEL_ATTEMPT_IDENTITY_SCHEMA.to_owned(),
            version: EXECUTION_IDENTITY_SCHEMA_VERSION,
            run_id,
            turn_id,
            step_id,
            model_attempt_id,
            model_call_id,
            attempt,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_ATTEMPT_IDENTITY_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_IDENTITY_SCHEMA_VERSION)
            || self.attempt == 0
        {
            return Err("model_attempt_identity_header_invalid".to_owned());
        }
        digest(&self.identity_digest, "model_attempt_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("model_attempt_identity_digest_mismatch".to_owned());
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
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
