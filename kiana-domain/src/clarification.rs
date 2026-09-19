//! Human clarification contracts.
//!
//! A clarification is a question about the continuation of one model step.  It is deliberately
//! not an approval: the answer has no capability request, grant, approval ID or tool arguments.
//! Core owns validation and persistence of the resulting fact; this module only defines the
//! versioned value objects and their fail-closed state transitions.

use crate::{json_digest, InteractionId, RequestId, RunId, SchemaVersion, StepId, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CLARIFICATION_REQUEST_SCHEMA: &str = "kiana.clarification-request.v1";
pub const CLARIFICATION_ANSWER_SCHEMA: &str = "kiana.clarification-answer.v1";
pub const CLARIFICATION_RESOLUTION_SCHEMA: &str = "kiana.clarification-resolution.v1";
pub const CLARIFICATION_WAIT_SCHEMA: &str = "kiana.clarification-wait.v1";
pub const CLARIFICATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const CLARIFICATION_WAITING_STATUS: &str = "waiting_for_input";
pub const MAX_CLARIFICATION_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_CLARIFICATION_OPTIONS: usize = 32;
pub const MAX_CLARIFICATION_ROLES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarificationStatus {
    Pending,
    Answered,
    Cancelled,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarificationCancelPolicy {
    UserOrRuntime,
    RuntimeOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarificationSource {
    Cli,
    Tty,
    Web,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationOption {
    pub id: String,
    pub label: String,
}

impl ClarificationOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if !bounded(&self.id, 128) || !bounded(&self.label, 1_024) {
            return Err("clarification_option_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationAnswer {
    pub schema: String,
    pub version: SchemaVersion,
    pub answer_id: RequestId,
    pub interaction_id: InteractionId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<StepId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    pub text: String,
    pub answered_by: String,
    pub responder_role: String,
    pub source: ClarificationSource,
    pub answered_at_unix_ms: u64,
    pub request_digest: String,
    pub answer_digest: String,
}

impl ClarificationAnswer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        answer_id: RequestId,
        interaction_id: InteractionId,
        run_id: RunId,
        turn_id: TurnId,
        step_id: Option<StepId>,
        option_id: Option<String>,
        text: impl Into<String>,
        answered_by: impl Into<String>,
        responder_role: impl Into<String>,
        source: ClarificationSource,
        answered_at_unix_ms: u64,
        request_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut answer = Self {
            schema: CLARIFICATION_ANSWER_SCHEMA.to_owned(),
            version: CLARIFICATION_VERSION,
            answer_id,
            interaction_id,
            run_id,
            turn_id,
            step_id,
            option_id,
            text: text.into(),
            answered_by: answered_by.into(),
            responder_role: responder_role.into(),
            source,
            answered_at_unix_ms,
            request_digest: request_digest.into(),
            answer_digest: String::new(),
        };
        answer.answer_digest = answer.digest();
        answer.validate().map(|()| answer)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLARIFICATION_ANSWER_SCHEMA
            || self.version != CLARIFICATION_VERSION
            || self.answer_id.as_uuid().is_nil()
            || self.interaction_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || !bounded(&self.text, MAX_CLARIFICATION_TEXT_BYTES)
            || !bounded(&self.answered_by, 256)
            || !bounded(&self.responder_role, 128)
            || self.answered_at_unix_ms == 0
            || !digest(&self.request_digest)
            || self.answer_digest != self.digest()
        {
            return Err("clarification_answer_invalid".to_owned());
        }
        if let Some(step_id) = self.step_id {
            if step_id.as_uuid().is_nil() {
                return Err("clarification_answer_step_invalid".to_owned());
            }
        }
        if self
            .option_id
            .as_deref()
            .is_some_and(|id| !bounded(id, 128))
        {
            return Err("clarification_answer_option_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "answer_id": self.answer_id,
            "interaction_id": self.interaction_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "option_id": self.option_id,
            "text": self.text,
            "answered_by": self.answered_by,
            "responder_role": self.responder_role,
            "source": self.source,
            "answered_at_unix_ms": self.answered_at_unix_ms,
            "request_digest": self.request_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub interaction_id: InteractionId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<StepId>,
    pub question: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ClarificationOption>,
    pub required: bool,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub cancel_policy: ClarificationCancelPolicy,
    pub created_by_role: String,
    pub responder_roles: Vec<String>,
    pub status: ClarificationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<ClarificationAnswer>,
    pub request_digest: String,
}

impl ClarificationRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        interaction_id: InteractionId,
        run_id: RunId,
        turn_id: TurnId,
        step_id: Option<StepId>,
        question: impl Into<String>,
        options: Vec<ClarificationOption>,
        required: bool,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        cancel_policy: ClarificationCancelPolicy,
        created_by_role: impl Into<String>,
        responder_roles: Vec<String>,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: CLARIFICATION_REQUEST_SCHEMA.to_owned(),
            version: CLARIFICATION_VERSION,
            interaction_id,
            run_id,
            turn_id,
            step_id,
            question: question.into(),
            options,
            required,
            created_at_unix_ms,
            expires_at_unix_ms,
            cancel_policy,
            created_by_role: created_by_role.into(),
            responder_roles,
            status: ClarificationStatus::Pending,
            answer: None,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate().map(|()| request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLARIFICATION_REQUEST_SCHEMA
            || self.version != CLARIFICATION_VERSION
            || self.interaction_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || !bounded(&self.question, MAX_CLARIFICATION_TEXT_BYTES)
            || self.created_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.created_at_unix_ms
            || !bounded(&self.created_by_role, 128)
            || self.responder_roles.is_empty()
            || self.responder_roles.len() > MAX_CLARIFICATION_ROLES
            || self.options.len() > MAX_CLARIFICATION_OPTIONS
            || !digest(&self.request_digest)
            || self.request_digest != self.digest()
        {
            return Err("clarification_request_invalid".to_owned());
        }
        if self.step_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("clarification_request_step_invalid".to_owned());
        }
        let mut option_ids = std::collections::HashSet::new();
        for option in &self.options {
            option.validate()?;
            if !option_ids.insert(option.id.as_str()) {
                return Err("clarification_option_duplicate".to_owned());
            }
        }
        let mut roles = std::collections::HashSet::new();
        if self
            .responder_roles
            .iter()
            .any(|role| !bounded(role, 128) || !roles.insert(role.as_str()))
        {
            return Err("clarification_responder_roles_invalid".to_owned());
        }
        match (&self.status, &self.answer) {
            (ClarificationStatus::Pending, None)
            | (ClarificationStatus::Cancelled, None)
            | (ClarificationStatus::Expired, None) => {}
            (ClarificationStatus::Answered, Some(answer)) => {
                answer.validate()?;
                self.validate_answer_binding(answer)?;
            }
            _ => return Err("clarification_status_answer_mismatch".to_owned()),
        }
        Ok(())
    }

    pub fn accept_answer(
        &self,
        answer: ClarificationAnswer,
        now_unix_ms: u64,
    ) -> Result<(Self, ClarificationResolution), String> {
        self.validate()?;
        answer.validate()?;
        self.validate_answer_binding(&answer)?;
        if self.status != ClarificationStatus::Pending {
            return Err("clarification_not_pending".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("clarification_expired".to_owned());
        }
        if !self
            .responder_roles
            .iter()
            .any(|role| role == &answer.responder_role)
        {
            return Err("clarification_responder_role_denied".to_owned());
        }
        if answer.answered_at_unix_ms < self.created_at_unix_ms
            || answer.answered_at_unix_ms > now_unix_ms
        {
            return Err("clarification_answer_time_invalid".to_owned());
        }
        if self.options.is_empty() {
            if answer.option_id.is_some() {
                return Err("clarification_option_unexpected".to_owned());
            }
        } else {
            let Some(option_id) = answer.option_id.as_deref() else {
                return Err("clarification_option_required".to_owned());
            };
            if !self.options.iter().any(|option| option.id == option_id) {
                return Err("clarification_option_unknown".to_owned());
            }
        }
        let mut next = self.clone();
        next.status = ClarificationStatus::Answered;
        next.answer = Some(answer.clone());
        next.validate()?;
        let resolution = ClarificationResolution::from_answer(&next, &answer)?;
        Ok((next, resolution))
    }

    pub fn cancel(&self, actor_role: &str, now_unix_ms: u64) -> Result<Self, String> {
        self.validate()?;
        if self.status != ClarificationStatus::Pending {
            return Err("clarification_not_pending".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("clarification_expired".to_owned());
        }
        let allowed = match self.cancel_policy {
            ClarificationCancelPolicy::UserOrRuntime => {
                actor_role == "runtime"
                    || self.responder_roles.iter().any(|role| role == actor_role)
            }
            ClarificationCancelPolicy::RuntimeOnly => actor_role == "runtime",
        };
        if !allowed {
            return Err("clarification_cancel_denied".to_owned());
        }
        let mut next = self.clone();
        next.status = ClarificationStatus::Cancelled;
        next.validate()?;
        Ok(next)
    }

    pub fn expire(&self, now_unix_ms: u64) -> Result<Self, String> {
        self.validate()?;
        if self.status != ClarificationStatus::Pending {
            return Err("clarification_not_pending".to_owned());
        }
        if now_unix_ms < self.expires_at_unix_ms {
            return Err("clarification_not_expired".to_owned());
        }
        let mut next = self.clone();
        next.status = ClarificationStatus::Expired;
        next.validate()?;
        Ok(next)
    }

    pub fn waiting_view(&self) -> Result<ClarificationWaitView, String> {
        self.validate()?;
        if self.status != ClarificationStatus::Pending {
            return Err("clarification_not_waiting".to_owned());
        }
        Ok(ClarificationWaitView {
            schema: CLARIFICATION_WAIT_SCHEMA.to_owned(),
            version: CLARIFICATION_VERSION,
            interaction_id: self.interaction_id,
            run_id: self.run_id,
            turn_id: self.turn_id,
            step_id: self.step_id,
            status: CLARIFICATION_WAITING_STATUS.to_owned(),
            waiting_for_input: true,
            required: self.required,
            expires_at_unix_ms: self.expires_at_unix_ms,
            options: self.options.clone(),
            actionable_id: self.interaction_id.to_string(),
        })
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "interaction_id": self.interaction_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "question": self.question,
            "options": self.options,
            "required": self.required,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "cancel_policy": self.cancel_policy,
            "created_by_role": self.created_by_role,
            "responder_roles": self.responder_roles,
        }))
    }

    fn validate_answer_binding(&self, answer: &ClarificationAnswer) -> Result<(), String> {
        if answer.interaction_id != self.interaction_id {
            return Err("clarification_answer_interaction_mismatch".to_owned());
        }
        if answer.run_id != self.run_id {
            return Err("clarification_answer_run_mismatch".to_owned());
        }
        if answer.turn_id != self.turn_id {
            return Err("clarification_answer_turn_mismatch".to_owned());
        }
        if answer.step_id != self.step_id {
            return Err("clarification_answer_step_mismatch".to_owned());
        }
        if answer.request_digest != self.request_digest {
            return Err("clarification_answer_request_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationResolution {
    pub schema: String,
    pub version: SchemaVersion,
    pub interaction_id: InteractionId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<StepId>,
    pub answer_id: RequestId,
    pub request_digest: String,
    pub answer_digest: String,
    pub resume_original_step: bool,
    pub waiting_for_input: bool,
}

impl ClarificationResolution {
    fn from_answer(
        request: &ClarificationRequest,
        answer: &ClarificationAnswer,
    ) -> Result<Self, String> {
        let resolution = Self {
            schema: CLARIFICATION_RESOLUTION_SCHEMA.to_owned(),
            version: CLARIFICATION_VERSION,
            interaction_id: request.interaction_id,
            run_id: request.run_id,
            turn_id: request.turn_id,
            step_id: request.step_id,
            answer_id: answer.answer_id,
            request_digest: request.request_digest.clone(),
            answer_digest: answer.answer_digest.clone(),
            resume_original_step: true,
            waiting_for_input: false,
        };
        resolution.validate()?;
        Ok(resolution)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLARIFICATION_RESOLUTION_SCHEMA
            || self.version != CLARIFICATION_VERSION
            || self.interaction_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.answer_id.as_uuid().is_nil()
            || !digest(&self.request_digest)
            || !digest(&self.answer_digest)
            || !self.resume_original_step
            || self.waiting_for_input
        {
            return Err("clarification_resolution_invalid".to_owned());
        }
        if self.step_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("clarification_resolution_step_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationWaitView {
    pub schema: String,
    pub version: SchemaVersion,
    pub interaction_id: InteractionId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<StepId>,
    pub status: String,
    pub waiting_for_input: bool,
    pub required: bool,
    pub expires_at_unix_ms: u64,
    pub options: Vec<ClarificationOption>,
    pub actionable_id: String,
}

impl ClarificationWaitView {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLARIFICATION_WAIT_SCHEMA
            || self.version != CLARIFICATION_VERSION
            || self.interaction_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.status != CLARIFICATION_WAITING_STATUS
            || !self.waiting_for_input
            || self.expires_at_unix_ms == 0
            || self.actionable_id != self.interaction_id.to_string()
        {
            return Err("clarification_wait_invalid".to_owned());
        }
        if self.step_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("clarification_wait_step_invalid".to_owned());
        }
        if self.options.len() > MAX_CLARIFICATION_OPTIONS {
            return Err("clarification_wait_options_invalid".to_owned());
        }
        for option in &self.options {
            option.validate()?;
        }
        Ok(())
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
