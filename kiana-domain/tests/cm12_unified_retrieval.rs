use kiana_domain::*;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn item(id: &str, text: &str, allowed: bool, created_at_ms: u64) -> RetrievalItem {
    RetrievalItem {
        id: id.to_owned(),
        text: text.to_owned(),
        source_digest: digest(if allowed { 'a' } else { 'b' }),
        permission_scope_digest: digest('s'),
        allowed,
        created_at_ms,
        dense_vector: None,
    }
}

#[test]
fn acl_filter_happens_before_ranking() {
    let profile = RetrievalProfile::unified();
    let request = RetrievalRequest::new("secret checkout", digest('s'), None).unwrap();
    let result = rank_retrieval(
        &profile,
        &request,
        &[
            item("denied", "secret checkout secret checkout", false, 20),
            item("allowed", "checkout", true, 10),
        ],
        10,
    )
    .unwrap();
    assert_eq!(result.acl_filtered_count, 1);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].id, "allowed");
    result.validate().unwrap();
}

#[test]
fn unified_ranking_is_stable_and_uses_versioned_rrf_mmr_profile() {
    let profile = RetrievalProfile::unified();
    let request = RetrievalRequest::new("部署 checkout", digest('s'), None).unwrap();
    let items = vec![
        item("b", "checkout 部署", true, 2),
        item("a", "checkout 部署", true, 1),
    ];
    let first = rank_retrieval(&profile, &request, &items, 2).unwrap();
    let second = rank_retrieval(&profile, &request, &items, 2).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.algorithm_version, "exact+bm25-cjk+dense+rrf60+mmr.v1");
    assert_eq!(first.hits[0].rank, 1);
}

#[test]
fn unfiltered_request_is_rejected_before_any_score_is_computed() {
    let mut request = RetrievalRequest::new("query", digest('s'), None).unwrap();
    request.acl_filtered = false;
    assert_eq!(
        rank_retrieval(&RetrievalProfile::unified(), &request, &[], 1).unwrap_err(),
        "retrieval_request_invalid"
    );
}
