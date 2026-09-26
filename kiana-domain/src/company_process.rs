//! Deterministic CompanyOS process planner over the existing Workflow planner.
//!
//! This is a typed business adapter, not an executor.  It consumes committed Company facts and
//! emits a stable intent for the existing ControlPlane/workflow path.  It never parses model text,
//! calls a Broker/Runner, or completes a gate merely because a model said it did.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const COMPANY_PROCESS_SCHEMA: &str = "kiana.company-process.v1";
pub const COMPANY_PROCESS_TEMPLATE_SCHEMA: &str = "kiana.company-process-template.v1";
pub const COMPANY_PROCESS_INTENT_SCHEMA: &str = "kiana.company-process-intent.v1";
pub const COMPANY_PROCESS_TEMPLATE_ID: &str = "company.default.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyProcessNode {
    Intake,
    Charter,
    Plan,
    Handoff,
    Build,
    Verify,
    HumanApproval,
    Deliver,
    Close,
    Complete,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompanyProcessEvent {
    IntakeAccepted {
        evidence_refs: Vec<String>,
    },
    CharterApproved {
        evidence_refs: Vec<String>,
    },
    PlanApproved {
        evidence_refs: Vec<String>,
    },
    HandoffAcknowledged {
        evidence_refs: Vec<String>,
    },
    RunCompleted {
        evidence_refs: Vec<String>,
    },
    ReviewAccepted {
        evidence_refs: Vec<String>,
    },
    SponsorDecision {
        approved: bool,
        evidence_refs: Vec<String>,
    },
    DeliveryConfirmed {
        evidence_refs: Vec<String>,
    },
    IncidentOpened {
        evidence_refs: Vec<String>,
    },
    IncidentResolved {
        evidence_refs: Vec<String>,
    },
}

impl CompanyProcessEvent {
    fn evidence_refs(&self) -> &[String] {
        match self {
            Self::IntakeAccepted { evidence_refs }
            | Self::CharterApproved { evidence_refs }
            | Self::PlanApproved { evidence_refs }
            | Self::HandoffAcknowledged { evidence_refs }
            | Self::RunCompleted { evidence_refs }
            | Self::ReviewAccepted { evidence_refs }
            | Self::SponsorDecision { evidence_refs, .. }
            | Self::DeliveryConfirmed { evidence_refs }
            | Self::IncidentOpened { evidence_refs }
            | Self::IncidentResolved { evidence_refs } => evidence_refs,
        }
    }

    fn validate_refs(&self) -> Result<(), &'static str> {
        if self.evidence_refs().iter().any(|reference| {
            reference.trim().is_empty()
                || reference.len() > 4_096
                || reference.contains('\0')
                || reference.to_ascii_lowercase().contains("private")
        }) {
            return Err("company_process_evidence_ref_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyProcessTemplate {
    pub schema: String,
    pub template_id: String,
    pub version: u64,
    pub role_order: Vec<String>,
    pub template_hash: String,
}

impl CompanyProcessTemplate {
    pub fn default_v1() -> Self {
        let mut template = Self {
            schema: COMPANY_PROCESS_TEMPLATE_SCHEMA.to_owned(),
            template_id: COMPANY_PROCESS_TEMPLATE_ID.to_owned(),
            version: 1,
            role_order: vec![
                "sponsor".to_owned(),
                "pm".to_owned(),
                "builder".to_owned(),
                "reviewer".to_owned(),
                "closer".to_owned(),
            ],
            template_hash: String::new(),
        };
        template.template_hash = template.canonical_hash();
        template
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PROCESS_TEMPLATE_SCHEMA
            || self.template_id != COMPANY_PROCESS_TEMPLATE_ID
            || self.version != 1
            || self.role_order != CompanyProcessTemplate::default_v1().role_order
            || self.template_hash != self.canonical_hash()
        {
            return Err("company_process_template_drift");
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "template_id": self.template_id,
            "version": self.version,
            "role_order": self.role_order,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyProcessState {
    pub schema: String,
    pub process_id: String,
    pub project_id: String,
    pub template_id: String,
    pub template_version: u64,
    pub template_hash: String,
    pub node: CompanyProcessNode,
    pub revision: u64,
    pub assignment_ref: String,
    pub workflow_instance_id: Option<String>,
    pub consumed_fact_digests: Vec<String>,
    pub last_fact_digest: Option<String>,
    pub digest: String,
}

impl CompanyProcessState {
    pub fn new(
        process_id: impl Into<String>,
        project_id: impl Into<String>,
        assignment_ref: impl Into<String>,
        workflow_instance_id: Option<String>,
        template: &CompanyProcessTemplate,
    ) -> Result<Self, &'static str> {
        template.validate()?;
        let mut state = Self {
            schema: COMPANY_PROCESS_SCHEMA.to_owned(),
            process_id: process_id.into(),
            project_id: project_id.into(),
            template_id: template.template_id.clone(),
            template_version: template.version,
            template_hash: template.template_hash.clone(),
            node: CompanyProcessNode::Intake,
            revision: 0,
            assignment_ref: assignment_ref.into(),
            workflow_instance_id,
            consumed_fact_digests: Vec::new(),
            last_fact_digest: None,
            digest: String::new(),
        };
        state.digest = state.canonical_digest();
        state.validate(template)?;
        Ok(state)
    }

    pub fn validate(&self, template: &CompanyProcessTemplate) -> Result<(), &'static str> {
        template.validate()?;
        if self.schema != COMPANY_PROCESS_SCHEMA
            || self.template_id != template.template_id
            || self.template_version != template.version
            || self.template_hash != template.template_hash
            || self.revision == u64::MAX
        {
            return Err("company_process_state_template_mismatch");
        }
        required(&self.process_id, "company_process_id_required")?;
        required(&self.project_id, "company_process_project_required")?;
        required(&self.assignment_ref, "company_process_assignment_required")?;
        if self
            .consumed_fact_digests
            .iter()
            .any(|digest| !digest.starts_with("sha256:") || digest.len() != 71)
        {
            return Err("company_process_fact_digest_invalid");
        }
        if self.digest != self.canonical_digest() {
            return Err("company_process_state_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "process_id": self.process_id,
            "project_id": self.project_id,
            "template_id": self.template_id,
            "template_version": self.template_version,
            "template_hash": self.template_hash,
            "node": self.node,
            "revision": self.revision,
            "assignment_ref": self.assignment_ref,
            "workflow_instance_id": self.workflow_instance_id,
            "consumed_fact_digests": self.consumed_fact_digests,
            "last_fact_digest": self.last_fact_digest,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyProcessIntent {
    pub schema: String,
    pub intent_id: String,
    pub process_id: String,
    pub expected_revision: u64,
    pub next_revision: u64,
    pub from_node: CompanyProcessNode,
    pub to_node: CompanyProcessNode,
    pub role_id: String,
    pub assignment_ref: String,
    pub human_task: bool,
    pub workflow_instance_id: Option<String>,
    pub fact_digest: String,
    pub input_refs: Vec<String>,
    pub digest: String,
}

impl CompanyProcessIntent {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PROCESS_INTENT_SCHEMA
            || self.next_revision != self.expected_revision.saturating_add(1)
            || self.intent_id.trim().is_empty()
            || self.process_id.trim().is_empty()
            || self.assignment_ref.trim().is_empty()
            || self.fact_digest.len() != 71
            || !self.fact_digest.starts_with("sha256:")
            || self.digest != self.canonical_digest()
        {
            return Err("company_process_intent_invalid");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "intent_id": self.intent_id,
            "process_id": self.process_id,
            "expected_revision": self.expected_revision,
            "next_revision": self.next_revision,
            "from_node": self.from_node,
            "to_node": self.to_node,
            "role_id": self.role_id,
            "assignment_ref": self.assignment_ref,
            "human_task": self.human_task,
            "workflow_instance_id": self.workflow_instance_id,
            "fact_digest": self.fact_digest,
            "input_refs": self.input_refs,
        }))
    }
}

fn transition(
    node: CompanyProcessNode,
    event: &CompanyProcessEvent,
) -> Result<(CompanyProcessNode, &'static str, bool), &'static str> {
    use CompanyProcessEvent::*;
    use CompanyProcessNode::*;
    match (node, event) {
        (Intake, IntakeAccepted { .. }) => Ok((Charter, "sponsor", false)),
        (Charter, CharterApproved { .. }) => Ok((Plan, "pm", false)),
        (Plan, PlanApproved { .. }) => Ok((Handoff, "pm", false)),
        (Handoff, HandoffAcknowledged { .. }) => Ok((Build, "builder", false)),
        (Build, RunCompleted { .. }) => Ok((Verify, "reviewer", false)),
        (Verify, ReviewAccepted { .. }) => Ok((HumanApproval, "sponsor", true)),
        (HumanApproval, SponsorDecision { approved: true, .. }) => Ok((Deliver, "closer", false)),
        (
            HumanApproval,
            SponsorDecision {
                approved: false, ..
            },
        ) => Ok((Blocked, "sponsor", true)),
        (Deliver, DeliveryConfirmed { .. }) => Ok((Close, "closer", false)),
        (Close, ReviewAccepted { .. }) => Ok((Complete, "closer", false)),
        (Blocked, IncidentResolved { .. }) => Ok((HumanApproval, "sponsor", true)),
        (_, IncidentOpened { .. }) => Ok((Blocked, "sponsor", true)),
        _ => Err("company_process_gate_not_open"),
    }
}

/// Plan and apply one typed Company fact. Duplicate facts are replay no-ops and never emit a new
/// intent; all other failures leave the input state unchanged.
pub fn plan_company_process(
    state: &CompanyProcessState,
    template: &CompanyProcessTemplate,
    event: &CompanyProcessEvent,
) -> Result<(CompanyProcessState, Option<CompanyProcessIntent>), &'static str> {
    state.validate(template)?;
    event.validate_refs()?;
    let fact_digest = json_digest(&json!(event));
    if state.consumed_fact_digests.contains(&fact_digest) {
        return Ok((state.clone(), None));
    }
    let (to_node, role_id, human_task) = transition(state.node, event)?;
    let next_revision = state
        .revision
        .checked_add(1)
        .ok_or("company_process_revision_exhausted")?;
    let intent_id = format!(
        "company-intent:{}",
        json_digest(&json!({
            "process_id": state.process_id,
            "revision": next_revision,
            "fact_digest": fact_digest,
            "to_node": to_node,
        }))
    );
    let mut intent = CompanyProcessIntent {
        schema: COMPANY_PROCESS_INTENT_SCHEMA.to_owned(),
        intent_id,
        process_id: state.process_id.clone(),
        expected_revision: state.revision,
        next_revision,
        from_node: state.node,
        to_node,
        role_id: role_id.to_owned(),
        assignment_ref: state.assignment_ref.clone(),
        human_task,
        workflow_instance_id: state.workflow_instance_id.clone(),
        fact_digest: fact_digest.clone(),
        input_refs: event.evidence_refs().to_vec(),
        digest: String::new(),
    };
    intent.digest = intent.canonical_digest();
    intent.validate()?;
    let mut next = state.clone();
    next.node = to_node;
    next.revision = next_revision;
    next.last_fact_digest = Some(fact_digest.clone());
    next.consumed_fact_digests.push(fact_digest);
    let mut seen = BTreeSet::new();
    if next
        .consumed_fact_digests
        .iter()
        .any(|digest| !seen.insert(digest.clone()))
    {
        return Err("company_process_fact_duplicate");
    }
    next.digest = next.canonical_digest();
    next.validate(template)?;
    Ok((next, Some(intent)))
}
