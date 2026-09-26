//! CompanyOS's explainable, read-only readiness projection.
//!
//! The legacy `ready_packets` function remains the single DAG/status primitive.  This module
//! supplies the Company-specific facts that the old WorkPacket cannot carry on its own: handoff
//! ACK state, assignment validity, declared dependency outcome kinds, input versions and
//! project/change gates.  It never claims a packet, reserves a budget or starts a run.

use crate::{ready_packets, CompanyHandoffStatus, PacketGraphError, WorkPacket};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_READINESS_SCHEMA: &str = "kiana.company-readiness.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRequirement {
    RequiresRunSuccess,
    RequiresAcceptance,
    RequiresArtifact,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyOutcome {
    Succeeded,
    Accepted,
    ArtifactPresent,
    Failed,
    Unknown,
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentReadiness {
    Active,
    Missing,
    Expired,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessBlocker {
    pub code: String,
    pub dependency_id: Option<String>,
    pub detail: String,
}

impl ReadinessBlocker {
    pub fn new(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            dependency_id: None,
            detail: detail.into(),
        }
    }

    fn dependency(
        code: impl Into<String>,
        dependency_id: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            dependency_id: Some(dependency_id.into()),
            detail: detail.into(),
        }
    }
}

/// Optional Company facts used by the one readiness projection. Empty maps preserve legacy
/// WorkPacket behavior for old callers while Company adapters can opt into stricter gates.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReadinessContext {
    #[serde(default)]
    pub project_active: Option<bool>,
    #[serde(default)]
    pub packet_approved: BTreeMap<String, bool>,
    #[serde(default)]
    pub handoffs: BTreeMap<String, CompanyHandoffStatus>,
    #[serde(default)]
    pub assignments: BTreeMap<String, AssignmentReadiness>,
    /// Required versions are keyed by the exact input reference used by the packet.
    #[serde(default)]
    pub required_input_versions: BTreeMap<String, u64>,
    #[serde(default)]
    pub input_versions: BTreeMap<String, u64>,
    /// Packet ID -> dependency ID -> declared outcome requirement.
    #[serde(default)]
    pub dependency_requirements: BTreeMap<String, BTreeMap<String, DependencyRequirement>>,
    #[serde(default)]
    pub dependency_outcomes: BTreeMap<String, DependencyOutcome>,
    #[serde(default)]
    pub pending_changes: BTreeSet<String>,
    #[serde(default)]
    pub extra_blockers: BTreeMap<String, Vec<String>>,
}

impl CompanyReadinessContext {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .required_input_versions
            .values()
            .chain(self.input_versions.values())
            .any(|version| *version == 0)
        {
            return Err("company_readiness_input_version_invalid");
        }
        for (packet_id, dependencies) in &self.dependency_requirements {
            if packet_id.trim().is_empty() || dependencies.keys().any(|id| id.trim().is_empty()) {
                return Err("company_readiness_dependency_identity_invalid");
            }
        }
        for blockers in self.extra_blockers.values() {
            if blockers.iter().any(|blocker| blocker.trim().is_empty()) {
                return Err("company_readiness_extra_blocker_invalid");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReadiness {
    pub schema: String,
    pub ready: Vec<String>,
    pub blocked: BTreeMap<String, String>,
    pub blockers: BTreeMap<String, Vec<ReadinessBlocker>>,
    pub expired_claims: Vec<String>,
    pub digest: String,
}

impl CompanyReadiness {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_READINESS_SCHEMA {
            return Err("company_readiness_schema_invalid");
        }
        if self.ready.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("company_readiness_ready_order_invalid");
        }
        if self.ready.iter().any(|id| self.blocked.contains_key(id)) {
            return Err("company_readiness_ready_blocked_overlap");
        }
        if self
            .blockers
            .iter()
            .any(|(id, blockers)| blockers.is_empty() || id.trim().is_empty())
        {
            return Err("company_readiness_blocker_missing");
        }
        if self.digest != self.canonical_digest() {
            return Err("company_readiness_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        crate::json_digest(&json!({
            "schema": self.schema,
            "ready": self.ready,
            "blocked": self.blocked,
            "blockers": self.blockers,
            "expired_claims": self.expired_claims,
        }))
    }
}

fn append_blocker(
    blocked: &mut BTreeMap<String, String>,
    blockers: &mut BTreeMap<String, Vec<ReadinessBlocker>>,
    packet_id: &str,
    blocker: ReadinessBlocker,
) {
    blocked
        .entry(packet_id.to_owned())
        .or_insert_with(|| blocker.detail.clone());
    blockers
        .entry(packet_id.to_owned())
        .or_default()
        .push(blocker);
}

fn base_blocker(reason: &str) -> ReadinessBlocker {
    let (code, dependency_id) = reason
        .split_once(':')
        .map_or((reason, None), |(code, dependency)| {
            (code, Some(dependency))
        });
    ReadinessBlocker {
        code: code.to_owned(),
        dependency_id: dependency_id.map(str::to_owned),
        detail: reason.to_owned(),
    }
}

fn dependency_blocker(
    packet_id: &str,
    dependency_id: &str,
    requirement: DependencyRequirement,
    outcome: DependencyOutcome,
) -> Option<ReadinessBlocker> {
    let accepted = match requirement {
        DependencyRequirement::RequiresRunSuccess => outcome == DependencyOutcome::Succeeded,
        DependencyRequirement::RequiresAcceptance => outcome == DependencyOutcome::Accepted,
        DependencyRequirement::RequiresArtifact => outcome == DependencyOutcome::ArtifactPresent,
    };
    if accepted {
        return None;
    }
    let code = match (requirement, outcome) {
        (DependencyRequirement::RequiresAcceptance, DependencyOutcome::Succeeded) => {
            "dependency_acceptance_required"
        }
        (_, DependencyOutcome::Failed) => "dependency_failed",
        (_, DependencyOutcome::Unknown) => "dependency_result_unknown",
        (_, DependencyOutcome::Missing) => "dependency_outcome_missing",
        (DependencyRequirement::RequiresArtifact, _) => "dependency_artifact_required",
        (DependencyRequirement::RequiresRunSuccess, _) => "dependency_run_success_required",
        (DependencyRequirement::RequiresAcceptance, _) => "dependency_acceptance_required",
    };
    Some(ReadinessBlocker::dependency(
        code,
        dependency_id,
        format!("{packet_id} requires {requirement:?} from {dependency_id}, observed {outcome:?}"),
    ))
}

/// Evaluate Company-specific gates over the canonical dependency/status predicate.
pub fn company_ready_packets(
    packets: &BTreeMap<String, WorkPacket>,
    now_ms: u64,
    context: &CompanyReadinessContext,
) -> Result<CompanyReadiness, PacketGraphError> {
    context.validate().map_err(|code| PacketGraphError {
        code: code.to_owned(),
        packet_id: String::new(),
        cycle: Vec::new(),
    })?;
    let base = ready_packets(packets, now_ms)?;
    let mut output = CompanyReadiness {
        schema: COMPANY_READINESS_SCHEMA.to_owned(),
        ready: base.ready.clone(),
        blocked: base.blocked.clone(),
        blockers: BTreeMap::new(),
        expired_claims: base.expired_claims.clone(),
        digest: String::new(),
    };
    for (packet_id, reason) in &base.blocked {
        output
            .blockers
            .entry(packet_id.clone())
            .or_default()
            .push(base_blocker(reason));
    }

    let mut context_blocked = BTreeSet::new();
    for (packet_id, packet) in packets {
        let mut packet_blockers = Vec::new();
        if context.project_active == Some(false) {
            packet_blockers.push(ReadinessBlocker::new(
                "project_paused",
                "project is paused or inactive",
            ));
        }
        if context.packet_approved.get(packet_id) == Some(&false) {
            packet_blockers.push(ReadinessBlocker::new(
                "packet_not_approved",
                "packet is not approved or assigned",
            ));
        }
        if let Some(status) = context.handoffs.get(packet_id) {
            match status {
                CompanyHandoffStatus::Pending => packet_blockers.push(ReadinessBlocker::new(
                    "handoff_ack_required",
                    "recipient has not acknowledged the handoff",
                )),
                CompanyHandoffStatus::Rejected => packet_blockers.push(ReadinessBlocker::new(
                    "handoff_rejected",
                    "recipient rejected the handoff; owner escalation is required",
                )),
                CompanyHandoffStatus::Expired => packet_blockers.push(ReadinessBlocker::new(
                    "handoff_expired",
                    "handoff expired before ACK; owner escalation is required",
                )),
                CompanyHandoffStatus::Acknowledged => {}
            }
        }
        if let Some(assignment) = context.assignments.get(packet_id) {
            match assignment {
                AssignmentReadiness::Active => {}
                AssignmentReadiness::Missing => packet_blockers.push(ReadinessBlocker::new(
                    "assignment_missing",
                    "target assignment is missing",
                )),
                AssignmentReadiness::Expired => packet_blockers.push(ReadinessBlocker::new(
                    "assignment_expired",
                    "target assignment has expired",
                )),
                AssignmentReadiness::Revoked => packet_blockers.push(ReadinessBlocker::new(
                    "assignment_revoked",
                    "target assignment has been revoked",
                )),
            }
        }
        if context.pending_changes.contains(packet_id) {
            packet_blockers.push(ReadinessBlocker::new(
                "change_pending",
                "packet is covered by a pending change",
            ));
        }
        for input in &packet.inputs {
            if let Some(required) = context.required_input_versions.get(input) {
                match context.input_versions.get(input) {
                    None => packet_blockers.push(ReadinessBlocker::new(
                        "input_version_missing",
                        format!("input {input} requires version {required}"),
                    )),
                    Some(observed) if observed != required => {
                        packet_blockers.push(ReadinessBlocker::new(
                            "input_version_stale",
                            format!(
                                "input {input} requires version {required}, observed {observed}"
                            ),
                        ))
                    }
                    Some(_) => {}
                }
            }
        }
        if let Some(dependencies) = context.dependency_requirements.get(packet_id) {
            for (dependency_id, requirement) in dependencies {
                let outcome = context
                    .dependency_outcomes
                    .get(dependency_id)
                    .copied()
                    .unwrap_or(DependencyOutcome::Missing);
                if let Some(blocker) =
                    dependency_blocker(packet_id, dependency_id, *requirement, outcome)
                {
                    packet_blockers.push(blocker);
                }
            }
        }
        for detail in context.extra_blockers.get(packet_id).into_iter().flatten() {
            packet_blockers.push(ReadinessBlocker::new("company_blocked", detail));
        }
        if !packet_blockers.is_empty() {
            context_blocked.insert(packet_id.clone());
            for blocker in packet_blockers {
                append_blocker(
                    &mut output.blocked,
                    &mut output.blockers,
                    packet_id,
                    blocker,
                );
            }
        }
    }
    output
        .ready
        .retain(|packet_id| !context_blocked.contains(packet_id));
    output.ready.sort();
    output.digest = output.canonical_digest();
    output.validate().map_err(|code| PacketGraphError {
        code: code.to_owned(),
        packet_id: String::new(),
        cycle: Vec::new(),
    })?;
    Ok(output)
}
