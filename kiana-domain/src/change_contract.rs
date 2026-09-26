//! Baseline change impact and atomic publication contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const CHANGE_IMPACT_SCHEMA: &str = "kiana.change-impact.v1";
pub const BASELINE_PUBLICATION_SCHEMA: &str = "kiana.baseline-publication.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeImpact {
    pub schema: String,
    pub change_id: String,
    pub project_id: String,
    pub old_baseline_version: u64,
    pub new_baseline_version: u64,
    pub affected_milestones: Vec<String>,
    pub affected_packets: Vec<String>,
    pub affected_runs: Vec<String>,
    pub invalidated_reviews: Vec<String>,
    pub invalidated_acceptances: Vec<String>,
    pub invalidated_deliveries: Vec<String>,
    pub affected_budget_ref: String,
    pub affected_schedule_ref: String,
    pub impact_digest: String,
}

impl ChangeImpact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CHANGE_IMPACT_SCHEMA
            || self.old_baseline_version == 0
            || self.new_baseline_version <= self.old_baseline_version
        {
            return Err("change_impact_version_invalid");
        }
        for (value, field) in [
            (&self.change_id, "change_impact_id_required"),
            (&self.project_id, "change_impact_project_required"),
            (&self.affected_budget_ref, "change_impact_budget_required"),
            (
                &self.affected_schedule_ref,
                "change_impact_schedule_required",
            ),
        ] {
            required(value, field)?;
        }
        if self.affected_milestones.is_empty() || self.affected_packets.is_empty() {
            return Err("change_impact_scope_required");
        }
        for list in [
            &self.affected_milestones,
            &self.affected_packets,
            &self.affected_runs,
            &self.invalidated_reviews,
            &self.invalidated_acceptances,
            &self.invalidated_deliveries,
        ] {
            if list.iter().any(|value| value.trim().is_empty())
                || list.iter().collect::<BTreeSet<_>>().len() != list.len()
            {
                return Err("change_impact_identity_invalid");
            }
        }
        digest(&self.impact_digest, "change_impact_digest_invalid")?;
        if self.impact_digest != self.canonical_digest() {
            return Err("change_impact_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema, "change_id": self.change_id, "project_id": self.project_id,
            "old_baseline_version": self.old_baseline_version, "new_baseline_version": self.new_baseline_version,
            "affected_milestones": self.affected_milestones, "affected_packets": self.affected_packets,
            "affected_runs": self.affected_runs, "invalidated_reviews": self.invalidated_reviews,
            "invalidated_acceptances": self.invalidated_acceptances, "invalidated_deliveries": self.invalidated_deliveries,
            "affected_budget_ref": self.affected_budget_ref, "affected_schedule_ref": self.affected_schedule_ref,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselinePublication {
    pub schema: String,
    pub project_id: String,
    pub change_id: String,
    pub old_baseline_version: u64,
    pub new_baseline_version: u64,
    pub charter_ref: String,
    pub plan_ref: String,
    pub packet_refs: Vec<String>,
    pub criteria_digest: String,
    pub budget_ref: String,
    pub schedule_ref: String,
    pub invalidated_refs: Vec<String>,
    pub published_by: String,
    pub published_at: u64,
    pub digest: String,
}

impl BaselinePublication {
    pub fn validate(&self, impact: &ChangeImpact) -> Result<(), &'static str> {
        if self.schema != BASELINE_PUBLICATION_SCHEMA
            || self.project_id != impact.project_id
            || self.change_id != impact.change_id
            || self.old_baseline_version != impact.old_baseline_version
            || self.new_baseline_version != impact.new_baseline_version
            || self.published_at == 0
        {
            return Err("baseline_publication_binding_invalid");
        }
        for (value, field) in [
            (&self.charter_ref, "baseline_publication_charter_required"),
            (&self.plan_ref, "baseline_publication_plan_required"),
            (&self.budget_ref, "baseline_publication_budget_required"),
            (&self.schedule_ref, "baseline_publication_schedule_required"),
            (&self.published_by, "baseline_publication_author_required"),
        ] {
            required(value, field)?;
        }
        if self.packet_refs != impact.affected_packets
            || self.criteria_digest.trim().is_empty()
            || self.invalidated_refs.is_empty()
        {
            return Err("baseline_publication_complete_set_required");
        }
        if self.invalidated_refs.iter().collect::<BTreeSet<_>>().len()
            != self.invalidated_refs.len()
        {
            return Err("baseline_publication_invalidated_duplicate");
        }
        digest(
            &self.criteria_digest,
            "baseline_publication_criteria_digest_invalid",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("baseline_publication_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema, "project_id": self.project_id, "change_id": self.change_id,
            "old_baseline_version": self.old_baseline_version, "new_baseline_version": self.new_baseline_version,
            "charter_ref": self.charter_ref, "plan_ref": self.plan_ref, "packet_refs": self.packet_refs,
            "criteria_digest": self.criteria_digest, "budget_ref": self.budget_ref, "schedule_ref": self.schedule_ref,
            "invalidated_refs": self.invalidated_refs, "published_by": self.published_by, "published_at": self.published_at,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChangePublicationLedger {
    pub publications: BTreeMap<String, BaselinePublication>,
    pub historical_impacts: BTreeMap<String, ChangeImpact>,
}

impl ChangePublicationLedger {
    pub fn publish(
        &mut self,
        impact: ChangeImpact,
        publication: BaselinePublication,
    ) -> Result<(), &'static str> {
        impact.validate()?;
        publication.validate(&impact)?;
        if let Some(existing) = self.publications.get(&impact.change_id) {
            if existing.digest == publication.digest {
                return Ok(());
            }
            return Err("baseline_publication_duplicate_digest_mismatch");
        }
        if self.historical_impacts.values().any(|old| {
            old.project_id == impact.project_id
                && old.new_baseline_version >= impact.new_baseline_version
        }) {
            return Err("baseline_publication_version_conflict");
        }
        self.historical_impacts
            .insert(impact.change_id.clone(), impact);
        self.publications
            .insert(publication.change_id.clone(), publication);
        Ok(())
    }
}
