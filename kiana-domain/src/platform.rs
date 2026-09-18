//! Human operations are projections and candidate records, never replacement runtime authority.
use crate::{EventId, RunId, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanInboxKind {
    Approval,
    Review,
    Acceptance,
    Incident,
    Reconciliation,
    Feedback,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HumanAction {
    pub id: String,
    pub label: String,
    pub command: String,
    pub arguments: Value,
    pub required_fields: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HumanInboxItem {
    pub item_id: String,
    pub kind: HumanInboxKind,
    pub title: String,
    pub source_ref: String,
    pub run_id: Option<RunId>,
    pub detail: Value,
    pub actions: Vec<HumanAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    Crash,
    Timeout,
    Cancel,
    DiskFull,
    McpFailure,
    ProviderUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPlanState {
    Proposed,
    Approved,
    Executing,
    Verified,
    Failed,
    Abandoned,
}

impl Default for RecoveryPlanState {
    fn default() -> Self {
        Self::Proposed
    }
}

impl RecoveryPlanState {
    pub fn transition(self, next: Self) -> Result<Self, &'static str> {
        let allowed = matches!(
            (self, next),
            (Self::Proposed, Self::Approved | Self::Abandoned)
                | (Self::Approved, Self::Executing | Self::Abandoned)
                | (
                    Self::Executing,
                    Self::Verified | Self::Failed | Self::Abandoned
                )
        );
        if allowed {
            Ok(next)
        } else {
            Err("recovery_plan_state_transition_invalid")
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Verified | Self::Failed | Self::Abandoned)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecoveryPlan {
    pub steps: Vec<String>,
    pub requires_reconciliation: bool,
    pub automatic_retry_allowed: bool,
    #[serde(default)]
    pub state: RecoveryPlanState,
    #[serde(default)]
    pub safe_actions: Vec<String>,
    #[serde(default)]
    pub forbidden_actions: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

impl RecoveryPlan {
    pub fn transition_with_evidence(
        &mut self,
        next: RecoveryPlanState,
        evidence_refs: impl IntoIterator<Item = String>,
    ) -> Result<(), &'static str> {
        self.state.transition(next)?;
        for evidence in evidence_refs {
            if !evidence.trim().is_empty() && !self.evidence_refs.contains(&evidence) {
                self.evidence_refs.push(evidence);
            }
        }
        self.evidence_refs.sort();
        self.state = next;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FailureIncident {
    pub incident_id: String,
    pub source_event_id: EventId,
    pub run_id: Option<RunId>,
    pub class: FailureClass,
    pub summary: String,
    pub recovery: RecoveryPlan,
    pub reconciliation: Option<Value>,
}

impl FailureClass {
    pub fn recovery(self, unknown: bool) -> RecoveryPlan {
        let steps = match self {
            Self::Crash => vec![
                "Inspect the persisted invocation and run facts",
                "Confirm stopped processes",
                "Preview an edit checkpoint before any restore",
            ],
            Self::Timeout => vec![
                "Confirm the timed-out process group stopped",
                "Inspect side effects before deciding on a fresh request",
            ],
            Self::Cancel => vec![
                "Confirm execution stopped",
                "Review edits and leave a new request for any further work",
            ],
            Self::DiskFull => vec![
                "Restore free space outside the running task",
                "Verify the event ledger tail and workspace before resuming",
            ],
            Self::McpFailure => vec![
                "Inspect the connector result and external state",
                "Record reconciliation evidence before another invocation",
            ],
            Self::ProviderUnknown => vec![
                "Inspect provider and tool receipts",
                "Resolve uncertain side effects with independent evidence",
            ],
        };
        let steps = steps.into_iter().map(str::to_owned).collect::<Vec<_>>();
        RecoveryPlan {
            steps: steps.clone(),
            requires_reconciliation: unknown,
            automatic_retry_allowed: false,
            state: RecoveryPlanState::Proposed,
            safe_actions: steps,
            forbidden_actions: vec![
                "automatic_retry".to_owned(),
                "self_approve".to_owned(),
                "close_unknown".to_owned(),
            ],
            evidence_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeedbackCandidate {
    pub candidate_id: String,
    pub submitted_by: String,
    pub session_id: SessionId,
    pub category: String,
    pub observation: String,
    pub proposed_change: String,
    pub evidence_refs: Vec<String>,
    pub submitted_at_ms: u64,
    pub review: Option<Value>,
}

/// Immutable, bounded text edits only. It does not claim to snapshot arbitrary shell effects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceFileSnapshot {
    pub path: String,
    pub contents: Option<String>,
    pub executable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceCheckpoint {
    pub schema: String,
    pub checkpoint_id: String,
    pub project_root: String,
    pub actor_id: String,
    pub session_id: SessionId,
    pub role_id: String,
    #[serde(default)]
    pub work_packet_id: Option<String>,
    #[serde(default)]
    pub path_allow: Vec<String>,
    pub run_id: Option<RunId>,
    pub transcript_offset: u64,
    pub invocation_id: Option<String>,
    pub reason: String,
    pub workspace_revision: String,
    pub data_epoch: Option<String>,
    pub files: Vec<WorkspaceFileSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckpointPreview {
    pub checkpoint_id: String,
    pub current_revision: String,
    pub target_revision: String,
    pub changes: Vec<Value>,
    pub writes_performed: bool,
}
