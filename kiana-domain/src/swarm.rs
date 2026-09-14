//! Bounded fan-out/fan-in contracts; execution still belongs to ControlPlane packet dispatch.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const SWARM_COMMAND: &str = "swarm.command.v1";
pub const SWARM_SNAPSHOT: &str = "swarm.snapshot.v1";
pub const SWARM_SCHEMA: &str = "kiana.swarm-command.v1";
pub const SWARM_CONTROLLER_TEMPLATE: &str = "swarm-controller.v1";
pub const SWARM_CHILD_TEMPLATE: &str = "swarm-child.v1";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmPlan {
    pub swarm_id: String,
    pub project_id: String,
    pub packet_ids: Vec<String>,
    pub reason_code: String,
    pub max_concurrency: u32,
    pub max_depth: u32,
    pub expires_at: u64,
    pub max_tokens: u64,
    pub max_model_calls: u64,
    pub merge_strategy: String,
    pub approval_ref: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwarmController {
    pub plan: SpawnPlan,
    pub cell: CellSpec,
    pub template: AgentTemplate,
    pub budget: BudgetLease,
    pub grant: CapabilityGrant,
    pub supervision: SupervisionLease,
    pub fingerprint: WorkFingerprint,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmStatus {
    Reserved,
    Ready,
    Running,
    ReadyToMerge,
    Completed,
    Failed,
    CancelRequested,
    Cancelled,
    ResultUnknown,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwarmChild {
    pub packet_id: String,
    pub dispatch_request_id: RequestId,
    pub session_id: SessionId,
    pub status: ExecutionStatus,
    pub run: Option<CompanyRun>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChildFailureReport {
    pub packet_id: String,
    pub status: ExecutionStatus,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildMergeDecision {
    pub packet_id: String,
    pub accepted: bool,
    pub reason: String,
    pub review_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoundedSwarm {
    pub plan: SwarmPlan,
    pub owner_id: String,
    pub controller: SwarmController,
    pub status: SwarmStatus,
    pub packets: BTreeMap<String, WorkPacket>,
    pub children: BTreeMap<String, SwarmChild>,
    pub failures: Vec<ChildFailureReport>,
    pub merge_decisions: Vec<ChildMergeDecision>,
    pub evidence_refs: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SwarmState {
    pub revision: u64,
    pub swarms: BTreeMap<String, BoundedSwarm>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmCommandRequest {
    pub schema: String,
    pub expected_revision: u64,
    pub idempotency_key: String,
    pub command: SwarmCommand,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SwarmCommand {
    Create {
        plan: SwarmPlan,
    },
    StartChild {
        swarm_id: String,
        packet_id: String,
        sandbox: Option<String>,
    },
    Reconcile {
        swarm_id: String,
    },
    Merge {
        swarm_id: String,
        decisions: Vec<ChildMergeDecision>,
    },
    Cancel {
        swarm_id: String,
        reason: String,
    },
    ControllerReady {
        swarm_id: String,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SwarmProof {
    pub controller: Option<SwarmController>,
    pub controller_ready: bool,
    pub packets: BTreeMap<String, WorkPacket>,
    pub runs: BTreeMap<String, CompanyRun>,
    pub reviews: BTreeMap<String, PacketReview>,
    pub evidence_refs: Vec<String>,
    pub budget: Option<CompanyBudgetPolicy>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwarmEvent {
    pub schema: String,
    pub request: SwarmCommandRequest,
    pub authority: AutomationAuthority,
    pub proof: SwarmProof,
}
impl SwarmState {
    pub fn transition(
        &self,
        c: &SwarmCommand,
        a: &AutomationAuthority,
        p: &SwarmProof,
    ) -> Result<Self, &'static str> {
        let mut next = self.clone();
        let actor = a
            .context
            .actor_id
            .clone()
            .filter(|id| !id.is_empty())
            .ok_or("swarm_actor_required")?;
        match c {
            SwarmCommand::Create { plan } => {
                if !matches!(a.context.role_id.as_str(), "sponsor" | "pm") {
                    return Err("swarm_planning_role_denied");
                }
                if plan.swarm_id.is_empty()
                    || plan.swarm_id.len() > 128
                    || self.swarms.contains_key(&plan.swarm_id)
                {
                    return Err("swarm_identity_invalid");
                }
                if plan.packet_ids.is_empty()
                    || plan.packet_ids.len() > 8
                    || plan.max_concurrency == 0
                    || plan.max_concurrency > 8
                    || plan.max_depth != 1
                    || plan.expires_at <= a.now_ms
                    || plan.expires_at > a.now_ms.saturating_add(300_000)
                    || plan.merge_strategy != "receipt_only"
                    || plan.reason_code.trim().is_empty()
                {
                    return Err("swarm_bounds_invalid");
                }
                if plan.packet_ids.iter().collect::<BTreeSet<_>>().len() != plan.packet_ids.len()
                    || !p.evidence_refs.contains(&plan.approval_ref)
                {
                    return Err("swarm_approval_or_partition_invalid");
                }
                let budget = p.budget.as_ref().ok_or("swarm_project_budget_required")?;
                let count = plan.packet_ids.len() as u64;
                if plan.max_tokens == 0
                    || plan.max_model_calls == 0
                    || count.saturating_mul(budget.runtime.max_tokens) > plan.max_tokens
                    || count.saturating_mul(budget.runtime.max_model_calls) > plan.max_model_calls
                {
                    return Err("swarm_runtime_budget_exceeded");
                }
                budget.quota.check_reservation(
                    plan.max_model_calls,
                    plan.max_tokens,
                    plan.max_concurrency,
                )?;
                if plan.max_tokens > budget.project.max_tokens || count > budget.project.max_runs {
                    return Err("swarm_project_budget_exceeded");
                }
                let mut paths: Vec<String> = Vec::new();
                let mut fingerprints = BTreeSet::new();
                for id in &plan.packet_ids {
                    let packet = p.packets.get(id).ok_or("swarm_packet_not_approved")?;
                    if packet.path_allow.is_empty()
                        || packet.status != WorkPacketStatus::Approved
                        || packet.claim.is_some()
                    {
                        return Err("swarm_packet_not_available");
                    }
                    for path in &packet.path_allow {
                        if paths.iter().any(|held| path_locks_conflict(held, path)) {
                            return Err("swarm_partition_overlap");
                        }
                        paths.push(path.clone());
                    }
                    let fp = WorkFingerprint::from_parts(
                        &packet.goal,
                        &packet.inputs,
                        &packet.id,
                        "kiana.run-result.v1",
                        "kiana.policy.v1",
                    )?;
                    if !fingerprints.insert(fp.as_str().to_owned()) {
                        return Err("swarm_duplicate_fingerprint");
                    }
                    if self.swarms.values().any(|s| {
                        !matches!(
                            s.status,
                            SwarmStatus::Completed | SwarmStatus::Failed | SwarmStatus::Cancelled
                        ) && s.plan.packet_ids.contains(id)
                    }) {
                        return Err("swarm_packet_already_reserved");
                    }
                }
                let controller = p.controller.clone().ok_or("swarm_controller_required")?;
                controller.cell.validate(&controller.template)?;
                controller.plan.validate()?;
                controller.grant.validate()?;
                if controller.cell.parent_cell_id.is_some()
                    || !controller.cell.owned_paths.is_empty()
                    || controller.cell.depth != 0
                    || controller.cell.spawn_quota != plan.max_concurrency
                    || controller.template.version != SWARM_CONTROLLER_TEMPLATE
                    || !controller.grant.delegation_allowed
                {
                    return Err("swarm_controller_invalid");
                }
                next.swarms.insert(
                    plan.swarm_id.clone(),
                    BoundedSwarm {
                        plan: plan.clone(),
                        owner_id: actor,
                        controller,
                        status: SwarmStatus::Reserved,
                        packets: p.packets.clone(),
                        children: BTreeMap::new(),
                        failures: Vec::new(),
                        merge_decisions: Vec::new(),
                        evidence_refs: p.evidence_refs.clone(),
                    },
                );
            }
            _ => {
                let id = match c {
                    SwarmCommand::StartChild { swarm_id, .. }
                    | SwarmCommand::Reconcile { swarm_id }
                    | SwarmCommand::Merge { swarm_id, .. }
                    | SwarmCommand::Cancel { swarm_id, .. }
                    | SwarmCommand::ControllerReady { swarm_id } => swarm_id,
                    _ => unreachable!(),
                };
                let swarm = next.swarms.get_mut(id).ok_or("swarm_not_found")?;
                if swarm.owner_id != actor {
                    return Err("swarm_owner_mismatch");
                }
                match c {
                    SwarmCommand::ControllerReady { .. } => {
                        if swarm.status != SwarmStatus::Reserved || !p.controller_ready {
                            return Err("swarm_controller_not_ready");
                        }
                        swarm.status = SwarmStatus::Ready;
                    }
                    SwarmCommand::StartChild { packet_id, .. } => {
                        if a.context.role_id != "builder"
                            || !matches!(swarm.status, SwarmStatus::Ready | SwarmStatus::Running)
                            || a.now_ms >= swarm.plan.expires_at
                            || !p.controller_ready
                        {
                            return Err("swarm_dispatch_denied");
                        }
                        if !swarm.plan.packet_ids.contains(packet_id)
                            || swarm.children.contains_key(packet_id)
                        {
                            return Err("swarm_partition_already_dispatched");
                        }
                        if swarm
                            .children
                            .values()
                            .filter(|c| !c.status.is_terminal())
                            .count()
                            >= swarm.plan.max_concurrency as usize
                        {
                            return Err("swarm_concurrency_exhausted");
                        }
                        swarm.children.insert(
                            packet_id.clone(),
                            SwarmChild {
                                packet_id: packet_id.clone(),
                                dispatch_request_id: a.execution_id,
                                session_id: a.session_id.clone(),
                                status: ExecutionStatus::Accepted,
                                run: None,
                            },
                        );
                        swarm.status = SwarmStatus::Running;
                    }
                    SwarmCommand::Reconcile { .. } => {
                        if matches!(
                            swarm.status,
                            SwarmStatus::Completed | SwarmStatus::Cancelled
                        ) {
                            return Err("swarm_terminal");
                        }
                        if swarm.status == SwarmStatus::Reserved {
                            swarm.status = if p.controller_ready {
                                SwarmStatus::Ready
                            } else if a.now_ms >= swarm.plan.expires_at {
                                SwarmStatus::Failed
                            } else {
                                return Err("swarm_controller_not_ready");
                            };
                        }
                        swarm.failures.clear();
                        for (id, child) in &mut swarm.children {
                            let observed = p.runs.get(id).ok_or("swarm_child_evidence_required")?;
                            if observed.author_session_id != child.session_id
                                || observed.project_id != swarm.plan.project_id
                            {
                                return Err("swarm_child_identity_mismatch");
                            }
                            if child.status.is_terminal() && observed.status != child.status {
                                return Err("swarm_child_terminal_conflict");
                            }
                            child.status = observed.status;
                            child.run = Some(observed.clone());
                            if child.status.is_terminal()
                                && child.status != ExecutionStatus::Completed
                            {
                                swarm.failures.push(ChildFailureReport {
                                    packet_id: id.clone(),
                                    status: child.status,
                                    reason: "child_did_not_complete".into(),
                                    evidence_refs: observed.evidence_refs.clone(),
                                });
                            }
                        }
                        if swarm
                            .children
                            .values()
                            .any(|c| c.status == ExecutionStatus::ResultUnknown)
                        {
                            swarm.status = SwarmStatus::ResultUnknown;
                        } else if swarm.status == SwarmStatus::CancelRequested
                            && swarm.children.values().all(|c| c.status.is_terminal())
                        {
                            swarm.status = SwarmStatus::Cancelled;
                        } else if !swarm.failures.is_empty() {
                            swarm.status = SwarmStatus::Failed;
                        } else if swarm.children.len() == swarm.plan.packet_ids.len()
                            && swarm
                                .children
                                .values()
                                .all(|c| c.status == ExecutionStatus::Completed)
                        {
                            swarm.status = SwarmStatus::ReadyToMerge;
                        }
                        swarm.evidence_refs.extend(p.evidence_refs.clone());
                        swarm.evidence_refs.sort();
                        swarm.evidence_refs.dedup();
                    }
                    SwarmCommand::Merge { decisions, .. } => {
                        if !matches!(a.context.role_id.as_str(), "reviewer" | "closer")
                            || !matches!(
                                swarm.status,
                                SwarmStatus::ReadyToMerge
                                    | SwarmStatus::Failed
                                    | SwarmStatus::ResultUnknown
                            )
                        {
                            return Err("swarm_merge_denied");
                        }
                        if decisions.len() != swarm.plan.packet_ids.len()
                            || decisions
                                .iter()
                                .map(|d| &d.packet_id)
                                .collect::<BTreeSet<_>>()
                                .len()
                                != decisions.len()
                        {
                            return Err("swarm_merge_partition_coverage_required");
                        }
                        for decision in decisions {
                            if !swarm.plan.packet_ids.contains(&decision.packet_id)
                                || decision.reason.trim().is_empty()
                            {
                                return Err("swarm_merge_decision_invalid");
                            }
                            if decision.accepted {
                                let child = swarm
                                    .children
                                    .get(&decision.packet_id)
                                    .ok_or("swarm_child_missing")?;
                                let run =
                                    child.run.as_ref().ok_or("swarm_child_evidence_required")?;
                                let review = decision
                                    .review_id
                                    .as_ref()
                                    .and_then(|id| p.reviews.get(id))
                                    .ok_or("swarm_review_required")?;
                                if child.status != ExecutionStatus::Completed
                                    || run.run_id != Some(review.author_run_id)
                                    || review.packet_id != decision.packet_id
                                    || child.session_id == a.context.session_id
                                    || review.reviewer_session_id == child.session_id
                                    || (a.context.role_id == "reviewer"
                                        && review.reviewer_session_id != a.context.session_id)
                                    || !review.criterion_results.values().all(|pass| *pass)
                                {
                                    return Err("swarm_merge_review_invalid");
                                }
                            }
                        }
                        swarm.merge_decisions = decisions.clone();
                        swarm.status = if swarm
                            .children
                            .values()
                            .any(|c| c.status == ExecutionStatus::ResultUnknown)
                        {
                            SwarmStatus::ResultUnknown
                        } else if decisions.iter().all(|d| d.accepted) {
                            SwarmStatus::Completed
                        } else {
                            SwarmStatus::Failed
                        };
                    }
                    SwarmCommand::Cancel { reason, .. } => {
                        if !matches!(a.context.role_id.as_str(), "sponsor" | "pm" | "builder")
                            || reason.trim().is_empty()
                            || matches!(
                                swarm.status,
                                SwarmStatus::Completed | SwarmStatus::Cancelled
                            )
                        {
                            return Err("swarm_cancel_denied");
                        }
                        swarm.status = SwarmStatus::CancelRequested;
                    }
                    _ => {}
                }
            }
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or("swarm_revision_exhausted")?;
        Ok(next)
    }
}

pub const fn swarm_controller_template_id() -> TemplateId {
    TemplateId::from_uuid(uuid::Uuid::from_u128(0xc66b27bafc634ac58836000000000001))
}
pub const fn swarm_child_template_id() -> TemplateId {
    TemplateId::from_uuid(uuid::Uuid::from_u128(0xc66b27bafc634ac58836000000000002))
}
