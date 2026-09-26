//! Project pause, resume and cancellation propagation contract.
//!
//! A project control plan is the durable intent shared by the Company command path and the
//! already existing run cancellation / dispatch / cell fencing paths.  It deliberately records
//! child stop observations instead of treating a cancel request as proof that work stopped.

use crate::{json_digest, ProjectStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const PROJECT_CONTROL_SCHEMA: &str = "kiana.project-control.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectControlAction {
    Pause,
    Resume,
    CancelRequest,
    CancelConfirm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildStopState {
    NotStarted,
    StopRequested,
    Stopped,
    Unknown,
}

impl ChildStopState {
    fn has_unconfirmed_effect(self) -> bool {
        matches!(self, Self::StopRequested | Self::Unknown)
    }
}

/// One packet/run propagated from a project control command.  The project and department are
/// repeated here so a plan cannot accidentally stop a similarly named packet in another project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectControlChild {
    pub project_id: String,
    pub packet_id: String,
    pub department_id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub dispatch_intent_id: Option<String>,
    pub stop_state: ChildStopState,
    pub authority_epoch: u64,
}

impl ProjectControlChild {
    fn validate(&self, project_id: &str) -> Result<(), &'static str> {
        if self.project_id != project_id {
            return Err("project_control_unrelated_project");
        }
        for (value, field) in [
            (&self.project_id, "project_control_child_project_required"),
            (&self.packet_id, "project_control_child_packet_required"),
            (
                &self.department_id,
                "project_control_child_department_required",
            ),
        ] {
            required(value, field)?;
        }
        if self.authority_epoch == 0 {
            return Err("project_control_child_epoch_required");
        }
        if self
            .run_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
            || self
                .dispatch_intent_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
        {
            return Err("project_control_child_reference_invalid");
        }
        match self.stop_state {
            ChildStopState::NotStarted if self.run_id.is_some() => {
                Err("project_control_not_started_run_invalid")
            }
            ChildStopState::NotStarted if self.dispatch_intent_id.is_some() => {
                Err("project_control_not_started_dispatch_invalid")
            }
            ChildStopState::StopRequested | ChildStopState::Stopped | ChildStopState::Unknown
                if self.run_id.is_none() =>
            {
                Err("project_control_stopped_run_required")
            }
            _ => Ok(()),
        }
    }
}

/// A plan is an append-only control decision.  It does not execute a run or retire a cell; the
/// existing ControlPlane cancellation, DispatchIntent and CellRegistry paths consume the plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectControlPlan {
    pub schema: String,
    pub control_id: String,
    pub project_id: String,
    pub action: ProjectControlAction,
    pub from_status: ProjectStatus,
    pub to_status: ProjectStatus,
    pub baseline_version: u64,
    pub authority_epoch: u64,
    #[serde(default)]
    pub approval_ref: Option<String>,
    pub reason: String,
    pub dispatch_blocked: bool,
    pub reconcile_required: bool,
    pub children: Vec<ProjectControlChild>,
    pub digest: String,
}

impl ProjectControlPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PROJECT_CONTROL_SCHEMA
            || self.baseline_version == 0
            || self.authority_epoch == 0
            || self.children.len() > 4_096
        {
            return Err("project_control_header_invalid");
        }
        for (value, field) in [
            (&self.control_id, "project_control_id_required"),
            (&self.project_id, "project_control_project_required"),
            (&self.reason, "project_control_reason_required"),
        ] {
            required(value, field)?;
        }
        if self
            .approval_ref
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err("project_control_approval_invalid");
        }

        let mut packets = BTreeSet::new();
        let mut runs = BTreeSet::new();
        let mut dispatches = BTreeSet::new();
        for child in &self.children {
            child.validate(&self.project_id)?;
            if !packets.insert(child.packet_id.as_str()) {
                return Err("project_control_duplicate_packet");
            }
            if let Some(run_id) = &child.run_id {
                if !runs.insert(run_id.as_str()) {
                    return Err("project_control_duplicate_run");
                }
            }
            if let Some(dispatch_id) = &child.dispatch_intent_id {
                if !dispatches.insert(dispatch_id.as_str()) {
                    return Err("project_control_duplicate_dispatch");
                }
            }
        }

        match self.action {
            ProjectControlAction::Pause => {
                if self.to_status != ProjectStatus::Paused
                    || self.dispatch_blocked == false
                    || self.reconcile_required
                    || !pause_source(self.from_status)
                {
                    return Err("project_control_pause_transition_invalid");
                }
            }
            ProjectControlAction::Resume => {
                if self.from_status != ProjectStatus::Paused
                    || self.to_status == ProjectStatus::Paused
                    || !resume_target(self.to_status)
                    || self.dispatch_blocked
                    || self.reconcile_required
                    || self.approval_ref.is_none()
                    || self
                        .children
                        .iter()
                        .any(|child| child.stop_state.has_unconfirmed_effect())
                {
                    return Err("project_control_resume_fence_invalid");
                }
            }
            ProjectControlAction::CancelRequest => {
                if self.to_status != ProjectStatus::CancelRequested
                    || !self.dispatch_blocked
                    || !cancel_source(self.from_status)
                {
                    return Err("project_control_cancel_request_invalid");
                }
            }
            ProjectControlAction::CancelConfirm => {
                if self.from_status != ProjectStatus::CancelRequested
                    || !self.dispatch_blocked
                    || !matches!(
                        self.to_status,
                        ProjectStatus::Cancelled | ProjectStatus::ResultUnknown
                    )
                {
                    return Err("project_control_cancel_confirm_invalid");
                }
                let unknown = self
                    .children
                    .iter()
                    .any(|child| child.stop_state.has_unconfirmed_effect());
                if self.to_status == ProjectStatus::Cancelled {
                    if unknown || self.reconcile_required {
                        return Err("project_cancel_with_unconfirmed_child");
                    }
                    if self.children.iter().any(|child| {
                        !matches!(
                            child.stop_state,
                            ChildStopState::NotStarted | ChildStopState::Stopped
                        )
                    }) {
                        return Err("project_cancel_stop_unconfirmed");
                    }
                } else if !unknown || !self.reconcile_required {
                    return Err("project_result_unknown_requires_reconciliation");
                }
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("project_control_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "control_id": self.control_id,
            "project_id": self.project_id,
            "action": self.action,
            "from_status": self.from_status,
            "to_status": self.to_status,
            "baseline_version": self.baseline_version,
            "authority_epoch": self.authority_epoch,
            "approval_ref": self.approval_ref,
            "reason": self.reason,
            "dispatch_blocked": self.dispatch_blocked,
            "reconcile_required": self.reconcile_required,
            "children": self.children,
        }))
    }
}

fn pause_source(status: ProjectStatus) -> bool {
    matches!(
        status,
        ProjectStatus::Proposed
            | ProjectStatus::Chartering
            | ProjectStatus::Approved
            | ProjectStatus::Planned
            | ProjectStatus::Active
            | ProjectStatus::AtRisk
            | ProjectStatus::ChangePending
            | ProjectStatus::ReadyForAcceptance
    )
}

fn cancel_source(status: ProjectStatus) -> bool {
    matches!(
        status,
        ProjectStatus::Proposed
            | ProjectStatus::Chartering
            | ProjectStatus::Approved
            | ProjectStatus::Planned
            | ProjectStatus::Active
            | ProjectStatus::Paused
            | ProjectStatus::AtRisk
            | ProjectStatus::ChangePending
            | ProjectStatus::ReadyForAcceptance
    )
}

fn resume_target(status: ProjectStatus) -> bool {
    matches!(
        status,
        ProjectStatus::Approved
            | ProjectStatus::Planned
            | ProjectStatus::Active
            | ProjectStatus::AtRisk
            | ProjectStatus::ChangePending
            | ProjectStatus::ReadyForAcceptance
    )
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectControlLedger {
    pub controls: BTreeMap<String, ProjectControlPlan>,
    pub latest_by_project: BTreeMap<String, String>,
}

impl ProjectControlLedger {
    pub fn record(&mut self, plan: ProjectControlPlan) -> Result<(), &'static str> {
        plan.validate()?;
        if let Some(existing) = self.controls.get(&plan.control_id) {
            if existing.digest == plan.digest {
                return Ok(());
            }
            return Err("project_control_duplicate_digest_mismatch");
        }
        if let Some(previous_id) = self.latest_by_project.get(&plan.project_id) {
            let previous = self
                .controls
                .get(previous_id)
                .ok_or("project_control_latest_missing")?;
            if matches!(
                previous.to_status,
                ProjectStatus::Cancelled | ProjectStatus::ResultUnknown
            ) {
                return Err("project_control_terminal_project");
            }
            if plan.baseline_version < previous.baseline_version
                || plan.authority_epoch < previous.authority_epoch
            {
                return Err("project_control_stale_fence");
            }
        }
        self.latest_by_project
            .insert(plan.project_id.clone(), plan.control_id.clone());
        self.controls.insert(plan.control_id.clone(), plan);
        Ok(())
    }

    pub fn latest(&self, project_id: &str) -> Option<&ProjectControlPlan> {
        self.latest_by_project
            .get(project_id)
            .and_then(|control_id| self.controls.get(control_id))
    }
}
