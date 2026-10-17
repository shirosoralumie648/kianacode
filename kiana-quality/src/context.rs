//! EQ-32 context and memory evaluation over caller-supplied retrieval evidence.
//!
//! ACL admission is verified *before* ranking and selection: a hit that was ranked or selected
//! without a recorded ACL check and an allowing ACL decision is an unauthorized memory hit.  Every
//! hit keeps a bounded identity and strict SHA-256 source/provenance digests inside the queried
//! scope and data epoch, freshness and evidence status stay explicit (stale material needs an
//! acknowledgement, unknown material is never silently selected), a compaction attempt preserves
//! the latest goal, pending pairs and verifiable completed actions while rejecting a stale commit,
//! and the token/wire budgets must be enforced without overflow.
//!
//! This is a pure diagnostic over the evidence it is given: it never searches Memory, ranks a
//! record, compiles a ContextPlan, compacts a transcript, contacts a model, or changes ACL, data
//! epoch, policy or budget authority.  Hits are ordered by a stable identity key before
//! evaluation, so the finding set does not depend on the caller's array order while duplicate
//! evidence stays visible.  Every caller-supplied value passes through [`Finding::redacted`], and
//! each finding cites the evidence section it was derived from.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::redact_text;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Versioned envelope for one context/memory evaluation input.
pub const CONTEXT_MEMORY_INPUT_SCHEMA: &str = "kiana.quality-context-memory-input.v1";
/// Stable registry identifier for the context/memory evaluator.
pub const CONTEXT_MEMORY_EVALUATOR_ID: &str = "context-memory";
const QUERY_SCHEMA: &str = "kiana.quality-context-query-evidence.v1";
const HIT_SCHEMA: &str = "kiana.quality-memory-hit-evidence.v1";
const BUDGET_SCHEMA: &str = "kiana.quality-context-budget-evidence.v1";
const COMPACTION_SCHEMA: &str = "kiana.quality-compaction-evidence.v1";
const MAX_HITS: usize = 512;
const MAX_TEXT_BYTES: usize = 512;
const QUERY_EVIDENCE: &str = "context:query";
const ACL_EVIDENCE: &str = "context:acl";
const RANKING_EVIDENCE: &str = "context:ranking";
const HIT_EVIDENCE: &str = "context:hit:";
const BUDGET_EVIDENCE: &str = "context:budget";
const COMPACTION_EVIDENCE: &str = "context:compaction";

/// Freshness of one memory hit, observed before selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextFreshness {
    Current,
    Stale,
    Unknown,
}

/// How well one memory hit is backed by evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextEvidenceStatus {
    Verified,
    Attributed,
    Unverifiable,
    Missing,
}

/// The query under evaluation: strict digests, the data epoch/generation it ran under and the
/// selection bound the caller declared.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextQueryEvidence {
    pub schema: String,
    pub query_digest: String,
    /// Scope digest every admitted hit must belong to.
    pub scope_digest: String,
    pub data_epoch: u64,
    pub generation: u64,
    /// Maximum number of hits the query may present.  Zero is rejected.
    pub top_k: u64,
}

/// One candidate/selected memory hit with its identity, scope, ACL decision, rank, provenance
/// digests, freshness and evidence status.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryHitEvidence {
    pub schema: String,
    pub record_ref: String,
    /// Scope digest of the retrieved record; must equal [`ContextQueryEvidence::scope_digest`].
    pub scope_digest: String,
    /// Data epoch of the retrieved record; must equal [`ContextQueryEvidence::data_epoch`].
    pub data_epoch: u64,
    pub acl_checked: bool,
    pub acl_allowed: bool,
    pub ranked: bool,
    pub selected: bool,
    /// One-based rank of a ranked hit; always zero for an unranked hit.
    pub rank: u32,
    pub source_revision: String,
    pub source_digest: String,
    pub provenance_digest: String,
    pub freshness: ContextFreshness,
    pub evidence: ContextEvidenceStatus,
    /// Explicit acknowledgement that a stale hit is knowingly presented.
    pub stale_acknowledged: bool,
}

/// Token and wire budget observed for the assembled context.
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

/// One compaction attempt and the material it must preserve.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionEvidence {
    pub schema: String,
    pub attempted: bool,
    pub summary_present: bool,
    /// Strict `sha256:` digest of the produced summary.  Required when
    /// [`Self::summary_present`] is set, and left empty without an attempt.
    pub summary_digest: String,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub latest_goal_preserved: bool,
    pub pending_pairs_preserved: bool,
    pub completed_actions_verifiable: bool,
    pub stale_commit_rejected: bool,
}

/// Versioned input envelope for [`ContextMemoryEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMemoryInput {
    pub schema: String,
    /// Whether the query admitted hits before ranking them.  A `false` value makes every ranked
    /// or selected hit unauthorized regardless of the per-hit ACL flags.
    pub acl_checked_before_ranking: bool,
    pub query: ContextQueryEvidence,
    #[serde(default)]
    pub hits: Vec<MemoryHitEvidence>,
    pub budget: ContextBudgetEvidence,
    pub compaction: CompactionEvidence,
}

impl ContextMemoryInput {
    pub fn new(
        acl_checked_before_ranking: bool,
        query: ContextQueryEvidence,
        hits: Vec<MemoryHitEvidence>,
        budget: ContextBudgetEvidence,
        compaction: CompactionEvidence,
    ) -> Self {
        Self {
            schema: CONTEXT_MEMORY_INPUT_SCHEMA.to_owned(),
            acl_checked_before_ranking,
            query,
            hits,
            budget,
            compaction,
        }
    }

    /// Hard input bounds.  Structural drift fails closed before any finding is produced; entry
    /// shape problems remain findings so the report explains *why* the evidence is unusable.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CONTEXT_MEMORY_INPUT_SCHEMA {
            return Err("context_memory_input_schema_invalid");
        }
        if self.hits.len() > MAX_HITS {
            return Err("context_memory_hit_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("context memory input is serializable")
    }
}

/// Pure context/memory evaluator.  All authority-bearing actions remain outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContextMemoryEvaluator;

impl DeterministicEvaluator for ContextMemoryEvaluator {
    fn evaluator_id(&self) -> &str {
        CONTEXT_MEMORY_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: ContextMemoryInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("context_memory"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_context_memory(&decoded)
    }
}

/// Evaluate the deny-first context/memory matrix without reading any store.
///
/// Check order is deliberate: every section schema, the query provenance and selection bound, the
/// ACL-before-ranking contract, the per-hit identity/scope/epoch/provenance contract, ranking and
/// selection coherence, freshness/evidence status of presented material, the token/wire budget,
/// and finally the compaction contract.  Hits are ordered by a stable identity key before
/// evaluation so the finding set does not depend on the caller's array order.
pub fn evaluate_context_memory(input: &ContextMemoryInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let query = &input.query;
    let budget = &input.budget;
    let compaction = &input.compaction;

    check_schema(
        &mut findings,
        "context.query_schema_invalid",
        &query.schema,
        QUERY_SCHEMA,
        QUERY_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "context.budget_schema_invalid",
        &budget.schema,
        BUDGET_SCHEMA,
        BUDGET_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "context.compaction_schema_invalid",
        &compaction.schema,
        COMPACTION_SCHEMA,
        COMPACTION_EVIDENCE,
    )?;

    // Query provenance: strict digests, a positive epoch/generation, and a positive selection
    // bound the caller may not exceed.
    if !is_sha256_digest(&query.query_digest) || !is_sha256_digest(&query.scope_digest) {
        emit(
            &mut findings,
            "context.query_provenance_invalid",
            text("sha256_query_and_scope_digests"),
            query_actual(query),
            QUERY_EVIDENCE,
        )?;
    }
    if query.data_epoch == 0 || query.generation == 0 {
        emit(
            &mut findings,
            "context.query_provenance_invalid",
            text("positive_epoch_and_generation"),
            query_actual(query),
            QUERY_EVIDENCE,
        )?;
    }
    if query.top_k == 0 || query.top_k > MAX_HITS as u64 {
        emit(
            &mut findings,
            "context.selection_bound_invalid",
            text("positive_top_k_within_hit_limit"),
            query_actual(query),
            QUERY_EVIDENCE,
        )?;
    }

    // ACL-before-ranking.  A caller claiming that ranking happened before admission taints every
    // ranked or selected hit in this report.
    let acl_ordered = input.acl_checked_before_ranking;
    if !acl_ordered {
        emit(
            &mut findings,
            "context.acl_order_invalid",
            text("acl_before_ranking"),
            text("ranking_before_acl"),
            ACL_EVIDENCE,
        )?;
    }

    let mut hits: Vec<&MemoryHitEvidence> = input.hits.iter().collect();
    hits.sort_by(|left, right| hit_sort_key(left).cmp(&hit_sort_key(right)));

    let mut record_refs = BTreeSet::new();
    let mut ranks = BTreeSet::new();
    let mut selected_ranks = BTreeSet::new();
    let mut selected_count = 0_u64;
    for (index, hit) in hits.iter().enumerate() {
        let hit_ref = format!("{HIT_EVIDENCE}{index}");
        check_schema(
            &mut findings,
            "context.hit_schema_invalid",
            &hit.schema,
            HIT_SCHEMA,
            &hit_ref,
        )?;
        if !is_bounded_reference(&hit.record_ref) || !record_refs.insert(hit.record_ref.clone()) {
            emit(
                &mut findings,
                "context.hit_identity_invalid",
                text("unique_bounded_record_ref"),
                hit_actual(hit),
                &hit_ref,
            )?;
        }
        if !is_bounded_reference(&hit.source_revision)
            || !is_sha256_digest(&hit.source_digest)
            || !is_sha256_digest(&hit.provenance_digest)
        {
            emit(
                &mut findings,
                "context.provenance_invalid",
                text("bounded_revision_and_sha256_source_provenance"),
                hit_actual(hit),
                &hit_ref,
            )?;
        }
        if !is_sha256_digest(&hit.scope_digest) || hit.scope_digest != query.scope_digest {
            emit(
                &mut findings,
                "context.scope_mismatch",
                text(&query.scope_digest),
                hit_actual(hit),
                &hit_ref,
            )?;
        }
        if hit.data_epoch == 0 || hit.data_epoch != query.data_epoch {
            emit(
                &mut findings,
                "context.epoch_mismatch",
                text(&query.data_epoch.to_string()),
                hit_actual(hit),
                &hit_ref,
            )?;
        }

        let admitted = acl_ordered && hit.acl_checked && hit.acl_allowed;
        if hit.acl_allowed && !hit.acl_checked {
            emit(
                &mut findings,
                "context.acl_evidence_incoherent",
                text("admission_requires_a_recorded_acl_check"),
                hit_actual(hit),
                &hit_ref,
            )?;
        }
        if hit.ranked && !admitted {
            emit(
                &mut findings,
                "context.unauthorized_memory_hit",
                text("acl_allowed_before_ranking"),
                hit_actual(hit),
                &hit_ref,
            )?;
        }
        if hit.selected && (!hit.ranked || !admitted) {
            emit(
                &mut findings,
                "context.unauthorized_memory_hit",
                text("selected_hit_is_acl_admitted"),
                hit_actual(hit),
                &hit_ref,
            )?;
        }

        if hit.ranked {
            if hit.rank == 0 {
                emit(
                    &mut findings,
                    "context.rank_invalid",
                    text("positive_rank_for_a_ranked_hit"),
                    hit_actual(hit),
                    &hit_ref,
                )?;
            } else if !ranks.insert(u64::from(hit.rank)) {
                emit(
                    &mut findings,
                    "context.rank_duplicate",
                    text("unique_rank_per_ranked_hit"),
                    hit_actual(hit),
                    &hit_ref,
                )?;
            }
        } else if hit.rank != 0 {
            emit(
                &mut findings,
                "context.rank_invalid",
                text("zero_rank_for_an_unranked_hit"),
                hit_actual(hit),
                &hit_ref,
            )?;
        }
        if hit.selected {
            selected_count += 1;
            if hit.ranked && hit.rank != 0 {
                selected_ranks.insert(u64::from(hit.rank));
            }
        }

        if hit.stale_acknowledged && hit.freshness != ContextFreshness::Stale {
            emit(
                &mut findings,
                "context.stale_acknowledgement_unexpected",
                text("acknowledgement_only_for_stale_material"),
                hit_actual(hit),
                &hit_ref,
            )?;
        }
        if hit.selected {
            match hit.freshness {
                ContextFreshness::Current => {}
                ContextFreshness::Stale if hit.stale_acknowledged => {}
                ContextFreshness::Stale => emit(
                    &mut findings,
                    "context.stale_not_acknowledged",
                    text("stale_selection_has_an_acknowledgement"),
                    hit_actual(hit),
                    &hit_ref,
                )?,
                ContextFreshness::Unknown => emit(
                    &mut findings,
                    "context.freshness_unknown",
                    text("current_or_explicit_stale_selection"),
                    hit_actual(hit),
                    &hit_ref,
                )?,
            }
            if matches!(
                hit.evidence,
                ContextEvidenceStatus::Unverifiable | ContextEvidenceStatus::Missing
            ) {
                emit(
                    &mut findings,
                    "context.evidence_missing",
                    text("verified_or_attributed_evidence"),
                    hit_actual(hit),
                    &hit_ref,
                )?;
            }
        }
    }

    // Ranking coherence: ranked hits carry dense, unique one-based ranks and the presented set is
    // the top-ranked prefix, bounded by the declared `top_k`.
    if let Some(max_rank) = ranks.iter().next_back() {
        if *max_rank != ranks.len() as u64 {
            emit(
                &mut findings,
                "context.rank_gap",
                text("dense_one_based_rank"),
                text(&max_rank.to_string()),
                RANKING_EVIDENCE,
            )?;
        }
    }
    let selected_prefix: Vec<u64> = selected_ranks.iter().copied().collect();
    let expected_prefix: Vec<u64> = (1..=selected_count).collect();
    if selected_prefix != expected_prefix {
        emit(
            &mut findings,
            "context.selection_order_invalid",
            text("selected_hits_are_the_top_ranked_prefix"),
            serde_json::json!(selected_prefix),
            RANKING_EVIDENCE,
        )?;
    }
    if selected_count > query.top_k {
        emit(
            &mut findings,
            "context.selection_over_top_k",
            text(&query.top_k.to_string()),
            text(&selected_count.to_string()),
            RANKING_EVIDENCE,
        )?;
    }

    // Token/wire budget: positive limits, an estimate that covers the usage, and no overflow.
    if budget.token_limit == 0 || budget.wire_limit == 0 {
        emit(
            &mut findings,
            "context.budget_invalid",
            text("positive_token_and_wire_limits"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }
    if budget.used_tokens > budget.estimated_tokens {
        emit(
            &mut findings,
            "context.budget_estimate_invalid",
            text("estimated_tokens_at_least_used_tokens"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }
    if budget.used_tokens > budget.token_limit {
        emit(
            &mut findings,
            "context.budget_overflow",
            text("used_tokens_within_token_limit"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }
    if budget.estimated_tokens > budget.token_limit {
        emit(
            &mut findings,
            "context.budget_overflow",
            text("estimated_tokens_within_token_limit"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }
    if budget.reserved_output_tokens > budget.token_limit {
        emit(
            &mut findings,
            "context.budget_overflow",
            text("reserved_output_within_token_limit"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }
    if budget
        .used_tokens
        .saturating_add(budget.reserved_output_tokens)
        > budget.token_limit
    {
        emit(
            &mut findings,
            "context.budget_overflow",
            text("used_and_reserved_within_token_limit"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }
    if budget.wire_bytes > budget.wire_limit {
        emit(
            &mut findings,
            "context.budget_overflow",
            text("wire_bytes_within_wire_limit"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }
    if !budget.budget_enforced {
        emit(
            &mut findings,
            "context.budget_unenforced",
            text("token_budget_enforced"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }
    if !budget.wire_budget_enforced {
        emit(
            &mut findings,
            "context.budget_unenforced",
            text("wire_budget_enforced"),
            budget_actual(budget),
            BUDGET_EVIDENCE,
        )?;
    }

    // Compaction: an attempt demands a digest-bound summary over an ordered source range that
    // preserves the latest goal, pending pairs and verifiable completed actions while rejecting a
    // stale commit.  A summary/cursor without any attempt is incoherent.
    if compaction.attempted {
        if !compaction.summary_present {
            emit(
                &mut findings,
                "context.compaction_summary_missing",
                text("summary_present"),
                compaction_actual(compaction),
                COMPACTION_EVIDENCE,
            )?;
        } else if !is_sha256_digest(&compaction.summary_digest) {
            emit(
                &mut findings,
                "context.compaction_summary_unbound",
                text("sha256_summary_digest"),
                compaction_actual(compaction),
                COMPACTION_EVIDENCE,
            )?;
        }
        if compaction.source_cursor_start == 0
            || compaction.source_cursor_start > compaction.source_cursor_end
        {
            emit(
                &mut findings,
                "context.compaction_cursor_invalid",
                text("positive_ordered_source_range"),
                compaction_actual(compaction),
                COMPACTION_EVIDENCE,
            )?;
        }
        if !compaction.latest_goal_preserved
            || !compaction.pending_pairs_preserved
            || !compaction.completed_actions_verifiable
        {
            emit(
                &mut findings,
                "context.compaction_integrity_invalid",
                text("goal_pending_and_completed_actions_preserved"),
                compaction_actual(compaction),
                COMPACTION_EVIDENCE,
            )?;
        }
        if !compaction.stale_commit_rejected {
            emit(
                &mut findings,
                "context.compaction_stale_commit_unfenced",
                text("stale_commit_rejected"),
                compaction_actual(compaction),
                COMPACTION_EVIDENCE,
            )?;
        }
    } else if compaction.summary_present
        || !compaction.summary_digest.is_empty()
        || compaction.source_cursor_start != 0
        || compaction.source_cursor_end != 0
    {
        emit(
            &mut findings,
            "context.compaction_unexpected",
            text("no_compaction_evidence_without_an_attempt"),
            compaction_actual(compaction),
            COMPACTION_EVIDENCE,
        )?;
    }

    Ok(findings)
}

fn hit_sort_key(hit: &MemoryHitEvidence) -> (&str, u32, &str, &str, &str) {
    (
        hit.record_ref.as_str(),
        hit.rank,
        hit.source_revision.as_str(),
        hit.source_digest.as_str(),
        hit.provenance_digest.as_str(),
    )
}

fn query_actual(query: &ContextQueryEvidence) -> Value {
    serde_json::json!({
        "query_digest": query.query_digest,
        "scope_digest": query.scope_digest,
        "data_epoch": query.data_epoch,
        "generation": query.generation,
        "top_k": query.top_k,
    })
}

fn hit_actual(hit: &MemoryHitEvidence) -> Value {
    serde_json::json!({
        "record_ref": hit.record_ref,
        "scope_digest": hit.scope_digest,
        "data_epoch": hit.data_epoch,
        "acl_checked": hit.acl_checked,
        "acl_allowed": hit.acl_allowed,
        "ranked": hit.ranked,
        "selected": hit.selected,
        "rank": hit.rank,
        "source_revision": hit.source_revision,
        "source_digest": hit.source_digest,
        "provenance_digest": hit.provenance_digest,
        "freshness": hit.freshness,
        "evidence": hit.evidence,
        "stale_acknowledged": hit.stale_acknowledged,
    })
}

fn budget_actual(budget: &ContextBudgetEvidence) -> Value {
    // Keys stay inside the shared redaction allowlist for token metrics so the observed numbers
    // survive `Finding::redacted` instead of being replaced by the secret placeholder.
    serde_json::json!({
        "token_budget": budget.token_limit,
        "estimated_tokens": budget.estimated_tokens,
        "tokens_used": budget.used_tokens,
        "reserved_tokens": budget.reserved_output_tokens,
        "budget_enforced": budget.budget_enforced,
        "wire_bytes": budget.wire_bytes,
        "wire_limit": budget.wire_limit,
        "wire_budget_enforced": budget.wire_budget_enforced,
    })
}

fn compaction_actual(compaction: &CompactionEvidence) -> Value {
    serde_json::json!({
        "attempted": compaction.attempted,
        "summary_present": compaction.summary_present,
        "summary_digest": compaction.summary_digest,
        "source_cursor_start": compaction.source_cursor_start,
        "source_cursor_end": compaction.source_cursor_end,
        "latest_goal_preserved": compaction.latest_goal_preserved,
        "pending_pairs_preserved": compaction.pending_pairs_preserved,
        "completed_actions_verifiable": compaction.completed_actions_verifiable,
        "stale_commit_rejected": compaction.stale_commit_rejected,
    })
}

fn check_schema(
    findings: &mut Vec<Finding>,
    code: &'static str,
    actual: &str,
    expected: &'static str,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if actual != expected {
        emit(findings, code, text(expected), text(actual), evidence_ref)?;
    }
    Ok(())
}

/// Bounded, single-line, secret-free reference accepted in hit and source identities.
fn is_bounded_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'#' | b'-' | b'@')
        })
        && redact_text(value) == value
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// Push one finding.  Caller-supplied values go through [`Finding::redacted`] so a secret-shaped
/// reference cannot escape through the finding itself.
fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: Value,
    actual: Value,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::redacted(
            code,
            expected,
            actual,
            format!("context and memory finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
