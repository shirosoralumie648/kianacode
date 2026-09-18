//! Deterministic snapshot/delta/final usage accumulation.
//!
//! This reducer is attempt-local and side-effect free. BQ-08+ owns durable reservation and
//! settlement; this module only rejects sequence conflicts, regression, overflow and snapshots
//! that no longer contain the known cumulative total.

use crate::{AttemptId, NormalizedUsage, UsageObservation, UsageVector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const USAGE_ACCUMULATOR_SCHEMA: &str = "kiana.usage-accumulator.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageApplyOutcome {
    Applied,
    Duplicate,
}

pub struct UsageAccumulator {
    attempt_id: AttemptId,
    latest: Option<UsageVector>,
    last_sequence: Option<u64>,
    seen: BTreeMap<u64, String>,
    finalized: bool,
}

impl UsageAccumulator {
    pub fn new(attempt_id: AttemptId) -> Result<Self, String> {
        if attempt_id.as_uuid().is_nil() {
            return Err("usage_accumulator_attempt_invalid".to_owned());
        }
        Ok(Self {
            attempt_id,
            latest: None,
            last_sequence: None,
            seen: BTreeMap::new(),
            finalized: false,
        })
    }

    pub fn schema(&self) -> &'static str {
        USAGE_ACCUMULATOR_SCHEMA
    }

    pub fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    pub fn latest(&self) -> Option<&UsageVector> {
        self.latest.as_ref()
    }

    pub fn finalized(&self) -> bool {
        self.finalized
    }

    pub fn apply(&mut self, usage: &NormalizedUsage) -> Result<UsageApplyOutcome, String> {
        usage.validate()?;
        if usage.attempt_id != self.attempt_id {
            return Err("usage_accumulator_attempt_mismatch".to_owned());
        }
        if self.finalized {
            return Err("usage_accumulator_after_final".to_owned());
        }
        let sequence = usage
            .sequence
            .filter(|sequence| *sequence > 0)
            .ok_or_else(|| "usage_accumulator_sequence_required".to_owned())?;
        if let Some(previous_digest) = self.seen.get(&sequence) {
            if previous_digest == &usage.usage_digest {
                return Ok(UsageApplyOutcome::Duplicate);
            }
            return Err("usage_sequence_conflict".to_owned());
        }
        if self.last_sequence.is_some_and(|last| sequence < last) {
            return Err("usage_sequence_regression".to_owned());
        }

        let next = match usage.observation {
            UsageObservation::Snapshot => {
                if let Some(current) = &self.latest {
                    ensure_contains(current, &usage.vector)?;
                }
                usage.vector.clone()
            }
            UsageObservation::Delta => add_delta(self.latest.as_ref(), &usage.vector)?,
            UsageObservation::Final => {
                if let Some(current) = &self.latest {
                    ensure_contains(current, &usage.vector)?;
                }
                usage.vector.clone()
            }
        };
        self.latest = Some(next);
        self.last_sequence = Some(sequence);
        self.seen.insert(sequence, usage.usage_digest.clone());
        if usage.observation == UsageObservation::Final {
            self.finalized = true;
        }
        Ok(UsageApplyOutcome::Applied)
    }
}

fn ensure_contains(current: &UsageVector, next: &UsageVector) -> Result<(), String> {
    for (name, before, after) in [
        ("input_tokens", current.input_tokens, next.input_tokens),
        ("output_tokens", current.output_tokens, next.output_tokens),
        (
            "cache_read_tokens",
            current.cache_read_tokens,
            next.cache_read_tokens,
        ),
        (
            "cache_write_tokens",
            current.cache_write_tokens,
            next.cache_write_tokens,
        ),
        (
            "reasoning_output_tokens",
            current.reasoning_output_tokens,
            next.reasoning_output_tokens,
        ),
        (
            "audio_input_tokens",
            current.audio_input_tokens,
            next.audio_input_tokens,
        ),
        (
            "audio_output_tokens",
            current.audio_output_tokens,
            next.audio_output_tokens,
        ),
    ] {
        if let (Some(before), Some(after)) = (before, after) {
            if after < before {
                return Err(format!("usage_snapshot_not_containing:{name}"));
            }
        } else if before.is_some() && after.is_none() {
            return Err(format!("usage_snapshot_dropped_known:{name}"));
        }
    }
    for (name, before, after) in [
        ("tool_calls", current.tool_calls, next.tool_calls),
        ("effect_count", current.effect_count, next.effect_count),
        ("wall_time_ms", current.wall_time_ms, next.wall_time_ms),
        ("output_bytes", current.output_bytes, next.output_bytes),
        (
            "artifact_bytes",
            current.artifact_bytes,
            next.artifact_bytes,
        ),
        ("storage_bytes", current.storage_bytes, next.storage_bytes),
    ] {
        if after < before {
            return Err(format!("usage_snapshot_not_containing:{name}"));
        }
    }
    Ok(())
}

fn add_delta(current: Option<&UsageVector>, delta: &UsageVector) -> Result<UsageVector, String> {
    let Some(current) = current else {
        return Err("usage_delta_without_snapshot".to_owned());
    };
    Ok(UsageVector {
        schema: delta.schema.clone(),
        input_tokens: add_optional(current.input_tokens, delta.input_tokens, "input_tokens")?,
        output_tokens: add_optional(current.output_tokens, delta.output_tokens, "output_tokens")?,
        cache_read_tokens: add_optional(
            current.cache_read_tokens,
            delta.cache_read_tokens,
            "cache_read_tokens",
        )?,
        cache_write_tokens: add_optional(
            current.cache_write_tokens,
            delta.cache_write_tokens,
            "cache_write_tokens",
        )?,
        reasoning_output_tokens: add_optional(
            current.reasoning_output_tokens,
            delta.reasoning_output_tokens,
            "reasoning_output_tokens",
        )?,
        audio_input_tokens: add_optional(
            current.audio_input_tokens,
            delta.audio_input_tokens,
            "audio_input_tokens",
        )?,
        audio_output_tokens: add_optional(
            current.audio_output_tokens,
            delta.audio_output_tokens,
            "audio_output_tokens",
        )?,
        tool_calls: current
            .tool_calls
            .checked_add(delta.tool_calls)
            .ok_or_else(|| "usage_accumulator_overflow:tool_calls".to_owned())?,
        effect_count: current
            .effect_count
            .checked_add(delta.effect_count)
            .ok_or_else(|| "usage_accumulator_overflow:effect_count".to_owned())?,
        wall_time_ms: current
            .wall_time_ms
            .checked_add(delta.wall_time_ms)
            .ok_or_else(|| "usage_accumulator_overflow:wall_time_ms".to_owned())?,
        output_bytes: current
            .output_bytes
            .checked_add(delta.output_bytes)
            .ok_or_else(|| "usage_accumulator_overflow:output_bytes".to_owned())?,
        artifact_bytes: current
            .artifact_bytes
            .checked_add(delta.artifact_bytes)
            .ok_or_else(|| "usage_accumulator_overflow:artifact_bytes".to_owned())?,
        storage_bytes: current
            .storage_bytes
            .checked_add(delta.storage_bytes)
            .ok_or_else(|| "usage_accumulator_overflow:storage_bytes".to_owned())?,
    })
}

fn add_optional(
    current: Option<u64>,
    delta: Option<u64>,
    name: &str,
) -> Result<Option<u64>, String> {
    match (current, delta) {
        (Some(current), Some(delta)) => current
            .checked_add(delta)
            .map(Some)
            .ok_or_else(|| format!("usage_accumulator_overflow:{name}")),
        (Some(current), None) => Ok(Some(current)),
        (None, Some(_)) => Err(format!("usage_delta_unknown_base:{name}")),
        (None, None) => Ok(None),
    }
}
