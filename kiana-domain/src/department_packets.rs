//! Versioned, non-executing department work contracts.
//!
//! A department packet describes one bounded unit of department work: intake investigation,
//! initiative assessment, Charter/Plan drafting, implementation, verification, delivery
//! preparation or knowledge capture. It binds the responsible department, the appointed target
//! role, a typed input basis and a result contract, and it is admitted against the
//! server-derived approval stage of its project.
//!
//! The contract never carries a Grant, Lease, budget, path grant or any other self-effective
//! runtime authority: those stay derived by the ControlPlane. The legacy Builder `WorkPacket`
//! remains the execution adapter and keeps its `packet_role_must_be_builder` semantics.
use crate::{json_digest, normalize_role_path, FrozenPlanGraph, RoleSpec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const DEPARTMENT_PACKET_SCHEMA: &str = "kiana.department-packet.v1";
pub const RESULT_CONTRACT_SCHEMA: &str = "kiana.result-contract.v1";
/// Server-owned approval-stage facts of one project, read by the packet admission.
pub const DEPARTMENT_STAGE_FACTS_SCHEMA: &str = "kiana.department-stage-facts.v1";
/// Server-owned target-role appointment facts; the CO-05 assignment maps into this view.
pub const DEPARTMENT_ROLE_APPOINTMENT_SCHEMA: &str = "kiana.department-role-appointment.v1";
/// Evidence value returned by an admitted department packet; it conveys no authority.
pub const DEPARTMENT_PACKET_ADMISSION_SCHEMA: &str = "kiana.department-packet-admission.v1";

fn required(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("department_packet_field_required_or_too_large")
    } else {
        Ok(())
    }
}

fn digest(value: &str, reason: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(reason);
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(reason);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepartmentPacketKind {
    IntakeAnalysis,
    InitiativeAssessment,
    CharterDraft,
    PlanDraft,
    Implementation,
    Verification,
    DeliveryPreparation,
    KnowledgeCapture,
}

impl DepartmentPacketKind {
    /// Registered role that owns this work kind.
    pub fn role_id(self) -> &'static str {
        match self {
            Self::IntakeAnalysis | Self::InitiativeAssessment => "analyst",
            Self::CharterDraft => "sponsor",
            Self::PlanDraft => "pm",
            Self::Implementation => "builder",
            Self::Verification => "qa",
            Self::DeliveryPreparation => "closer",
            Self::KnowledgeCapture => "librarian",
        }
    }

    /// Responsible department of this work kind; the packet cannot name a different one.
    pub fn department_id(self) -> &'static str {
        match self {
            Self::IntakeAnalysis | Self::InitiativeAssessment | Self::CharterDraft => "initiating",
            Self::PlanDraft => "planning",
            Self::Implementation => "executing",
            Self::Verification => "monitoring",
            Self::DeliveryPreparation | Self::KnowledgeCapture => "closing",
        }
    }

    /// Versioned output schema of this work kind.
    pub fn output_schema(self) -> &'static str {
        match self {
            Self::IntakeAnalysis => "kiana.analysis-result.v1",
            Self::InitiativeAssessment => "kiana.assessment-result.v1",
            Self::CharterDraft => "kiana.charter-draft-result.v1",
            Self::PlanDraft => "kiana.plan-draft-result.v1",
            Self::Implementation => "kiana.implementation-result.v1",
            Self::Verification => "kiana.verification-result.v1",
            Self::DeliveryPreparation => "kiana.delivery-preparation-result.v1",
            Self::KnowledgeCapture => "kiana.knowledge-capture-result.v1",
        }
    }

    /// Which approved basis a work kind may start from.
    fn allowed_input(self, basis: PacketInputBasis) -> bool {
        match self {
            Self::IntakeAnalysis => basis == PacketInputBasis::Intake,
            Self::InitiativeAssessment => basis == PacketInputBasis::Initiative,
            Self::CharterDraft => matches!(
                basis,
                PacketInputBasis::Intake | PacketInputBasis::Initiative
            ),
            Self::PlanDraft => basis == PacketInputBasis::Charter,
            Self::Implementation
            | Self::Verification
            | Self::DeliveryPreparation
            | Self::KnowledgeCapture => basis == PacketInputBasis::Plan,
        }
    }

    /// Controlled proposal/source prefixes of this work kind. Only an implementation packet may
    /// name a workspace path; every other kind stays inside its proposal directory.
    fn write_prefixes(self) -> &'static [&'static str] {
        match self {
            Self::IntakeAnalysis | Self::InitiativeAssessment => &[],
            Self::CharterDraft => &["charter/"],
            Self::PlanDraft => &["plan/", "packet/"],
            Self::Implementation => &["."],
            Self::Verification => &["gate/"],
            Self::DeliveryPreparation => &["receipt/"],
            Self::KnowledgeCapture => &["lessons/"],
        }
    }
}

/// Which approval stage the packet's input material comes from.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketInputBasis {
    Intake,
    Initiative,
    Charter,
    Plan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultContract {
    pub schema: String,
    pub version: u64,
    pub output_schema: String,
    pub required_refs: Vec<String>,
    pub max_bytes: u64,
}

impl ResultContract {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != RESULT_CONTRACT_SCHEMA || self.version == 0 || self.max_bytes == 0 {
            return Err("result_contract_invalid");
        }
        required(&self.output_schema)?;
        if self
            .required_refs
            .iter()
            .any(|reference| required(reference).is_err())
        {
            return Err("result_contract_reference_invalid");
        }
        Ok(())
    }
}

/// The exact approved Plan revision an implementation (or later) packet executes.
///
/// The binding is compared against the server-side frozen Plan graph, so a caller cannot execute
/// against a plan revision the Company aggregate never admitted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentPlanBinding {
    pub plan_version: u64,
    pub plan_digest: String,
    pub charter_baseline_version: u64,
}

impl DepartmentPlanBinding {
    fn validate(&self) -> Result<(), &'static str> {
        if self.plan_version == 0 || self.charter_baseline_version == 0 {
            return Err("department_packet_plan_binding_invalid");
        }
        digest(&self.plan_digest, "department_packet_plan_binding_invalid")
    }
}

/// Server-owned approval-stage facts of one project.
///
/// The admission reads this projection instead of trusting the packet author's claims about
/// which approval stage exists, which is what removes the "a plan is needed before a plan can be
/// drafted" cycle: intake-basis analysis is admitted as soon as the intake is recorded, while a
/// Plan-basis implementation packet still needs a frozen Plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentStageFacts {
    pub schema: String,
    pub project_id: String,
    /// The intake was authorized: the project exists and its intake record is present.
    pub intake_authorized: bool,
    /// An Initiative is in the assessment lane, or one converted into this project.
    pub initiative_in_assessment: bool,
    /// Approved Charter baseline version; 0 means no approved Charter exists yet.
    pub charter_baseline_version: u64,
    /// Approved Plan revision (CO-12 frozen graph), when one exists.
    pub frozen_plan: Option<FrozenPlanGraph>,
}

impl DepartmentStageFacts {
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            schema: DEPARTMENT_STAGE_FACTS_SCHEMA.to_owned(),
            project_id: project_id.into(),
            intake_authorized: false,
            initiative_in_assessment: false,
            charter_baseline_version: 0,
            frozen_plan: None,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DEPARTMENT_STAGE_FACTS_SCHEMA || self.project_id.trim().is_empty() {
            return Err("department_stage_facts_invalid");
        }
        if let Some(plan) = self.frozen_plan.as_ref() {
            plan.validate()?;
            if plan.project_id != self.project_id {
                return Err("department_stage_facts_invalid");
            }
        }
        Ok(())
    }
}

/// Server-owned target-role appointment facts.
///
/// The Company aggregate maps its admitted assignment record into this view; a packet can never
/// bring its own appointment, and an inactive or mismatched appointment refuses the packet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentRoleAppointment {
    pub schema: String,
    pub assignment_id: String,
    pub project_id: String,
    pub role_id: String,
    pub department_id: String,
    pub owner_principal_id: String,
    pub expires_at_unix_ms: u64,
    pub revoked: bool,
    pub assignment_version: u64,
    pub appointment_digest: String,
}

impl DepartmentRoleAppointment {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        assignment_id: impl Into<String>,
        project_id: impl Into<String>,
        role_id: impl Into<String>,
        owner_principal_id: impl Into<String>,
        expires_at_unix_ms: u64,
        revoked: bool,
        assignment_version: u64,
    ) -> Result<Self, &'static str> {
        let role_id = role_id.into();
        let role = RoleSpec::lookup(&role_id).ok_or("department_appointment_role_unknown")?;
        let mut appointment = Self {
            schema: DEPARTMENT_ROLE_APPOINTMENT_SCHEMA.to_owned(),
            assignment_id: assignment_id.into(),
            project_id: project_id.into(),
            role_id,
            department_id: role.department_id.clone(),
            owner_principal_id: owner_principal_id.into(),
            expires_at_unix_ms,
            revoked,
            assignment_version,
            appointment_digest: String::new(),
        };
        appointment.appointment_digest = appointment.canonical_digest();
        appointment.validate()?;
        Ok(appointment)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DEPARTMENT_ROLE_APPOINTMENT_SCHEMA
            || self.expires_at_unix_ms == 0
            || self.assignment_version == 0
            || self.assignment_id.trim().is_empty()
            || self.project_id.trim().is_empty()
            || self.owner_principal_id.trim().is_empty()
        {
            return Err("department_appointment_invalid");
        }
        let role = RoleSpec::lookup(&self.role_id).ok_or("department_appointment_role_unknown")?;
        if role.department_id != self.department_id {
            return Err("department_appointment_department_mismatch");
        }
        digest(
            &self.appointment_digest,
            "department_appointment_digest_invalid",
        )?;
        if self.appointment_digest != self.canonical_digest() {
            return Err("department_appointment_digest_mismatch");
        }
        Ok(())
    }

    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        !self.revoked && now_unix_ms < self.expires_at_unix_ms
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "assignment_id": self.assignment_id,
            "project_id": self.project_id,
            "role_id": self.role_id,
            "department_id": self.department_id,
            "owner_principal_id": self.owner_principal_id,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "revoked": self.revoked,
            "assignment_version": self.assignment_version,
        }))
    }
}

/// Evidence value of one admitted department packet.
///
/// It records the packet identity, the appointment and the approved basis the admission was
/// checked against. It never conveys capability, path, budget or lease authority
/// (`runtime_authority` is always false); runtime authority is still derived by ControlPlane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentPacketAdmission {
    pub schema: String,
    pub packet_id: String,
    pub packet_digest: String,
    pub project_id: String,
    pub kind: DepartmentPacketKind,
    pub from_department: String,
    pub target_role: String,
    pub assignment_ref: String,
    pub assignment_version: u64,
    pub input_basis: PacketInputBasis,
    pub charter_baseline_version: u64,
    pub plan_digest: Option<String>,
    pub plan_node_ref: Option<String>,
    /// Always false: the business contract can never smuggle runtime authority.
    pub runtime_authority: bool,
    pub admission_digest: String,
}

impl DepartmentPacketAdmission {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DEPARTMENT_PACKET_ADMISSION_SCHEMA
            || self.runtime_authority
            || self.assignment_version == 0
            || self.packet_id.trim().is_empty()
            || self.project_id.trim().is_empty()
            || self.target_role.trim().is_empty()
            || self.assignment_ref.trim().is_empty()
            || self.from_department != self.kind.department_id()
        {
            return Err("department_packet_admission_invalid");
        }
        digest(&self.packet_digest, "department_packet_admission_invalid")?;
        if let Some(plan) = self.plan_digest.as_deref() {
            digest(plan, "department_packet_admission_invalid")?;
        }
        if self.plan_digest.is_some() != self.plan_node_ref.is_some() {
            return Err("department_packet_admission_invalid");
        }
        if self.admission_digest != self.canonical_digest() {
            return Err("department_packet_admission_invalid");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "packet_id": self.packet_id,
            "packet_digest": self.packet_digest,
            "project_id": self.project_id,
            "kind": self.kind,
            "from_department": self.from_department,
            "target_role": self.target_role,
            "assignment_ref": self.assignment_ref,
            "assignment_version": self.assignment_version,
            "input_basis": self.input_basis,
            "charter_baseline_version": self.charter_baseline_version,
            "plan_digest": self.plan_digest,
            "plan_node_ref": self.plan_node_ref,
            "runtime_authority": self.runtime_authority,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentPacket {
    pub schema: String,
    pub packet_id: String,
    pub project_id: String,
    pub version: u64,
    pub kind: DepartmentPacketKind,
    pub from_department: String,
    pub target_role: String,
    pub assignment_ref: String,
    pub input_basis: PacketInputBasis,
    pub input_refs: Vec<String>,
    pub result: ResultContract,
    #[serde(default)]
    pub write_scope: Vec<String>,
    /// Charter baseline a Charter-basis draft plans against.
    #[serde(default)]
    pub charter_baseline_version: u64,
    /// Stable plan node this packet executes; required for every Plan-basis kind.
    #[serde(default)]
    pub plan_ref: Option<String>,
    /// Exact approved Plan revision; required for every Plan-basis kind.
    #[serde(default)]
    pub plan_binding: Option<DepartmentPlanBinding>,
    /// Explicit compatibility fields make an attempted capability smuggling visible and
    /// rejectable instead of silently ignoring a caller-provided grant or lease.
    #[serde(default)]
    pub runtime_grant: Option<String>,
    #[serde(default)]
    pub budget_lease: Option<String>,
}

impl DepartmentPacket {
    /// Structural contract of one department packet: versioned kind, responsible department,
    /// registered target role, output schema of the kind, typed input basis, controlled write
    /// scope and the Plan binding shape. No approval stage is read here; that is [`Self::admit`].
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DEPARTMENT_PACKET_SCHEMA || self.version == 0 {
            return Err("department_packet_schema_or_version_invalid");
        }
        for value in [
            &self.packet_id,
            &self.project_id,
            &self.from_department,
            &self.target_role,
            &self.assignment_ref,
        ] {
            required(value)?;
        }
        if self.input_refs.is_empty()
            || self
                .input_refs
                .iter()
                .any(|reference| required(reference).is_err())
        {
            return Err("department_packet_input_refs_required");
        }
        self.result.validate()?;
        if self.result.output_schema != self.kind.output_schema() {
            return Err("department_packet_output_schema_mismatch");
        }
        if self.runtime_grant.is_some() || self.budget_lease.is_some() {
            return Err("department_packet_runtime_authority_forbidden");
        }
        let role = RoleSpec::lookup(&self.target_role).ok_or("department_packet_role_unknown")?;
        if role.role_id != self.kind.role_id()
            || role.department_id != self.from_department
            || self.from_department != self.kind.department_id()
        {
            return Err("department_packet_role_department_mismatch");
        }
        if !self.kind.allowed_input(self.input_basis) {
            return Err("department_packet_input_basis_invalid");
        }
        if self.input_basis == PacketInputBasis::Plan {
            if self
                .plan_ref
                .as_deref()
                .is_none_or(|reference| required(reference).is_err())
            {
                return Err("department_packet_plan_required");
            }
            let binding = self
                .plan_binding
                .as_ref()
                .ok_or("department_packet_plan_required")?;
            binding.validate()?;
        } else {
            if self.charter_baseline_version != 0
                || self.plan_ref.is_some()
                || self.plan_binding.is_some()
            {
                return Err("department_packet_plan_reference_mismatch");
            }
            if self.input_basis == PacketInputBasis::Charter && self.charter_baseline_version == 0 {
                return Err("department_packet_charter_baseline_required");
            }
        }
        if self.kind == DepartmentPacketKind::Implementation && self.write_scope.is_empty() {
            return Err("department_packet_write_scope_required");
        }
        let mut paths = BTreeSet::new();
        for path in &self.write_scope {
            let normalized =
                normalize_role_path(path).ok_or("department_packet_write_scope_invalid")?;
            if !paths.insert(normalized.clone()) {
                return Err("department_packet_write_scope_duplicate");
            }
            if !self.kind.write_prefixes().iter().any(|prefix| {
                *prefix == "." || normalized == *prefix || normalized.starts_with(prefix)
            }) {
                return Err("department_packet_write_scope_denied");
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "packet_id": self.packet_id,
            "project_id": self.project_id,
            "version": self.version,
            "kind": self.kind,
            "from_department": self.from_department,
            "target_role": self.target_role,
            "assignment_ref": self.assignment_ref,
            "input_basis": self.input_basis,
            "input_refs": self.input_refs,
            "result": self.result,
            "write_scope": self.write_scope,
            "charter_baseline_version": self.charter_baseline_version,
            "plan_ref": self.plan_ref,
            "plan_binding": self.plan_binding,
            "runtime_grant": self.runtime_grant,
            "budget_lease": self.budget_lease,
        }))
    }

    /// Admit this work contract against the server-derived approval stage and the appointed
    /// target role.
    ///
    /// Refusal order is stable: structural contract, stage project identity, appointment
    /// identity, appointment activity, then the input-basis gate (intake, initiative, approved
    /// Charter with matching baseline, frozen Plan with matching node/version/digest/baseline).
    /// Admission returns evidence only; it grants no capability, path, budget or lease.
    pub fn admit(
        &self,
        stage: &DepartmentStageFacts,
        appointment: &DepartmentRoleAppointment,
        now_unix_ms: u64,
    ) -> Result<DepartmentPacketAdmission, &'static str> {
        self.validate()?;
        stage.validate()?;
        appointment.validate()?;
        if stage.project_id != self.project_id {
            return Err("department_packet_stage_project_mismatch");
        }
        if appointment.assignment_id != self.assignment_ref
            || appointment.project_id != self.project_id
            || appointment.role_id != self.target_role
            || appointment.department_id != self.kind.department_id()
        {
            return Err("department_packet_appointment_mismatch");
        }
        if !appointment.active_at(now_unix_ms) {
            return Err("department_packet_appointment_inactive");
        }
        match self.input_basis {
            PacketInputBasis::Intake => {
                if !stage.intake_authorized {
                    return Err("department_packet_stage_not_admitted");
                }
            }
            PacketInputBasis::Initiative => {
                if !stage.initiative_in_assessment {
                    return Err("department_packet_stage_not_admitted");
                }
            }
            PacketInputBasis::Charter => {
                if stage.charter_baseline_version == 0 {
                    return Err("department_packet_charter_not_approved");
                }
                if stage.charter_baseline_version != self.charter_baseline_version {
                    return Err("department_packet_charter_baseline_mismatch");
                }
            }
            PacketInputBasis::Plan => {
                let frozen = stage
                    .frozen_plan
                    .as_ref()
                    .ok_or("department_packet_plan_not_approved")?;
                let binding = self
                    .plan_binding
                    .as_ref()
                    .ok_or("department_packet_plan_required")?;
                if binding.plan_version != frozen.plan_version {
                    return Err("department_packet_plan_version_mismatch");
                }
                if frozen.charter_baseline_version != stage.charter_baseline_version
                    || binding.charter_baseline_version != frozen.charter_baseline_version
                {
                    return Err("department_packet_plan_baseline_mismatch");
                }
                if binding.plan_digest != frozen.plan_digest {
                    return Err("department_packet_plan_mismatch");
                }
                let node = self.plan_ref.as_deref().unwrap_or_default();
                if !frozen.node_versions.contains_key(node) {
                    return Err("department_packet_plan_node_unknown");
                }
            }
        }
        let mut admission = DepartmentPacketAdmission {
            schema: DEPARTMENT_PACKET_ADMISSION_SCHEMA.to_owned(),
            packet_id: self.packet_id.clone(),
            packet_digest: self.digest(),
            project_id: self.project_id.clone(),
            kind: self.kind,
            from_department: self.from_department.clone(),
            target_role: self.target_role.clone(),
            assignment_ref: self.assignment_ref.clone(),
            assignment_version: appointment.assignment_version,
            input_basis: self.input_basis,
            charter_baseline_version: self.charter_baseline_version,
            plan_digest: self
                .plan_binding
                .as_ref()
                .map(|binding| binding.plan_digest.clone()),
            plan_node_ref: self.plan_ref.clone(),
            runtime_authority: false,
            admission_digest: String::new(),
        };
        admission.admission_digest = admission.canonical_digest();
        admission.validate()?;
        Ok(admission)
    }
}
