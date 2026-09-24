//! Versioned baseline changes, local delivery and honest business closure.
//! All proof values are populated by ControlPlane from the same journal.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, &'static str>;
fn require(ok: bool, code: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(code)
    }
}
fn nonempty(s: &str) -> Result<()> {
    require(
        !s.trim().is_empty() && s.len() <= 16384,
        "business_field_invalid",
    )
}
fn verified(refs: &[String], proof: &CompanyProof) -> Result<()> {
    require(
        !refs.is_empty() && refs.len() <= 1024 && refs.iter().all(|r| proof.events.contains(r)),
        "business_evidence_unverified",
    )
}

fn runtime_bundle_valid(bundle: &EvidenceBundle) -> bool {
    let Some(receipt) = bundle.runtime_receipt.as_ref() else {
        return false;
    };
    let Some(runtime_evidence) = bundle.runtime_evidence.as_ref() else {
        return false;
    };
    receipt.validate().is_ok()
        && receipt.event_refs == bundle.event_refs
        && runtime_evidence.validate().is_ok()
        && runtime_evidence.receipt == *receipt
        && runtime_evidence.artifact_refs == bundle.artifact_refs
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryManifestEntry {
    pub artifact_id: String,
    pub relative_path: String,
    pub sha256: String,
    pub content_utf8: String,
    pub producer_runs: Vec<RunId>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessDeliveryManifest {
    pub schema: String,
    pub delivery_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub acceptance_id: String,
    pub acceptance_digest: String,
    pub channel: String,
    pub destination: String,
    pub recipient: String,
    pub residual_obligations: Vec<String>,
    pub entries: Vec<DeliveryManifestEntry>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessDeliveryStatus {
    Prepared,
    Approved,
    DispatchRequested,
    Delivered,
    Confirmed,
    ResultUnknown,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessDelivery {
    pub manifest: BusinessDeliveryManifest,
    pub digest: String,
    pub status: BusinessDeliveryStatus,
    pub approval: Option<BusinessDeliveryApproval>,
    pub dispatch_request_id: Option<RequestId>,
    pub sender_session: Option<SessionId>,
    pub package_receipt: Option<BusinessPackageObservation>,
    pub confirmation: Option<BusinessDeliveryConfirmation>,
    pub created_at: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessDeliveryApproval {
    pub manifest_digest: String,
    pub approved_by: String,
    pub approved_at: u64,
    pub expires_at: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessDeliveryConfirmation {
    pub recipient: String,
    pub manifest_digest: String,
    pub package_sha256: String,
    pub received_at: u64,
    pub decision_request_id: RequestId,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessPackageObservation {
    pub request_id: RequestId,
    pub package_id: String,
    pub manifest_digest: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub event_ref: String,
    pub status: ExecutionStatus,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessCloseKind {
    Success,
    Failure,
    Cancelled,
    Waived,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessClosingReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub close_kind: BusinessCloseKind,
    pub reason: String,
    pub objective_ids: Vec<String>,
    pub packet_versions: BTreeMap<String, u64>,
    pub run_ids: Vec<RunId>,
    pub authors: Vec<BusinessAuthor>,
    pub acceptance_ids: Vec<String>,
    pub delivery_ids: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub residual_obligations: Vec<String>,
    pub budget_snapshot: Option<CompanyBudgetPolicy>,
    pub usage: BusinessUsageSummary,
    pub closed_by: String,
    pub closed_at: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BusinessUsageSummary {
    pub run_count: u64,
    pub model_calls: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub monetary_cost_micros: Option<u64>,
    pub evidence_refs: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessBaselineProposal {
    pub project_id: String,
    pub old_baseline_version: u64,
    pub charter_ref: String,
    pub scope: String,
    pub criteria: Vec<BusinessCriterion>,
    pub milestones: Vec<Milestone>,
    pub packets: Vec<BusinessPlannedPacket>,
    pub budget: CompanyBudgetPolicy,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessBaselineChange {
    pub change_id: String,
    pub proposal: BusinessBaselineProposal,
    pub digest: String,
    pub status: ChangeStatus,
    pub requested_by: String,
    pub decision_by: Option<String>,
    pub decision_ref: Option<String>,
    pub prior_project_status: ProjectStatus,
    pub affected_acceptances: Vec<String>,
    pub affected_deliveries: Vec<String>,
    pub published_version: Option<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementAggregation {
    Mean,
    Minimum,
    Maximum,
    Latest,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessMeasurementPlan {
    pub plan_id: String,
    pub objective_id: String,
    pub project_id: String,
    pub metric: String,
    pub unit: String,
    pub dataset_id: String,
    pub method: String,
    pub owner: String,
    pub window_start: u64,
    pub window_end: u64,
    pub minimum_samples: u32,
    pub aggregation: MeasurementAggregation,
    pub fixture: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessMetricData {
    pub schema: String,
    pub observation_id: String,
    pub dataset_id: String,
    pub metric: String,
    pub unit: String,
    pub value: f64,
    pub observed_at: u64,
    pub fixture: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessMetricObservation {
    pub plan_id: String,
    pub data: BusinessMetricData,
    pub artifact_ref: String,
    pub content_hash: String,
    pub recorded_at: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessOutcomeAssessment {
    pub assessment_id: String,
    pub plan_id: String,
    pub target: Objective,
    pub observation_ids: Vec<String>,
    pub aggregate: Option<f64>,
    pub status: Option<OutcomeStatus>,
    pub missing_data: bool,
    pub fixture: bool,
    pub evidence_refs: Vec<String>,
    pub assessed_at: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyCloseoutState {
    pub deliveries: BTreeMap<String, BusinessDelivery>,
    pub receipts: BTreeMap<String, BusinessClosingReceipt>,
    pub changes: BTreeMap<String, BusinessBaselineChange>,
    pub measurement_plans: BTreeMap<String, BusinessMeasurementPlan>,
    pub observations: BTreeMap<String, BusinessMetricObservation>,
    pub assessments: BTreeMap<String, BusinessOutcomeAssessment>,
    pub cancel_reasons: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyCloseoutProof {
    pub package: Option<BusinessPackageObservation>,
    pub all_runs_stopped: bool,
    pub run_evidence: Vec<String>,
    pub usage: BusinessUsageSummary,
    pub metric: Option<BusinessMetricData>,
    pub metric_hash: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BusinessCloseoutAction {
    PrepareDelivery {
        delivery_id: String,
        acceptance_id: String,
        destination: String,
        recipient: String,
        residual_obligations: Vec<String>,
    },
    ApproveDelivery {
        delivery_id: String,
        manifest_digest: String,
        expires_at: u64,
    },
    DispatchDelivery {
        delivery_id: String,
    },
    ReconcileDelivery {
        delivery_id: String,
    },
    ConfirmDelivery {
        delivery_id: String,
        manifest_digest: String,
        package_sha256: String,
    },
    Close {
        project_id: String,
        receipt_id: String,
        close_kind: BusinessCloseKind,
        reason: String,
        evidence_refs: Vec<String>,
        residual_obligations: Vec<String>,
    },
    ProposeChange {
        change_id: String,
        proposal: Box<BusinessBaselineProposal>,
    },
    DecideChange {
        change_id: String,
        digest: String,
        approve: bool,
        decision_ref: String,
    },
    ImplementChange {
        change_id: String,
        digest: String,
    },
    VerifyChange {
        change_id: String,
        evidence_refs: Vec<String>,
    },
    RequestCancel {
        project_id: String,
        reason: String,
    },
    ReconcileCancel {
        project_id: String,
    },
    PlanMeasurement {
        plan: BusinessMeasurementPlan,
    },
    ObserveMetric {
        plan_id: String,
        artifact_ref: String,
    },
    AssessOutcome {
        plan_id: String,
        assessment_id: String,
    },
    AchieveObjective {
        objective_id: String,
        assessment_id: String,
        decision_ref: String,
    },
}
impl BusinessCloseoutAction {
    pub fn roles(&self) -> &'static [&'static str] {
        match self {
            Self::PrepareDelivery { .. }
            | Self::DispatchDelivery { .. }
            | Self::ReconcileDelivery { .. }
            | Self::Close { .. }
            | Self::ObserveMetric { .. }
            | Self::AssessOutcome { .. } => &["closer", "sponsor"],
            Self::ProposeChange { .. }
            | Self::ImplementChange { .. }
            | Self::VerifyChange { .. } => &["pm"],
            Self::RequestCancel { .. } | Self::ReconcileCancel { .. } => {
                &["pm", "sponsor", "closer"]
            }
            Self::ApproveDelivery { .. }
            | Self::ConfirmDelivery { .. }
            | Self::DecideChange { .. }
            | Self::PlanMeasurement { .. }
            | Self::AchieveObjective { .. } => &["sponsor"],
        }
    }
    pub fn project_id(&self, s: &CompanyState) -> Option<String> {
        match self {
            Self::PrepareDelivery { acceptance_id, .. } => s
                .business
                .acceptances
                .get(acceptance_id)
                .map(|a| a.project_id.clone()),
            Self::ApproveDelivery { delivery_id, .. }
            | Self::DispatchDelivery { delivery_id }
            | Self::ReconcileDelivery { delivery_id }
            | Self::ConfirmDelivery { delivery_id, .. } => s
                .business
                .closeout
                .deliveries
                .get(delivery_id)
                .map(|d| d.manifest.project_id.clone()),
            Self::Close { project_id, .. }
            | Self::RequestCancel { project_id, .. }
            | Self::ReconcileCancel { project_id } => Some(project_id.clone()),
            Self::ProposeChange { proposal, .. } => Some(proposal.project_id.clone()),
            Self::DecideChange { change_id, .. }
            | Self::ImplementChange { change_id, .. }
            | Self::VerifyChange { change_id, .. } => s
                .business
                .closeout
                .changes
                .get(change_id)
                .map(|c| c.proposal.project_id.clone()),
            Self::PlanMeasurement { plan } => Some(plan.project_id.clone()),
            Self::ObserveMetric { plan_id, .. } | Self::AssessOutcome { plan_id, .. } => s
                .business
                .closeout
                .measurement_plans
                .get(plan_id)
                .map(|p| p.project_id.clone()),
            Self::AchieveObjective { assessment_id, .. } => s
                .business
                .closeout
                .assessments
                .get(assessment_id)
                .and_then(|a| s.business.closeout.measurement_plans.get(&a.plan_id))
                .map(|p| p.project_id.clone()),
        }
    }
    pub fn references(&self) -> Vec<String> {
        match self {
            Self::Close { evidence_refs, .. } | Self::VerifyChange { evidence_refs, .. } => {
                evidence_refs.clone()
            }
            Self::DecideChange { decision_ref, .. }
            | Self::AchieveObjective { decision_ref, .. } => vec![decision_ref.clone()],
            Self::ProposeChange { proposal, .. } => vec![proposal.charter_ref.clone()],
            Self::ObserveMetric { artifact_ref, .. } => vec![artifact_ref.clone()],
            _ => Vec::new(),
        }
    }
}

impl CompanyState {
    pub(crate) fn apply_closeout(
        &mut self,
        c: &BusinessCloseoutAction,
        a: &CompanyAuthority,
        p: &CompanyProof,
    ) -> Result<()> {
        let proof = &p.closeout;
        require(
            c.roles().contains(&a.role_id.as_str()),
            "business_closeout_role_denied",
        )?;
        match c {
            BusinessCloseoutAction::PrepareDelivery {
                delivery_id,
                acceptance_id,
                destination,
                recipient,
                residual_obligations,
            } => {
                require(
                    uuid::Uuid::parse_str(delivery_id).is_ok(),
                    "business_delivery_id_invalid",
                )?;
                require(
                    !self.business.closeout.deliveries.contains_key(delivery_id),
                    "business_delivery_exists",
                )?;
                let ac = self
                    .business
                    .acceptances
                    .get(acceptance_id)
                    .ok_or("business_acceptance_missing")?;
                let baseline = self
                    .business
                    .baselines
                    .get(&ac.project_id)
                    .ok_or("business_baseline_missing")?;
                require(
                    ac.target == AcceptanceTarget::Project(ac.project_id.clone())
                        && ac.status == AcceptanceStatus::Accepted
                        && ac.baseline_version == baseline.version
                        && self.projects[&ac.project_id].status == ProjectStatus::Accepted,
                    "business_delivery_current_project_acceptance_required",
                )?;
                require(
                    !self.business.closeout.deliveries.values().any(|d| {
                        d.manifest.acceptance_id == *acceptance_id
                            && d.status != BusinessDeliveryStatus::Failed
                    }),
                    "business_acceptance_delivery_exists",
                )?;
                require(
                    baseline
                        .stakeholders
                        .iter()
                        .any(|s| s.principal_id == *recipient && s.delivery_recipient),
                    "business_delivery_recipient_not_stakeholder",
                )?;
                require(
                    destination == &format!("lessons/deliveries/{delivery_id}.json"),
                    "business_delivery_destination_invalid",
                )?;
                require(
                    residual_obligations.len() <= 128,
                    "business_delivery_obligations_limit",
                )?;
                require(
                    ac.bundle_ids.iter().all(|id| {
                        self.business
                            .bundles
                            .get(id)
                            .is_some_and(runtime_bundle_valid)
                    }),
                    "business_delivery_runtime_evidence_missing",
                )?;
                let ids = ac
                    .bundle_ids
                    .iter()
                    .filter_map(|b| self.business.bundles.get(b))
                    .flat_map(|b| b.artifact_refs.iter().cloned())
                    .collect::<BTreeSet<_>>();
                require(
                    !ids.is_empty() && ids.len() <= 128,
                    "business_delivery_artifact_set_invalid",
                )?;
                let mut entries = Vec::new();
                for reference in ids {
                    let id = reference
                        .strip_prefix("artifact:")
                        .ok_or("business_delivery_artifact_required")?;
                    let artifact = self
                        .artifacts
                        .get(id)
                        .ok_or("business_delivery_artifact_missing")?;
                    let metadata = self
                        .business
                        .artifacts
                        .get(id)
                        .ok_or("business_delivery_artifact_unversioned")?;
                    require(
                        metadata.project_id == ac.project_id
                            && metadata.baseline_version == baseline.version
                            && metadata.content_hash == journal_sha256(artifact.text.as_bytes())
                            && p.events.contains(&reference),
                        "business_delivery_artifact_changed_or_unverified",
                    )?;
                    entries.push(DeliveryManifestEntry {
                        artifact_id: id.into(),
                        relative_path: artifact.relative_path.clone(),
                        sha256: format!("sha256:{}", metadata.content_hash),
                        content_utf8: artifact.text.clone(),
                        producer_runs: metadata.producer_runs.clone(),
                    });
                }
                require(
                    entries.iter().map(|e| e.content_utf8.len()).sum::<usize>() <= 1024 * 1024,
                    "business_delivery_package_too_large",
                )?;
                let manifest = BusinessDeliveryManifest {
                    schema: "kiana.company-delivery-manifest.v2".into(),
                    delivery_id: delivery_id.clone(),
                    project_id: ac.project_id.clone(),
                    baseline_version: baseline.version,
                    acceptance_id: acceptance_id.clone(),
                    acceptance_digest: ac.evidence_digest.clone(),
                    channel: "local_package".into(),
                    destination: destination.clone(),
                    recipient: recipient.clone(),
                    residual_obligations: residual_obligations.clone(),
                    entries,
                };
                let digest = json_digest(&serde_json::json!(manifest));
                self.business.closeout.deliveries.insert(
                    delivery_id.clone(),
                    BusinessDelivery {
                        manifest,
                        digest,
                        status: BusinessDeliveryStatus::Prepared,
                        approval: None,
                        dispatch_request_id: None,
                        sender_session: None,
                        package_receipt: None,
                        confirmation: None,
                        created_at: a.now_ms,
                    },
                );
            }
            BusinessCloseoutAction::ApproveDelivery {
                delivery_id,
                manifest_digest,
                expires_at,
            } => {
                require(
                    p.business.actor_is_human,
                    "business_human_delivery_approval_required",
                )?;
                let delivery = self
                    .business
                    .closeout
                    .deliveries
                    .get_mut(delivery_id)
                    .ok_or("business_delivery_missing")?;
                require(
                    delivery.status == BusinessDeliveryStatus::Prepared
                        && delivery.digest == *manifest_digest
                        && *expires_at > a.now_ms
                        && *expires_at <= a.now_ms.saturating_add(86_400_000),
                    "business_delivery_approval_stale_or_invalid",
                )?;
                require(
                    self.business.baselines[&delivery.manifest.project_id].version
                        == delivery.manifest.baseline_version
                        && self.projects[&delivery.manifest.project_id].status
                            == ProjectStatus::Accepted,
                    "business_delivery_baseline_stale",
                )?;
                delivery.approval = Some(BusinessDeliveryApproval {
                    manifest_digest: manifest_digest.clone(),
                    approved_by: a.actor_id.clone(),
                    approved_at: a.now_ms,
                    expires_at: *expires_at,
                });
                delivery.status = BusinessDeliveryStatus::Approved;
            }
            BusinessCloseoutAction::DispatchDelivery { delivery_id } => {
                let delivery = self
                    .business
                    .closeout
                    .deliveries
                    .get_mut(delivery_id)
                    .ok_or("business_delivery_missing")?;
                require(
                    delivery.status == BusinessDeliveryStatus::Approved
                        && delivery.dispatch_request_id.is_none()
                        && delivery.approval.as_ref().is_some_and(|approval| {
                            approval.manifest_digest == delivery.digest
                                && approval.expires_at > a.now_ms
                        }),
                    "business_delivery_approval_expired_or_consumed",
                )?;
                require(
                    self.business.baselines[&delivery.manifest.project_id].version
                        == delivery.manifest.baseline_version
                        && self.projects[&delivery.manifest.project_id].status
                            == ProjectStatus::Accepted,
                    "business_delivery_baseline_stale",
                )?;
                require(
                    delivery
                        .manifest
                        .entries
                        .iter()
                        .all(|e| p.events.contains(&format!("artifact:{}", e.artifact_id))),
                    "business_delivery_source_revoked",
                )?;
                delivery.status = BusinessDeliveryStatus::DispatchRequested;
                delivery.dispatch_request_id = Some(a.execution_request_id);
                delivery.sender_session = Some(a.session_id.clone());
            }
            BusinessCloseoutAction::ReconcileDelivery { delivery_id } => {
                let delivery = self
                    .business
                    .closeout
                    .deliveries
                    .get_mut(delivery_id)
                    .ok_or("business_delivery_missing")?;
                require(
                    matches!(
                        delivery.status,
                        BusinessDeliveryStatus::DispatchRequested
                            | BusinessDeliveryStatus::ResultUnknown
                    ),
                    "business_delivery_not_reconcilable",
                )?;
                let observation = proof
                    .package
                    .as_ref()
                    .ok_or("business_package_observation_missing")?;
                require(
                    Some(observation.request_id) == delivery.dispatch_request_id,
                    "business_package_observation_request_mismatch",
                )?;
                if observation.status == ExecutionStatus::ResultUnknown {
                    delivery.status = BusinessDeliveryStatus::ResultUnknown;
                    delivery.package_receipt = Some(observation.clone());
                    let incident_id = format!("delivery-unknown:{}", delivery_id);
                    self.incidents
                        .entry(incident_id.clone())
                        .or_insert(Incident {
                        incident_id,
                        project_id: Some(delivery.manifest.project_id.clone()),
                        run_id: None,
                        risk_id: None,
                        delivery_id: Some(delivery_id.clone()),
                        severity: "error".into(),
                        detected_at: a.now_ms,
                        impact:
                            "Local package effect is unknown; sender claims do not confirm delivery"
                                .into(),
                        timeline: vec!["delivery_result_unknown".into()],
                        owner_id: a.actor_id.clone(),
                        response_actions: vec![
                            "Inspect the original execution receipt; do not dispatch again".into(),
                        ],
                        evidence_refs: vec![observation.event_ref.clone()],
                        escalation_target: Some(delivery.manifest.recipient.clone()),
                        status: IncidentStatus::Open,
                    });
                    return Ok(());
                }
                require(
                    observation.package_id == *delivery_id
                        && observation.manifest_digest == delivery.digest
                        && observation.path == delivery.manifest.destination,
                    "business_package_observation_mismatch",
                )?;
                delivery.status = match observation.status {
                    ExecutionStatus::Completed => BusinessDeliveryStatus::Delivered,
                    ExecutionStatus::Failed
                    | ExecutionStatus::Blocked
                    | ExecutionStatus::Cancelled => BusinessDeliveryStatus::Failed,
                    _ => BusinessDeliveryStatus::ResultUnknown,
                };
                delivery.package_receipt = Some(observation.clone());
            }
            BusinessCloseoutAction::ConfirmDelivery {
                delivery_id,
                manifest_digest,
                package_sha256,
            } => {
                require(
                    p.business.actor_is_human,
                    "business_human_recipient_confirmation_required",
                )?;
                let delivery = self
                    .business
                    .closeout
                    .deliveries
                    .get_mut(delivery_id)
                    .ok_or("business_delivery_missing")?;
                let observation = proof
                    .package
                    .as_ref()
                    .ok_or("business_package_observation_missing")?;
                require(
                    delivery.status == BusinessDeliveryStatus::Delivered
                        && delivery.manifest.recipient == a.actor_id
                        && delivery.digest == *manifest_digest
                        && delivery
                            .package_receipt
                            .as_ref()
                            .is_some_and(|r| r.sha256 == *package_sha256)
                        && observation.status == ExecutionStatus::Completed
                        && observation.sha256 == *package_sha256
                        && observation.manifest_digest == delivery.digest,
                    "business_recipient_or_package_mismatch",
                )?;
                delivery.confirmation = Some(BusinessDeliveryConfirmation {
                    recipient: a.actor_id.clone(),
                    manifest_digest: manifest_digest.clone(),
                    package_sha256: package_sha256.clone(),
                    received_at: a.now_ms,
                    decision_request_id: a.execution_request_id,
                });
                delivery.status = BusinessDeliveryStatus::Confirmed;
            }
            BusinessCloseoutAction::RequestCancel { project_id, reason } => {
                nonempty(reason)?;
                require(p.business.actor_is_human, "business_human_cancel_required")?;
                let project = self
                    .projects
                    .get_mut(project_id)
                    .ok_or("business_project_missing")?;
                require(
                    !matches!(
                        project.status,
                        ProjectStatus::Closed
                            | ProjectStatus::Archived
                            | ProjectStatus::Failed
                            | ProjectStatus::Cancelled
                            | ProjectStatus::Rejected
                            | ProjectStatus::CancelRequested
                            | ProjectStatus::ResultUnknown
                    ),
                    "business_project_not_cancellable",
                )?;
                project.status = ProjectStatus::CancelRequested;
                project.version += 1;
                self.business
                    .closeout
                    .cancel_reasons
                    .insert(project_id.clone(), reason.clone());
            }
            BusinessCloseoutAction::ReconcileCancel { project_id } => {
                let project = self
                    .projects
                    .get_mut(project_id)
                    .ok_or("business_project_missing")?;
                require(
                    matches!(
                        project.status,
                        ProjectStatus::CancelRequested | ProjectStatus::ResultUnknown
                    ),
                    "business_project_not_cancelling",
                )?;
                if proof.all_runs_stopped
                    && !self.business.closeout.deliveries.values().any(|d| {
                        d.manifest.project_id == *project_id
                            && matches!(
                                d.status,
                                BusinessDeliveryStatus::DispatchRequested
                                    | BusinessDeliveryStatus::ResultUnknown
                            )
                    })
                {
                    project.status = ProjectStatus::Cancelled;
                    for packet in self
                        .packets
                        .values_mut()
                        .filter(|p| p.project_id == *project_id)
                    {
                        packet.packet.claim = None;
                    }
                } else {
                    project.status = ProjectStatus::ResultUnknown;
                    let id = format!("company-cancel:{project_id}");
                    project.incident_id = Some(id.clone());
                    self.incidents.entry(id.clone()).or_insert(Incident{incident_id:id,project_id:Some(project_id.clone()),run_id:None,risk_id:None,delivery_id:None,severity:"error".into(),detected_at:a.now_ms,impact:"Cancellation lacks confirmed stop or package outcome".into(),timeline:vec!["company_cancel_reconciliation_required".into()],owner_id:a.actor_id.clone(),response_actions:vec!["Inspect original run and package receipts; never repeat unknown effects".into()],evidence_refs:proof.run_evidence.clone(),escalation_target:Some(a.actor_id.clone()),status:IncidentStatus::Open});
                }
                project.version += 1;
            }
            BusinessCloseoutAction::Close {
                project_id,
                receipt_id,
                close_kind,
                reason,
                evidence_refs,
                residual_obligations,
            } => {
                nonempty(receipt_id)?;
                nonempty(reason)?;
                verified(evidence_refs, p)?;
                require(
                    p.business.actor_is_human
                        && !self.business.closeout.receipts.contains_key(receipt_id),
                    "business_closing_identity_or_duplicate",
                )?;
                require(
                    proof.all_runs_stopped,
                    "business_closing_unresolved_effects",
                )?;
                let project = self
                    .projects
                    .get(project_id)
                    .ok_or("business_project_missing")?
                    .clone();
                let baseline = self
                    .business
                    .baselines
                    .get(project_id)
                    .ok_or("business_baseline_missing")?;
                require(
                    !self
                        .business
                        .closeout
                        .receipts
                        .values()
                        .any(|r| r.project_id == *project_id),
                    "business_project_already_closed",
                )?;
                require(
                    self.incidents
                        .values()
                        .filter(|i| i.project_id.as_deref() == Some(project_id))
                        .all(|i| {
                            matches!(i.status, IncidentStatus::Resolved | IncidentStatus::Closed)
                        }),
                    "business_closing_incident_unresolved",
                )?;
                let deliveries = self
                    .business
                    .closeout
                    .deliveries
                    .values()
                    .filter(|d| d.manifest.project_id == *project_id)
                    .collect::<Vec<_>>();
                require(
                    deliveries.iter().all(|d| {
                        !matches!(
                            d.status,
                            BusinessDeliveryStatus::DispatchRequested
                                | BusinessDeliveryStatus::ResultUnknown
                                | BusinessDeliveryStatus::Delivered
                        )
                    }),
                    "business_closing_delivery_obligation_unresolved",
                )?;
                let acceptances = self
                    .business
                    .acceptances
                    .values()
                    .filter(|ac| {
                        ac.project_id == *project_id && ac.baseline_version == baseline.version
                    })
                    .collect::<Vec<_>>();
                require(
                    self.business
                        .bundles
                        .values()
                        .filter(|bundle| bundle.project_id == *project_id)
                        .all(runtime_bundle_valid),
                    "business_closing_runtime_evidence_missing",
                )?;
                match close_kind {
                    BusinessCloseKind::Success => {
                        require(
                            project.status == ProjectStatus::Accepted
                                && self.business_accepted(
                                    &AcceptanceTarget::Project(project_id.clone()),
                                    baseline.version,
                                )
                                && baseline.active_packets.iter().all(|id| {
                                    self.business_accepted(
                                        &AcceptanceTarget::Packet(id.clone()),
                                        baseline.version,
                                    )
                                })
                                && project.milestone_refs.iter().all(|id| {
                                    self.business_accepted(
                                        &AcceptanceTarget::Milestone(id.clone()),
                                        baseline.version,
                                    )
                                }),
                            "business_closing_required_acceptance_missing",
                        )?;
                        require(
                            deliveries.iter().any(|d| {
                                d.manifest.baseline_version == baseline.version
                                    && d.status == BusinessDeliveryStatus::Confirmed
                            }) && residual_obligations.is_empty(),
                            "business_closing_confirmed_delivery_required",
                        )?;
                    }
                    BusinessCloseKind::Failure => require(
                        matches!(
                            project.status,
                            ProjectStatus::Active
                                | ProjectStatus::AtRisk
                                | ProjectStatus::ReadyForAcceptance
                                | ProjectStatus::Failed
                        ),
                        "business_failure_close_state_invalid",
                    )?,
                    BusinessCloseKind::Cancelled => require(
                        project.status == ProjectStatus::Cancelled,
                        "business_cancel_close_requires_confirmed_stop",
                    )?,
                    BusinessCloseKind::Waived => require(
                        a.role_id == "sponsor"
                            && !residual_obligations.is_empty()
                            && acceptances.iter().any(|ac| {
                                ac.target == AcceptanceTarget::Project(project_id.clone())
                                    && ac.status == AcceptanceStatus::Waived
                                    && !ac.waiver_refs.is_empty()
                            }),
                        "business_waived_close_requires_named_waiver",
                    )?,
                }
                let mut authors = Vec::new();
                for author in self
                    .business
                    .bundles
                    .values()
                    .filter(|b| b.project_id == *project_id)
                    .flat_map(|b| &b.authors)
                {
                    if !authors.contains(author) {
                        authors.push(author.clone());
                    }
                }
                for run in p
                    .business
                    .runs
                    .values()
                    .filter(|r| r.project_id == *project_id)
                {
                    if let Some(author) = run
                        .run_id
                        .and_then(|r| p.business.authors.get(&r.to_string()))
                    {
                        if !authors.contains(author) {
                            authors.push(author.clone());
                        }
                    }
                }
                require(
                    self.runs
                        .values()
                        .filter(|r| r.project_id == *project_id)
                        .filter_map(|r| r.run_id)
                        .all(|run| authors.iter().any(|a| a.run_id == run)),
                    "business_closing_author_set_incomplete",
                )?;
                require(
                    authors.iter().all(|author| {
                        author.session_id != a.session_id && author.principal_id != a.actor_id
                    }),
                    "business_closing_author_cannot_self_close",
                )?;
                let receipt = BusinessClosingReceipt {
                    schema: "kiana.company-closing-receipt.v2".into(),
                    receipt_id: receipt_id.clone(),
                    project_id: project_id.clone(),
                    baseline_version: baseline.version,
                    close_kind: *close_kind,
                    reason: reason.clone(),
                    objective_ids: project.objective_refs.clone(),
                    packet_versions: self
                        .packets
                        .iter()
                        .filter(|(_, p)| p.project_id == *project_id)
                        .map(|(id, p)| (id.clone(), p.version))
                        .collect(),
                    run_ids: self
                        .runs
                        .values()
                        .filter(|r| r.project_id == *project_id)
                        .filter_map(|r| r.run_id)
                        .collect(),
                    authors,
                    acceptance_ids: acceptances
                        .iter()
                        .map(|a| a.acceptance_id.clone())
                        .collect(),
                    delivery_ids: deliveries
                        .iter()
                        .map(|d| d.manifest.delivery_id.clone())
                        .collect(),
                    evidence_refs: evidence_refs
                        .iter()
                        .chain(&proof.run_evidence)
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    residual_obligations: residual_obligations.clone(),
                    budget_snapshot: self.budgets.get(project_id).cloned(),
                    usage: proof.usage.clone(),
                    closed_by: a.actor_id.clone(),
                    closed_at: a.now_ms,
                };
                self.business
                    .closeout
                    .receipts
                    .insert(receipt_id.clone(), receipt);
                let project = self.projects.get_mut(project_id).unwrap();
                project.status = match close_kind {
                    BusinessCloseKind::Success | BusinessCloseKind::Waived => ProjectStatus::Closed,
                    BusinessCloseKind::Failure => ProjectStatus::Failed,
                    BusinessCloseKind::Cancelled => ProjectStatus::Cancelled,
                };
                project.closing_receipt_id = Some(receipt_id.clone());
                project.version += 1;
            }
            BusinessCloseoutAction::ProposeChange {
                change_id,
                proposal,
            } => {
                nonempty(change_id)?;
                nonempty(&proposal.reason)?;
                nonempty(&proposal.scope)?;
                verified(&[proposal.charter_ref.clone()], p)?;
                require(
                    !self.business.closeout.changes.contains_key(change_id),
                    "business_change_exists",
                )?;
                let baseline = self
                    .business
                    .baselines
                    .get(&proposal.project_id)
                    .ok_or("business_baseline_missing")?;
                let project = self
                    .projects
                    .get_mut(&proposal.project_id)
                    .ok_or("business_project_missing")?;
                require(
                    baseline.version == proposal.old_baseline_version
                        && matches!(
                            project.status,
                            ProjectStatus::Planned
                                | ProjectStatus::Active
                                | ProjectStatus::AtRisk
                                | ProjectStatus::ReadyForAcceptance
                                | ProjectStatus::Accepted
                        ),
                    "business_change_baseline_or_state_invalid",
                )?;
                require(
                    !self.business.closeout.deliveries.values().any(|d| {
                        d.manifest.project_id == proposal.project_id
                            && matches!(
                                d.status,
                                BusinessDeliveryStatus::DispatchRequested
                                    | BusinessDeliveryStatus::Delivered
                                    | BusinessDeliveryStatus::Confirmed
                                    | BusinessDeliveryStatus::ResultUnknown
                            )
                    }),
                    "business_change_delivery_already_in_flight",
                )?;
                let change = BusinessBaselineChange {
                    change_id: change_id.clone(),
                    proposal: (**proposal).clone(),
                    digest: json_digest(&serde_json::json!(proposal)),
                    status: ChangeStatus::PendingDecision,
                    requested_by: a.actor_id.clone(),
                    decision_by: None,
                    decision_ref: None,
                    prior_project_status: project.status,
                    affected_acceptances: self
                        .business
                        .acceptances
                        .values()
                        .filter(|ac| {
                            ac.project_id == proposal.project_id
                                && ac.baseline_version == baseline.version
                        })
                        .map(|a| a.acceptance_id.clone())
                        .collect(),
                    affected_deliveries: self
                        .business
                        .closeout
                        .deliveries
                        .values()
                        .filter(|d| d.manifest.project_id == proposal.project_id)
                        .map(|d| d.manifest.delivery_id.clone())
                        .collect(),
                    published_version: None,
                };
                project.status = ProjectStatus::ChangePending;
                project.version += 1;
                self.business
                    .closeout
                    .changes
                    .insert(change_id.clone(), change);
            }
            BusinessCloseoutAction::DecideChange {
                change_id,
                digest,
                approve,
                decision_ref,
            } => {
                require(
                    p.business.actor_is_human,
                    "business_human_change_decision_required",
                )?;
                verified(&[decision_ref.clone()], p)?;
                let change = self
                    .business
                    .closeout
                    .changes
                    .get_mut(change_id)
                    .ok_or("business_change_missing")?;
                require(
                    change.status == ChangeStatus::PendingDecision
                        && change.digest == *digest
                        && self.business.baselines[&change.proposal.project_id].version
                            == change.proposal.old_baseline_version,
                    "business_change_decision_stale",
                )?;
                change.status = if *approve {
                    ChangeStatus::Approved
                } else {
                    ChangeStatus::Rejected
                };
                change.decision_by = Some(a.actor_id.clone());
                change.decision_ref = Some(decision_ref.clone());
                if !approve {
                    let project = self.projects.get_mut(&change.proposal.project_id).unwrap();
                    project.status = change.prior_project_status;
                    project.version += 1;
                }
            }
            BusinessCloseoutAction::ImplementChange { change_id, digest } => {
                let change = self
                    .business
                    .closeout
                    .changes
                    .get(change_id)
                    .ok_or("business_change_missing")?
                    .clone();
                require(
                    change.status == ChangeStatus::Approved
                        && change.digest == *digest
                        && proof.all_runs_stopped,
                    "business_change_approval_or_stop_required",
                )?;
                let proposal = &change.proposal;
                let old = self
                    .business
                    .baselines
                    .get(&proposal.project_id)
                    .ok_or("business_baseline_missing")?
                    .clone();
                require(
                    old.version == proposal.old_baseline_version
                        && self.projects[&proposal.project_id].status
                            == ProjectStatus::ChangePending,
                    "business_change_baseline_stale",
                )?;
                require(
                    proposal
                        .packets
                        .iter()
                        .all(|p| !self.packets.contains_key(&p.packet.id))
                        && proposal
                            .milestones
                            .iter()
                            .all(|m| !self.milestones.contains_key(&m.milestone_id)),
                    "business_change_new_object_versions_required",
                )?;
                proposal.budget.runtime.validate()?;
                require(
                    proposal.budget.project.project_id.to_string() == proposal.project_id
                        && proposal.budget.quota.scope == proposal.project_id,
                    "business_change_budget_scope_invalid",
                )?;
                let run_count = self
                    .runs
                    .values()
                    .filter(|r| r.project_id == proposal.project_id)
                    .count() as u64;
                let prior = self
                    .budgets
                    .get(&proposal.project_id)
                    .ok_or("business_budget_missing")?;
                require(
                    proposal.budget.project.max_runs >= run_count
                        && proposal.budget.project.max_tokens
                            >= run_count.saturating_mul(prior.runtime.max_tokens),
                    "business_change_cannot_erase_reserved_budget",
                )?;
                let project_criteria = proposal
                    .criteria
                    .iter()
                    .filter(|c| c.target == AcceptanceTarget::Project(proposal.project_id.clone()))
                    .cloned()
                    .collect::<Vec<_>>();
                require(
                    !project_criteria.is_empty()
                        && project_criteria.iter().all(|c| c.refines.is_empty())
                        && project_criteria
                            .iter()
                            .map(|c| &c.criterion_id)
                            .collect::<BTreeSet<_>>()
                            .len()
                            == project_criteria.len(),
                    "business_change_project_criteria_invalid",
                )?;
                let project = self.projects.get_mut(&proposal.project_id).unwrap();
                project.status = ProjectStatus::Approved;
                project.scope_baseline = proposal.scope.clone();
                project.charter_ref = proposal.charter_ref.clone();
                project.success_criteria = project_criteria
                    .iter()
                    .map(|c| c.description.clone())
                    .collect();
                project.milestone_refs.clear();
                project.acceptance_id = None;
                project.version += 1;
                let baseline = self
                    .business
                    .baselines
                    .get_mut(&proposal.project_id)
                    .unwrap();
                baseline.version += 1;
                baseline.scope = proposal.scope.clone();
                baseline.charter_ref = proposal.charter_ref.clone();
                baseline.criteria = project_criteria
                    .into_iter()
                    .map(|c| (c.criterion_id.clone(), c))
                    .collect();
                baseline.plan_version = 0;
                baseline.active_packets = proposal
                    .packets
                    .iter()
                    .map(|p| p.packet.id.clone())
                    .collect();
                baseline.approved_by = change
                    .decision_by
                    .clone()
                    .ok_or("business_change_decision_missing")?;
                baseline.approved_at = a.now_ms;
                let new_version = baseline.version;
                let mut plan_proof = p.clone();
                plan_proof.events.push(
                    change
                        .decision_ref
                        .clone()
                        .ok_or("business_change_decision_missing")?,
                );
                self.apply_business(
                    &CompanyBusinessAction::PublishPlan {
                        project_id: proposal.project_id.clone(),
                        baseline_version: new_version,
                        milestones: proposal.milestones.clone(),
                        packets: proposal.packets.clone(),
                        criteria: proposal
                            .criteria
                            .iter()
                            .filter(|c| !matches!(c.target, AcceptanceTarget::Project(_)))
                            .cloned()
                            .collect(),
                        decision_ref: change.decision_ref.clone().unwrap(),
                    },
                    a,
                    &plan_proof,
                )?;
                self.budgets
                    .insert(proposal.project_id.clone(), proposal.budget.clone());
                let baseline = self
                    .business
                    .baselines
                    .get_mut(&proposal.project_id)
                    .unwrap();
                baseline.plan_version = old.plan_version + 1;
                let change = self.business.closeout.changes.get_mut(change_id).unwrap();
                change.status = ChangeStatus::Implementing;
                change.published_version = Some(new_version);
            }
            BusinessCloseoutAction::VerifyChange {
                change_id,
                evidence_refs,
            } => {
                verified(evidence_refs, p)?;
                let change = self
                    .business
                    .closeout
                    .changes
                    .get_mut(change_id)
                    .ok_or("business_change_missing")?;
                require(
                    change.status == ChangeStatus::Implementing
                        && change.published_version
                            == self
                                .business
                                .baselines
                                .get(&change.proposal.project_id)
                                .map(|b| b.version),
                    "business_change_not_current",
                )?;
                change.status = ChangeStatus::Verified;
            }
            BusinessCloseoutAction::PlanMeasurement { plan } => {
                require(
                    p.business.actor_is_human && plan.owner == a.actor_id,
                    "business_human_measurement_owner_required",
                )?;
                for s in [&plan.plan_id, &plan.dataset_id, &plan.method] {
                    nonempty(s)?;
                }
                let objective = self
                    .objectives
                    .get(&plan.objective_id)
                    .ok_or("business_objective_missing")?;
                let project = self
                    .projects
                    .get(&plan.project_id)
                    .ok_or("business_project_missing")?;
                require(
                    project.objective_refs.contains(&plan.objective_id)
                        && plan.metric == objective.metric
                        && plan.unit == objective.unit
                        && plan.owner == objective.owner_principal_id
                        && plan.window_start == objective.period_start
                        && plan.window_end == objective.period_end
                        && a.now_ms <= plan.window_start
                        && (1..=10000).contains(&plan.minimum_samples),
                    "business_measurement_plan_invalid_or_late",
                )?;
                require(
                    !self
                        .business
                        .closeout
                        .measurement_plans
                        .contains_key(&plan.plan_id)
                        && !self.business.closeout.measurement_plans.values().any(|p| {
                            p.objective_id == plan.objective_id && p.project_id == plan.project_id
                        }),
                    "business_measurement_plan_already_frozen",
                )?;
                self.business
                    .closeout
                    .measurement_plans
                    .insert(plan.plan_id.clone(), plan.clone());
            }
            BusinessCloseoutAction::ObserveMetric {
                plan_id,
                artifact_ref,
            } => {
                verified(&[artifact_ref.clone()], p)?;
                let plan = self
                    .business
                    .closeout
                    .measurement_plans
                    .get(plan_id)
                    .ok_or("business_measurement_plan_missing")?;
                let data = proof
                    .metric
                    .as_ref()
                    .ok_or("business_metric_artifact_invalid")?;
                require(
                    data.schema == "kiana.company-metric-observation.v2"
                        && data.dataset_id == plan.dataset_id
                        && data.metric == plan.metric
                        && data.unit == plan.unit
                        && data.fixture == plan.fixture
                        && data.value.is_finite()
                        && data.observed_at >= plan.window_start
                        && data.observed_at <= plan.window_end
                        && data.observed_at <= a.now_ms,
                    "business_metric_observation_invalid",
                )?;
                require(
                    !self
                        .business
                        .closeout
                        .observations
                        .contains_key(&data.observation_id)
                        && self
                            .business
                            .closeout
                            .observations
                            .values()
                            .filter(|o| o.plan_id == *plan_id)
                            .count()
                            < 10000
                        && !self.business.closeout.observations.values().any(|o| {
                            o.plan_id == *plan_id && o.data.observed_at == data.observed_at
                        }),
                    "business_metric_duplicate_or_limit",
                )?;
                self.business.closeout.observations.insert(
                    data.observation_id.clone(),
                    BusinessMetricObservation {
                        plan_id: plan_id.clone(),
                        data: data.clone(),
                        artifact_ref: artifact_ref.clone(),
                        content_hash: proof
                            .metric_hash
                            .clone()
                            .ok_or("business_metric_hash_missing")?,
                        recorded_at: a.now_ms,
                    },
                );
            }
            BusinessCloseoutAction::AssessOutcome {
                plan_id,
                assessment_id,
            } => {
                nonempty(assessment_id)?;
                require(
                    !self
                        .business
                        .closeout
                        .assessments
                        .contains_key(assessment_id),
                    "business_outcome_assessment_exists",
                )?;
                let plan = self
                    .business
                    .closeout
                    .measurement_plans
                    .get(plan_id)
                    .ok_or("business_measurement_plan_missing")?;
                require(
                    a.now_ms > plan.window_end
                        && self
                            .business
                            .closeout
                            .receipts
                            .values()
                            .any(|r| r.project_id == plan.project_id),
                    "business_outcome_window_or_closure_incomplete",
                )?;
                let objective = self
                    .objectives
                    .get(&plan.objective_id)
                    .ok_or("business_objective_missing")?
                    .clone();
                let mut observations = self
                    .business
                    .closeout
                    .observations
                    .values()
                    .filter(|o| o.plan_id == *plan_id)
                    .collect::<Vec<_>>();
                observations.sort_by_key(|o| (o.data.observed_at, &o.data.observation_id));
                let missing = observations.len() < plan.minimum_samples as usize;
                let aggregate = if missing {
                    None
                } else {
                    Some(match plan.aggregation {
                        MeasurementAggregation::Mean => observations
                            .iter()
                            .map(|o| o.data.value / observations.len() as f64)
                            .sum(),
                        MeasurementAggregation::Minimum => observations
                            .iter()
                            .map(|o| o.data.value)
                            .fold(f64::INFINITY, f64::min),
                        MeasurementAggregation::Maximum => observations
                            .iter()
                            .map(|o| o.data.value)
                            .fold(f64::NEG_INFINITY, f64::max),
                        MeasurementAggregation::Latest => observations.last().unwrap().data.value,
                    })
                };
                require(
                    aggregate.is_none_or(f64::is_finite),
                    "business_outcome_nonfinite",
                )?;
                let status = aggregate.map(|v| {
                    if objective.target_met(v) {
                        OutcomeStatus::Realized
                    } else if match objective.direction {
                        MetricDirection::AtLeast => v > objective.baseline,
                        MetricDirection::AtMost => v < objective.baseline,
                    } {
                        OutcomeStatus::PartiallyRealized
                    } else {
                        OutcomeStatus::NotRealized
                    }
                });
                self.business.closeout.assessments.insert(
                    assessment_id.clone(),
                    BusinessOutcomeAssessment {
                        assessment_id: assessment_id.clone(),
                        plan_id: plan_id.clone(),
                        target: objective,
                        observation_ids: observations
                            .iter()
                            .map(|o| o.data.observation_id.clone())
                            .collect(),
                        aggregate,
                        status,
                        missing_data: missing,
                        fixture: plan.fixture,
                        evidence_refs: observations
                            .iter()
                            .map(|o| o.artifact_ref.clone())
                            .collect(),
                        assessed_at: a.now_ms,
                    },
                );
            }
            BusinessCloseoutAction::AchieveObjective {
                objective_id,
                assessment_id,
                decision_ref,
            } => {
                verified(&[decision_ref.clone()], p)?;
                require(
                    p.business.actor_is_human,
                    "business_human_objective_decision_required",
                )?;
                let assessment = self
                    .business
                    .closeout
                    .assessments
                    .get(assessment_id)
                    .ok_or("business_assessment_missing")?;
                let plan = self
                    .business
                    .closeout
                    .measurement_plans
                    .get(&assessment.plan_id)
                    .ok_or("business_measurement_plan_missing")?;
                require(
                    assessment.target.objective_id == *objective_id
                        && assessment.status == Some(OutcomeStatus::Realized)
                        && !assessment.fixture
                        && !assessment.missing_data
                        && assessment.observation_ids.len()
                            == self
                                .business
                                .closeout
                                .observations
                                .values()
                                .filter(|o| o.plan_id == assessment.plan_id)
                                .count(),
                    "business_objective_current_realized_observations_required",
                )?;
                let objective = self
                    .objectives
                    .get_mut(objective_id)
                    .ok_or("business_objective_missing")?;
                require(
                    objective.owner_principal_id == a.actor_id
                        && objective.metric == plan.metric
                        && objective.target == assessment.target.target
                        && objective.version == assessment.target.version,
                    "business_objective_owner_or_target_stale",
                )?;
                objective.status = objective.status.transition(ObjectiveStatus::Achieved)?;
                objective.version += 1;
            }
        }
        Ok(())
    }
}
