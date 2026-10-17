//! Provider round recovery and in-flight reconciliation contracts.
//!
//! This module is a bounded, read-only recovery boundary. It consumes already committed model
//! and capability facts, verifies the exact route/data/authority material needed for an explicit
//! resume, and classifies uncertain work. It never opens a provider connection, dispatches a
//! capability, or treats a transcript/UI projection as authority.

use crate::{
    json_digest, EventCursor, EventId, InvocationId, ModelAttemptId, ModelFactPersistenceStatus,
    ModelMessage, ModelProtocol, ModelRole, ModelRoute, ProtectedReplayMaterial,
    ProtectedReplayRef, ProviderContinuation, RequestId, RunId, SchemaVersion, StepId, TurnId,
    PROVIDER_CONTINUATION_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const PROVIDER_RECOVERY_SCHEMA: &str = "kiana.provider-recovery.v1";
pub const PROVIDER_RESUME_BINDING_SCHEMA: &str = "kiana.provider-resume-binding.v1";
pub const PROVIDER_HISTORY_FACT_SCHEMA: &str = "kiana.provider-history-fact.v1";
pub const PROVIDER_HISTORY_PROJECTION_SCHEMA: &str = "kiana.provider-history-projection.v1";
pub const PROVIDER_IN_FLIGHT_SCHEMA: &str = "kiana.provider-in-flight.v1";
pub const PROVIDER_RECONCILIATION_SCHEMA: &str = "kiana.provider-reconciliation.v1";
pub const PROVIDER_ROUND_PLAN_SCHEMA: &str = "kiana.provider-round-plan.v1";
pub const PROVIDER_RECOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_PROVIDER_HISTORY_FACTS: usize = 4_096;
pub const MAX_PROVIDER_HISTORY_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_PROVIDER_HISTORY_TOOL_CALLS: usize = 256;
pub const MAX_PROVIDER_ROUND_OBSERVATIONS: usize = 256;
pub const PROVIDER_RECOVERY_REPLAY_MATERIAL_MISSING: &str =
    "missing_reasoning_material_blocks_resume";
pub const PROVIDER_RECOVERY_STALE_ROUTE: &str = "stale_route_authority_cannot_resume";
pub const PROVIDER_RECOVERY_NO_MODEL_RESUBMIT: &str =
    "inflight_model_attempt_is_not_resubmitted_on_restart";
pub const PROVIDER_RECOVERY_NO_CAPABILITY_RESUBMIT: &str =
    "inflight_capability_is_not_resubmitted_on_restart";
pub const PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH: &str = "provider_recovery_round_scope_mismatch";
pub const PROVIDER_RECOVERY_ROUND_OBSERVATION_CONFLICT: &str =
    "provider_recovery_round_observation_conflict";
pub const PROVIDER_RECOVERY_ROUND_INPUT_MISSING: &str = "provider_recovery_round_input_missing";
pub const PROVIDER_RECOVERY_ROUND_RESUME_BLOCKED: &str =
    "provider_recovery_round_resume_blocked_by_unknown";
pub const PROVIDER_RECOVERY_ROUND_ATTEMPT_CONSUMED: &str =
    "provider_recovery_round_attempt_already_consumed";
pub const PROVIDER_RECOVERY_ROUND_HISTORY_INCOMPLETE: &str = "model_history_incomplete_batch";
pub const PROVIDER_RESUME_CHECK_INVALID: &str = "provider_resume_check_invalid";

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

fn uuid_present<T>(value: T, field: &str) -> Result<(), String>
where
    T: Copy + IntoUuid,
{
    if value.into_uuid().is_nil() {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

trait IntoUuid {
    fn into_uuid(self) -> uuid::Uuid;
}

macro_rules! impl_into_uuid {
    ($($ty:ty),+ $(,)?) => {
        $(impl IntoUuid for $ty {
            fn into_uuid(self) -> uuid::Uuid {
                self.as_uuid()
            }
        })+
    };
}

impl_into_uuid!(
    RunId,
    TurnId,
    StepId,
    RequestId,
    ModelAttemptId,
    EventId,
    InvocationId
);

/// The crash boundary observed from committed facts. `SentAwaitingResponse` and later model
/// phases are never safe to submit a second provider request automatically.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInFlightPhase {
    PreparedBeforeSend,
    SentAwaitingResponse,
    ModelCompletedBeforeCommit,
    CapabilityCompletedBeforeDelivery,
    TerminalCommitted,
}

impl ProviderInFlightPhase {
    pub const fn action(self) -> ProviderRecoveryAction {
        match self {
            Self::PreparedBeforeSend => ProviderRecoveryAction::ReauthorizeAndResume,
            Self::SentAwaitingResponse | Self::ModelCompletedBeforeCommit => {
                ProviderRecoveryAction::ReconcileModelUnknown
            }
            Self::CapabilityCompletedBeforeDelivery => {
                ProviderRecoveryAction::ReconcileCapabilityUnknown
            }
            Self::TerminalCommitted => ProviderRecoveryAction::NoAction,
        }
    }

    pub const fn is_unknown(self) -> bool {
        matches!(
            self,
            Self::SentAwaitingResponse
                | Self::ModelCompletedBeforeCommit
                | Self::CapabilityCompletedBeforeDelivery
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRecoveryAction {
    ReauthorizeAndResume,
    ReconcileModelUnknown,
    ReconcileCapabilityUnknown,
    NoAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUsageDisposition {
    NotObserved,
    Unknown,
    Final,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderReconciliationKind {
    ModelAttempt,
    CapabilityInvocation,
}

/// Route/profile/dialect/replay material that a fresh process may use after the ControlPlane has
/// explicitly re-admitted the same run. The value is reference-only; private replay bytes stay in
/// the protected artifact adapter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderResumeBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub model_call_id: RequestId,
    pub model_attempt_id: ModelAttemptId,
    pub route: ModelRoute,
    pub route_digest: String,
    pub profile: String,
    pub dialect: ModelProtocol,
    pub prompt_digest: String,
    pub schema_digest: String,
    pub tool_catalog_digest: String,
    pub data_revision: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub expires_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<ProtectedReplayRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<ProviderContinuation>,
    pub binding_digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderResumeSource {
    RemoteContinuation,
    LocalReplay,
}

impl ProviderResumeBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        step_id: StepId,
        model_call_id: RequestId,
        model_attempt_id: ModelAttemptId,
        route: ModelRoute,
        prompt_digest: impl Into<String>,
        schema_digest: impl Into<String>,
        tool_catalog_digest: impl Into<String>,
        data_revision: impl Into<String>,
        authority_epoch: u64,
        data_epoch: u64,
        expires_at_unix_ms: u64,
        replay: Option<ProtectedReplayRef>,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: PROVIDER_RESUME_BINDING_SCHEMA.to_owned(),
            version: PROVIDER_RECOVERY_VERSION,
            run_id,
            turn_id,
            step_id,
            model_call_id,
            model_attempt_id,
            profile: route.profile.clone(),
            dialect: route.protocol,
            route_digest: route.digest(),
            route,
            prompt_digest: prompt_digest.into(),
            schema_digest: schema_digest.into(),
            tool_catalog_digest: tool_catalog_digest.into(),
            data_revision: data_revision.into(),
            authority_epoch,
            data_epoch,
            expires_at_unix_ms,
            replay,
            continuation: None,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn with_continuation(mut self, continuation: ProviderContinuation) -> Result<Self, String> {
        self.continuation = Some(continuation);
        self.binding_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_RESUME_BINDING_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_RECOVERY_VERSION)
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.expires_at_unix_ms == 0
        {
            return Err("provider_resume_binding_header_invalid".to_owned());
        }
        uuid_present(self.run_id, "provider_resume_run_id")?;
        uuid_present(self.turn_id, "provider_resume_turn_id")?;
        uuid_present(self.step_id, "provider_resume_step_id")?;
        uuid_present(self.model_call_id, "provider_resume_model_call_id")?;
        uuid_present(self.model_attempt_id, "provider_resume_model_attempt_id")?;
        for (value, field, max) in [
            (&self.route.provider_id, "provider_resume_provider_id", 128),
            (
                &self.route.connection_id,
                "provider_resume_connection_id",
                256,
            ),
            (&self.route.model_id, "provider_resume_model_id", 256),
            (
                &self.route.configuration_revision,
                "provider_resume_configuration_revision",
                256,
            ),
            (&self.profile, "provider_resume_profile", 256),
        ] {
            required(value, field, max)?;
        }
        if self.profile != self.route.profile
            || self.dialect != self.route.protocol
            || self.route_digest != self.route.digest()
        {
            return Err("provider_resume_route_binding_invalid".to_owned());
        }
        for (value, field) in [
            (&self.route_digest, "provider_resume_route_digest"),
            (&self.prompt_digest, "provider_resume_prompt_digest"),
            (&self.schema_digest, "provider_resume_schema_digest"),
            (
                &self.tool_catalog_digest,
                "provider_resume_tool_catalog_digest",
            ),
            (&self.binding_digest, "provider_resume_binding_digest"),
        ] {
            digest(value, field)?;
        }
        required(&self.data_revision, "provider_resume_data_revision", 256)?;
        if let Some(replay) = &self.replay {
            replay
                .validate_for_call(&self.route, self.model_call_id, self.expires_at_unix_ms)
                .map_err(|error| error.code)?;
        }
        if let Some(continuation) = &self.continuation {
            // A remote continuation is an untrusted optimization. An expired, malformed or
            // route-mismatched ID remains inspectable so validate_for_resume can select a complete
            // local protected artifact; it never grants execution by itself.
            if continuation.schema != PROVIDER_CONTINUATION_SCHEMA {
                return Err("provider_continuation_scope_mismatch".to_owned());
            }
        }
        if self.binding_digest != self.digest() {
            return Err("provider_resume_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Re-check all volatile scope before a resumed provider request. A valid remote continuation
    /// wins; an invalid/missing remote continuation can fall back only to complete local material.
    pub fn validate_for_resume(
        &self,
        route: &ModelRoute,
        prompt_digest: &str,
        schema_digest: &str,
        tool_catalog_digest: &str,
        data_revision: &str,
        authority_epoch: u64,
        data_epoch: u64,
        now_unix_ms: u64,
        local_material: Option<&ProtectedReplayMaterial>,
    ) -> Result<ProviderResumeSource, String> {
        // Validate the digest envelope first, but permit a stale/invalid remote continuation to
        // be replaced by an independently validated local protected artifact.
        if self.schema != PROVIDER_RESUME_BINDING_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_RECOVERY_VERSION)
            || self.binding_digest != self.digest()
        {
            return Err("provider_resume_binding_invalid".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("provider_resume_expired".to_owned());
        }
        if authority_epoch != self.authority_epoch || data_epoch != self.data_epoch {
            return Err(PROVIDER_RECOVERY_STALE_ROUTE.to_owned());
        }
        if route != &self.route
            || prompt_digest != self.prompt_digest
            || schema_digest != self.schema_digest
            || tool_catalog_digest != self.tool_catalog_digest
            || data_revision != self.data_revision
        {
            return Err(PROVIDER_RECOVERY_STALE_ROUTE.to_owned());
        }
        if let Some(continuation) = &self.continuation {
            if continuation.validate().is_ok()
                && continuation.provider_id == route.provider_id
                && continuation.protocol == route.protocol
                && continuation.route_digest == self.route_digest
            {
                return Ok(ProviderResumeSource::RemoteContinuation);
            }
        }
        let Some(reference) = self.replay.as_ref() else {
            return Err(PROVIDER_RECOVERY_REPLAY_MATERIAL_MISSING.to_owned());
        };
        let Some(material) = local_material else {
            return Err(PROVIDER_RECOVERY_REPLAY_MATERIAL_MISSING.to_owned());
        };
        material
            .validate_for(reference, route, now_unix_ms)
            .map_err(|error| error.code)?;
        Ok(ProviderResumeSource::LocalReplay)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "model_call_id": self.model_call_id,
            "model_attempt_id": self.model_attempt_id,
            "route": self.route,
            "route_digest": self.route_digest,
            "profile": self.profile,
            "dialect": self.dialect,
            "prompt_digest": self.prompt_digest,
            "schema_digest": self.schema_digest,
            "tool_catalog_digest": self.tool_catalog_digest,
            "data_revision": self.data_revision,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "replay": self.replay,
            "continuation": self.continuation,
        }))
    }
}

/// One committed prompt/model/tool fact used to rebuild model-visible history. The source cursor
/// and commitment status prevent a partial response or an uncommitted write from becoming an
/// assistant message during recovery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderHistoryFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub source_cursor: EventCursor,
    pub source_event_id: EventId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_attempt_id: Option<ModelAttemptId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<InvocationId>,
    pub commitment: ModelFactPersistenceStatus,
    pub message: ModelMessage,
    pub fact_digest: String,
}

impl ProviderHistoryFact {
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        source_cursor: EventCursor,
        source_event_id: EventId,
        model_attempt_id: Option<ModelAttemptId>,
        invocation_id: Option<InvocationId>,
        commitment: ModelFactPersistenceStatus,
        message: ModelMessage,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: PROVIDER_HISTORY_FACT_SCHEMA.to_owned(),
            version: PROVIDER_RECOVERY_VERSION,
            run_id,
            turn_id,
            source_cursor,
            source_event_id,
            model_attempt_id,
            invocation_id,
            commitment,
            message,
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_HISTORY_FACT_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_RECOVERY_VERSION)
            || self.source_cursor == 0
        {
            return Err("provider_history_fact_header_invalid".to_owned());
        }
        uuid_present(self.run_id, "provider_history_run_id")?;
        uuid_present(self.turn_id, "provider_history_turn_id")?;
        uuid_present(self.source_event_id, "provider_history_source_event_id")?;
        if let Some(attempt) = self.model_attempt_id {
            uuid_present(attempt, "provider_history_model_attempt_id")?;
        }
        if let Some(invocation) = self.invocation_id {
            uuid_present(invocation, "provider_history_invocation_id")?;
        }
        if self.message.text.len() > MAX_PROVIDER_HISTORY_TEXT_BYTES
            || self.message.text.contains('\0')
            || self.message.tool_calls.len() > MAX_PROVIDER_HISTORY_TOOL_CALLS
        {
            return Err("provider_history_message_unbounded".to_owned());
        }
        self.message
            .validate_content()
            .map_err(|error| error.code)?;
        match self.message.role {
            ModelRole::Assistant if self.model_attempt_id.is_none() => {
                return Err("provider_history_assistant_attempt_required".to_owned())
            }
            ModelRole::Tool if self.invocation_id.is_none() => {
                return Err("provider_history_tool_invocation_required".to_owned())
            }
            ModelRole::Tool if self.message.tool_call_id.is_none() => {
                return Err("provider_history_tool_call_required".to_owned())
            }
            ModelRole::Tool => {}
            _ if self.invocation_id.is_some() => {
                return Err("provider_history_invocation_role_mismatch".to_owned())
            }
            _ => {}
        }
        digest(&self.fact_digest, "provider_history_fact_digest")?;
        if self.fact_digest != self.digest() {
            return Err("provider_history_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "source_cursor": self.source_cursor,
            "source_event_id": self.source_event_id,
            "model_attempt_id": self.model_attempt_id,
            "invocation_id": self.invocation_id,
            "commitment": self.commitment,
            "message": self.message,
        }))
    }
}

/// Read-only history projection rebuilt from committed facts. It is not a checkpoint and cannot
/// itself authorize a resumed runner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderHistoryProjection {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub source_cursor: EventCursor,
    pub messages: Vec<ModelMessage>,
    pub completed_invocations: Vec<InvocationId>,
    pub projection_digest: String,
}

impl ProviderHistoryProjection {
    pub fn rebuild(mut facts: Vec<ProviderHistoryFact>) -> Result<Self, String> {
        if facts.len() > MAX_PROVIDER_HISTORY_FACTS {
            return Err("provider_history_fact_limit_exceeded".to_owned());
        }
        for fact in &facts {
            fact.validate()?;
            if fact.commitment != ModelFactPersistenceStatus::Committed {
                return Err("provider_history_uncommitted_fact_not_resumable".to_owned());
            }
        }
        facts.sort_by_key(|fact| fact.source_cursor);
        let mut run_id = None;
        let mut turn_id = None;
        let mut last_cursor = 0;
        let mut source_ids = BTreeSet::new();
        let mut messages = Vec::with_capacity(facts.len());
        let mut completed = BTreeSet::new();
        for fact in facts {
            if fact.source_cursor <= last_cursor {
                return Err("provider_history_source_cursor_conflict".to_owned());
            }
            if !source_ids.insert(fact.source_event_id) {
                return Err("provider_history_source_event_duplicate".to_owned());
            }
            if let Some(expected) = run_id {
                if expected != fact.run_id {
                    return Err("provider_history_run_mismatch".to_owned());
                }
            } else {
                run_id = Some(fact.run_id);
            }
            if let Some(expected) = turn_id {
                if expected != fact.turn_id {
                    return Err("provider_history_turn_mismatch".to_owned());
                }
            } else {
                turn_id = Some(fact.turn_id);
            }
            if fact.message.role == ModelRole::Tool {
                let invocation = fact
                    .invocation_id
                    .ok_or_else(|| "provider_history_tool_invocation_required".to_owned())?;
                completed.insert(invocation);
            }
            last_cursor = fact.source_cursor;
            messages.push(fact.message);
        }
        crate::validate_model_history(&messages).map_err(|error| error.code)?;
        let run_id = run_id.ok_or_else(|| "provider_history_run_missing".to_owned())?;
        let turn_id = turn_id.ok_or_else(|| "provider_history_turn_missing".to_owned())?;
        let completed_invocations = completed.into_iter().collect::<Vec<_>>();
        let mut projection = Self {
            schema: PROVIDER_HISTORY_PROJECTION_SCHEMA.to_owned(),
            version: PROVIDER_RECOVERY_VERSION,
            run_id,
            turn_id,
            source_cursor: last_cursor,
            messages,
            completed_invocations,
            projection_digest: String::new(),
        };
        projection.projection_digest = projection.digest();
        projection.validate()?;
        Ok(projection)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_HISTORY_PROJECTION_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_RECOVERY_VERSION)
            || self.source_cursor == 0
            || self
                .completed_invocations
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("provider_history_projection_invalid".to_owned());
        }
        uuid_present(self.run_id, "provider_history_projection_run_id")?;
        uuid_present(self.turn_id, "provider_history_projection_turn_id")?;
        for invocation in &self.completed_invocations {
            uuid_present(*invocation, "provider_history_projection_invocation_id")?;
        }
        crate::validate_model_history(&self.messages).map_err(|error| error.code)?;
        digest(
            &self.projection_digest,
            "provider_history_projection_digest",
        )?;
        if self.projection_digest != self.digest() {
            return Err("provider_history_projection_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn invocation_completed(&self, invocation_id: InvocationId) -> bool {
        self.completed_invocations.contains(&invocation_id)
    }

    /// A completed invocation is never re-executed by a recovery projection. A caller must issue
    /// a fresh ControlPlane request after reconciliation if more work is desired.
    pub const fn may_reexecute_completed_invocation(&self) -> bool {
        false
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "source_cursor": self.source_cursor,
            "messages": self.messages,
            "completed_invocations": self.completed_invocations,
        }))
    }
}

/// A committed observation at one crash point. It intentionally stores no provider payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInFlightObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub model_call_id: RequestId,
    pub model_attempt_id: ModelAttemptId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<InvocationId>,
    pub phase: ProviderInFlightPhase,
    pub request_digest: String,
    pub route_digest: String,
    pub usage: ProviderUsageDisposition,
    pub source_cursor: EventCursor,
    pub observation_digest: String,
}

impl ProviderInFlightObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        step_id: StepId,
        model_call_id: RequestId,
        model_attempt_id: ModelAttemptId,
        invocation_id: Option<InvocationId>,
        phase: ProviderInFlightPhase,
        request_digest: impl Into<String>,
        route_digest: impl Into<String>,
        usage: ProviderUsageDisposition,
        source_cursor: EventCursor,
    ) -> Result<Self, String> {
        let mut observation = Self {
            schema: PROVIDER_IN_FLIGHT_SCHEMA.to_owned(),
            version: PROVIDER_RECOVERY_VERSION,
            run_id,
            turn_id,
            step_id,
            model_call_id,
            model_attempt_id,
            invocation_id,
            phase,
            request_digest: request_digest.into(),
            route_digest: route_digest.into(),
            usage,
            source_cursor,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_IN_FLIGHT_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_RECOVERY_VERSION)
            || self.source_cursor == 0
        {
            return Err("provider_in_flight_header_invalid".to_owned());
        }
        uuid_present(self.run_id, "provider_in_flight_run_id")?;
        uuid_present(self.turn_id, "provider_in_flight_turn_id")?;
        uuid_present(self.step_id, "provider_in_flight_step_id")?;
        uuid_present(self.model_call_id, "provider_in_flight_model_call_id")?;
        uuid_present(self.model_attempt_id, "provider_in_flight_model_attempt_id")?;
        if self.phase == ProviderInFlightPhase::CapabilityCompletedBeforeDelivery
            && self.invocation_id.is_none()
        {
            return Err("provider_in_flight_capability_invocation_required".to_owned());
        }
        if self.phase != ProviderInFlightPhase::CapabilityCompletedBeforeDelivery
            && self.invocation_id.is_some()
        {
            return Err("provider_in_flight_invocation_unexpected".to_owned());
        }
        if let Some(invocation) = self.invocation_id {
            uuid_present(invocation, "provider_in_flight_invocation_id")?;
        }
        digest(&self.request_digest, "provider_in_flight_request_digest")?;
        digest(&self.route_digest, "provider_in_flight_route_digest")?;
        digest(
            &self.observation_digest,
            "provider_in_flight_observation_digest",
        )?;
        if self.observation_digest != self.digest() {
            return Err("provider_in_flight_observation_digest_mismatch".to_owned());
        }
        if self.phase.is_unknown() && self.usage == ProviderUsageDisposition::Final {
            return Err("provider_in_flight_unknown_usage_cannot_be_final".to_owned());
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
            "model_call_id": self.model_call_id,
            "model_attempt_id": self.model_attempt_id,
            "invocation_id": self.invocation_id,
            "phase": self.phase,
            "request_digest": self.request_digest,
            "route_digest": self.route_digest,
            "usage": self.usage,
            "source_cursor": self.source_cursor,
        }))
    }
}

/// Classification emitted to the existing Incident/RecoveryPlan projection. It does not append
/// facts or retry either side effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReconciliationCase {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub step_id: StepId,
    pub model_attempt_id: ModelAttemptId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<InvocationId>,
    pub kind: ProviderReconciliationKind,
    pub phase: ProviderInFlightPhase,
    pub action: ProviderRecoveryAction,
    pub usage: ProviderUsageDisposition,
    pub source_observation_digest: String,
    pub reconciliation_required: bool,
    pub automatic_retry_allowed: bool,
    pub case_digest: String,
}

impl ProviderReconciliationCase {
    pub fn from_observation(observation: &ProviderInFlightObservation) -> Result<Self, String> {
        observation.validate()?;
        let kind = if observation.phase == ProviderInFlightPhase::CapabilityCompletedBeforeDelivery
        {
            ProviderReconciliationKind::CapabilityInvocation
        } else {
            ProviderReconciliationKind::ModelAttempt
        };
        let mut case = Self {
            schema: PROVIDER_RECONCILIATION_SCHEMA.to_owned(),
            version: PROVIDER_RECOVERY_VERSION,
            run_id: observation.run_id,
            turn_id: observation.turn_id,
            step_id: observation.step_id,
            model_attempt_id: observation.model_attempt_id,
            invocation_id: observation.invocation_id,
            kind,
            phase: observation.phase,
            action: observation.phase.action(),
            usage: if observation.phase.is_unknown() {
                ProviderUsageDisposition::Unknown
            } else {
                observation.usage
            },
            source_observation_digest: observation.observation_digest.clone(),
            reconciliation_required: observation.phase.is_unknown(),
            automatic_retry_allowed: false,
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_RECONCILIATION_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_RECOVERY_VERSION)
            || self.reconciliation_required != self.phase.is_unknown()
            || self.automatic_retry_allowed
        {
            return Err("provider_reconciliation_case_invalid".to_owned());
        }
        uuid_present(self.run_id, "provider_reconciliation_run_id")?;
        uuid_present(self.turn_id, "provider_reconciliation_turn_id")?;
        uuid_present(self.step_id, "provider_reconciliation_step_id")?;
        uuid_present(
            self.model_attempt_id,
            "provider_reconciliation_model_attempt_id",
        )?;
        if self.kind == ProviderReconciliationKind::CapabilityInvocation
            && self.invocation_id.is_none()
        {
            return Err("provider_reconciliation_invocation_required".to_owned());
        }
        if let Some(invocation) = self.invocation_id {
            uuid_present(invocation, "provider_reconciliation_invocation_id")?;
        }
        if self.action != self.phase.action() {
            return Err("provider_reconciliation_action_mismatch".to_owned());
        }
        if self.phase.is_unknown() && self.usage != ProviderUsageDisposition::Unknown {
            return Err("provider_reconciliation_unknown_usage_required".to_owned());
        }
        digest(
            &self.source_observation_digest,
            "provider_reconciliation_source_digest",
        )?;
        digest(&self.case_digest, "provider_reconciliation_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("provider_reconciliation_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn provider_resubmission_allowed(&self) -> bool {
        self.kind == ProviderReconciliationKind::ModelAttempt
            && self.action == ProviderRecoveryAction::ReauthorizeAndResume
    }

    pub fn capability_resubmission_allowed(&self) -> bool {
        false
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "model_attempt_id": self.model_attempt_id,
            "invocation_id": self.invocation_id,
            "kind": self.kind,
            "phase": self.phase,
            "action": self.action,
            "usage": self.usage,
            "source_observation_digest": self.source_observation_digest,
            "reconciliation_required": self.reconciliation_required,
            "automatic_retry_allowed": self.automatic_retry_allowed,
        }))
    }
}

/// Live, borrowed scope used to re-authorize exactly one resumed provider request. The planner
/// never persists a route, a prompt or private replay bytes; the caller must supply the values it
/// observed in this process after the ControlPlane re-admitted the run.
#[derive(Clone, Copy, Debug)]
pub struct ProviderResumeCheck<'a> {
    pub route: &'a ModelRoute,
    pub prompt_digest: &'a str,
    pub schema_digest: &'a str,
    pub tool_catalog_digest: &'a str,
    pub data_revision: &'a str,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub now_unix_ms: u64,
    pub local_material: Option<&'a ProtectedReplayMaterial>,
}

impl ProviderResumeCheck<'_> {
    pub fn validate(&self) -> Result<(), String> {
        if self.authority_epoch == 0 || self.data_epoch == 0 || self.now_unix_ms == 0 {
            return Err(PROVIDER_RESUME_CHECK_INVALID.to_owned());
        }
        for (value, field) in [
            (self.prompt_digest, "prompt_digest"),
            (self.schema_digest, "schema_digest"),
            (self.tool_catalog_digest, "tool_catalog_digest"),
        ] {
            digest(value, field).map_err(|_| PROVIDER_RESUME_CHECK_INVALID.to_owned())?;
        }
        required(self.data_revision, "data_revision", 256)
            .map_err(|_| PROVIDER_RESUME_CHECK_INVALID.to_owned())?;
        Ok(())
    }
}

/// Complete-round recovery decision.
///
/// It binds the committed model/tool facts of exactly one run/turn to every committed in-flight
/// observation of that same round, maps each crash point to a reconciliation case, and refuses to
/// authorize a resumed provider request while any previous attempt is unknown. A round that
/// cannot be folded into a complete model-visible history (a capability that completed but whose
/// result is not committed yet) keeps `history = None` plus a stable reason instead of fabricating
/// an assistant/tool pair.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRoundPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub source_cursor: EventCursor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<ProviderHistoryProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_incomplete_reason: Option<String>,
    pub completed_invocations: Vec<InvocationId>,
    pub cases: Vec<ProviderReconciliationCase>,
    pub plan_digest: String,
}

impl ProviderRoundPlan {
    /// Fold committed facts and committed observations of one round into a single decision.
    pub fn plan(
        facts: Vec<ProviderHistoryFact>,
        observations: &[ProviderInFlightObservation],
    ) -> Result<Self, String> {
        if facts.is_empty() && observations.is_empty() {
            return Err(PROVIDER_RECOVERY_ROUND_INPUT_MISSING.to_owned());
        }
        if facts.len() > MAX_PROVIDER_HISTORY_FACTS
            || observations.len() > MAX_PROVIDER_ROUND_OBSERVATIONS
        {
            return Err("provider_recovery_round_limit_exceeded".to_owned());
        }
        for fact in &facts {
            fact.validate()?;
        }
        let (run_id, turn_id) = match facts.first() {
            Some(fact) => (fact.run_id, fact.turn_id),
            None => (observations[0].run_id, observations[0].turn_id),
        };
        let mut source_cursor = 0;
        let mut completed = BTreeSet::new();
        for fact in &facts {
            if fact.run_id != run_id || fact.turn_id != turn_id {
                return Err(PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH.to_owned());
            }
            source_cursor = source_cursor.max(fact.source_cursor);
            if fact.message.role == ModelRole::Tool {
                if let Some(invocation) = fact.invocation_id {
                    completed.insert(invocation);
                }
            }
        }
        let mut cases = Vec::with_capacity(observations.len());
        let mut observed_attempts = BTreeSet::new();
        let mut observed_cursors = BTreeSet::new();
        for observation in observations {
            observation.validate()?;
            if observation.run_id != run_id || observation.turn_id != turn_id {
                return Err(PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH.to_owned());
            }
            if !observed_attempts.insert(observation.model_attempt_id)
                || !observed_cursors.insert(observation.source_cursor)
            {
                return Err(PROVIDER_RECOVERY_ROUND_OBSERVATION_CONFLICT.to_owned());
            }
            source_cursor = source_cursor.max(observation.source_cursor);
            cases.push(ProviderReconciliationCase::from_observation(observation)?);
        }
        cases.sort_by_key(|case| case.model_attempt_id.as_uuid());
        let capability_undelivered = cases
            .iter()
            .any(|case| case.phase == ProviderInFlightPhase::CapabilityCompletedBeforeDelivery);
        // An unmatched trailing tool call is exactly the "capability completed but its result is
        // not committed yet" crash point, and only that crash point may keep the history open.
        let (history, history_incomplete_reason) = match ProviderHistoryProjection::rebuild(facts) {
            Ok(history) => (Some(history), None),
            Err(code)
                if code == PROVIDER_RECOVERY_ROUND_HISTORY_INCOMPLETE && capability_undelivered =>
            {
                (None, Some(code))
            }
            Err(code) => return Err(code),
        };
        let completed_invocations = completed.into_iter().collect::<Vec<_>>();
        if let Some(history) = &history {
            if history.source_cursor > source_cursor {
                return Err(PROVIDER_RECOVERY_ROUND_OBSERVATION_CONFLICT.to_owned());
            }
        }
        let mut plan = Self {
            schema: PROVIDER_ROUND_PLAN_SCHEMA.to_owned(),
            version: PROVIDER_RECOVERY_VERSION,
            run_id,
            turn_id,
            source_cursor,
            history,
            history_incomplete_reason,
            completed_invocations,
            cases,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_ROUND_PLAN_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_RECOVERY_VERSION)
            || self.source_cursor == 0
            || self.cases.len() > MAX_PROVIDER_ROUND_OBSERVATIONS
            || self
                .completed_invocations
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("provider_recovery_round_plan_invalid".to_owned());
        }
        uuid_present(self.run_id, "provider_recovery_round_run_id")?;
        uuid_present(self.turn_id, "provider_recovery_round_turn_id")?;
        for invocation in &self.completed_invocations {
            uuid_present(*invocation, "provider_recovery_round_invocation_id")?;
        }
        match (&self.history, &self.history_incomplete_reason) {
            (Some(history), None) => {
                history.validate()?;
                if history.run_id != self.run_id
                    || history.turn_id != self.turn_id
                    || history.source_cursor > self.source_cursor
                {
                    return Err(PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH.to_owned());
                }
            }
            (None, Some(reason))
                if reason.as_str() == PROVIDER_RECOVERY_ROUND_HISTORY_INCOMPLETE
                    && self.cases.iter().any(|case| {
                        case.phase == ProviderInFlightPhase::CapabilityCompletedBeforeDelivery
                    }) => {}
            _ => return Err("provider_recovery_round_plan_invalid".to_owned()),
        }
        if self
            .cases
            .windows(2)
            .any(|pair| pair[0].model_attempt_id >= pair[1].model_attempt_id)
        {
            return Err(PROVIDER_RECOVERY_ROUND_OBSERVATION_CONFLICT.to_owned());
        }
        for case in &self.cases {
            case.validate()?;
            if case.run_id != self.run_id || case.turn_id != self.turn_id {
                return Err(PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH.to_owned());
            }
        }
        digest(&self.plan_digest, "provider_recovery_round_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("provider_recovery_round_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The complete model-visible history of the round, or `None` while a capability result is
    /// still undelivered. Callers must not substitute a transcript when this is `None`.
    pub fn model_visible_history(&self) -> Option<&ProviderHistoryProjection> {
        self.history.as_ref()
    }

    pub fn completed_invocations(&self) -> &[InvocationId] {
        &self.completed_invocations
    }

    pub fn reconciliation_cases(&self) -> &[ProviderReconciliationCase] {
        &self.cases
    }

    pub fn requires_reconciliation(&self) -> bool {
        self.cases.iter().any(|case| case.reconciliation_required)
    }

    pub fn model_unknown_attempts(&self) -> Vec<ModelAttemptId> {
        self.cases
            .iter()
            .filter(|case| {
                case.kind == ProviderReconciliationKind::ModelAttempt
                    && case.reconciliation_required
            })
            .map(|case| case.model_attempt_id)
            .collect()
    }

    pub fn capability_unknown_invocations(&self) -> Vec<InvocationId> {
        self.cases
            .iter()
            .filter(|case| {
                case.kind == ProviderReconciliationKind::CapabilityInvocation
                    && case.reconciliation_required
            })
            .filter_map(|case| case.invocation_id)
            .collect()
    }

    /// Attempts that were prepared before send and therefore may be re-submitted after an explicit
    /// ControlPlane re-admission. Every other phase must be reconciled first.
    pub fn prepared_attempts(&self) -> Vec<ModelAttemptId> {
        self.cases
            .iter()
            .filter(|case| case.phase == ProviderInFlightPhase::PreparedBeforeSend)
            .map(|case| case.model_attempt_id)
            .collect()
    }

    /// Authorize one resumed provider request for this round. Unknown in-flight work, a missing
    /// history, a foreign run/turn or an already consumed attempt all fail closed before the
    /// resume binding's own hash/TTL/authority checks run.
    pub fn authorize_resume(
        &self,
        binding: &ProviderResumeBinding,
        check: &ProviderResumeCheck<'_>,
    ) -> Result<ProviderResumeSource, String> {
        self.validate()?;
        check.validate()?;
        if self.history.is_none() || self.requires_reconciliation() {
            return Err(PROVIDER_RECOVERY_ROUND_RESUME_BLOCKED.to_owned());
        }
        if binding.run_id != self.run_id || binding.turn_id != self.turn_id {
            return Err(PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH.to_owned());
        }
        if self.cases.iter().any(|case| {
            case.model_attempt_id == binding.model_attempt_id
                && case.phase != ProviderInFlightPhase::PreparedBeforeSend
        }) {
            return Err(PROVIDER_RECOVERY_ROUND_ATTEMPT_CONSUMED.to_owned());
        }
        binding.validate_for_resume(
            check.route,
            check.prompt_digest,
            check.schema_digest,
            check.tool_catalog_digest,
            check.data_revision,
            check.authority_epoch,
            check.data_epoch,
            check.now_unix_ms,
            check.local_material,
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "source_cursor": self.source_cursor,
            "history": self.history,
            "history_incomplete_reason": self.history_incomplete_reason,
            "completed_invocations": self.completed_invocations,
            "cases": self.cases,
        }))
    }
}

/// Stable helper used by source guards and adapters that need to state the no-resubmit rule.
pub fn recovery_action_for_phase(phase: ProviderInFlightPhase) -> ProviderRecoveryAction {
    phase.action()
}
