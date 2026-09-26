//! Rebuildable, scope-bound Company project read model.
//!
//! This DTO is derived from a CompanyState snapshot and explicit cursor/epoch inputs. It contains
//! blockers, responsible references, allowed read-side actions and evidence links, but no chat or
//! client-owned progress. It is a view, never a second authority.

use crate::{
    json_digest, AcceptanceStatus, CompanyState, DeliveryStatus, ExecutionStatus, ProjectStatus,
    WorkPacketStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_READ_MODEL_SCHEMA: &str = "kiana.company-read-model.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyReadModelFreshness {
    CaughtUp,
    Pending,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReadModelBlocker {
    pub code: String,
    pub detail: String,
    pub owner_ref: Option<String>,
    pub allowed_actions: Vec<String>,
}

impl CompanyReadModelBlocker {
    fn validate(&self) -> Result<(), &'static str> {
        required(&self.code, "company_view_blocker_code_required")?;
        required(&self.detail, "company_view_blocker_detail_required")?;
        if self.allowed_actions.is_empty() || self.allowed_actions.len() > 16 {
            return Err("company_view_blocker_actions_required");
        }
        for action in &self.allowed_actions {
            required(action, "company_view_blocker_action_invalid")?;
        }
        if self
            .owner_ref
            .as_deref()
            .is_some_and(|owner| owner.trim().is_empty())
        {
            return Err("company_view_blocker_owner_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPacketView {
    pub packet_id: String,
    pub milestone_id: String,
    pub status: WorkPacketStatus,
    pub run_status: Option<ExecutionStatus>,
    pub blockers: Vec<CompanyReadModelBlocker>,
    pub evidence_links: Vec<String>,
}

impl CompanyPacketView {
    fn validate(&self) -> Result<(), &'static str> {
        required(&self.packet_id, "company_view_packet_id_required")?;
        required(&self.milestone_id, "company_view_milestone_id_required")?;
        for blocker in &self.blockers {
            blocker.validate()?;
        }
        for evidence in &self.evidence_links {
            required(evidence, "company_view_evidence_link_invalid")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyProjectView {
    pub project_id: String,
    pub project_status: ProjectStatus,
    pub baseline_version: Option<u64>,
    pub packet_views: Vec<CompanyPacketView>,
    pub milestone_ids: Vec<String>,
    pub acceptance_status: Option<AcceptanceStatus>,
    pub delivery_status: Option<DeliveryStatus>,
    pub closing_receipt_id: Option<String>,
    pub blockers: Vec<CompanyReadModelBlocker>,
    pub evidence_links: Vec<String>,
}

impl CompanyProjectView {
    fn validate(&self) -> Result<(), &'static str> {
        required(&self.project_id, "company_view_project_id_required")?;
        if self.baseline_version == Some(0) {
            return Err("company_view_baseline_invalid");
        }
        for packet in &self.packet_views {
            packet.validate()?;
        }
        for milestone in &self.milestone_ids {
            required(milestone, "company_view_milestone_invalid")?;
        }
        if self
            .closing_receipt_id
            .as_deref()
            .is_some_and(|receipt| receipt.trim().is_empty())
        {
            return Err("company_view_closing_receipt_invalid");
        }
        for blocker in &self.blockers {
            blocker.validate()?;
        }
        for evidence in &self.evidence_links {
            required(evidence, "company_view_evidence_link_invalid")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReadModelSnapshot {
    pub schema: String,
    pub project_id: String,
    pub source_cursor: u64,
    pub projection_cursor: Option<u64>,
    pub revision: u64,
    pub authority_epoch: u64,
    pub freshness: CompanyReadModelFreshness,
    pub view: CompanyProjectView,
    pub snapshot_digest: String,
}

impl CompanyReadModelSnapshot {
    pub fn new(
        project_id: impl Into<String>,
        source_cursor: u64,
        projection_cursor: Option<u64>,
        revision: u64,
        authority_epoch: u64,
        view: CompanyProjectView,
    ) -> Result<Self, &'static str> {
        let project_id = project_id.into();
        let freshness = if projection_cursor.is_none() {
            CompanyReadModelFreshness::Unknown
        } else if projection_cursor < Some(source_cursor) {
            CompanyReadModelFreshness::Pending
        } else {
            CompanyReadModelFreshness::CaughtUp
        };
        let mut snapshot = Self {
            schema: COMPANY_READ_MODEL_SCHEMA.to_owned(),
            project_id,
            source_cursor,
            projection_cursor,
            revision,
            authority_epoch,
            freshness,
            view,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.canonical_digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_READ_MODEL_SCHEMA
            || self.source_cursor == 0
            || self.projection_cursor == Some(0)
            || self
                .projection_cursor
                .is_some_and(|cursor| cursor > self.source_cursor)
            || self.revision == 0
            || self.authority_epoch == 0
            || self.project_id != self.view.project_id
        {
            return Err("company_view_snapshot_header_invalid");
        }
        let expected = if self.projection_cursor.is_none() {
            CompanyReadModelFreshness::Unknown
        } else if self.projection_cursor < Some(self.source_cursor) {
            CompanyReadModelFreshness::Pending
        } else {
            CompanyReadModelFreshness::CaughtUp
        };
        if self.freshness != expected {
            return Err("company_view_snapshot_freshness_invalid");
        }
        self.view.validate()?;
        digest(
            &self.snapshot_digest,
            "company_view_snapshot_digest_invalid",
        )?;
        if self.snapshot_digest != self.canonical_digest() {
            return Err("company_view_snapshot_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "project_id": self.project_id,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "freshness": self.freshness,
            "view": self.view,
        }))
    }
}

fn blocker(
    code: &str,
    detail: &str,
    owner_ref: Option<String>,
    actions: &[&str],
) -> CompanyReadModelBlocker {
    CompanyReadModelBlocker {
        code: code.to_owned(),
        detail: detail.to_owned(),
        owner_ref,
        allowed_actions: actions.iter().map(|action| (*action).to_owned()).collect(),
    }
}

/// Build a project view from one CompanyState snapshot and an explicit authorization scope.
pub fn project_read_model(
    state: &CompanyState,
    project_id: &str,
    authorized_project_ids: &BTreeSet<String>,
    source_cursor: u64,
    projection_cursor: Option<u64>,
    revision: u64,
    authority_epoch: u64,
) -> Result<CompanyReadModelSnapshot, &'static str> {
    if !authorized_project_ids.contains(project_id) {
        return Err("company_view_scope_denied");
    }
    let project = state
        .projects
        .get(project_id)
        .ok_or("company_view_project_not_found")?;
    let acceptance = project
        .acceptance_id
        .as_deref()
        .and_then(|id| state.acceptances.get(id));
    let delivery = acceptance.and_then(|acceptance| {
        state.deliveries.values().find(|delivery| {
            delivery.project_id == project_id && delivery.acceptance_id == acceptance.acceptance_id
        })
    });
    let mut packet_views = Vec::new();
    let mut evidence_links = Vec::new();
    let mut blockers = Vec::new();
    for (packet_id, packet) in state
        .packets
        .iter()
        .filter(|(_, packet)| packet.project_id == project_id)
    {
        let run = state.runs.get(packet_id);
        let mut packet_blockers = Vec::new();
        if let Some(run) = run {
            evidence_links.extend(run.evidence_refs.clone());
            if run.status == ExecutionStatus::ResultUnknown {
                packet_blockers.push(blocker(
                    "result_unknown",
                    "run result requires reconciliation",
                    Some(packet.packet.assignee_role.clone()),
                    &["inspect", "reconcile"],
                ));
            }
        } else if !matches!(
            packet.packet.status,
            WorkPacketStatus::Succeeded | WorkPacketStatus::Cancelled
        ) {
            packet_blockers.push(blocker(
                "run_missing",
                "packet has no observed run",
                Some(packet.packet.assignee_role.clone()),
                &["inspect", "start"],
            ));
        }
        packet_views.push(CompanyPacketView {
            packet_id: packet_id.clone(),
            milestone_id: packet.milestone_id.clone(),
            status: packet.packet.status,
            run_status: run.map(|run| run.status),
            blockers: packet_blockers,
            evidence_links: run.map(|run| run.evidence_refs.clone()).unwrap_or_default(),
        });
    }
    for milestone in state
        .milestones
        .values()
        .filter(|milestone| milestone.project_id == project_id)
    {
        if milestone.status == crate::MilestoneStatus::Blocked {
            blockers.push(blocker(
                "milestone_blocked",
                "milestone is blocked by its dependency or packet graph",
                None,
                &["inspect", "reconcile"],
            ));
        }
    }
    if matches!(
        project.status,
        ProjectStatus::Paused | ProjectStatus::CancelRequested | ProjectStatus::ResultUnknown
    ) {
        blockers.push(blocker(
            "project_control_pending",
            "project control state requires the corresponding server decision",
            Some(project.sponsor_id.clone()),
            &["inspect", "resume", "reconcile"],
        ));
    }
    if let Some(acceptance) = acceptance {
        evidence_links.extend(acceptance.evidence_refs.clone());
    }
    if let Some(delivery) = delivery {
        if let Some(receipt) = &delivery.handoff_receipt_ref {
            evidence_links.push(receipt.clone());
        }
    }
    evidence_links.sort();
    evidence_links.dedup();
    let mut milestone_ids = state
        .milestones
        .values()
        .filter(|milestone| milestone.project_id == project_id)
        .map(|milestone| milestone.milestone_id.clone())
        .collect::<Vec<_>>();
    milestone_ids.sort();
    let view = CompanyProjectView {
        project_id: project_id.to_owned(),
        project_status: project.status,
        baseline_version: state
            .business
            .baselines
            .get(project_id)
            .map(|baseline| baseline.version),
        packet_views,
        milestone_ids,
        acceptance_status: acceptance.map(|acceptance| acceptance.status),
        delivery_status: delivery.map(|delivery| delivery.status),
        closing_receipt_id: state
            .closing_receipts
            .values()
            .find(|receipt| receipt.project_id == project_id)
            .map(|receipt| receipt.receipt_id.clone()),
        blockers,
        evidence_links,
    };
    CompanyReadModelSnapshot::new(
        project_id,
        source_cursor,
        projection_cursor,
        revision,
        authority_epoch,
        view,
    )
}
