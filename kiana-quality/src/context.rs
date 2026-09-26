//! EQ-32 context and memory evaluation over caller-supplied retrieval evidence.
//!
//! ACL admission is checked before ranking/selection, and provenance, freshness, compaction and
//! token/wire budgets are checked as independent quality dimensions. This module never searches
//! Memory, ranks a record, compacts a context, or sends a model request.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTEXT_MEMORY_INPUT_SCHEMA: &str = "kiana.quality-context-memory-input.v1";
pub const CONTEXT_MEMORY_EVALUATOR_ID: &str = "context-memory";
const QUERY_SCHEMA: &str = "kiana.quality-context-query-evidence.v1";
const HIT_SCHEMA: &str = "kiana.quality-memory-hit-evidence.v1";
const BUDGET_SCHEMA: &str = "kiana.quality-context-budget-evidence.v1";
const COMPACTION_SCHEMA: &str = "kiana.quality-compaction-evidence.v1";
const MAX_HITS: usize = 512;
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextFreshness {
    Current,
    Stale,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextEvidenceStatus {
    Verified,
    Attributed,
    Unverifiable,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextQueryEvidence {
    pub schema: String,
    pub query_digest: String,
    pub scope_digest: String,
    pub data_epoch: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryHitEvidence {
    pub schema: String,
    pub record_ref: String,
    pub acl_checked: bool,
    pub acl_allowed: bool,
    pub ranked: bool,
    pub selected: bool,
    pub rank: u32,
    pub source_revision: String,
    pub source_digest: String,
    pub provenance_digest: String,
    pub freshness: ContextFreshness,
    pub evidence: ContextEvidenceStatus,
    pub stale_acknowledged: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBudgetEvidence {
    pub schema: String,
    pub token_limit: u64,
    pub estimated_tokens: u64,
    pub used_tokens: u64,
    pub reserved_output_tokens: u64,
    pub budget_enforced: bool,
    pub wire_bytes: u64,
    pub wire_limit: u64,
    pub wire_budget_enforced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionEvidence {
    pub schema: String,
    pub attempted: bool,
    pub summary_present: bool,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub latest_goal_preserved: bool,
    pub pending_pairs_preserved: bool,
    pub completed_actions_verifiable: bool,
    pub stale_commit_rejected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMemoryInput {
    pub schema: String,
    pub query: ContextQueryEvidence,
    pub acl_checked_before_ranking: bool,
    pub hits: Vec<MemoryHitEvidence>,
    pub budget: ContextBudgetEvidence,
    pub compaction: CompactionEvidence,
}

impl ContextMemoryInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CONTEXT_MEMORY_INPUT_SCHEMA {
            return Err("context_memory_input_schema_invalid");
        }
        if self.hits.len() > MAX_HITS {
            return Err("context_memory_hit_limit_exceeded");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ContextMemoryEvaluator;

impl DeterministicEvaluator for ContextMemoryEvaluator {
    fn evaluator_id(&self) -> &str {
        CONTEXT_MEMORY_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: ContextMemoryInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("context_memory"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_context_memory(&decoded)
    }
}

pub fn evaluate_context_memory(input: &ContextMemoryInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    check_schema(
        &mut findings,
        "context.query_schema_invalid",
        &input.query.schema,
        QUERY_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "context.budget_schema_invalid",
        &input.budget.schema,
        BUDGET_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "context.compaction_schema_invalid",
        &input.compaction.schema,
        COMPACTION_SCHEMA,
    )?;

    if !valid_digest(&input.query.query_digest) || !valid_digest(&input.query.scope_digest) {
        emit(
            &mut findings,
            "context.query_provenance_invalid",
            "sha256_query_and_scope",
            "invalid",
        )?;
    }
    if input.query.data_epoch == 0 || input.query.generation == 0 {
        emit(
            &mut findings,
            "context.query_provenance_invalid",
            "positive_epoch_and_generation",
            "invalid",
        )?;
    }
    if !input.acl_checked_before_ranking {
        emit(
            &mut findings,
            "context.acl_order_invalid",
            "acl_before_ranking",
            "ranking_before_acl",
        )?;
    }

    let mut record_refs = BTreeSet::new();
    let mut previous_rank = 0;
    for hit in &input.hits {
        check_schema(
            &mut findings,
            "context.hit_schema_invalid",
            &hit.schema,
            HIT_SCHEMA,
        )?;
        if !valid_reference(&hit.record_ref) || !record_refs.insert(&hit.record_ref) {
            emit(
                &mut findings,
                "context.hit_identity_invalid",
                "unique_bounded_record_ref",
                "invalid_or_duplicate",
            )?;
        }
        if !valid_reference(&hit.source_revision)
            || !valid_digest(&hit.source_digest)
            || !valid_digest(&hit.provenance_digest)
        {
            emit(
                &mut findings,
                "context.provenance_invalid",
                "revision_and_sha256_source_provenance",
                "invalid",
            )?;
        }
        if hit.ranked && (!hit.acl_checked || !hit.acl_allowed) {
            emit(
                &mut findings,
                "context.unauthorized_memory_hit",
                "acl_allowed_before_ranking",
                "ranked_without_admission",
            )?;
        }
        if hit.selected && (!hit.ranked || !hit.acl_allowed) {
            emit(
                &mut findings,
                "context.unauthorized_memory_hit",
                "selected_hit_is_acl_admitted",
                "selected_without_admission",
            )?;
        }
        if hit.ranked {
            if hit.rank == 0 || (previous_rank != 0 && hit.rank < previous_rank) {
                emit(
                    &mut findings,
                    "context.rank_order_invalid",
                    "positive_monotonic_rank",
                    "invalid",
                )?;
            }
            previous_rank = hit.rank;
        } else if hit.rank != 0 {
            emit(
                &mut findings,
                "context.rank_order_invalid",
                "unranked_hit_has_zero_rank",
                "nonzero",
            )?;
        }
        if hit.selected {
            match hit.freshness {
                ContextFreshness::Current => {}
                ContextFreshness::Stale if hit.stale_acknowledged => {}
                ContextFreshness::Stale => emit(
                    &mut findings,
                    "context.stale_not_acknowledged",
                    "stale_selection_has_acknowledgement",
                    "missing",
                )?,
                ContextFreshness::Unknown => emit(
                    &mut findings,
                    "context.freshness_unknown",
                    "current_or_explicit_stale",
                    "unknown",
                )?,
            }
            if matches!(
                hit.evidence,
                ContextEvidenceStatus::Missing | ContextEvidenceStatus::Unverifiable
            ) {
                emit(
                    &mut findings,
                    "context.evidence_missing",
                    "verified_or_attributed_evidence",
                    "missing",
                )?;
            }
        }
    }

    let budget = &input.budget;
    if budget.token_limit == 0
        || budget.wire_limit == 0
        || budget.used_tokens > budget.token_limit
        || budget.estimated_tokens > budget.token_limit
        || budget.reserved_output_tokens > budget.token_limit
        || budget.wire_bytes > budget.wire_limit
    {
        emit(
            &mut findings,
            "context.budget_overflow",
            "token_and_wire_limits_respected",
            "overflow",
        )?;
    }
    if !budget.budget_enforced {
        emit(
            &mut findings,
            "context.budget_unenforced",
            "token_budget_enforced",
            "disabled",
        )?;
    }
    if !budget.wire_budget_enforced {
        emit(
            &mut findings,
            "context.budget_unenforced",
            "wire_budget_enforced",
            "disabled",
        )?;
    }

    let compaction = &input.compaction;
    if compaction.attempted {
        if !compaction.summary_present {
            emit(
                &mut findings,
                "context.compaction_summary_missing",
                "summary_present",
                "missing",
            )?;
        }
        if compaction.source_cursor_start == 0
            || compaction.source_cursor_start > compaction.source_cursor_end
        {
            emit(
                &mut findings,
                "context.compaction_cursor_invalid",
                "positive_ordered_source_range",
                "invalid",
            )?;
        }
        if !compaction.latest_goal_preserved
            || !compaction.pending_pairs_preserved
            || !compaction.completed_actions_verifiable
        {
            emit(
                &mut findings,
                "context.compaction_integrity_invalid",
                "goal_pending_and_completed_actions_preserved",
                "missing",
            )?;
        }
        if !compaction.stale_commit_rejected {
            emit(
                &mut findings,
                "context.compaction_stale_commit_unfenced",
                "stale_commit_rejected",
                "missing",
            )?;
        }
    }

    Ok(findings)
}

fn check_schema(
    findings: &mut Vec<Finding>,
    code: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), EvaluatorError> {
    if actual != expected {
        emit(findings, code, expected, "invalid")?;
    }
    Ok(())
}

fn valid_reference(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: &'static str,
    actual: &'static str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::new(
            code,
            serde_json::Value::String(expected.to_owned()),
            serde_json::Value::String(actual.to_owned()),
            format!("context and memory finding: {code}"),
            "context:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
