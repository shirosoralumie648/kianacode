//! SW-18 deny-first swarm release evidence gate.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SWARM_RELEASE_GATE_SCHEMA: &str = "kiana.swarm-release-gate.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmReleaseScenario {
    Deny,
    Unknown,
    Replay,
    Crash,
    Race,
    FakeGolden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmReleaseStatus {
    Verified,
    Blocked,
    Partial,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmReleaseCase {
    pub scenario: SwarmReleaseScenario,
    pub status: SwarmReleaseStatus,
    pub handler_calls: u32,
    pub effect_count: u32,
    pub unknown_preserved: bool,
    pub replay_fenced: bool,
    pub same_spine: bool,
    pub secret_free: bool,
    pub evidence_digest: Option<String>,
    pub limitation: String,
    pub case_digest: String,
}

impl SwarmReleaseCase {
    pub fn validate(&self) -> Result<(), String> {
        if !self.same_spine
            || !self.secret_free
            || !valid_text(&self.limitation)
            || !valid_digest(&self.case_digest)
            || self.case_digest != self.digest()
            || self.status == SwarmReleaseStatus::Verified
                && self
                    .evidence_digest
                    .as_deref()
                    .is_none_or(|value| !valid_digest(value))
        {
            return Err("swarm_release_case_invalid".to_owned());
        }
        match self.scenario {
            SwarmReleaseScenario::Deny | SwarmReleaseScenario::Race => {
                if self.status == SwarmReleaseStatus::Verified
                    && (self.handler_calls != 0 || self.effect_count != 0)
                {
                    return Err("swarm_release_deny_effect_invalid".to_owned());
                }
            }
            SwarmReleaseScenario::Unknown => {
                if self.status == SwarmReleaseStatus::Verified
                    && (!self.unknown_preserved || self.effect_count > 1)
                {
                    return Err("swarm_release_unknown_invalid".to_owned());
                }
            }
            SwarmReleaseScenario::Replay => {
                if self.status == SwarmReleaseStatus::Verified
                    && (!self.replay_fenced || self.effect_count > 1)
                {
                    return Err("swarm_release_replay_invalid".to_owned());
                }
            }
            SwarmReleaseScenario::Crash | SwarmReleaseScenario::FakeGolden => {}
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "scenario": self.scenario,
            "status": self.status,
            "handler_calls": self.handler_calls,
            "effect_count": self.effect_count,
            "unknown_preserved": self.unknown_preserved,
            "replay_fenced": self.replay_fenced,
            "same_spine": self.same_spine,
            "secret_free": self.secret_free,
            "evidence_digest": self.evidence_digest,
            "limitation": self.limitation,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmReleaseGate {
    pub schema: String,
    pub cases: Vec<SwarmReleaseCase>,
    pub status: SwarmReleaseStatus,
    pub gate_digest: String,
}

impl SwarmReleaseGate {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_RELEASE_GATE_SCHEMA
            || self.cases.len() != 6
            || !valid_digest(&self.gate_digest)
            || self.gate_digest != self.digest()
        {
            return Err("swarm_release_gate_invalid".to_owned());
        }
        let mut scenarios = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !scenarios.insert(case.scenario) {
                return Err("swarm_release_scenario_duplicate".to_owned());
            }
        }
        let expected = if self
            .cases
            .iter()
            .any(|case| case.status == SwarmReleaseStatus::Blocked)
        {
            SwarmReleaseStatus::Blocked
        } else if self
            .cases
            .iter()
            .any(|case| case.status == SwarmReleaseStatus::Partial)
        {
            SwarmReleaseStatus::Partial
        } else {
            SwarmReleaseStatus::Verified
        };
        if self.status != expected {
            return Err("swarm_release_status_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "cases": self.cases,
            "status": self.status,
        }))
    }
}

pub fn validate_swarm_release_gate(gate: &SwarmReleaseGate) -> Result<(), String> {
    gate.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\r', '\n'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
