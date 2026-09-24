//! Human operations are projections and candidate records, never replacement runtime authority.
use crate::{
    canonical_journal_bytes, json_digest, redact_text, redact_value, EventId, Notification, RunId,
    SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanInboxKind {
    Question,
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

/// Versioned bridge metadata for a HumanTask-backed notification.
///
/// The bridge intentionally carries the immutable task identity and action/evidence bindings,
/// but not a copied task status.  Status remains authoritative in the original HumanTask or
/// approval/acceptance object and must be queried again when an action is submitted.
pub const HUMAN_TASK_BRIDGE_SCHEMA: &str = "kiana.human-task-bridge.v1";
pub const NOTIFICATION_MATERIALIZATION_SCHEMA: &str = "kiana.notification-materialization.v1";
pub const MAX_HUMAN_TASK_BRIDGE_TEXT_BYTES: usize = 256;
pub const MAX_HUMAN_TASK_BRIDGE_ACTIONS: usize = 16;
pub const MAX_HUMAN_TASK_BRIDGE_EVIDENCE: usize = 64;
pub const MAX_NOTIFICATION_MATERIALIZATION_SUMMARY_BYTES: usize = 512;

impl HumanAction {
    /// Validate an action as a bounded, non-executable UI reference.
    pub fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (&self.id, "human_action_id"),
            (&self.label, "human_action_label"),
            (&self.command, "human_action_command"),
        ] {
            if value.trim().is_empty()
                || value.len() > MAX_HUMAN_TASK_BRIDGE_TEXT_BYTES
                || value.contains(['\0', '\n', '\r'])
                || redact_text(value) != value
            {
                return Err(format!("{field}_invalid"));
            }
        }
        let encoded = canonical_journal_bytes(&self.arguments)
            .map_err(|_| "human_action_arguments_invalid".to_owned())?;
        if encoded.len() > 16 * 1024 || redact_value(&self.arguments) != self.arguments.clone() {
            return Err("human_action_arguments_invalid".to_owned());
        }
        if self.required_fields.len() > 32 {
            return Err("human_action_required_fields_invalid".to_owned());
        }
        if self.required_fields.iter().any(|field| {
            field.trim().is_empty()
                || field.len() > MAX_HUMAN_TASK_BRIDGE_TEXT_BYTES
                || field.contains(['\0', '\n', '\r'])
                || redact_text(field) != field
        }) || self.required_fields.iter().collect::<BTreeSet<_>>().len()
            != self.required_fields.len()
        {
            return Err("human_action_required_fields_noncanonical".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTaskBridge {
    pub schema: String,
    pub task_id: String,
    pub target_kind: String,
    pub target_id: String,
    pub target_revision: u64,
    pub target_digest: String,
    pub source_event_id: EventId,
    pub source_cursor: u64,
    pub decider_principal_id: String,
    pub due_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub evidence_refs: Vec<String>,
    pub actions: Vec<HumanAction>,
    pub bridge_digest: String,
}

impl HumanTaskBridge {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_id: impl Into<String>,
        target_kind: impl Into<String>,
        target_id: impl Into<String>,
        target_revision: u64,
        target_digest: impl Into<String>,
        source_event_id: EventId,
        source_cursor: u64,
        decider_principal_id: impl Into<String>,
        due_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        evidence_refs: Vec<String>,
        actions: Vec<HumanAction>,
    ) -> Result<Self, String> {
        let mut bridge = Self {
            schema: HUMAN_TASK_BRIDGE_SCHEMA.to_owned(),
            task_id: task_id.into(),
            target_kind: target_kind.into(),
            target_id: target_id.into(),
            target_revision,
            target_digest: target_digest.into(),
            source_event_id,
            source_cursor,
            decider_principal_id: decider_principal_id.into(),
            due_at_unix_ms,
            expires_at_unix_ms,
            evidence_refs,
            actions,
            bridge_digest: String::new(),
        };
        bridge.evidence_refs.sort();
        bridge.actions.sort_by(|left, right| left.id.cmp(&right.id));
        bridge.bridge_digest = bridge.digest();
        bridge.validate()?;
        Ok(bridge)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HUMAN_TASK_BRIDGE_SCHEMA
            || self.task_id.trim().is_empty()
            || self.target_kind.trim().is_empty()
            || self.target_id.trim().is_empty()
            || self.target_revision == 0
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.decider_principal_id.trim().is_empty()
            || self.due_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.due_at_unix_ms
            || self.task_id.len() > MAX_HUMAN_TASK_BRIDGE_TEXT_BYTES
            || self.target_kind.len() > MAX_HUMAN_TASK_BRIDGE_TEXT_BYTES
            || self.target_id.len() > MAX_HUMAN_TASK_BRIDGE_TEXT_BYTES
            || self.decider_principal_id.len() > MAX_HUMAN_TASK_BRIDGE_TEXT_BYTES
        {
            return Err("human_task_bridge_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.task_id, "human_task_bridge_task_id"),
            (&self.target_kind, "human_task_bridge_target_kind"),
            (&self.target_id, "human_task_bridge_target_id"),
            (&self.decider_principal_id, "human_task_bridge_decider"),
        ] {
            if value.contains(['\0', '\n', '\r']) || redact_text(value) != value {
                return Err(format!("{field}_invalid"));
            }
        }
        validate_digest(&self.target_digest, "human_task_bridge_target_digest")?;
        if self.evidence_refs.is_empty()
            || self.evidence_refs.len() > MAX_HUMAN_TASK_BRIDGE_EVIDENCE
        {
            return Err("human_task_bridge_evidence_required".to_owned());
        }
        for evidence in &self.evidence_refs {
            validate_ref(evidence, "human_task_bridge_evidence")?;
        }
        if self.evidence_refs.windows(2).any(|pair| pair[0] >= pair[1])
            || self.evidence_refs.iter().collect::<BTreeSet<_>>().len() != self.evidence_refs.len()
        {
            return Err("human_task_bridge_evidence_noncanonical".to_owned());
        }
        if self.actions.is_empty() || self.actions.len() > MAX_HUMAN_TASK_BRIDGE_ACTIONS {
            return Err("human_task_bridge_actions_required".to_owned());
        }
        for action in &self.actions {
            action.validate()?;
        }
        if self.actions.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err("human_task_bridge_actions_noncanonical".to_owned());
        }
        validate_digest(&self.bridge_digest, "human_task_bridge_digest")?;
        if self.bridge_digest != self.digest() {
            return Err("human_task_bridge_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "task_id": self.task_id,
            "target_kind": self.target_kind,
            "target_id": self.target_id,
            "target_revision": self.target_revision,
            "target_digest": self.target_digest,
            "source_event_id": self.source_event_id,
            "source_cursor": self.source_cursor,
            "decider_principal_id": self.decider_principal_id,
            "due_at_unix_ms": self.due_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "evidence_refs": self.evidence_refs,
            "actions": self.actions,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationMaterialization {
    pub schema: String,
    pub notification: Notification,
    pub source_event_id: EventId,
    pub source_cursor: u64,
    pub kind: HumanInboxKind,
    pub title: String,
    pub redacted_summary: String,
    pub due_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub evidence_refs: Vec<String>,
    pub actions: Vec<HumanAction>,
    pub human_task: Option<HumanTaskBridge>,
    pub materialization_digest: String,
}

impl NotificationMaterialization {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        notification: Notification,
        source_event_id: EventId,
        source_cursor: u64,
        kind: HumanInboxKind,
        title: impl Into<String>,
        redacted_summary: impl Into<String>,
        due_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        evidence_refs: Vec<String>,
        actions: Vec<HumanAction>,
        human_task: Option<HumanTaskBridge>,
    ) -> Result<Self, String> {
        let mut materialization = Self {
            schema: NOTIFICATION_MATERIALIZATION_SCHEMA.to_owned(),
            notification,
            source_event_id,
            source_cursor,
            kind,
            title: title.into(),
            redacted_summary: redacted_summary.into(),
            due_at_unix_ms,
            expires_at_unix_ms,
            evidence_refs,
            actions,
            human_task,
            materialization_digest: String::new(),
        };
        materialization.evidence_refs.sort();
        materialization
            .actions
            .sort_by(|left, right| left.id.cmp(&right.id));
        materialization.materialization_digest = materialization.digest();
        materialization.validate()?;
        Ok(materialization)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_MATERIALIZATION_SCHEMA
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.title.trim().is_empty()
            || self.redacted_summary.trim().is_empty()
            || self.title.len() > MAX_NOTIFICATION_MATERIALIZATION_SUMMARY_BYTES
            || self.redacted_summary.len() > MAX_NOTIFICATION_MATERIALIZATION_SUMMARY_BYTES
            || self.due_at_unix_ms == 0
            || self.due_at_unix_ms < self.notification.created_at_unix_ms
            || self.expires_at_unix_ms <= self.due_at_unix_ms
            || self.expires_at_unix_ms > self.notification.expires_at_unix_ms
        {
            return Err("notification_materialization_header_invalid".to_owned());
        }
        self.notification.validate()?;
        for (value, field) in [
            (&self.title, "notification_materialization_title"),
            (
                &self.redacted_summary,
                "notification_materialization_summary",
            ),
        ] {
            if value.contains(['\0', '\n', '\r']) || redact_text(value) != value {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.evidence_refs.is_empty()
            || self.evidence_refs.len() > MAX_HUMAN_TASK_BRIDGE_EVIDENCE
        {
            return Err("notification_materialization_evidence_required".to_owned());
        }
        for evidence in &self.evidence_refs {
            validate_ref(evidence, "notification_materialization_evidence")?;
        }
        if self.evidence_refs.windows(2).any(|pair| pair[0] >= pair[1])
            || self.evidence_refs.iter().collect::<BTreeSet<_>>().len() != self.evidence_refs.len()
        {
            return Err("notification_materialization_evidence_noncanonical".to_owned());
        }
        if self.actions.is_empty() || self.actions.len() > MAX_HUMAN_TASK_BRIDGE_ACTIONS {
            return Err("notification_materialization_actions_required".to_owned());
        }
        for action in &self.actions {
            action.validate()?;
        }
        if self.actions.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err("notification_materialization_actions_noncanonical".to_owned());
        }
        if let Some(task) = &self.human_task {
            task.validate()?;
            if task.source_event_id != self.source_event_id
                || task.source_cursor != self.source_cursor
                || task.decider_principal_id != self.notification.recipient_id
                || task.due_at_unix_ms != self.due_at_unix_ms
                || task.expires_at_unix_ms != self.expires_at_unix_ms
                || task.evidence_refs != self.evidence_refs
                || task.actions != self.actions
            {
                return Err("notification_materialization_human_task_mismatch".to_owned());
            }
        }
        validate_digest(
            &self.materialization_digest,
            "notification_materialization_digest",
        )?;
        if self.materialization_digest != self.digest() {
            return Err("notification_materialization_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Project into the existing display-only Human Inbox DTO.  The due/expiry/source/evidence
    /// metadata is included in `detail`; no HumanTask status is copied into this view.
    pub fn to_human_inbox_item(&self) -> Result<HumanInboxItem, String> {
        self.validate()?;
        Ok(HumanInboxItem {
            item_id: format!("notification:{}", self.notification.notification_id),
            kind: self.kind.clone(),
            title: self.title.clone(),
            source_ref: format!("event:{}", self.source_event_id),
            run_id: None,
            detail: json!({
                "schema": NOTIFICATION_MATERIALIZATION_SCHEMA,
                "notification": self.notification,
                "source_event_id": self.source_event_id,
                "source_cursor": self.source_cursor,
                "redacted_summary": self.redacted_summary,
                "due_at_unix_ms": self.due_at_unix_ms,
                "expires_at_unix_ms": self.expires_at_unix_ms,
                "evidence_refs": self.evidence_refs,
                "human_task": self.human_task,
            }),
            actions: self.actions.clone(),
        })
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification": self.notification,
            "source_event_id": self.source_event_id,
            "source_cursor": self.source_cursor,
            "kind": self.kind,
            "title": self.title,
            "redacted_summary": self.redacted_summary,
            "due_at_unix_ms": self.due_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "evidence_refs": self.evidence_refs,
            "actions": self.actions,
            "human_task": self.human_task,
        }))
    }
}

fn validate_ref(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_HUMAN_TASK_BRIDGE_TEXT_BYTES
        || value.contains(['\0', '\n', '\r'])
        || value.chars().any(char::is_whitespace)
        || redact_text(value) != value
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
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
