//! Deployment operation lifecycle facts and a pure replay reducer.
//!
//! This module owns no filesystem, process, network or EventLog effect.  It validates a
//! server-owned deployment revision and folds versioned operation transitions into a bounded
//! journal.  The EventStore remains the durable fact source; this journal is the deterministic
//! operation projection that later deployment steps can persist and rebuild.

use crate::{json_digest, EventCursor, OperationId, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const OPERATION_LIFECYCLE_SCHEMA: &str = "kiana.operation-lifecycle.v1";
pub const OPERATION_LIFECYCLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_OPERATION_TRANSITIONS: usize = 256;
pub const MAX_OPERATION_TEXT_BYTES: usize = 512;
pub const MAX_OPERATION_EVIDENCE_REFS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Preflight,
    Draining,
    Executing,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Created,
    Preflight,
    Draining,
    Executing,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

impl OperationState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }

    const fn phase(self) -> OperationPhase {
        match self {
            Self::Created | Self::Preflight => OperationPhase::Preflight,
            Self::Draining => OperationPhase::Draining,
            Self::Executing => OperationPhase::Executing,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unknown => {
                OperationPhase::Terminal
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationTransitionKind {
    PreflightComplete,
    DrainStart,
    DrainTimeout,
    ExecuteStart,
    Complete,
    Fail,
    Cancel,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseDeadline {
    pub schema: String,
    pub version: SchemaVersion,
    pub phase: OperationPhase,
    pub deadline_unix_ms: u64,
    pub deadline_digest: String,
}

impl PhaseDeadline {
    pub fn new(phase: OperationPhase, deadline_unix_ms: u64) -> Result<Self, String> {
        let mut deadline = Self {
            schema: OPERATION_LIFECYCLE_SCHEMA.to_owned(),
            version: OPERATION_LIFECYCLE_VERSION,
            phase,
            deadline_unix_ms,
            deadline_digest: String::new(),
        };
        deadline.deadline_digest = deadline.digest();
        deadline.validate()?;
        Ok(deadline)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LIFECYCLE_SCHEMA
            || self.version != OPERATION_LIFECYCLE_VERSION
            || self.deadline_unix_ms == 0
            || self.phase == OperationPhase::Terminal
        {
            return Err("operation_deadline_header_invalid".to_owned());
        }
        valid_digest(&self.deadline_digest, "operation_deadline_digest")?;
        if self.deadline_digest != self.digest() {
            return Err("operation_deadline_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "phase": self.phase,
            "deadline_unix_ms": self.deadline_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationTransition {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub revision_id: String,
    pub revision_digest: String,
    pub sequence: u64,
    pub source_cursor: EventCursor,
    pub from_state: OperationState,
    pub state: OperationState,
    pub phase: OperationPhase,
    pub kind: OperationTransitionKind,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub evidence_digest: String,
    pub observed_at_unix_ms: u64,
    pub deadline: Option<PhaseDeadline>,
    pub transition_digest: String,
}

impl OperationTransition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        revision_id: impl Into<String>,
        revision_digest: impl Into<String>,
        sequence: u64,
        source_cursor: EventCursor,
        from_state: OperationState,
        kind: OperationTransitionKind,
        reason: impl Into<String>,
        evidence_refs: Vec<String>,
        observed_at_unix_ms: u64,
        deadline: Option<PhaseDeadline>,
    ) -> Result<Self, String> {
        let (state, phase) = expected_transition(kind, from_state)?;
        let mut transition = Self {
            schema: OPERATION_LIFECYCLE_SCHEMA.to_owned(),
            version: OPERATION_LIFECYCLE_VERSION,
            operation_id,
            revision_id: revision_id.into(),
            revision_digest: revision_digest.into(),
            sequence,
            source_cursor,
            from_state,
            state,
            phase,
            kind,
            reason: reason.into(),
            evidence_digest: evidence_digest(&evidence_refs),
            evidence_refs,
            observed_at_unix_ms,
            deadline,
            transition_digest: String::new(),
        };
        transition.transition_digest = transition.digest();
        transition.validate()?;
        Ok(transition)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LIFECYCLE_SCHEMA
            || self.version != OPERATION_LIFECYCLE_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.sequence == 0
            || self.source_cursor == 0
            || self.observed_at_unix_ms == 0
        {
            return Err("operation_transition_header_invalid".to_owned());
        }
        bounded(&self.revision_id, "operation_revision_id")?;
        valid_digest(&self.revision_digest, "operation_revision_digest")?;
        bounded(&self.reason, "operation_reason")?;
        validate_evidence(&self.evidence_refs, &self.evidence_digest)?;
        if let Some(deadline) = &self.deadline {
            deadline.validate()?;
            let deadline_phase_matches = deadline.phase == self.phase
                || (self.kind == OperationTransitionKind::DrainTimeout
                    && deadline.phase == OperationPhase::Draining);
            if !deadline_phase_matches {
                return Err("operation_deadline_phase_mismatch".to_owned());
            }
        }
        let (expected_state, expected_phase) = expected_transition(self.kind, self.from_state)?;
        if self.state != expected_state || self.phase != expected_phase {
            return Err("operation_transition_state_mismatch".to_owned());
        }
        if self.kind == OperationTransitionKind::DrainStart && self.deadline.is_none() {
            return Err("operation_drain_deadline_required".to_owned());
        }
        if self.kind == OperationTransitionKind::DrainTimeout && self.deadline.is_none() {
            return Err("operation_drain_timeout_deadline_required".to_owned());
        }
        valid_digest(&self.transition_digest, "operation_transition_digest")?;
        if self.transition_digest != self.digest() {
            return Err("operation_transition_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "revision_id": self.revision_id,
            "revision_digest": self.revision_digest,
            "sequence": self.sequence,
            "source_cursor": self.source_cursor,
            "from_state": self.from_state,
            "state": self.state,
            "phase": self.phase,
            "kind": self.kind,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
            "evidence_digest": self.evidence_digest,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "deadline": self.deadline,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationJournal {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub revision_id: String,
    pub revision_digest: String,
    pub current_state: OperationState,
    pub current_phase: OperationPhase,
    pub preflight_completed: bool,
    pub sequence: u64,
    pub source_cursor: EventCursor,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub evidence_digest: String,
    pub active_deadline: Option<PhaseDeadline>,
    pub transitions: Vec<OperationTransition>,
    pub journal_digest: String,
}

impl OperationJournal {
    pub fn new(
        operation_id: OperationId,
        revision_id: impl Into<String>,
        revision_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let evidence_refs = vec!["operation.created".to_owned()];
        let mut journal = Self {
            schema: OPERATION_LIFECYCLE_SCHEMA.to_owned(),
            version: OPERATION_LIFECYCLE_VERSION,
            operation_id,
            revision_id: revision_id.into(),
            revision_digest: revision_digest.into(),
            current_state: OperationState::Created,
            current_phase: OperationPhase::Preflight,
            preflight_completed: false,
            sequence: 0,
            source_cursor: 0,
            reason: "created".to_owned(),
            evidence_digest: evidence_digest(&evidence_refs),
            evidence_refs,
            active_deadline: None,
            transitions: Vec::new(),
            journal_digest: String::new(),
        };
        journal.journal_digest = journal.digest();
        journal.validate()?;
        Ok(journal)
    }

    pub fn replay(
        operation_id: OperationId,
        revision_id: impl Into<String>,
        revision_digest: impl Into<String>,
        transitions: impl IntoIterator<Item = OperationTransition>,
    ) -> Result<Self, String> {
        let mut journal = Self::new(operation_id, revision_id, revision_digest)?;
        for transition in transitions {
            journal.append(transition)?;
        }
        Ok(journal)
    }

    pub fn append(&mut self, transition: OperationTransition) -> Result<(), String> {
        self.validate()?;
        if self.current_state.is_terminal() {
            return Err("operation_terminal_immutable".to_owned());
        }
        transition.validate()?;
        if transition.operation_id != self.operation_id {
            return Err("operation_id_mismatch".to_owned());
        }
        if transition.revision_id != self.revision_id
            || transition.revision_digest != self.revision_digest
        {
            return Err("operation_revision_late_event".to_owned());
        }
        if transition.sequence != self.sequence.saturating_add(1) {
            return Err("operation_sequence_not_contiguous".to_owned());
        }
        if transition.source_cursor <= self.source_cursor {
            return Err("operation_source_cursor_regressed".to_owned());
        }
        let (state, preflight_completed, active_deadline) = validate_against(
            self.current_state,
            self.preflight_completed,
            self.active_deadline.as_ref(),
            &transition,
        )?;
        self.current_state = state;
        self.current_phase = state.phase();
        self.preflight_completed = preflight_completed;
        self.sequence = transition.sequence;
        self.source_cursor = transition.source_cursor;
        self.reason = transition.reason.clone();
        self.evidence_refs = transition.evidence_refs.clone();
        self.evidence_digest = transition.evidence_digest.clone();
        self.active_deadline = active_deadline;
        self.transitions.push(transition);
        self.journal_digest = self.digest();
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPERATION_LIFECYCLE_SCHEMA
            || self.version != OPERATION_LIFECYCLE_VERSION
            || self.operation_id.as_uuid().is_nil()
        {
            return Err("operation_journal_header_invalid".to_owned());
        }
        bounded(&self.revision_id, "operation_revision_id")?;
        valid_digest(&self.revision_digest, "operation_revision_digest")?;
        bounded(&self.reason, "operation_reason")?;
        validate_evidence(&self.evidence_refs, &self.evidence_digest)?;
        if self.transitions.len() > MAX_OPERATION_TRANSITIONS
            || self.sequence != self.transitions.len() as u64
        {
            return Err("operation_transition_limit_or_sequence_invalid".to_owned());
        }
        if let Some(deadline) = &self.active_deadline {
            deadline.validate()?;
            if deadline.phase != self.current_phase || self.current_state.is_terminal() {
                return Err("operation_active_deadline_invalid".to_owned());
            }
        }
        let mut state = OperationState::Created;
        let mut preflight_completed = false;
        let mut active_deadline = None;
        let mut cursor = 0;
        let mut expected_sequence = 1;
        for transition in &self.transitions {
            if transition.operation_id != self.operation_id
                || transition.revision_id != self.revision_id
                || transition.revision_digest != self.revision_digest
                || transition.sequence != expected_sequence
                || transition.source_cursor <= cursor
            {
                return Err("operation_transition_chain_invalid".to_owned());
            }
            let (next_state, next_preflight, next_deadline) = validate_against(
                state,
                preflight_completed,
                active_deadline.as_ref(),
                transition,
            )?;
            state = next_state;
            preflight_completed = next_preflight;
            active_deadline = next_deadline;
            cursor = transition.source_cursor;
            expected_sequence = expected_sequence.saturating_add(1);
        }
        if state != self.current_state
            || state.phase() != self.current_phase
            || preflight_completed != self.preflight_completed
            || cursor != self.source_cursor
            || active_deadline != self.active_deadline
        {
            return Err("operation_journal_projection_mismatch".to_owned());
        }
        valid_digest(&self.journal_digest, "operation_journal_digest")?;
        if self.journal_digest != self.digest() {
            return Err("operation_journal_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "revision_id": self.revision_id,
            "revision_digest": self.revision_digest,
            "current_state": self.current_state,
            "current_phase": self.current_phase,
            "preflight_completed": self.preflight_completed,
            "sequence": self.sequence,
            "source_cursor": self.source_cursor,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
            "evidence_digest": self.evidence_digest,
            "active_deadline": self.active_deadline,
            "transitions": self.transitions,
        }))
    }
}

fn expected_transition(
    kind: OperationTransitionKind,
    from: OperationState,
) -> Result<(OperationState, OperationPhase), String> {
    let expected = match kind {
        OperationTransitionKind::PreflightComplete if from == OperationState::Created => {
            (OperationState::Preflight, OperationPhase::Preflight)
        }
        OperationTransitionKind::DrainStart if from == OperationState::Preflight => {
            (OperationState::Draining, OperationPhase::Draining)
        }
        OperationTransitionKind::DrainTimeout if from == OperationState::Draining => {
            (OperationState::Unknown, OperationPhase::Terminal)
        }
        OperationTransitionKind::ExecuteStart
            if matches!(from, OperationState::Preflight | OperationState::Draining) =>
        {
            (OperationState::Executing, OperationPhase::Executing)
        }
        OperationTransitionKind::Complete if from == OperationState::Executing => {
            (OperationState::Completed, OperationPhase::Terminal)
        }
        OperationTransitionKind::Fail
            if matches!(
                from,
                OperationState::Preflight | OperationState::Draining | OperationState::Executing
            ) =>
        {
            (OperationState::Failed, OperationPhase::Terminal)
        }
        OperationTransitionKind::Cancel
            if matches!(
                from,
                OperationState::Preflight | OperationState::Draining | OperationState::Executing
            ) =>
        {
            (OperationState::Cancelled, OperationPhase::Terminal)
        }
        OperationTransitionKind::ResultUnknown
            if matches!(from, OperationState::Draining | OperationState::Executing) =>
        {
            (OperationState::Unknown, OperationPhase::Terminal)
        }
        _ => return Err("operation_transition_not_allowed".to_owned()),
    };
    Ok(expected)
}

fn validate_against(
    current_state: OperationState,
    preflight_completed: bool,
    active_deadline: Option<&PhaseDeadline>,
    transition: &OperationTransition,
) -> Result<(OperationState, bool, Option<PhaseDeadline>), String> {
    if transition.from_state != current_state {
        return Err("operation_from_state_mismatch".to_owned());
    }
    let mut next_deadline = active_deadline.cloned();
    let mut next_preflight = preflight_completed;
    match transition.kind {
        OperationTransitionKind::PreflightComplete => {
            if preflight_completed {
                return Err("operation_preflight_repeated".to_owned());
            }
            if transition.deadline.is_some() {
                return Err("operation_preflight_deadline_unexpected".to_owned());
            }
            next_preflight = true;
            next_deadline = None;
        }
        OperationTransitionKind::DrainStart => {
            if !preflight_completed {
                return Err("operation_preflight_required".to_owned());
            }
            let deadline = transition
                .deadline
                .as_ref()
                .ok_or_else(|| "operation_drain_deadline_required".to_owned())?;
            if deadline.phase != OperationPhase::Draining
                || deadline.deadline_unix_ms <= transition.observed_at_unix_ms
            {
                return Err("operation_drain_deadline_invalid".to_owned());
            }
            next_deadline = Some(deadline.clone());
        }
        OperationTransitionKind::DrainTimeout => {
            let active =
                active_deadline.ok_or_else(|| "operation_drain_deadline_missing".to_owned())?;
            let deadline = transition
                .deadline
                .as_ref()
                .ok_or_else(|| "operation_drain_timeout_deadline_required".to_owned())?;
            if deadline != active || transition.observed_at_unix_ms < deadline.deadline_unix_ms {
                return Err("operation_drain_timeout_not_due".to_owned());
            }
            next_deadline = None;
        }
        OperationTransitionKind::ExecuteStart => {
            if !preflight_completed {
                return Err("operation_preflight_required".to_owned());
            }
            if let Some(deadline) = &transition.deadline {
                if deadline.phase != OperationPhase::Executing
                    || deadline.deadline_unix_ms <= transition.observed_at_unix_ms
                {
                    return Err("operation_execute_deadline_invalid".to_owned());
                }
                next_deadline = Some(deadline.clone());
            } else {
                next_deadline = None;
            }
        }
        OperationTransitionKind::Complete
        | OperationTransitionKind::Fail
        | OperationTransitionKind::Cancel
        | OperationTransitionKind::ResultUnknown => {
            if transition.deadline.is_some() {
                return Err("operation_terminal_deadline_unexpected".to_owned());
            }
            next_deadline = None;
        }
    }
    Ok((transition.state, next_preflight, next_deadline))
}

fn validate_evidence(refs: &[String], digest: &str) -> Result<(), String> {
    if refs.len() > MAX_OPERATION_EVIDENCE_REFS {
        return Err("operation_evidence_limit".to_owned());
    }
    let mut seen = BTreeSet::new();
    for value in refs {
        bounded(value, "operation_evidence_ref")?;
        if !seen.insert(value) {
            return Err("operation_evidence_duplicate".to_owned());
        }
    }
    valid_digest(digest, "operation_evidence_digest")?;
    if digest != evidence_digest(refs) {
        return Err("operation_evidence_digest_mismatch".to_owned());
    }
    Ok(())
}

fn evidence_digest(refs: &[String]) -> String {
    json_digest(&serde_json::json!(refs))
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_OPERATION_TEXT_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
