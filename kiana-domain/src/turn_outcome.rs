//! Structured output validation and the single turn-outcome decision contract.
//!
//! A model's text is only a candidate result.  The outcome reducer checks pending work and an
//! optional server-owned output contract before it can be projected as completed.  Waiting and
//! blocked outcomes remain resumable; they are not terminal success and cannot be manufactured by
//! model prose.

use crate::{json_digest, validate_json_limits, RunId, SchemaVersion, StepId, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const OUTPUT_CONTRACT_SCHEMA: &str = "kiana.output-contract.v1";
pub const TURN_OUTCOME_SCHEMA: &str = "kiana.turn-outcome.v1";
pub const TURN_OUTCOME_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_OUTPUT_CONTRACT_FIELDS: usize = 128;
pub const MAX_TURN_OUTCOME_REFS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputValueType {
    Any,
    Null,
    Boolean,
    Number,
    String,
    Object,
    Array,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    pub schema: String,
    pub version: SchemaVersion,
    pub name: String,
    pub contract_version: u32,
    pub required: Vec<String>,
    pub properties: BTreeMap<String, OutputValueType>,
    pub additional_properties: bool,
    pub contract_digest: String,
}

impl OutputContract {
    pub fn new(
        name: impl Into<String>,
        contract_version: u32,
        required: Vec<String>,
        properties: BTreeMap<String, OutputValueType>,
        additional_properties: bool,
    ) -> Result<Self, String> {
        let mut contract = Self {
            schema: OUTPUT_CONTRACT_SCHEMA.to_owned(),
            version: TURN_OUTCOME_VERSION,
            name: name.into(),
            contract_version,
            required,
            properties,
            additional_properties,
            contract_digest: String::new(),
        };
        contract.contract_digest = contract.digest();
        contract.validate()?;
        Ok(contract)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OUTPUT_CONTRACT_SCHEMA
            || self.version != TURN_OUTCOME_VERSION
            || !bounded(&self.name, 256)
            || self.contract_version == 0
            || self.required.len() > MAX_OUTPUT_CONTRACT_FIELDS
            || self.properties.len() > MAX_OUTPUT_CONTRACT_FIELDS
            || !digest(&self.contract_digest)
            || self.contract_digest != self.digest()
        {
            return Err("output_contract_invalid".to_owned());
        }
        let mut required = std::collections::HashSet::new();
        for field in &self.required {
            if !bounded(field, 128)
                || !required.insert(field.as_str())
                || !self.properties.contains_key(field)
            {
                return Err("output_contract_required_invalid".to_owned());
            }
        }
        if self.properties.keys().any(|field| !bounded(field, 128)) {
            return Err("output_contract_property_invalid".to_owned());
        }
        Ok(())
    }

    pub fn validate_output(&self, output: &Value) -> Result<(), String> {
        self.validate()?;
        validate_json_limits(output)?;
        let Value::Object(object) = output else {
            return Err("turn_output_object_required".to_owned());
        };
        for field in &self.required {
            if !object.contains_key(field) {
                return Err(format!("turn_output_required_missing:{field}"));
            }
        }
        for (field, value) in object {
            let Some(expected) = self.properties.get(field) else {
                if self.additional_properties {
                    continue;
                }
                return Err(format!("turn_output_field_unknown:{field}"));
            };
            if !matches_output_type(*expected, value) {
                return Err(format!("turn_output_field_type:{field}"));
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "name": self.name,
            "contract_version": self.contract_version,
            "required": self.required,
            "properties": self.properties,
            "additional_properties": self.additional_properties,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcomeKind {
    Answered,
    Completed,
    AwaitingInput,
    AwaitingApproval,
    Blocked,
    Cancelled,
    Failed,
    ResultUnknown,
}

impl TurnOutcomeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Completed => "completed",
            Self::AwaitingInput => "awaiting_input",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::ResultUnknown => "result_unknown",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Answered | Self::Completed | Self::Cancelled | Self::Failed | Self::ResultUnknown
        )
    }

    pub const fn is_waiting(self) -> bool {
        matches!(self, Self::AwaitingInput | Self::AwaitingApproval)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TurnOutcomeInput {
    pub run_id: RunId,
    pub turn_id: Option<TurnId>,
    pub step_id: Option<StepId>,
    pub output: Value,
    pub output_contract: Option<OutputContract>,
    pub answered: bool,
    pub pending_invocations: u32,
    pub pending_background_jobs: u32,
    pub pending_steering: bool,
    pub pending_clarification: bool,
    pub pending_approval: bool,
    pub cancelled: bool,
    pub effect_unknown: bool,
    pub failure: Option<String>,
    pub artifact_refs: Vec<String>,
    pub verification_refs: Vec<String>,
}

impl TurnOutcomeInput {
    pub fn completed(run_id: RunId, output: Value) -> Self {
        Self {
            run_id,
            turn_id: None,
            step_id: None,
            output,
            output_contract: None,
            answered: false,
            pending_invocations: 0,
            pending_background_jobs: 0,
            pending_steering: false,
            pending_clarification: false,
            pending_approval: false,
            cancelled: false,
            effect_unknown: false,
            failure: None,
            artifact_refs: Vec::new(),
            verification_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnOutcome {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<StepId>,
    pub kind: TurnOutcomeKind,
    pub status: String,
    pub terminal: bool,
    pub output_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_digest: Option<String>,
    pub artifact_refs: Vec<String>,
    pub verification_refs: Vec<String>,
    pub blockers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub outcome_digest: String,
}

impl TurnOutcome {
    pub fn decide(input: TurnOutcomeInput) -> Result<Self, String> {
        if input.run_id.as_uuid().is_nil()
            || input.turn_id.is_some_and(|id| id.as_uuid().is_nil())
            || input.step_id.is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("turn_outcome_identity_invalid".to_owned());
        }
        if let Some(contract) = input.output_contract.as_ref() {
            contract.validate()?;
        }
        validate_refs(&input.artifact_refs, "artifact")?;
        validate_refs(&input.verification_refs, "verification")?;

        let contract_digest = input
            .output_contract
            .as_ref()
            .map(|contract| contract.contract_digest.clone());
        let (kind, reason, blockers) = if input.effect_unknown {
            (
                TurnOutcomeKind::ResultUnknown,
                Some("effect_unknown_requires_reconciliation".to_owned()),
                vec!["result_unknown".to_owned()],
            )
        } else if input.cancelled {
            (
                TurnOutcomeKind::Cancelled,
                Some("cancelled".to_owned()),
                vec!["cancelled".to_owned()],
            )
        } else if let Some(failure) = input.failure.as_deref().filter(|value| !value.is_empty()) {
            (
                TurnOutcomeKind::Failed,
                Some(failure.to_owned()),
                vec!["failure".to_owned()],
            )
        } else if input.pending_clarification {
            (
                TurnOutcomeKind::AwaitingInput,
                Some("waiting_for_input".to_owned()),
                vec!["clarification".to_owned()],
            )
        } else if input.pending_approval {
            (
                TurnOutcomeKind::AwaitingApproval,
                Some("awaiting_approval".to_owned()),
                vec!["approval".to_owned()],
            )
        } else if input.pending_invocations > 0
            || input.pending_background_jobs > 0
            || input.pending_steering
        {
            let mut blockers = Vec::new();
            if input.pending_invocations > 0 {
                blockers.push("invocation".to_owned());
            }
            if input.pending_background_jobs > 0 {
                blockers.push("background_job".to_owned());
            }
            if input.pending_steering {
                blockers.push("steering".to_owned());
            }
            (
                TurnOutcomeKind::Blocked,
                Some("outstanding_work".to_owned()),
                blockers,
            )
        } else if let Some(contract) = input.output_contract.as_ref() {
            if let Err(error) = contract.validate_output(&input.output) {
                (
                    TurnOutcomeKind::Failed,
                    Some(format!("output_contract_invalid:{error}")),
                    vec!["output_contract".to_owned()],
                )
            } else if input.answered {
                (TurnOutcomeKind::Answered, None, Vec::new())
            } else {
                (TurnOutcomeKind::Completed, None, Vec::new())
            }
        } else if input.answered {
            (TurnOutcomeKind::Answered, None, Vec::new())
        } else {
            (TurnOutcomeKind::Completed, None, Vec::new())
        };

        let mut outcome = Self {
            schema: TURN_OUTCOME_SCHEMA.to_owned(),
            version: TURN_OUTCOME_VERSION,
            run_id: input.run_id,
            turn_id: input.turn_id,
            step_id: input.step_id,
            kind,
            status: kind.as_str().to_owned(),
            terminal: kind.is_terminal(),
            output_digest: json_digest(&input.output),
            contract_digest,
            artifact_refs: input.artifact_refs,
            verification_refs: input.verification_refs,
            blockers,
            reason,
            outcome_digest: String::new(),
        };
        outcome.outcome_digest = outcome.digest();
        outcome.validate()?;
        Ok(outcome)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TURN_OUTCOME_SCHEMA
            || self.version != TURN_OUTCOME_VERSION
            || self.run_id.as_uuid().is_nil()
            || self.status != self.kind.as_str()
            || self.terminal != self.kind.is_terminal()
            || !digest(&self.output_digest)
            || self.artifact_refs.len() > MAX_TURN_OUTCOME_REFS
            || self.verification_refs.len() > MAX_TURN_OUTCOME_REFS
            || self.blockers.len() > MAX_TURN_OUTCOME_REFS
            || !digest(&self.outcome_digest)
            || self.outcome_digest != self.digest()
        {
            return Err("turn_outcome_invalid".to_owned());
        }
        if self.turn_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.step_id.is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("turn_outcome_identity_invalid".to_owned());
        }
        if let Some(contract_digest) = &self.contract_digest {
            if !digest(contract_digest) {
                return Err("turn_outcome_contract_digest_invalid".to_owned());
            }
        }
        validate_refs(&self.artifact_refs, "artifact")?;
        validate_refs(&self.verification_refs, "verification")?;
        if self.blockers.iter().any(|blocker| !bounded(blocker, 128)) {
            return Err("turn_outcome_blockers_invalid".to_owned());
        }
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| !bounded(reason, 4_096))
        {
            return Err("turn_outcome_reason_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "kind": self.kind,
            "status": self.status,
            "terminal": self.terminal,
            "output_digest": self.output_digest,
            "contract_digest": self.contract_digest,
            "artifact_refs": self.artifact_refs,
            "verification_refs": self.verification_refs,
            "blockers": self.blockers,
            "reason": self.reason,
        }))
    }

    pub fn is_resumable(&self) -> bool {
        !self.terminal
    }
}

fn matches_output_type(expected: OutputValueType, value: &Value) -> bool {
    matches!(
        (expected, value),
        (OutputValueType::Any, _)
            | (OutputValueType::Null, Value::Null)
            | (OutputValueType::Boolean, Value::Bool(_))
            | (OutputValueType::Number, Value::Number(_))
            | (OutputValueType::String, Value::String(_))
            | (OutputValueType::Object, Value::Object(_))
            | (OutputValueType::Array, Value::Array(_))
    )
}

fn validate_refs(refs: &[String], kind: &str) -> Result<(), String> {
    if refs.len() > MAX_TURN_OUTCOME_REFS || refs.iter().any(|reference| !bounded(reference, 512)) {
        return Err(format!("turn_outcome_{kind}_refs_invalid"));
    }
    Ok(())
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
