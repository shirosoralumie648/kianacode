//! Typed parent-Harness to child-Harness delegation contracts.
//!
//! These values describe a narrowed SpawnPlan/Cell handoff and a bounded child result.  They do
//! not start a process, send free-form messages or grant capabilities; Core still admits the
//! existing SpawnPlan through CellRegistry and the child uses the same KianaHarness route.

use crate::{json_digest, CellId, RunId, SchemaVersion, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CHILD_HARNESS_INTENT_SCHEMA: &str = "kiana.child-harness-intent.v1";
pub const CHILD_HARNESS_OUTCOME_SCHEMA: &str = "kiana.child-harness-outcome.v1";
pub const CHILD_HARNESS_CANCEL_SCHEMA: &str = "kiana.child-harness-cancel.v1";
pub const CHILD_HARNESS_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_CHILD_INPUT_REFS: usize = 64;
pub const MAX_CHILD_RESULT_REFS: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildHarnessBudget {
    pub max_tokens: u64,
    pub max_tool_calls: u32,
    pub max_effects: u32,
    pub max_wall_clock_ms: u64,
    pub max_concurrency: u32,
}

impl ChildHarnessBudget {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_tokens == 0
            || self.max_tool_calls == 0
            || self.max_effects == 0
            || self.max_wall_clock_ms == 0
            || self.max_concurrency == 0
        {
            return Err("child_harness_budget_invalid".to_owned());
        }
        Ok(())
    }

    pub fn contained_by(&self, parent: &Self) -> bool {
        self.max_tokens <= parent.max_tokens
            && self.max_tool_calls <= parent.max_tool_calls
            && self.max_effects <= parent.max_effects
            && self.max_wall_clock_ms <= parent.max_wall_clock_ms
            && self.max_concurrency <= parent.max_concurrency
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildHarnessBounds {
    pub parent_depth: u32,
    pub max_depth: u32,
    pub budget: ChildHarnessBudget,
    pub allowed_tools: BTreeSet<String>,
    pub allowed_paths: BTreeSet<String>,
    pub sandbox: String,
}

impl ChildHarnessBounds {
    pub fn validate(&self) -> Result<(), String> {
        if self.parent_depth >= self.max_depth
            || self.allowed_tools.len() > 32
            || self.allowed_paths.len() > 256
            || !bounded(&self.sandbox, 64)
            || self
                .allowed_tools
                .iter()
                .chain(self.allowed_paths.iter())
                .any(|value| !bounded(value, 512))
        {
            return Err("child_harness_bounds_invalid".to_owned());
        }
        self.budget.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildHarnessIntent {
    pub schema: String,
    pub version: SchemaVersion,
    pub parent_run_id: RunId,
    pub parent_turn_id: TurnId,
    pub parent_cell_id: CellId,
    pub child_cell_id: CellId,
    pub child_packet_id: String,
    pub depth: u32,
    pub budget: ChildHarnessBudget,
    pub tools: BTreeSet<String>,
    pub paths: BTreeSet<String>,
    pub sandbox: String,
    pub input_refs: Vec<String>,
    pub output_contract: String,
    pub intent_digest: String,
}

impl ChildHarnessIntent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        parent_run_id: RunId,
        parent_turn_id: TurnId,
        parent_cell_id: CellId,
        child_cell_id: CellId,
        child_packet_id: impl Into<String>,
        depth: u32,
        budget: ChildHarnessBudget,
        tools: BTreeSet<String>,
        paths: BTreeSet<String>,
        sandbox: impl Into<String>,
        input_refs: Vec<String>,
        output_contract: impl Into<String>,
    ) -> Result<Self, String> {
        let mut intent = Self {
            schema: CHILD_HARNESS_INTENT_SCHEMA.to_owned(),
            version: CHILD_HARNESS_VERSION,
            parent_run_id,
            parent_turn_id,
            parent_cell_id,
            child_cell_id,
            child_packet_id: child_packet_id.into(),
            depth,
            budget,
            tools,
            paths,
            sandbox: sandbox.into(),
            input_refs,
            output_contract: output_contract.into(),
            intent_digest: String::new(),
        };
        intent.intent_digest = intent.digest();
        intent.validate()?;
        Ok(intent)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CHILD_HARNESS_INTENT_SCHEMA
            || self.version != CHILD_HARNESS_VERSION
            || self.parent_run_id.as_uuid().is_nil()
            || self.parent_turn_id.as_uuid().is_nil()
            || self.parent_cell_id.as_uuid().is_nil()
            || self.child_cell_id.as_uuid().is_nil()
            || self.parent_cell_id == self.child_cell_id
            || self.depth == 0
            || !bounded(&self.child_packet_id, 256)
            || self.tools.len() > 32
            || self.paths.len() > 256
            || self.input_refs.len() > MAX_CHILD_INPUT_REFS
            || !bounded(&self.sandbox, 64)
            || !bounded(&self.output_contract, 256)
            || !digest(&self.intent_digest)
            || self.intent_digest != self.digest()
        {
            return Err("child_harness_intent_invalid".to_owned());
        }
        self.budget.validate()?;
        if self
            .tools
            .iter()
            .chain(self.paths.iter())
            .any(|value| !bounded(value, 512))
            || self.input_refs.iter().any(|value| !bounded(value, 512))
        {
            return Err("child_harness_intent_scope_invalid".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, parent: &ChildHarnessBounds) -> Result<(), String> {
        self.validate()?;
        parent.validate()?;
        if self.depth != parent.parent_depth.saturating_add(1) || self.depth > parent.max_depth {
            return Err("child_harness_depth_exceeded".to_owned());
        }
        if !self.budget.contained_by(&parent.budget)
            || !self.tools.is_subset(&parent.allowed_tools)
            || !self.paths.is_subset(&parent.allowed_paths)
            || !sandbox_contained(&parent.sandbox, &self.sandbox)
        {
            return Err("child_harness_scope_widening".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "parent_run_id": self.parent_run_id,
            "parent_turn_id": self.parent_turn_id,
            "parent_cell_id": self.parent_cell_id,
            "child_cell_id": self.child_cell_id,
            "child_packet_id": self.child_packet_id,
            "depth": self.depth,
            "budget": self.budget,
            "tools": self.tools,
            "paths": self.paths,
            "sandbox": self.sandbox,
            "input_refs": self.input_refs,
            "output_contract": self.output_contract,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildHarnessOutcomeKind {
    Completed,
    Failed,
    Cancelled,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildHarnessOutcome {
    pub schema: String,
    pub version: SchemaVersion,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub child_cell_id: CellId,
    pub kind: ChildHarnessOutcomeKind,
    pub summary: String,
    pub artifact_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub transcript_forwarded: bool,
    pub outcome_digest: String,
}

impl ChildHarnessOutcome {
    pub fn new(
        parent_run_id: RunId,
        child_run_id: RunId,
        child_cell_id: CellId,
        kind: ChildHarnessOutcomeKind,
        summary: impl Into<String>,
        artifact_refs: Vec<String>,
        evidence_refs: Vec<String>,
    ) -> Result<Self, String> {
        let mut outcome = Self {
            schema: CHILD_HARNESS_OUTCOME_SCHEMA.to_owned(),
            version: CHILD_HARNESS_VERSION,
            parent_run_id,
            child_run_id,
            child_cell_id,
            kind,
            summary: summary.into(),
            artifact_refs,
            evidence_refs,
            transcript_forwarded: false,
            outcome_digest: String::new(),
        };
        outcome.outcome_digest = outcome.digest();
        outcome.validate()?;
        Ok(outcome)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CHILD_HARNESS_OUTCOME_SCHEMA
            || self.version != CHILD_HARNESS_VERSION
            || self.parent_run_id.as_uuid().is_nil()
            || self.child_run_id.as_uuid().is_nil()
            || self.child_cell_id.as_uuid().is_nil()
            || !bounded(&self.summary, 8 * 1024)
            || self.artifact_refs.len() > MAX_CHILD_RESULT_REFS
            || self.evidence_refs.len() > MAX_CHILD_RESULT_REFS
            || self.transcript_forwarded
            || !digest(&self.outcome_digest)
            || self.outcome_digest != self.digest()
        {
            return Err("child_harness_outcome_invalid".to_owned());
        }
        if self
            .artifact_refs
            .iter()
            .chain(self.evidence_refs.iter())
            .any(|reference| !bounded(reference, 512))
        {
            return Err("child_harness_result_refs_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "parent_run_id": self.parent_run_id,
            "child_run_id": self.child_run_id,
            "child_cell_id": self.child_cell_id,
            "kind": self.kind,
            "summary": self.summary,
            "artifact_refs": self.artifact_refs,
            "evidence_refs": self.evidence_refs,
            "transcript_forwarded": self.transcript_forwarded,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildHarnessCancellation {
    pub schema: String,
    pub version: SchemaVersion,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub reason: String,
    pub stop_confirmed: bool,
    pub effect_known: bool,
    pub result_unknown: bool,
    pub cancellation_digest: String,
}

impl ChildHarnessCancellation {
    pub fn new(
        parent_run_id: RunId,
        child_run_id: RunId,
        reason: impl Into<String>,
        stop_confirmed: bool,
        effect_known: bool,
    ) -> Result<Self, String> {
        let mut cancellation = Self {
            schema: CHILD_HARNESS_CANCEL_SCHEMA.to_owned(),
            version: CHILD_HARNESS_VERSION,
            parent_run_id,
            child_run_id,
            reason: reason.into(),
            stop_confirmed,
            effect_known,
            result_unknown: !stop_confirmed || !effect_known,
            cancellation_digest: String::new(),
        };
        cancellation.cancellation_digest = cancellation.digest();
        cancellation.validate()?;
        Ok(cancellation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CHILD_HARNESS_CANCEL_SCHEMA
            || self.version != CHILD_HARNESS_VERSION
            || self.parent_run_id.as_uuid().is_nil()
            || self.child_run_id.as_uuid().is_nil()
            || !bounded(&self.reason, 1_024)
            || self.result_unknown != (!self.stop_confirmed || !self.effect_known)
            || !digest(&self.cancellation_digest)
            || self.cancellation_digest != self.digest()
        {
            return Err("child_harness_cancellation_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "parent_run_id": self.parent_run_id,
            "child_run_id": self.child_run_id,
            "reason": self.reason,
            "stop_confirmed": self.stop_confirmed,
            "effect_known": self.effect_known,
            "result_unknown": self.result_unknown,
        }))
    }
}

fn sandbox_contained(parent: &str, child: &str) -> bool {
    parent == child || (parent == "workspace-write" && child == "read-only")
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
