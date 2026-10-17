//! CM-14 Domain fixture：检索结果的来源证据、freshness、health 与重试边界。
//!
//! 这些用例只驱动 Domain 合同与统一排名器（纯输入/输出），不读取索引、不生成 embedding、不执行工具。
//! 覆盖重点：每个命中都有来源引用与快照 generation、空结果必须带原因、embedding/通道失败以
//! Degraded + 原因可见且只有显式声明的瞬时失败才可安全重试、命中证据被改写或错位时整份结果被拒绝。

use kiana_domain::*;
use std::collections::BTreeMap;

fn digest(value: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (value as u32) & 0xffff).repeat(16)
    )
}

/// 独立构造的 `RetrievalEvidence` fixture：用于 response/health 形状合同与 generation 错位用例。
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

/// 统一排名器的候选 fixture：来源证据是固定标签，generation 由调用方传入。
fn item(id: &str, text: &str, generation: u64) -> RetrievalItem {
    RetrievalItem {
        id: id.to_owned(),
        text: text.to_owned(),
        source_digest: digest('a'),
        permission_scope_digest: digest('s'),
        provenance: RetrievalItemProvenance::new(
            format!("workspace:src/{id}.rs"),
            "revision:2",
            generation,
            Freshness::Current,
            EvidenceStatus::Attributed,
        )
        .unwrap(),
        allowed: true,
        created_at_ms: 1,
        dense_vector: None,
    }
}

#[test]
fn every_hit_has_source_and_generation() {
    // 每个命中都必须带来源证据，并与候选 provenance、结果快照 generation 对齐。
    let request = RetrievalRequest::new("checkout deployment", digest('s'), None).unwrap();
    let result = rank_retrieval(
        &RetrievalProfile::unified(),
        &request,
        &[
            item("alpha", "checkout deployment rollout", 7),
            item("beta", "checkout deployment", 7),
        ],
        2,
    )
    .unwrap();
    assert_eq!(result.source_generation, 7);
    assert_eq!(result.health.status, RetrievalHealthStatus::Ready);
    assert!(result.empty_reason.is_none());
    assert_eq!(result.hits.len(), 2);
    for (index, hit) in result.hits.iter().enumerate() {
        let provenance = &hit.provenance;
        assert_eq!(provenance.candidate_id, hit.id);
        assert_eq!(provenance.rank, hit.rank);
        assert_eq!(provenance.rank, index as u32 + 1);
        assert_eq!(provenance.generation, 7);
        assert_eq!(provenance.source_snapshot_digest, hit.source_digest);
        assert_eq!(provenance.freshness, Freshness::Current);
        assert_eq!(provenance.evidence, EvidenceStatus::Attributed);
        assert!(provenance.source_ref.starts_with("workspace:src/"));
        assert_eq!(provenance.source_revision, "revision:2");
        // rank components 是机器可读通道记录：至少含 RRF，且不含非法浮点数。
        assert!(provenance.rank_components.contains_key("rrf"));
        assert!(provenance
            .rank_components
            .values()
            .all(|value| value.is_finite()));
    }
    result.validate().unwrap();

    // 命中 ID 与证据 candidate_id 错位时，validator 拒绝整份结果。
    let mut mismatched = result.clone();
    mismatched.hits[0].id = "tampered".to_owned();
    assert_eq!(
        mismatched.validate().unwrap_err(),
        "retrieval_result_hit_evidence_mismatch"
    );

    // 证据自身被改写（generation 与证据摘要不再一致）时，证据级校验先拒绝。
    let mut tampered = result.clone();
    tampered.hits[0].provenance.generation = 99;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "retrieval_evidence_digest_mismatch"
    );

    // Denied 状态不允许同时携带命中：拒绝与结果不能同时成立。
    let mut denied_with_hits = result.clone();
    denied_with_hits.health = RetrievalHealth::denied(
        "acl_scope_denied_all_candidates",
        denied_with_hits.source_generation,
        denied_with_hits.hits.len() as u32,
    )
    .unwrap();
    assert_eq!(
        denied_with_hits.validate().unwrap_err(),
        "retrieval_result_denied_with_hits"
    );
}

#[test]
fn embedding_failure_is_visible_and_retryable_only_when_safe() {
    // 请求声明了查询向量，但没有任何候选给出同维正分向量：dense/embedding 通道失败必须可见。
    let request =
        RetrievalRequest::new("checkout deployment", digest('s'), Some(vec![1.0, 0.0])).unwrap();
    let degraded = rank_retrieval(
        &RetrievalProfile::unified(),
        &request,
        &[item("alpha", "checkout deployment", 3)],
        4,
    )
    .unwrap();
    assert_eq!(
        degraded.degraded_reasons,
        vec!["dense_unavailable_or_dimension_mismatch".to_owned()]
    );
    assert_eq!(degraded.health.status, RetrievalHealthStatus::Degraded);
    assert_eq!(
        degraded.health.reason.as_deref(),
        Some("dense_unavailable_or_dimension_mismatch")
    );
    // 配置/词表类降级是确定性的：Domain 不会把它标成可重试。
    assert!(!degraded.health.retryable);
    assert!(!degraded.health.retry_safe);
    // 稀疏命中仍然返回，并继续携带来源证据。
    assert_eq!(degraded.hits.len(), 1);
    assert_eq!(degraded.hits[0].provenance.generation, 3);
    assert_eq!(
        degraded.hits[0].provenance.rank_components.get("dense"),
        None
    );
    degraded.validate().unwrap();

    // 拒绝路径：全部候选被上游过滤 → Denied + 空结果原因，且拒绝不可重试、不携带命中。
    let denied = rank_retrieval(
        &RetrievalProfile::unified(),
        &request,
        &[{
            let mut item = item("denied", "checkout deployment", 3);
            item.allowed = false;
            item
        }],
        4,
    )
    .unwrap();
    assert_eq!(denied.health.status, RetrievalHealthStatus::Denied);
    assert_eq!(
        denied.health.reason.as_deref(),
        Some("acl_scope_denied_all_candidates")
    );
    assert_eq!(
        denied.empty_reason.as_deref(),
        Some("no_permitted_candidates")
    );
    assert!(denied.hits.is_empty());
    assert!(!denied.health.retryable);
    denied.validate().unwrap();

    // 只有显式声明瞬时故障的通道才拿到可安全重试的 health；`retryable` 没有 `retry_safe` 会被合同拒绝。
    assert!(RetrievalHealth::new(
        RetrievalHealthStatus::Degraded,
        Some("embedding_channel_transient_unavailable".to_owned()),
        true,
        false,
        1,
        0,
    )
    .is_err());
    let safe = RetrievalHealth::degraded_channel(
        "embedding_channel_transient_unavailable",
        RetrievalRetryPolicy::Safe,
        3,
        1,
    )
    .unwrap();
    assert!(safe.retryable && safe.retry_safe);
    // Denied 一律不可重试，即使调用方想把它标成 Safe。
    assert!(RetrievalHealth::new(
        RetrievalHealthStatus::Denied,
        Some("acl_scope_denied_all_candidates".to_owned()),
        true,
        true,
        3,
        0,
    )
    .is_err());
}

#[test]
fn empty_results_require_reason_and_never_silently_deny() {
    // 范围内没有候选：Ready（无通道故障）+ 空结果原因，而不是伪装成拒绝。
    let request = RetrievalRequest::new("checkout", digest('s'), None).unwrap();
    let empty = rank_retrieval(&RetrievalProfile::unified(), &request, &[], 5).unwrap();
    assert!(empty.hits.is_empty());
    assert_eq!(
        empty.empty_reason.as_deref(),
        Some("no_candidates_in_scope")
    );
    assert_eq!(empty.health.status, RetrievalHealthStatus::Ready);
    empty.validate().unwrap();

    // 有候选但查询无匹配：原因与“无候选”区分开。
    let no_match = rank_retrieval(
        &RetrievalProfile::unified(),
        &request,
        &[item("alpha", "deployment receipts", 4)],
        5,
    )
    .unwrap();
    assert!(no_match.hits.is_empty());
    assert_eq!(no_match.empty_reason.as_deref(), Some("no_query_match"));
    assert_eq!(no_match.source_generation, 4);
    no_match.validate().unwrap();

    // 空结果必须有原因：手工去掉原因后 validator 拒绝。
    let mut missing_reason = no_match.clone();
    missing_reason.empty_reason = None;
    assert_eq!(
        missing_reason.validate().unwrap_err(),
        "retrieval_result_empty_reason_invalid"
    );
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
