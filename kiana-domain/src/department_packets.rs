//! Versioned, non-executing department work contracts.
//!
//! These packets describe proposals and outputs for planning/analysis/verification work. They do
//! not carry a Grant or Lease and never replace the legacy Builder `WorkPacket` execution path.
use crate::{normalize_role_path, RoleSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEPARTMENT_PACKET_SCHEMA: &str = "kiana.department-packet.v1";
pub const RESULT_CONTRACT_SCHEMA: &str = "kiana.result-contract.v1";

fn required(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("department_packet_field_required_or_too_large")
    } else {
        Ok(())
    }
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
    fn role_id(self) -> &'static str {
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
    #[serde(default)]
    pub plan_ref: Option<String>,
    /// Explicit compatibility fields make an attempted capability smuggling visible and
    /// rejectable instead of silently ignoring a caller-provided grant or lease.
    #[serde(default)]
    pub runtime_grant: Option<String>,
    #[serde(default)]
    pub budget_lease: Option<String>,
}

impl DepartmentPacket {
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
        if self.runtime_grant.is_some() || self.budget_lease.is_some() {
            return Err("department_packet_runtime_authority_forbidden");
        }
        let role = RoleSpec::lookup(&self.target_role).ok_or("department_packet_role_unknown")?;
        if role.role_id != self.kind.role_id() || role.department_id != self.from_department {
            return Err("department_packet_role_department_mismatch");
        }
        if !self.kind.allowed_input(self.input_basis) {
            return Err("department_packet_input_basis_invalid");
        }
        if self.kind == DepartmentPacketKind::Implementation {
            if self
                .plan_ref
                .as_deref()
                .is_none_or(|reference| required(reference).is_err())
            {
                return Err("department_packet_plan_required");
            }
            if self.write_scope.is_empty() {
                return Err("department_packet_write_scope_required");
            }
        } else if let Some(plan_ref) = self.plan_ref.as_deref() {
            required(plan_ref)?;
            if self.input_basis != PacketInputBasis::Plan {
                return Err("department_packet_plan_reference_mismatch");
            }
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
}
