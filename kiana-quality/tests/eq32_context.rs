//! EQ-32 context/memory fixtures.
//!
//! Rejection paths come first. Every dimension named by the card - ACL-before-ranking,
//! provenance, freshness, compaction and budget - has a dedicated rejection fixture before the
//! success paths that keep a fully admitted retrieval report green.

use kiana_quality::{
    canonical_findings, digest_findings, CompactionEvidence, ContextBudgetEvidence,
    ContextEvidenceStatus, ContextFreshness, ContextMemoryEvaluator, ContextMemoryInput,
    ContextQueryEvidence, DeterministicEvaluator, EvaluatorRegistry, Finding, MemoryHitEvidence,
    CONTEXT_MEMORY_EVALUATOR_ID, CONTEXT_MEMORY_INPUT_SCHEMA, EVALUATION_RESULT_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const QUERY_SCHEMA: &str = "kiana.quality-context-query-evidence.v1";
const HIT_SCHEMA: &str = "kiana.quality-memory-hit-evidence.v1";
const BUDGET_SCHEMA: &str = "kiana.quality-context-budget-evidence.v1";
const COMPACTION_SCHEMA: &str = "kiana.quality-compaction-evidence.v1";

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn codes(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect()
}

fn count(findings: &[Finding], code: &str) -> usize {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .count()
}

fn evidence_refs(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.evidence_ref.clone())
        .collect()
}

fn evaluate(input: &Value) -> Vec<Finding> {
    ContextMemoryEvaluator
        .evaluate(input)
        .expect("context memory evidence decodes")
}

fn rejected(input: &Value) -> bool {
    ContextMemoryEvaluator.evaluate(input).is_err()
}

fn valid_input() -> Value {
    json!({
        "schema": CONTEXT_MEMORY_INPUT_SCHEMA,
        "acl_checked_before_ranking": true,
        "query": {
            "schema": QUERY_SCHEMA,
            "query_digest": DIGEST,
            "scope_digest": DIGEST,
            "data_epoch": 4,
            "generation": 7,
            "top_k": 2,
        },
        "hits": [
            {
                "schema": HIT_SCHEMA,
                "record_ref": "memory:one",
                "scope_digest": DIGEST,
                "data_epoch": 4,
                "acl_checked": true,
                "acl_allowed": true,
                "ranked": true,
                "selected": true,
                "rank": 1,
                "source_revision": "revision:one",
                "source_digest": DIGEST,
                "provenance_digest": DIGEST,
                "freshness": "current",
                "evidence": "verified",
                "stale_acknowledged": false,
            },
            {
                "schema": HIT_SCHEMA,
                "record_ref": "memory:two",
                "scope_digest": DIGEST,
                "data_epoch": 4,
                "acl_checked": true,
                "acl_allowed": true,
                "ranked": true,
                "selected": false,
                "rank": 2,
                "source_revision": "revision:two",
                "source_digest": OTHER_DIGEST,
                "provenance_digest": OTHER_DIGEST,
                "freshness": "stale",
                "evidence": "attributed",
                "stale_acknowledged": false,
            },
        ],
        "budget": {
            "schema": BUDGET_SCHEMA,
            "token_limit": 100,
            "estimated_tokens": 70,
            "used_tokens": 60,
            "reserved_output_tokens": 20,
            "budget_enforced": true,
            "wire_bytes": 700,
            "wire_limit": 1000,
            "wire_budget_enforced": true,
        },
        "compaction": {
            "schema": COMPACTION_SCHEMA,
            "attempted": true,
            "summary_present": true,
            "summary_digest": DIGEST,
            "source_cursor_start": 10,
            "source_cursor_end": 12,
            "latest_goal_preserved": true,
            "pending_pairs_preserved": true,
            "completed_actions_verifiable": true,
            "stale_commit_rejected": true,
        },
    })
}

fn violated_input() -> Value {
    let mut input = valid_input();
    input["acl_checked_before_ranking"] = json!(false);
    input["hits"][0]["freshness"] = json!("unknown");
    input["hits"][0]["evidence"] = json!("missing");
    input["budget"]["estimated_tokens"] = json!(101);
    input["budget"]["budget_enforced"] = json!(false);
    input["compaction"]["latest_goal_preserved"] = json!(false);
    input
}

#[test]
fn unauthorized_memory_hit_is_blocking() {
    let mut input = valid_input();
    input["acl_checked_before_ranking"] = json!(false);
    input["hits"] = json!([{
        "schema": HIT_SCHEMA,
        "record_ref": "memory:unauthorized",
        "scope_digest": DIGEST,
        "data_epoch": 4,
        "acl_checked": false,
        "acl_allowed": false,
        "ranked": true,
        "selected": true,
        "rank": 1,
        "source_revision": "revision:one",
        "source_digest": DIGEST,
        "provenance_digest": DIGEST,
        "freshness": "unknown",
        "evidence": "missing",
        "stale_acknowledged": false,
    }]);

    let findings = evaluate(&input);
    assert_eq!(
        codes(&findings),
        set(&[
            "context.acl_order_invalid",
            "context.unauthorized_memory_hit",
            "context.freshness_unknown",
            "context.evidence_missing",
        ])
    );
    assert_eq!(count(&findings, "context.unauthorized_memory_hit"), 2);
    let unauthorized: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.code == "context.unauthorized_memory_hit")
        .collect();
    for finding in unauthorized {
        assert_eq!(finding.evidence_ref, "context:hit:0");
        assert_eq!(finding.actual["record_ref"], json!("memory:unauthorized"));
        assert_eq!(finding.actual["acl_checked"], json!(false));
        assert_eq!(finding.actual["acl_allowed"], json!(false));
        assert_eq!(finding.actual["ranked"], json!(true));
        assert_eq!(finding.actual["selected"], json!(true));
    }
    let order = findings
        .iter()
        .find(|finding| finding.code == "context.acl_order_invalid")
        .unwrap();
    assert_eq!(order.evidence_ref, "context:acl");
    assert_eq!(order.expected, json!("acl_before_ranking"));
    assert_eq!(order.actual, json!("ranking_before_acl"));

    // Presented material without a recorded admission never becomes a success: the ranked and the
    // selected violations stay separate findings with their own expectations.
    let expectations: BTreeSet<String> = findings
        .iter()
        .filter(|finding| finding.code == "context.unauthorized_memory_hit")
        .map(|finding| finding.expected.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        expectations,
        set(&["acl_allowed_before_ranking", "selected_hit_is_acl_admitted"])
    );
}

#[test]
fn admitted_current_context_has_no_findings() {
    let findings = evaluate(&valid_input());
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn acl_evidence_cannot_be_claimed_without_a_recorded_check() {
    let mut input = valid_input();
    input["hits"][0]["acl_checked"] = json!(false);
    input["hits"][0]["acl_allowed"] = json!(true);
    input["hits"][0]["ranked"] = json!(false);
    input["hits"][0]["selected"] = json!(false);
    input["hits"][0]["rank"] = json!(0);
    input["hits"][1]["ranked"] = json!(false);
    input["hits"][1]["rank"] = json!(0);

    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["context.acl_evidence_incoherent"]));
    let incoherent = &findings[0];
    assert_eq!(incoherent.evidence_ref, "context:hit:0");
    assert_eq!(incoherent.actual["acl_checked"], json!(false));
    assert_eq!(incoherent.actual["acl_allowed"], json!(true));

    // The same claim combined with ranking/selection also violates ACL-before-ranking.
    let mut ranked = valid_input();
    ranked["hits"][0]["acl_checked"] = json!(false);
    ranked["hits"][0]["acl_allowed"] = json!(true);
    let findings = evaluate(&ranked);
    assert_eq!(
        codes(&findings),
        set(&[
            "context.acl_evidence_incoherent",
            "context.unauthorized_memory_hit"
        ])
    );
    assert_eq!(count(&findings, "context.unauthorized_memory_hit"), 2);
}

#[test]
fn acl_denied_hits_are_never_ranked_or_selected() {
    let mut input = valid_input();
    input["hits"][0]["acl_allowed"] = json!(false);

    let findings = evaluate(&input);
    assert_eq!(codes(&findings), set(&["context.unauthorized_memory_hit"]));
    assert_eq!(count(&findings, "context.unauthorized_memory_hit"), 2);

    // An unranked, unselected denied hit stays silent: denial alone is not a quality violation.
    let mut denied = valid_input();
    denied["hits"] = json!([{
        "schema": HIT_SCHEMA,
        "record_ref": "memory:denied",
        "scope_digest": DIGEST,
        "data_epoch": 4,
        "acl_checked": true,
        "acl_allowed": false,
        "ranked": false,
        "selected": false,
        "rank": 0,
        "source_revision": "revision:one",
        "source_digest": DIGEST,
        "provenance_digest": DIGEST,
        "freshness": "current",
        "evidence": "verified",
        "stale_acknowledged": false,
    }]);
    assert!(evaluate(&denied).is_empty());
}

#[test]
fn rank_order_must_be_unique_and_dense() {
    // Duplicate ranks: two ranked hits claim the same position.
    let mut duplicate = valid_input();
    duplicate["hits"][1]["rank"] = json!(1);
    let findings = evaluate(&duplicate);
    assert_eq!(codes(&findings), set(&["context.rank_duplicate"]));
    assert_eq!(findings[0].evidence_ref, "context:hit:1");

    // A gap in the one-based rank order is not a dense ranking.
    let mut gap = valid_input();
    gap["hits"][1]["rank"] = json!(3);
    let findings = evaluate(&gap);
    assert_eq!(codes(&findings), set(&["context.rank_gap"]));
    assert_eq!(findings[0].actual, json!("3"));

    // A ranked hit without a positive rank.
    let mut zero = valid_input();
    zero["hits"][0]["selected"] = json!(false);
    zero["hits"][0]["rank"] = json!(0);
    zero["hits"][1]["ranked"] = json!(false);
    zero["hits"][1]["rank"] = json!(0);
    let findings = evaluate(&zero);
    assert_eq!(codes(&findings), set(&["context.rank_invalid"]));
    assert_eq!(
        findings[0].expected,
        json!("positive_rank_for_a_ranked_hit")
    );

    // An unranked hit must not carry a rank.
    let mut unranked = valid_input();
    unranked["hits"][1]["ranked"] = json!(false);
    unranked["hits"][1]["rank"] = json!(2);
    let findings = evaluate(&unranked);
    assert_eq!(codes(&findings), set(&["context.rank_invalid"]));
    assert_eq!(findings[0].expected, json!("zero_rank_for_an_unranked_hit"));
}

#[test]
fn selection_respects_the_top_k_bound_and_the_rank_prefix() {
    // Presenting more hits than the declared selection bound.
    let mut overflow = valid_input();
    overflow["query"]["top_k"] = json!(1);
    overflow["hits"][1]["selected"] = json!(true);
    overflow["hits"][1]["freshness"] = json!("current");
    overflow["hits"][1]["evidence"] = json!("verified");
    let findings = evaluate(&overflow);
    assert_eq!(codes(&findings), set(&["context.selection_over_top_k"]));
    assert_eq!(findings[0].evidence_ref, "context:ranking");
    assert_eq!(findings[0].expected, json!("1"));
    assert_eq!(findings[0].actual, json!("2"));

    // The presented set must be the top-ranked prefix, not an arbitrary later rank.
    let mut skipped = valid_input();
    skipped["hits"][0]["selected"] = json!(false);
    skipped["hits"][1]["selected"] = json!(true);
    skipped["hits"][1]["freshness"] = json!("current");
    skipped["hits"][1]["evidence"] = json!("verified");
    let findings = evaluate(&skipped);
    assert_eq!(codes(&findings), set(&["context.selection_order_invalid"]));
    assert_eq!(findings[0].actual, json!([2]));

    // A zero selection bound is refused up front.
    let mut zero = valid_input();
    zero["query"]["top_k"] = json!(0);
    zero["hits"] = json!([]);
    let findings = evaluate(&zero);
    assert_eq!(codes(&findings), set(&["context.selection_bound_invalid"]));
}

#[test]
fn hit_identity_and_provenance_are_bounded() {
    let mut revision = valid_input();
    revision["hits"][0]["source_revision"] = json!("revision one");
    let findings = evaluate(&revision);
    assert_eq!(codes(&findings), set(&["context.provenance_invalid"]));

    let mut short_digest = valid_input();
    short_digest["hits"][0]["source_digest"] = json!(format!("sha256:{}", "a".repeat(63)));
    let findings = evaluate(&short_digest);
    assert_eq!(codes(&findings), set(&["context.provenance_invalid"]));

    let mut prefixed = valid_input();
    prefixed["hits"][0]["provenance_digest"] = json!("deadbeef");
    let findings = evaluate(&prefixed);
    assert_eq!(codes(&findings), set(&["context.provenance_invalid"]));

    // Duplicate record identities stay visible: the second occurrence is bound to its own index.
    let mut duplicated = valid_input();
    duplicated["hits"][1]["record_ref"] = json!("memory:one");
    let findings = evaluate(&duplicated);
    assert_eq!(codes(&findings), set(&["context.hit_identity_invalid"]));
    assert_eq!(findings[0].evidence_ref, "context:hit:1");
    assert_eq!(findings[0].actual["record_ref"], json!("memory:one"));
}

#[test]
fn hit_scope_and_epoch_must_match_the_query() {
    let mut drifted_scope = valid_input();
    drifted_scope["hits"][0]["scope_digest"] = json!(OTHER_DIGEST);
    let findings = evaluate(&drifted_scope);
    assert_eq!(codes(&findings), set(&["context.scope_mismatch"]));
    assert_eq!(findings[0].expected, json!(DIGEST));
    assert_eq!(findings[0].actual["scope_digest"], json!(OTHER_DIGEST));

    let mut invalid_scope = valid_input();
    invalid_scope["hits"][0]["scope_digest"] = json!("not-a-digest");
    let findings = evaluate(&invalid_scope);
    assert_eq!(codes(&findings), set(&["context.scope_mismatch"]));

    let mut drifted_epoch = valid_input();
    drifted_epoch["hits"][0]["data_epoch"] = json!(5);
    let findings = evaluate(&drifted_epoch);
    assert_eq!(codes(&findings), set(&["context.epoch_mismatch"]));
    assert_eq!(findings[0].expected, json!("4"));

    let mut zero_epoch = valid_input();
    zero_epoch["hits"][0]["data_epoch"] = json!(0);
    let findings = evaluate(&zero_epoch);
    assert_eq!(codes(&findings), set(&["context.epoch_mismatch"]));
}

#[test]
fn stale_selection_requires_an_acknowledgement() {
    let mut stale = valid_input();
    stale["hits"][0]["freshness"] = json!("stale");
    let findings = evaluate(&stale);
    assert_eq!(codes(&findings), set(&["context.stale_not_acknowledged"]));
    assert_eq!(findings[0].evidence_ref, "context:hit:0");

    let mut acknowledged = valid_input();
    acknowledged["hits"][0]["freshness"] = json!("stale");
    acknowledged["hits"][0]["stale_acknowledged"] = json!(true);
    assert!(evaluate(&acknowledged).is_empty());

    // An acknowledgement outside stale material is incoherent evidence.
    let mut unexpected = valid_input();
    unexpected["hits"][0]["stale_acknowledged"] = json!(true);
    let findings = evaluate(&unexpected);
    assert_eq!(
        codes(&findings),
        set(&["context.stale_acknowledgement_unexpected"])
    );

    // Stale material that is not presented needs no acknowledgement.
    assert!(evaluate(&valid_input()).is_empty());
}

#[test]
fn freshness_unknown_is_never_silently_selected() {
    let mut unknown = valid_input();
    unknown["hits"][0]["freshness"] = json!("unknown");
    let findings = evaluate(&unknown);
    assert_eq!(codes(&findings), set(&["context.freshness_unknown"]));
    assert_eq!(findings[0].evidence_ref, "context:hit:0");

    // A ranked but unselected unknown candidate may stay in the report.
    let mut candidate = valid_input();
    candidate["hits"][1]["freshness"] = json!("unknown");
    assert!(evaluate(&candidate).is_empty());
}

#[test]
fn evidence_status_must_be_explicit_for_selected_hits() {
    for status in ["missing", "unverifiable"] {
        let mut input = valid_input();
        input["hits"][0]["evidence"] = json!(status);
        let findings = evaluate(&input);
        assert_eq!(
            codes(&findings),
            set(&["context.evidence_missing"]),
            "unexpected findings for evidence status {status}"
        );
        assert_eq!(findings[0].evidence_ref, "context:hit:0");
    }

    let mut attributed = valid_input();
    attributed["hits"][0]["evidence"] = json!("attributed");
    assert!(evaluate(&attributed).is_empty());
}

#[test]
fn query_provenance_is_fail_closed() {
    let mut bad_query_digest = valid_input();
    bad_query_digest["hits"] = json!([]);
    bad_query_digest["query"]["query_digest"] = json!("sha256:not-hex");
    let findings = evaluate(&bad_query_digest);
    assert_eq!(codes(&findings), set(&["context.query_provenance_invalid"]));

    let mut bad_scope_digest = valid_input();
    bad_scope_digest["query"]["scope_digest"] = json!("not-a-digest");
    let findings = evaluate(&bad_scope_digest);
    assert_eq!(count(&findings, "context.query_provenance_invalid"), 1);
    assert_eq!(count(&findings, "context.scope_mismatch"), 2);

    let mut zero_epoch = valid_input();
    zero_epoch["hits"] = json!([]);
    zero_epoch["query"]["data_epoch"] = json!(0);
    let findings = evaluate(&zero_epoch);
    assert_eq!(codes(&findings), set(&["context.query_provenance_invalid"]));

    let mut zero_generation = valid_input();
    zero_generation["hits"] = json!([]);
    zero_generation["query"]["generation"] = json!(0);
    let findings = evaluate(&zero_generation);
    assert_eq!(codes(&findings), set(&["context.query_provenance_invalid"]));
}

#[test]
fn budget_overflow_and_estimate_bounds_are_enforced() {
    let mut estimate = valid_input();
    estimate["budget"]["estimated_tokens"] = json!(101);
    let findings = evaluate(&estimate);
    assert_eq!(codes(&findings), set(&["context.budget_overflow"]));
    assert_eq!(
        findings[0].expected,
        json!("estimated_tokens_within_token_limit")
    );

    let mut wire = valid_input();
    wire["budget"]["wire_bytes"] = json!(1001);
    let findings = evaluate(&wire);
    assert_eq!(codes(&findings), set(&["context.budget_overflow"]));
    assert_eq!(findings[0].expected, json!("wire_bytes_within_wire_limit"));

    // Used plus reserved output must fit the same token window.
    let mut reserved = valid_input();
    reserved["budget"]["reserved_output_tokens"] = json!(90);
    let findings = evaluate(&reserved);
    assert_eq!(codes(&findings), set(&["context.budget_overflow"]));
    assert_eq!(
        findings[0].expected,
        json!("used_and_reserved_within_token_limit")
    );

    // An estimate below the observed usage is not an estimate.
    let mut under = valid_input();
    under["budget"]["used_tokens"] = json!(80);
    let findings = evaluate(&under);
    assert_eq!(codes(&findings), set(&["context.budget_estimate_invalid"]));

    // Zero limits cannot bound anything.
    let mut zero_token_limit = valid_input();
    zero_token_limit["budget"] = json!({
        "schema": BUDGET_SCHEMA,
        "token_limit": 0,
        "estimated_tokens": 0,
        "used_tokens": 0,
        "reserved_output_tokens": 0,
        "budget_enforced": true,
        "wire_bytes": 0,
        "wire_limit": 1000,
        "wire_budget_enforced": true,
    });
    let findings = evaluate(&zero_token_limit);
    assert_eq!(codes(&findings), set(&["context.budget_invalid"]));

    let mut zero_wire_limit = valid_input();
    zero_wire_limit["budget"]["wire_limit"] = json!(0);
    zero_wire_limit["budget"]["wire_bytes"] = json!(0);
    let findings = evaluate(&zero_wire_limit);
    assert_eq!(codes(&findings), set(&["context.budget_invalid"]));
}

#[test]
fn budget_must_be_enforced_for_tokens_and_wire() {
    let mut token = valid_input();
    token["budget"]["budget_enforced"] = json!(false);
    let findings = evaluate(&token);
    assert_eq!(codes(&findings), set(&["context.budget_unenforced"]));
    assert_eq!(findings[0].expected, json!("token_budget_enforced"));

    let mut wire = valid_input();
    wire["budget"]["wire_budget_enforced"] = json!(false);
    let findings = evaluate(&wire);
    assert_eq!(codes(&findings), set(&["context.budget_unenforced"]));
    assert_eq!(findings[0].expected, json!("wire_budget_enforced"));

    let mut both = valid_input();
    both["budget"]["budget_enforced"] = json!(false);
    both["budget"]["wire_budget_enforced"] = json!(false);
    let findings = evaluate(&both);
    assert_eq!(codes(&findings), set(&["context.budget_unenforced"]));
    assert_eq!(findings.len(), 2);
}

#[test]
fn compaction_requires_summary_cursor_and_preservation() {
    let mut summary = valid_input();
    summary["compaction"]["summary_present"] = json!(false);
    let findings = evaluate(&summary);
    assert_eq!(
        codes(&findings),
        set(&["context.compaction_summary_missing"])
    );

    let mut reversed = valid_input();
    reversed["compaction"]["source_cursor_start"] = json!(12);
    reversed["compaction"]["source_cursor_end"] = json!(10);
    let findings = evaluate(&reversed);
    assert_eq!(
        codes(&findings),
        set(&["context.compaction_cursor_invalid"])
    );

    let mut zero_cursor = valid_input();
    zero_cursor["compaction"]["source_cursor_start"] = json!(0);
    let findings = evaluate(&zero_cursor);
    assert_eq!(
        codes(&findings),
        set(&["context.compaction_cursor_invalid"])
    );

    for field in [
        "latest_goal_preserved",
        "pending_pairs_preserved",
        "completed_actions_verifiable",
    ] {
        let mut input = valid_input();
        input["compaction"][field] = json!(false);
        let findings = evaluate(&input);
        assert_eq!(
            codes(&findings),
            set(&["context.compaction_integrity_invalid"]),
            "unexpected findings without {field}"
        );
    }

    let mut stale_commit = valid_input();
    stale_commit["compaction"]["stale_commit_rejected"] = json!(false);
    let findings = evaluate(&stale_commit);
    assert_eq!(
        codes(&findings),
        set(&["context.compaction_stale_commit_unfenced"])
    );
}

#[test]
fn compaction_summary_digest_must_be_a_strict_sha256() {
    let mut valid = valid_input();
    valid["compaction"]["summary_digest"] = json!(OTHER_DIGEST);
    assert!(evaluate(&valid).is_empty());

    let mut short = valid_input();
    short["compaction"]["summary_digest"] = json!(format!("sha256:{}", "a".repeat(63)));
    let findings = evaluate(&short);
    assert_eq!(
        codes(&findings),
        set(&["context.compaction_summary_unbound"])
    );

    let mut empty = valid_input();
    empty["compaction"]["summary_digest"] = json!("");
    let findings = evaluate(&empty);
    assert_eq!(
        codes(&findings),
        set(&["context.compaction_summary_unbound"])
    );
}

#[test]
fn compaction_evidence_without_an_attempt_is_unexpected() {
    let mut none = valid_input();
    none["compaction"] = json!({
        "schema": COMPACTION_SCHEMA,
        "attempted": false,
        "summary_present": false,
        "summary_digest": "",
        "source_cursor_start": 0,
        "source_cursor_end": 0,
        "latest_goal_preserved": false,
        "pending_pairs_preserved": false,
        "completed_actions_verifiable": false,
        "stale_commit_rejected": false,
    });
    assert!(evaluate(&none).is_empty());

    let mut stray_summary = none.clone();
    stray_summary["compaction"]["summary_present"] = json!(true);
    let findings = evaluate(&stray_summary);
    assert_eq!(codes(&findings), set(&["context.compaction_unexpected"]));

    let mut stray_cursor = none.clone();
    stray_cursor["compaction"]["source_cursor_start"] = json!(10);
    let findings = evaluate(&stray_cursor);
    assert_eq!(codes(&findings), set(&["context.compaction_unexpected"]));

    let mut stray_digest = none.clone();
    stray_digest["compaction"]["summary_digest"] = json!(DIGEST);
    let findings = evaluate(&stray_digest);
    assert_eq!(codes(&findings), set(&["context.compaction_unexpected"]));
}

#[test]
fn schema_drift_on_every_context_section_is_rejected() {
    let mut query = valid_input();
    query["query"]["schema"] = json!("kiana.quality-context-query-evidence.v0");
    let findings = evaluate(&query);
    assert_eq!(codes(&findings), set(&["context.query_schema_invalid"]));

    let mut hit = valid_input();
    hit["hits"][0]["schema"] = json!("kiana.quality-memory-hit-evidence.v0");
    let findings = evaluate(&hit);
    assert_eq!(codes(&findings), set(&["context.hit_schema_invalid"]));
    assert_eq!(findings[0].evidence_ref, "context:hit:0");

    let mut budget = valid_input();
    budget["budget"]["schema"] = json!("kiana.quality-context-budget-evidence.v0");
    let findings = evaluate(&budget);
    assert_eq!(codes(&findings), set(&["context.budget_schema_invalid"]));

    let mut compaction = valid_input();
    compaction["compaction"]["schema"] = json!("kiana.quality-compaction-evidence.v0");
    let findings = evaluate(&compaction);
    assert_eq!(
        codes(&findings),
        set(&["context.compaction_schema_invalid"])
    );
}

#[test]
fn unknown_fields_and_envelope_drift_fail_closed() {
    let mut top_level = valid_input();
    top_level["unexpected"] = json!(true);
    assert!(rejected(&top_level));

    let mut query = valid_input();
    query["query"]["unexpected"] = json!(1);
    assert!(rejected(&query));

    let mut hit = valid_input();
    hit["hits"][0]["unexpected"] = json!(1);
    assert!(rejected(&hit));

    let mut budget = valid_input();
    budget["budget"]["unexpected"] = json!(1);
    assert!(rejected(&budget));

    let mut compaction = valid_input();
    compaction["compaction"]["unexpected"] = json!(1);
    assert!(rejected(&compaction));

    let mut envelope = valid_input();
    envelope["schema"] = json!("kiana.quality-context-memory-input.v0");
    assert!(rejected(&envelope));

    let mut wrong_hits = valid_input();
    wrong_hits["hits"] = json!({"not": "an array"});
    assert!(rejected(&wrong_hits));

    let mut missing_compaction = valid_input();
    missing_compaction
        .as_object_mut()
        .unwrap()
        .remove("compaction");
    assert!(rejected(&missing_compaction));

    let mut wrong_flag = valid_input();
    wrong_flag["acl_checked_before_ranking"] = json!("yes");
    assert!(rejected(&wrong_flag));

    let mut negative_rank = valid_input();
    negative_rank["hits"][0]["rank"] = json!(-1);
    assert!(rejected(&negative_rank));

    let mut wrong_freshness = valid_input();
    wrong_freshness["hits"][0]["freshness"] = json!("ancient");
    assert!(rejected(&wrong_freshness));

    assert!(rejected(&json!("not an object")));
    assert!(rejected(&json!({})));

    // `hits` is optional and an empty report stays evaluable.
    let mut no_hits = valid_input();
    no_hits.as_object_mut().unwrap().remove("hits");
    assert!(evaluate(&no_hits).is_empty());
}

#[test]
fn bounded_references_fail_closed() {
    let mut oversized_ref = valid_input();
    oversized_ref["hits"][0]["record_ref"] = json!("m".repeat(513));
    let findings = evaluate(&oversized_ref);
    assert_eq!(codes(&findings), set(&["context.hit_identity_invalid"]));

    let mut oversized_revision = valid_input();
    oversized_revision["hits"][0]["source_revision"] = json!("r".repeat(513));
    let findings = evaluate(&oversized_revision);
    assert_eq!(codes(&findings), set(&["context.provenance_invalid"]));

    let mut newline = valid_input();
    newline["hits"][0]["record_ref"] = json!("memory:one\ntwo");
    let findings = evaluate(&newline);
    assert_eq!(codes(&findings), set(&["context.hit_identity_invalid"]));

    let mut empty_ref = valid_input();
    empty_ref["hits"][0]["record_ref"] = json!("");
    let findings = evaluate(&empty_ref);
    assert_eq!(codes(&findings), set(&["context.hit_identity_invalid"]));

    let mut empty_revision = valid_input();
    empty_revision["hits"][0]["source_revision"] = json!("");
    let findings = evaluate(&empty_revision);
    assert_eq!(codes(&findings), set(&["context.provenance_invalid"]));
}

#[test]
fn oversized_hits_fail_closed_before_evaluation() {
    let base = valid_input();
    let hit = base["hits"][0].clone();

    let hits: Vec<Value> = (0..513)
        .map(|index| {
            let mut hit = hit.clone();
            hit["record_ref"] = json!(format!("memory:hit-{index}"));
            hit
        })
        .collect();
    let mut oversized = base.clone();
    oversized["hits"] = json!(hits);
    let typed: ContextMemoryInput = serde_json::from_value(oversized).unwrap();
    assert_eq!(typed.validate(), Err("context_memory_hit_limit_exceeded"));
    assert!(ContextMemoryEvaluator.evaluate(&typed.as_value()).is_err());

    let hits: Vec<Value> = (0..512)
        .map(|index| {
            let mut hit = hit.clone();
            hit["record_ref"] = json!(format!("memory:hit-{index}"));
            hit
        })
        .collect();
    let mut bounded = base;
    bounded["hits"] = json!(hits);
    let typed: ContextMemoryInput = serde_json::from_value(bounded).unwrap();
    assert_eq!(typed.validate(), Ok(()));
}

#[test]
fn hit_order_does_not_change_the_findings() {
    let mut input = valid_input();
    input["hits"][0]["freshness"] = json!("stale");
    input["hits"][1]["selected"] = json!(true);
    input["hits"][1]["freshness"] = json!("unknown");
    input["hits"][1]["evidence"] = json!("verified");

    let mut reversed = input.clone();
    let mut hits = reversed["hits"].as_array().unwrap().clone();
    hits.reverse();
    reversed["hits"] = json!(hits);

    let first = canonical_findings(evaluate(&input)).unwrap();
    let second = canonical_findings(evaluate(&reversed)).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        evidence_refs(&first),
        set(&["context:hit:0", "context:hit:1"])
    );

    // Duplicate evidence stays duplicated and visible under distinct evidence references.
    let mut duplicate = valid_input();
    let mut first_hit = duplicate["hits"][0].clone();
    first_hit["record_ref"] = json!("memory:dup");
    first_hit["freshness"] = json!("stale");
    let mut second_hit = first_hit.clone();
    second_hit["rank"] = json!(2);
    second_hit["selected"] = json!(true);
    duplicate["hits"] = json!([first_hit, second_hit]);

    let findings = evaluate(&duplicate);
    assert_eq!(count(&findings, "context.hit_identity_invalid"), 1);
    assert_eq!(count(&findings, "context.stale_not_acknowledged"), 2);
    assert_eq!(
        evidence_refs(&findings),
        set(&["context:hit:0", "context:hit:1"])
    );
}

#[test]
fn redaction_boundary_covers_caller_supplied_values() {
    let mut identity = valid_input();
    let mut hit = identity["hits"][0].clone();
    hit["record_ref"] = json!("token=supersecret");
    identity["hits"] = json!([hit]);
    let findings = evaluate(&identity);
    let invalid = findings
        .iter()
        .find(|finding| finding.code == "context.hit_identity_invalid")
        .unwrap();
    assert_eq!(invalid.evidence_ref, "context:hit:0");
    assert_eq!(invalid.actual["record_ref"], json!("token=[REDACTED]"));
    assert!(!json!(findings).to_string().contains("supersecret"));

    let mut revision = valid_input();
    revision["hits"][0]["source_revision"] = json!("token=supersecret");
    let findings = evaluate(&revision);
    let provenance = findings
        .iter()
        .find(|finding| finding.code == "context.provenance_invalid")
        .unwrap();
    assert_eq!(
        provenance.actual["source_revision"],
        json!("token=[REDACTED]")
    );
    assert!(!json!(findings).to_string().contains("supersecret"));
}

#[test]
fn context_contracts_round_trip_through_typed_input() {
    let typed = ContextMemoryInput::new(
        true,
        ContextQueryEvidence {
            schema: QUERY_SCHEMA.to_owned(),
            query_digest: DIGEST.to_owned(),
            scope_digest: DIGEST.to_owned(),
            data_epoch: 4,
            generation: 7,
            top_k: 2,
        },
        vec![MemoryHitEvidence {
            schema: HIT_SCHEMA.to_owned(),
            record_ref: "memory:one".to_owned(),
            scope_digest: DIGEST.to_owned(),
            data_epoch: 4,
            acl_checked: true,
            acl_allowed: true,
            ranked: true,
            selected: true,
            rank: 1,
            source_revision: "revision:one".to_owned(),
            source_digest: DIGEST.to_owned(),
            provenance_digest: DIGEST.to_owned(),
            freshness: ContextFreshness::Current,
            evidence: ContextEvidenceStatus::Verified,
            stale_acknowledged: false,
        }],
        ContextBudgetEvidence {
            schema: BUDGET_SCHEMA.to_owned(),
            token_limit: 100,
            estimated_tokens: 70,
            used_tokens: 60,
            reserved_output_tokens: 20,
            budget_enforced: true,
            wire_bytes: 700,
            wire_limit: 1000,
            wire_budget_enforced: true,
        },
        CompactionEvidence {
            schema: COMPACTION_SCHEMA.to_owned(),
            attempted: true,
            summary_present: true,
            summary_digest: DIGEST.to_owned(),
            source_cursor_start: 10,
            source_cursor_end: 12,
            latest_goal_preserved: true,
            pending_pairs_preserved: true,
            completed_actions_verifiable: true,
            stale_commit_rejected: true,
        },
    );
    assert_eq!(typed.validate(), Ok(()));
    let value = typed.as_value();
    assert_eq!(value["schema"], json!(CONTEXT_MEMORY_INPUT_SCHEMA));
    let decoded: ContextMemoryInput = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded, typed);
    assert!(ContextMemoryEvaluator.evaluate(&value).unwrap().is_empty());

    let mut tampered = value;
    tampered["schema"] = json!("kiana.quality-context-memory-input.v0");
    assert!(ContextMemoryEvaluator.evaluate(&tampered).is_err());
}

#[test]
fn context_output_is_deterministic_evidence_bound_and_registry_wired() {
    let input = violated_input();
    let first = evaluate(&input);
    let second = evaluate(&input);
    assert_eq!(first, second);
    assert_eq!(
        digest_findings(&first).unwrap(),
        digest_findings(&second).unwrap()
    );
    assert_eq!(
        codes(&first),
        set(&[
            "context.acl_order_invalid",
            "context.unauthorized_memory_hit",
            "context.freshness_unknown",
            "context.evidence_missing",
            "context.budget_overflow",
            "context.budget_unenforced",
            "context.compaction_integrity_invalid",
        ])
    );

    for finding in &first {
        assert!(
            finding.code.starts_with("context."),
            "unstable finding code: {}",
            finding.code
        );
        assert!(
            finding.evidence_ref.starts_with("context:"),
            "missing evidence reference: {}",
            finding.evidence_ref
        );
    }

    let mut registry = EvaluatorRegistry::new();
    registry.register(ContextMemoryEvaluator).unwrap();
    assert_eq!(registry.evaluator_ids(), vec![CONTEXT_MEMORY_EVALUATOR_ID]);
    let canonical = canonical_findings(first.clone()).unwrap();
    assert_eq!(registry.evaluate(&input).unwrap(), canonical);

    let result = registry.evaluate_with_digest(&input).unwrap();
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert_eq!(result.findings, canonical);
    assert_eq!(result.findings_digest, digest_findings(&canonical).unwrap());
    assert!(registry
        .evaluate(&json!({"schema": "kiana.quality-context-memory-input.v0"}))
        .is_err());
}
