//! Company-facing bounded parallel plan/settlement contract.
//!
//! The existing SwarmWorkGraph/ControlPlane owns admission and dispatch. This wrapper binds the
//! graph to one Company project/parent packet and makes merge eligibility explicit without adding a
//! message bus or another executor.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_PARALLEL_SCHEMA: &str = "kiana.company-parallel.v1";

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
pub enum CompanyPartitionOutcome {
    Pending,
    Succeeded,
    Failed,
    Cancelled,
    ResultUnknown,
}

impl CompanyPartitionOutcome {
    fn terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyParallelPlan {
    pub schema: String,
    pub parallel_id: String,
    pub project_id: String,
    pub parent_packet_id: String,
    pub merge_owner: String,
    pub partition_ids: Vec<String>,
    pub input_version: u64,
    pub authority_epoch: u64,
    pub isolation_digest: String,
    pub output_contract: String,
    pub max_concurrency: u32,
    pub expires_at: u64,
    pub digest: String,
}

impl CompanyParallelPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PARALLEL_SCHEMA
            || self.input_version == 0
            || self.authority_epoch == 0
            || self.max_concurrency == 0
            || self.partition_ids.is_empty()
            || self.partition_ids.len() > 64
            || self.expires_at == 0
        {
            return Err("company_parallel_header_invalid");
        }
        for (value, field) in [
            (&self.parallel_id, "company_parallel_id_required"),
            (&self.project_id, "company_parallel_project_required"),
            (&self.parent_packet_id, "company_parallel_parent_required"),
            (&self.merge_owner, "company_parallel_merge_owner_required"),
            (
                &self.output_contract,
                "company_parallel_output_contract_required",
            ),
        ] {
            required(value, field)?;
        }
        if self
            .partition_ids
            .iter()
            .any(|partition| partition.trim().is_empty())
            || self.partition_ids.iter().collect::<BTreeSet<_>>().len() != self.partition_ids.len()
        {
            return Err("company_parallel_partition_identity_invalid");
        }
        digest(
            &self.isolation_digest,
            "company_parallel_isolation_digest_invalid",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("company_parallel_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "parallel_id": self.parallel_id,
            "project_id": self.project_id,
            "parent_packet_id": self.parent_packet_id,
            "merge_owner": self.merge_owner,
            "partition_ids": self.partition_ids,
            "input_version": self.input_version,
            "authority_epoch": self.authority_epoch,
            "isolation_digest": self.isolation_digest,
            "output_contract": self.output_contract,
            "max_concurrency": self.max_concurrency,
            "expires_at": self.expires_at,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyParallelSettlement {
    pub schema: String,
    pub parallel_id: String,
    pub project_id: String,
    pub plan_digest: String,
    pub partition_outcomes: BTreeMap<String, CompanyPartitionOutcome>,
    pub evidence_refs: Vec<String>,
    pub all_settled: bool,
    pub merge_allowed: bool,
    pub digest: String,
}

impl CompanyParallelSettlement {
    pub fn from_outcomes(
        plan: &CompanyParallelPlan,
        partition_outcomes: BTreeMap<String, CompanyPartitionOutcome>,
        evidence_refs: Vec<String>,
    ) -> Result<Self, &'static str> {
        plan.validate()?;
        if partition_outcomes.len() != plan.partition_ids.len()
            || partition_outcomes
                .keys()
                .any(|partition| !plan.partition_ids.contains(partition))
            || evidence_refs.is_empty()
        {
            return Err("company_parallel_settlement_coverage_invalid");
        }
        let all_settled = partition_outcomes
            .values()
            .all(|outcome| outcome.terminal());
        let merge_allowed = all_settled
            && partition_outcomes
                .values()
                .all(|outcome| *outcome == CompanyPartitionOutcome::Succeeded);
        let mut settlement = Self {
            schema: COMPANY_PARALLEL_SCHEMA.to_owned(),
            parallel_id: plan.parallel_id.clone(),
            project_id: plan.project_id.clone(),
            plan_digest: plan.digest.clone(),
            partition_outcomes,
            evidence_refs,
            all_settled,
            merge_allowed,
            digest: String::new(),
        };
        settlement.digest = settlement.canonical_digest();
        settlement.validate_against(plan)?;
        Ok(settlement)
    }

    pub fn validate_against(&self, plan: &CompanyParallelPlan) -> Result<(), &'static str> {
        plan.validate()?;
        if self.schema != COMPANY_PARALLEL_SCHEMA
            || self.parallel_id != plan.parallel_id
            || self.project_id != plan.project_id
            || self.plan_digest != plan.digest
            || self.partition_outcomes.len() != plan.partition_ids.len()
            || self.evidence_refs.is_empty()
        {
            return Err("company_parallel_settlement_binding_invalid");
        }
        let expected_settled = self
            .partition_outcomes
            .values()
            .all(|outcome| outcome.terminal());
        let expected_merge = expected_settled
            && self
                .partition_outcomes
                .values()
                .all(|outcome| *outcome == CompanyPartitionOutcome::Succeeded);
        if self.all_settled != expected_settled || self.merge_allowed != expected_merge {
            return Err("company_parallel_settlement_state_invalid");
        }
        digest(
            &self.plan_digest,
            "company_parallel_settlement_plan_digest_invalid",
        )?;
        digest(&self.digest, "company_parallel_settlement_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_parallel_settlement_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "parallel_id": self.parallel_id,
            "project_id": self.project_id,
            "plan_digest": self.plan_digest,
            "partition_outcomes": self.partition_outcomes,
            "evidence_refs": self.evidence_refs,
            "all_settled": self.all_settled,
            "merge_allowed": self.merge_allowed,
        }))
    }
}
