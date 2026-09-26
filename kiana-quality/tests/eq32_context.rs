use kiana_quality::{ContextMemoryEvaluator, DeterministicEvaluator, CONTEXT_MEMORY_INPUT_SCHEMA};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn valid_input() -> Value {
    json!({
        "schema": CONTEXT_MEMORY_INPUT_SCHEMA,
        "query": {
            "schema": "kiana.quality-context-query-evidence.v1",
            "query_digest": DIGEST,
            "scope_digest": DIGEST,
            "data_epoch": 4,
            "generation": 7,
        },
        "acl_checked_before_ranking": true,
        "hits": [{
            "schema": "kiana.quality-memory-hit-evidence.v1",
            "record_ref": "memory:one",
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
        }],
        "budget": {
            "schema": "kiana.quality-context-budget-evidence.v1",
            "token_limit": 100,
            "estimated_tokens": 70,
            "used_tokens": 70,
            "reserved_output_tokens": 20,
            "budget_enforced": true,
            "wire_bytes": 700,
            "wire_limit": 1000,
            "wire_budget_enforced": true,
        },
        "compaction": {
            "schema": "kiana.quality-compaction-evidence.v1",
            "attempted": true,
            "summary_present": true,
            "source_cursor_start": 10,
            "source_cursor_end": 12,
            "latest_goal_preserved": true,
            "pending_pairs_preserved": true,
            "completed_actions_verifiable": true,
            "stale_commit_rejected": true,
        },
    })
}

#[test]
fn admitted_current_memory_and_context_budget_have_no_findings() {
    let findings = ContextMemoryEvaluator.evaluate(&valid_input()).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn unauthorized_memory_hit_is_blocking_before_ranking() {
    let mut input = valid_input();
    input["acl_checked_before_ranking"] = json!(false);
    input["hits"][0]["acl_checked"] = json!(false);
    input["hits"][0]["acl_allowed"] = json!(false);
    input["hits"][0]["freshness"] = json!("unknown");
    input["hits"][0]["evidence"] = json!("missing");
    input["hits"][0]["source_digest"] = json!(OTHER_DIGEST);

    let findings = ContextMemoryEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"context.acl_order_invalid"));
    assert!(codes.contains(&"context.unauthorized_memory_hit"));
    assert!(codes.contains(&"context.freshness_unknown"));
    assert!(codes.contains(&"context.evidence_missing"));
}

#[test]
fn stale_budget_and_compaction_drift_are_visible() {
    let mut input = valid_input();
    input["hits"][0]["freshness"] = json!("stale");
    input["budget"]["estimated_tokens"] = json!(101);
    input["budget"]["wire_bytes"] = json!(1001);
    input["budget"]["budget_enforced"] = json!(false);
    input["budget"]["wire_budget_enforced"] = json!(false);
    input["compaction"]["summary_present"] = json!(false);
    input["compaction"]["pending_pairs_preserved"] = json!(false);
    input["compaction"]["stale_commit_rejected"] = json!(false);

    let findings = ContextMemoryEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"context.stale_not_acknowledged"));
    assert!(codes.contains(&"context.budget_overflow"));
    assert!(codes.contains(&"context.budget_unenforced"));
    assert!(codes.contains(&"context.compaction_summary_missing"));
    assert!(codes.contains(&"context.compaction_integrity_invalid"));
    assert!(codes.contains(&"context.compaction_stale_commit_unfenced"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut input = valid_input();
    input["unexpected"] = json!(true);
    assert!(ContextMemoryEvaluator.evaluate(&input).is_err());
}
