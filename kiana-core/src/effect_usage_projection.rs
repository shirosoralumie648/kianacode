//! Read-only BQ-15 usage projection from committed terminal invocation facts.
//!
//! The projection intentionally ignores model text, transcript/tool intent and UI counters.  A
//! usage observation is accepted only when it is attached to a terminal EventLog fact and its
//! Run/Invocation/attempt/source cursor and server-owned effect dimensions agree with that fact.
//! It never invokes Broker/handler or changes a reservation.

use kiana_domain::{
    EffectUsageObservation, EffectUsageReceipt, EffectUsageState, EventId, RunId, RuntimeEvent,
};
use thiserror::Error;

pub const EFFECT_USAGE_PROJECTION_SCHEMA: &str = "kiana.effect-usage-projection.v1";

const TERMINAL_EFFECT_EVENTS: &[&str] = &[
    "capability.blocked",
    "capability.completed",
    "capability.failed",
    "capability.cancelled",
    "capability.result_unknown",
    "run.capability_blocked",
    "execution.result_committed",
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EffectUsageProjectionError {
    #[error("effect_usage_projection_run_invalid")]
    RunInvalid,
    #[error("effect_usage_projection_identity_mismatch:{0}")]
    IdentityMismatch(String),
    #[error("effect_usage_projection_invalid:{0}")]
    Invalid(String),
}

/// Rebuild one run's BQ-15 usage receipt. `Ok(None)` means this legacy/event slice has no
/// terminal usage facts yet; callers must keep the absence visible instead of writing zero.
pub fn project_effect_usage(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Option<EffectUsageReceipt>, EffectUsageProjectionError> {
    if run_id.as_uuid().is_nil() {
        return Err(EffectUsageProjectionError::RunInvalid);
    }
    let run_text = run_id.to_string();
    let mut observations = Vec::new();
    let mut source_events = std::collections::BTreeSet::<EventId>::new();
    for event in events {
        if !TERMINAL_EFFECT_EVENTS.contains(&event.kind.as_str()) {
            continue;
        }
        let Some(value) = event.data.get("effect_usage") else {
            continue;
        };
        let usage = EffectUsageObservation::from_json(value)
            .map_err(EffectUsageProjectionError::Invalid)?;
        if usage.run_id != run_id
            || event.data.get("run_id").and_then(serde_json::Value::as_str)
                != Some(run_text.as_str())
            || event.sequence == 0
            || usage.source_event_id != event.event_id
            || usage.source_cursor != event.sequence
        {
            return Err(EffectUsageProjectionError::IdentityMismatch(
                "run_or_source_fence".to_owned(),
            ));
        }
        if let Some(event_invocation) = event
            .data
            .get("invocation_id")
            .and_then(serde_json::Value::as_str)
        {
            if event_invocation != usage.invocation_id.to_string() {
                return Err(EffectUsageProjectionError::IdentityMismatch(
                    "invocation".to_owned(),
                ));
            }
        }
        if let Some(event_attempt) = event
            .data
            .get("attempt")
            .and_then(serde_json::Value::as_u64)
        {
            if event_attempt != u64::from(usage.attempt) {
                return Err(EffectUsageProjectionError::IdentityMismatch(
                    "attempt".to_owned(),
                ));
            }
        }
        for (field, expected) in [
            ("owner_digest", usage.owner_digest.as_str()),
            ("lease_digest", usage.lease_digest.as_str()),
            ("resource_digest", usage.resource_digest.as_str()),
        ] {
            if let Some(observed) = event.data.get(field).and_then(serde_json::Value::as_str) {
                if observed != expected {
                    return Err(EffectUsageProjectionError::IdentityMismatch(
                        field.to_owned(),
                    ));
                }
            }
        }
        if event
            .data
            .get("effect_started")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
            && !usage.state.is_not_started()
        {
            return Err(EffectUsageProjectionError::Invalid(
                "effect_started_false_but_usage_started".to_owned(),
            ));
        }
        if event
            .data
            .get("effect_started")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
            && usage.state.is_not_started()
        {
            return Err(EffectUsageProjectionError::Invalid(
                "effect_started_true_but_usage_not_started".to_owned(),
            ));
        }
        match event.kind.as_str() {
            "capability.blocked" | "run.capability_blocked" | "capability.cancelled"
                if !usage.state.is_not_started() =>
            {
                return Err(EffectUsageProjectionError::Invalid(
                    "denied_or_cancelled_effect_started".to_owned(),
                ));
            }
            "capability.result_unknown" if usage.state != EffectUsageState::Unknown => {
                return Err(EffectUsageProjectionError::Invalid(
                    "unknown_terminal_usage_required".to_owned(),
                ));
            }
            _ => {}
        }
        if !source_events.insert(event.event_id) {
            return Err(EffectUsageProjectionError::IdentityMismatch(
                "duplicate_source_event".to_owned(),
            ));
        }
        observations.push(usage);
    }
    if observations.is_empty() {
        return Ok(None);
    }
    EffectUsageReceipt::from_observations(run_id, observations)
        .map(Some)
        .map_err(EffectUsageProjectionError::Invalid)
}
