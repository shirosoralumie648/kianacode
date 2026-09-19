//! Bounded progress and stall detection for Harness turns.
//!
//! Progress is evidence-based: an observation digest, workspace/artifact revision, verification
//! reference or advancing JobHandle cursor can prove movement.  Model prose alone cannot.  The
//! reducer is pure and bounded so repeated failures, A→B→A cycles, empty turns and stop-hook
//! feedback cannot create an unbounded repair loop.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PROGRESS_EVIDENCE_SCHEMA: &str = "kiana.progress-evidence.v1";
pub const PROGRESS_TRACKER_SCHEMA: &str = "kiana.progress-tracker.v1";
pub const PROGRESS_DECISION_SCHEMA: &str = "kiana.progress-decision.v1";
pub const PROGRESS_VERSION: u32 = 1;
pub const DEFAULT_PROGRESS_WINDOW: usize = 8;
pub const DEFAULT_NO_PROGRESS_LIMIT: u32 = 3;
pub const MAX_PROGRESS_WINDOW: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressAction {
    Continue,
    FeedbackOnce,
    RequestClarification,
    Blocked,
}

impl ProgressAction {
    pub const fn is_blocked(self) -> bool {
        matches!(self, Self::Blocked)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressEvidence {
    pub schema: String,
    pub version: u32,
    pub observation_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_cursor: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat: Option<u64>,
    pub evidence_digest: String,
}

impl ProgressEvidence {
    pub fn new(
        observation: &Value,
        workspace_revision: Option<String>,
        artifact_revision: Option<String>,
        verification_refs: &[String],
        job_cursor: Option<u64>,
        heartbeat: Option<u64>,
    ) -> Result<Self, String> {
        let mut refs = verification_refs.to_vec();
        refs.sort();
        refs.dedup();
        if refs.iter().any(|reference| !bounded(reference, 512)) {
            return Err("progress_verification_refs_invalid".to_owned());
        }
        let mut evidence = Self {
            schema: PROGRESS_EVIDENCE_SCHEMA.to_owned(),
            version: PROGRESS_VERSION,
            observation_digest: json_digest(observation),
            workspace_revision,
            artifact_revision,
            verification_digest: (!refs.is_empty()).then(|| json_digest(&json!(refs))),
            job_cursor,
            heartbeat,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROGRESS_EVIDENCE_SCHEMA
            || self.version != PROGRESS_VERSION
            || !digest(&self.observation_digest)
            || !self
                .workspace_revision
                .as_deref()
                .is_none_or(|value| bounded(value, 512))
            || !self
                .artifact_revision
                .as_deref()
                .is_none_or(|value| bounded(value, 512))
            || !self.verification_digest.as_deref().is_none_or(digest)
            || !digest(&self.evidence_digest)
            || self.evidence_digest != self.digest()
        {
            return Err("progress_evidence_invalid".to_owned());
        }
        Ok(())
    }

    pub fn progressed_from(&self, previous: &Self) -> Result<bool, String> {
        self.validate()?;
        previous.validate()?;
        Ok(self.observation_digest != previous.observation_digest
            || self.workspace_revision != previous.workspace_revision
            || self.artifact_revision != previous.artifact_revision
            || self.verification_digest != previous.verification_digest
            || self.job_cursor > previous.job_cursor
            || self.heartbeat > previous.heartbeat)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "observation_digest": self.observation_digest,
            "workspace_revision": self.workspace_revision,
            "artifact_revision": self.artifact_revision,
            "verification_digest": self.verification_digest,
            "job_cursor": self.job_cursor,
            "heartbeat": self.heartbeat,
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProgressInput {
    pub evidence: ProgressEvidence,
    #[allow(dead_code)]
    pub failure_digest: Option<String>,
    pub empty_turn: bool,
    pub stop_hook_feedback: Option<String>,
    pub stop_hook_budget_remaining: u32,
}

impl ProgressInput {
    pub fn new(evidence: ProgressEvidence) -> Self {
        Self {
            evidence,
            failure_digest: None,
            empty_turn: false,
            stop_hook_feedback: None,
            stop_hook_budget_remaining: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressDecision {
    pub schema: String,
    pub version: u32,
    pub action: ProgressAction,
    pub progressed: bool,
    pub cycle_detected: bool,
    pub consecutive_stall: u32,
    pub repeated_failure_count: u32,
    pub reason: String,
    pub stop_hook_budget_remaining: u32,
    pub evidence_digest: String,
    pub decision_digest: String,
}

impl ProgressDecision {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROGRESS_DECISION_SCHEMA
            || self.version != PROGRESS_VERSION
            || !bounded(&self.reason, 512)
            || !digest(&self.evidence_digest)
            || !digest(&self.decision_digest)
            || self.decision_digest != self.digest()
        {
            return Err("progress_decision_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "action": self.action,
            "progressed": self.progressed,
            "cycle_detected": self.cycle_detected,
            "consecutive_stall": self.consecutive_stall,
            "repeated_failure_count": self.repeated_failure_count,
            "reason": self.reason,
            "stop_hook_budget_remaining": self.stop_hook_budget_remaining,
            "evidence_digest": self.evidence_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressTracker {
    pub schema: String,
    pub version: u32,
    pub window_limit: usize,
    pub no_progress_limit: u32,
    pub consecutive_stall: u32,
    pub history: Vec<ProgressEvidence>,
    pub failure_history: Vec<String>,
}

impl Default for ProgressTracker {
    fn default() -> Self {
        Self::new(DEFAULT_PROGRESS_WINDOW, DEFAULT_NO_PROGRESS_LIMIT)
            .expect("built-in progress tracker is valid")
    }
}

impl ProgressTracker {
    pub fn new(window_limit: usize, no_progress_limit: u32) -> Result<Self, String> {
        let tracker = Self {
            schema: PROGRESS_TRACKER_SCHEMA.to_owned(),
            version: PROGRESS_VERSION,
            window_limit,
            no_progress_limit,
            consecutive_stall: 0,
            history: Vec::new(),
            failure_history: Vec::new(),
        };
        tracker.validate()?;
        Ok(tracker)
    }

    pub fn observe(&mut self, input: ProgressInput) -> Result<ProgressDecision, String> {
        self.validate()?;
        input.evidence.validate()?;
        if let Some(failure) = &input.failure_digest {
            if !digest(failure) {
                return Err("progress_failure_digest_invalid".to_owned());
            }
        }
        let previous = self.history.last();
        let progressed = previous
            .map(|previous| input.evidence.progressed_from(previous))
            .transpose()?
            .unwrap_or(true);
        let cycle_detected = self.history.len() >= 2
            && self.history[self.history.len() - 2].evidence_digest
                == input.evidence.evidence_digest
            && self
                .history
                .last()
                .is_some_and(|last| last.evidence_digest != input.evidence.evidence_digest);
        let effective_progress = progressed && !cycle_detected;
        self.consecutive_stall = if effective_progress {
            0
        } else {
            self.consecutive_stall.saturating_add(1)
        };
        if let Some(failure) = &input.failure_digest {
            self.failure_history.push(failure.clone());
            if self.failure_history.len() > self.window_limit {
                self.failure_history.remove(0);
            }
        }
        let repeated_failure_count = input
            .failure_digest
            .as_ref()
            .map(|failure| {
                self.failure_history
                    .iter()
                    .rev()
                    .take(self.window_limit)
                    .take_while(|candidate| *candidate == failure)
                    .count() as u32
            })
            .unwrap_or(0);
        let mut action = ProgressAction::Continue;
        let mut reason = if effective_progress {
            "evidence_progress".to_owned()
        } else if input.empty_turn {
            "empty_turn_no_progress".to_owned()
        } else if cycle_detected {
            "progress_cycle_detected".to_owned()
        } else {
            "no_progress".to_owned()
        };
        let mut stop_hook_budget_remaining = input.stop_hook_budget_remaining;
        if repeated_failure_count >= self.no_progress_limit
            || self.consecutive_stall >= self.no_progress_limit
        {
            action = ProgressAction::Blocked;
            reason = if repeated_failure_count >= self.no_progress_limit {
                "repeated_failure_limit".to_owned()
            } else {
                "no_progress_limit".to_owned()
            };
        } else if input.stop_hook_feedback.is_some() {
            if stop_hook_budget_remaining == 0 {
                action = ProgressAction::Blocked;
                reason = "stop_hook_budget_exhausted".to_owned();
            } else {
                stop_hook_budget_remaining -= 1;
                action = ProgressAction::FeedbackOnce;
                reason = "bounded_stop_hook_feedback".to_owned();
            }
        } else if !effective_progress && self.consecutive_stall >= 2 {
            action = ProgressAction::RequestClarification;
            reason = "stall_requires_clarification".to_owned();
        } else if !effective_progress && self.consecutive_stall == 1 {
            action = ProgressAction::FeedbackOnce;
        }
        self.history.push(input.evidence.clone());
        if self.history.len() > self.window_limit {
            self.history.remove(0);
        }
        let mut decision = ProgressDecision {
            schema: PROGRESS_DECISION_SCHEMA.to_owned(),
            version: PROGRESS_VERSION,
            action,
            progressed: effective_progress,
            cycle_detected,
            consecutive_stall: self.consecutive_stall,
            repeated_failure_count,
            reason,
            stop_hook_budget_remaining,
            evidence_digest: input.evidence.evidence_digest,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROGRESS_TRACKER_SCHEMA
            || self.version != PROGRESS_VERSION
            || self.window_limit == 0
            || self.window_limit > MAX_PROGRESS_WINDOW
            || self.no_progress_limit == 0
            || self.no_progress_limit as usize > self.window_limit
            || self.history.len() > self.window_limit
            || self.failure_history.len() > self.window_limit
        {
            return Err("progress_tracker_invalid".to_owned());
        }
        for evidence in &self.history {
            evidence.validate()?;
        }
        if self.failure_history.iter().any(|failure| !digest(failure)) {
            return Err("progress_tracker_failure_history_invalid".to_owned());
        }
        Ok(())
    }
}

pub fn failure_digest(reason: &str) -> Result<String, String> {
    if !bounded(reason, 4_096) {
        return Err("progress_failure_reason_invalid".to_owned());
    }
    Ok(json_digest(&json!({"reason": reason})))
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
