//! CP-29 deterministic authority/state-machine acceptance contract.
//!
//! This module describes the invariants that a ControlPlane command/replay/crash fixture must
//! prove. It is deliberately a pure validator: it does not start a process, kill a child, call a
//! Broker or turn a simulated crash into runtime evidence. The committed EventLog/TransitionBatch
//! and the existing ControlPlane remain the authorities; this contract only checks their evidence.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const CONTROL_PLANE_AUTHORITY_SCHEMA: &str = "kiana.control-plane-authority.v1";
pub const CONTROL_PLANE_COMMAND_FACT_SCHEMA: &str = "kiana.control-plane-command-fact.v1";
pub const CONTROL_PLANE_CRASH_OBSERVATION_SCHEMA: &str = "kiana.control-plane-crash-observation.v1";

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cp29LifecycleState {
    Queued,
    Running,
    Paused,
    CancelRequested,
    Completed,
    Failed,
    Cancelled,
    ResultUnknown,
}

impl Cp29LifecycleState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

fn transition_allowed(from: Cp29LifecycleState, to: Cp29LifecycleState) -> bool {
    use Cp29LifecycleState::*;
    matches!(
        (from, to),
        (Queued, Running)
            | (Queued, Paused)
            | (Queued, CancelRequested)
            | (Queued, Failed)
            | (Running, Paused)
            | (Running, CancelRequested)
            | (Running, Completed)
            | (Running, Failed)
            | (Running, ResultUnknown)
            | (Paused, Running)
            | (Paused, CancelRequested)
            | (Paused, Failed)
            | (Paused, ResultUnknown)
            | (CancelRequested, Cancelled)
            | (CancelRequested, ResultUnknown)
            | (ResultUnknown, Paused)
    )
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cp29CrashPoint {
    WriteFrame,
    Sync,
    Approval,
    Prepare,
    Dispatch,
    Result,
    Replay,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cp29CommandFact {
    pub schema: String,
    pub sequence: u64,
    pub command_id: String,
    pub command_digest: String,
    pub state_before: Cp29LifecycleState,
    pub state_after: Cp29LifecycleState,
    pub authority_epoch: u64,
    pub permission_rank_before: u32,
    pub permission_rank_after: u32,
    pub budget_limit: u64,
    pub budget_reserved: u64,
    pub budget_settled: u64,
    pub effect_count: u32,
    pub replayed: bool,
    pub result_unknown: bool,
    pub digest: String,
}

impl Cp29CommandFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sequence: u64,
        command_id: impl Into<String>,
        command_digest: impl Into<String>,
        state_before: Cp29LifecycleState,
        state_after: Cp29LifecycleState,
        authority_epoch: u64,
        permission_rank_before: u32,
        permission_rank_after: u32,
        budget_limit: u64,
        budget_reserved: u64,
        budget_settled: u64,
        effect_count: u32,
        replayed: bool,
        result_unknown: bool,
    ) -> Self {
        let mut fact = Self {
            schema: CONTROL_PLANE_COMMAND_FACT_SCHEMA.to_owned(),
            sequence,
            command_id: command_id.into(),
            command_digest: command_digest.into(),
            state_before,
            state_after,
            authority_epoch,
            permission_rank_before,
            permission_rank_after,
            budget_limit,
            budget_reserved,
            budget_settled,
            effect_count,
            replayed,
            result_unknown,
            digest: String::new(),
        };
        fact.digest = fact.canonical_digest();
        fact
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_COMMAND_FACT_SCHEMA
            || self.sequence == 0
            || self.authority_epoch == 0
            || self.permission_rank_after > self.permission_rank_before
            || self.budget_reserved > self.budget_limit
            || self.budget_settled > self.budget_reserved
        {
            return Err("cp29_command_fact_header_or_monotonicity_invalid".to_owned());
        }
        required(&self.command_id, "cp29_command_id_required")?;
        digest(&self.command_digest, "cp29_command_digest_invalid")?;
        digest(&self.digest, "cp29_command_fact_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp29_command_fact_digest_mismatch".to_owned());
        }
        if self.state_after == Cp29LifecycleState::ResultUnknown && !self.result_unknown {
            return Err("cp29_unknown_state_requires_unknown_fact".to_owned());
        }
        if self.result_unknown && self.state_after != Cp29LifecycleState::ResultUnknown {
            return Err("cp29_unknown_fact_state_mismatch".to_owned());
        }
        if self.state_before.terminal() && !self.replayed {
            return Err("cp29_terminal_cannot_revive".to_owned());
        }
        if self.replayed && self.effect_count != 0 {
            return Err("cp29_replay_effect_count_nonzero".to_owned());
        }
        if self.state_before != self.state_after
            && !transition_allowed(self.state_before, self.state_after)
        {
            return Err("cp29_illegal_state_transition".to_owned());
        }
        if self.state_before == self.state_after && self.effect_count > 0 {
            return Err("cp29_same_state_effect_without_transition".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "sequence": self.sequence,
            "command_id": self.command_id,
            "command_digest": self.command_digest,
            "state_before": self.state_before,
            "state_after": self.state_after,
            "authority_epoch": self.authority_epoch,
            "permission_rank_before": self.permission_rank_before,
            "permission_rank_after": self.permission_rank_after,
            "budget_limit": self.budget_limit,
            "budget_reserved": self.budget_reserved,
            "budget_settled": self.budget_settled,
            "effect_count": self.effect_count,
            "replayed": self.replayed,
            "result_unknown": self.result_unknown,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cp29CrashObservation {
    pub schema: String,
    pub point: Cp29CrashPoint,
    pub effect_started: bool,
    pub effect_confirmed: bool,
    pub final_state: Cp29LifecycleState,
    pub resource_fenced: bool,
    pub duplicate_effect: bool,
    pub source_ref: String,
    pub digest: String,
}

impl Cp29CrashObservation {
    pub fn new(
        point: Cp29CrashPoint,
        effect_started: bool,
        effect_confirmed: bool,
        final_state: Cp29LifecycleState,
        resource_fenced: bool,
        source_ref: impl Into<String>,
    ) -> Self {
        let mut observation = Self {
            schema: CONTROL_PLANE_CRASH_OBSERVATION_SCHEMA.to_owned(),
            point,
            effect_started,
            effect_confirmed,
            final_state,
            resource_fenced,
            duplicate_effect: false,
            source_ref: source_ref.into(),
            digest: String::new(),
        };
        observation.digest = observation.canonical_digest();
        observation
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_CRASH_OBSERVATION_SCHEMA || self.duplicate_effect {
            return Err("cp29_crash_observation_header_or_duplicate_invalid".to_owned());
        }
        required(&self.source_ref, "cp29_crash_source_ref_required")?;
        digest(&self.digest, "cp29_crash_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp29_crash_digest_mismatch".to_owned());
        }
        if self.effect_started && !self.effect_confirmed {
            if self.final_state != Cp29LifecycleState::ResultUnknown || !self.resource_fenced {
                return Err("cp29_crash_started_effect_requires_unknown_fence".to_owned());
            }
        }
        if self.effect_confirmed && self.final_state == Cp29LifecycleState::ResultUnknown {
            return Err("cp29_crash_confirmed_effect_cannot_remain_unknown".to_owned());
        }
        if !self.effect_started && self.final_state == Cp29LifecycleState::Completed {
            return Err("cp29_crash_not_started_cannot_complete".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "point": self.point,
            "effect_started": self.effect_started,
            "effect_confirmed": self.effect_confirmed,
            "final_state": self.final_state,
            "resource_fenced": self.resource_fenced,
            "duplicate_effect": self.duplicate_effect,
            "source_ref": self.source_ref,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cp29AuthorityScenario {
    pub schema: String,
    pub scenario_id: String,
    pub command_facts: Vec<Cp29CommandFact>,
    pub crash_observations: Vec<Cp29CrashObservation>,
    pub online_digest: String,
    pub replay_digest: String,
    pub digest: String,
}

impl Cp29AuthorityScenario {
    pub fn new(
        scenario_id: impl Into<String>,
        command_facts: Vec<Cp29CommandFact>,
        crash_observations: Vec<Cp29CrashObservation>,
        online_digest: impl Into<String>,
        replay_digest: impl Into<String>,
    ) -> Self {
        let mut scenario = Self {
            schema: CONTROL_PLANE_AUTHORITY_SCHEMA.to_owned(),
            scenario_id: scenario_id.into(),
            command_facts,
            crash_observations,
            online_digest: online_digest.into(),
            replay_digest: replay_digest.into(),
            digest: String::new(),
        };
        scenario.digest = scenario.canonical_digest();
        scenario
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_AUTHORITY_SCHEMA
            || self.command_facts.is_empty()
            || self.command_facts.len() > 4_096
            || self.crash_observations.is_empty()
            || self.crash_observations.len() > 64
        {
            return Err("cp29_scenario_header_invalid".to_owned());
        }
        required(&self.scenario_id, "cp29_scenario_id_required")?;
        digest(&self.online_digest, "cp29_online_digest_invalid")?;
        digest(&self.replay_digest, "cp29_replay_digest_invalid")?;
        if self.online_digest != self.replay_digest
            || self.online_digest != Self::fold_digest(&self.command_facts)
        {
            return Err("cp29_online_replay_digest_mismatch".to_owned());
        }
        digest(&self.digest, "cp29_scenario_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp29_scenario_digest_mismatch".to_owned());
        }

        let mut command_digests = BTreeMap::new();
        let mut last_epoch = 0;
        let mut previous_reserved = 0;
        let mut previous_settled = 0;
        let mut previous_state = None;
        for (index, fact) in self.command_facts.iter().enumerate() {
            fact.validate()?;
            if fact.sequence != (index as u64).saturating_add(1) {
                return Err("cp29_command_sequence_gap".to_owned());
            }
            if fact.authority_epoch < last_epoch
                || fact.budget_reserved < previous_reserved
                || fact.budget_settled < previous_settled
            {
                return Err("cp29_authority_or_budget_rollback".to_owned());
            }
            if let Some(previous) = previous_state {
                if previous.terminal() && !fact.replayed {
                    return Err("cp29_terminal_cannot_revive".to_owned());
                }
            }
            if let Some(original_digest) = command_digests.get(&fact.command_id) {
                if original_digest != &fact.command_digest || !fact.replayed {
                    return Err("cp29_command_idempotency_or_payload_drift".to_owned());
                }
            } else if fact.replayed {
                return Err("cp29_first_command_cannot_be_replay".to_owned());
            } else {
                command_digests.insert(fact.command_id.clone(), fact.command_digest.clone());
            }
            last_epoch = fact.authority_epoch;
            previous_reserved = fact.budget_reserved;
            previous_settled = fact.budget_settled;
            previous_state = Some(fact.state_after);
        }

        let mut points = BTreeSet::new();
        for observation in &self.crash_observations {
            observation.validate()?;
            if !points.insert(observation.point) {
                return Err("cp29_crash_point_duplicate".to_owned());
            }
        }
        let expected_points = [
            Cp29CrashPoint::WriteFrame,
            Cp29CrashPoint::Sync,
            Cp29CrashPoint::Approval,
            Cp29CrashPoint::Prepare,
            Cp29CrashPoint::Dispatch,
            Cp29CrashPoint::Result,
            Cp29CrashPoint::Replay,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if points != expected_points {
            return Err("cp29_crash_matrix_incomplete".to_owned());
        }
        Ok(())
    }

    pub fn fold_digest(command_facts: &[Cp29CommandFact]) -> String {
        json_digest(&json!({
            "last_sequence": command_facts.last().map(|fact| fact.sequence).unwrap_or(0),
            "final_state": command_facts.last().map(|fact| fact.state_after),
            "authority_epoch": command_facts.last().map(|fact| fact.authority_epoch).unwrap_or(0),
            "permission_rank": command_facts.last().map(|fact| fact.permission_rank_after).unwrap_or(0),
            "budget_limit": command_facts.last().map(|fact| fact.budget_limit).unwrap_or(0),
            "budget_reserved": command_facts.last().map(|fact| fact.budget_reserved).unwrap_or(0),
            "budget_settled": command_facts.last().map(|fact| fact.budget_settled).unwrap_or(0),
            "effect_count": command_facts.iter().map(|fact| u64::from(fact.effect_count)).sum::<u64>(),
            "unknown_fence": command_facts.iter().any(|fact| fact.result_unknown),
            "command_facts": command_facts.iter().map(|fact| &fact.digest).collect::<Vec<_>>(),
        }))
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "scenario_id": self.scenario_id,
            "command_facts": self.command_facts,
            "crash_observations": self.crash_observations,
            "online_digest": self.online_digest,
            "replay_digest": self.replay_digest,
        }))
    }
}
