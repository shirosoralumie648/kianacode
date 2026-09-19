use kiana_domain::*;
use std::collections::BTreeMap;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn evidence(rank: u32, generation: u64) -> RetrievalEvidence {
    RetrievalEvidence::new(
        format!("candidate-{rank}"),
        format!("workspace:src/{rank}.rs"),
        "revision:1",
        digest('s'),
        generation,
        Freshness::Current,
        EvidenceStatus::Attributed,
        rank,
        BTreeMap::from([("rrf".to_owned(), 0.5)]),
    )
    .unwrap()
}

#[test]
fn every_hit_has_source_and_generation() {
    let health =
        RetrievalHealth::new(RetrievalHealthStatus::Ready, None, false, false, 7, 2).unwrap();
    let response = RetrievalResponse::new(
        digest('q'),
        7,
        vec![evidence(1, 7), evidence(2, 7)],
        health,
        None,
    )
    .unwrap();
    response.validate().unwrap();
}

#[test]
fn empty_and_degraded_results_require_reason_and_safe_retry() {
    let denied = RetrievalHealth::new(
        RetrievalHealthStatus::Denied,
        Some("acl_scope_denied".to_owned()),
        false,
        false,
        1,
        0,
    )
    .unwrap();
    let response = RetrievalResponse::new(
        digest('q'),
        1,
        Vec::new(),
        denied,
        Some("no permitted sources".to_owned()),
    )
    .unwrap();
    response.validate().unwrap();

    assert!(RetrievalHealth::new(
        RetrievalHealthStatus::Degraded,
        Some("embedding_unavailable".to_owned()),
        true,
        false,
        1,
        0,
    )
    .is_err());
    let safe = RetrievalHealth::new(
        RetrievalHealthStatus::Degraded,
        Some("embedding_unavailable".to_owned()),
        true,
        true,
        1,
        0,
    )
    .unwrap();
    assert!(safe.retryable && safe.retry_safe);
}

#[test]
fn generation_mismatch_is_rejected() {
    let health =
        RetrievalHealth::new(RetrievalHealthStatus::Ready, None, false, false, 2, 1).unwrap();
    assert!(RetrievalResponse::new(digest('q'), 2, vec![evidence(1, 1)], health, None).is_err());
}
