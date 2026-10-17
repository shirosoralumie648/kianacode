//! CM-12 Domain 统一检索算法的纯输入/输出 fixture。
//!
//! 测试直接构造 profile、request 和候选项，再调用 Domain reducer；它们不启动检索索引或产品入口，也不
//! 验证权限摘要的真实来源。覆盖重点：拒绝候选不影响排名或分数、相同输入稳定输出、ACL 标志为 false 时在
//! 打分前失败、跨 scope 候选 fail-closed、dense 通道参与 RRF 融合与维度不匹配降级，以及 MMR 在近似重复
//! 之间选择更不重复的条目。

use kiana_domain::*;

/// 生成外观符合 SHA-256 字符串合同的 fixture 标签。
///
/// 这里重复一个固定四位十六进制片段 16 次，得到 `sha256:` 加 64 个 hex 字符；它不是对某个源内容计算的
/// 真正 SHA-256，只用于构造满足 Domain 格式 validator 的合成值。
fn digest(value: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (value as u32) & 0xffff).repeat(16)
    )
}

/// 建立不带向量的候选记录，并用 `allowed` 单独控制是否进入本次排名。
/// source/scope digest 都是上面的格式 fixture，时间戳只用于最终排序的平局条件。
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

/// 建立带 dense 向量的候选；查询/条目向量都由 fixture 直接给出，本算法不生成 embedding。
fn item_with_vector(
    id: &str,
    text: &str,
    created_at_ms: u64,
    dense_vector: Vec<f64>,
) -> RetrievalItem {
    RetrievalItem {
        dense_vector: Some(dense_vector),
        ..item(id, text, true, created_at_ms)
    }
}

#[test]
/// 一个分数很高但被标为 denied 的候选不能进入返回结果，也不能改变允许候选的分数、文档频率或平均长度。
/// 这里把同一批允许候选在“有 denied 项”和“没有 denied 项”两种输入下的通道分数逐字比较；它仍不证明
/// `allowed` 是从真实授权服务得到的。
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

    // 被拒绝项不参与文档数、平均长度或文档频率：删除它不会改变允许候选的任何通道分数。
    let without_denied = rank_retrieval(
        &profile,
        &request,
        &[item("allowed", "checkout", true, 10)],
        10,
    )
    .unwrap();
    assert_eq!(without_denied.acl_filtered_count, 0);
    assert_eq!(result.hits[0].bm25_score, without_denied.hits[0].bm25_score);
    assert_eq!(
        result.hits[0].exact_score,
        without_denied.hits[0].exact_score
    );
    assert_eq!(result.hits[0].rrf_score, without_denied.hits[0].rrf_score);
    assert_eq!(result.hits[0].mmr_score, without_denied.hits[0].mmr_score);
    assert_eq!(
        result.hits[0].matched_terms,
        without_denied.hits[0].matched_terms
    );
}

#[test]
/// 固定 profile 与同一 request/snapshot 重复排序得到完全相同的 DTO，并保留算法版本标签与 profile 常数。
/// 两个条目同分时结果还会走稳定 tie-break；本 fixture 未提供 dense 向量。
fn unified_ranking_is_stable_and_uses_versioned_rrf_mmr_profile() {
    let profile = RetrievalProfile::unified();
    // 版本化 profile 是排名的一部分：换成别的常数就是另一个算法版本。
    assert_eq!(profile.rrf_k, 60);
    assert_eq!(profile.bm25_k1, 1.2);
    assert_eq!(profile.bm25_b, 0.75);
    assert_eq!(profile.mmr_lambda, 0.7);
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
    // RRF-k=60 的名次一分数是 1/(60+1)，不是 1/rank，也不是可变平滑值。
    assert!((first.hits[0].rrf_score - 1.0 / 61.0).abs() < 1e-12);
    assert_eq!(first.hits[0].sparse_rank, Some(1));
}

#[test]
/// ACL 过滤声明为 false 时，排名器应在候选打分前拒绝请求。
/// 该测试同时修改公开 DTO 的一个字段，validator 在检查 request digest 之前命中 header/标志错误分支。
fn unfiltered_request_is_rejected_before_any_score_is_computed() {
    let mut request = RetrievalRequest::new("query", digest('s'), None).unwrap();
    request.acl_filtered = false;
    assert_eq!(
        rank_retrieval(&RetrievalProfile::unified(), &request, &[], 1).unwrap_err(),
        "retrieval_request_invalid"
    );
}

#[test]
/// 携带另一个 scope 摘要的候选在打分前 fail-closed：混合范围的快照不能悄悄进入同一个结果集合。
fn cross_scope_candidate_fails_closed_before_scoring() {
    let mut foreign = item("foreign", "checkout", true, 1);
    foreign.permission_scope_digest = digest('o');
    assert_eq!(
        rank_retrieval(
            &RetrievalProfile::unified(),
            &RetrievalRequest::new("checkout", digest('s'), None).unwrap(),
            &[item("allowed", "checkout", true, 1), foreign],
            5,
        )
        .unwrap_err(),
        "retrieval_item_scope_mismatch"
    );
}

#[test]
/// dense 通道单独命中也能进入结果并参与 RRF；查询向量有维度不匹配的条目时降级原因可见。
fn dense_channel_joins_rrf_and_dimension_mismatch_is_degraded() {
    // 查询向量存在：文本无命中的条目仍可凭 cosine>0 进入结果，且不会让整批请求失败。
    let request = RetrievalRequest::new("checkout", digest('s'), Some(vec![1.0, 0.0])).unwrap();
    let result = rank_retrieval(
        &RetrievalProfile::unified(),
        &request,
        &[
            item("text-hit", "checkout", true, 1),
            item_with_vector("vector-hit", "无关内容", 1, vec![1.0, 0.0]),
            item_with_vector("dim-mismatch", "checkout", 1, vec![1.0, 0.0, 0.0]),
        ],
        3,
    )
    .unwrap();
    assert!(result.degraded_reasons.is_empty());
    assert_eq!(result.hits.len(), 3);
    let by_id = |id: &str| {
        result
            .hits
            .iter()
            .find(|hit| hit.id == id)
            .unwrap_or_else(|| panic!("missing hit {id}"))
    };
    let vector_hit = by_id("vector-hit");
    assert_eq!(vector_hit.sparse_rank, None);
    assert_eq!(vector_hit.bm25_score, 0.0);
    assert_eq!(vector_hit.dense_rank, Some(1));
    assert_eq!(vector_hit.dense_score, Some(1.0));
    assert!(vector_hit.matched_terms.is_empty());
    let dimension_mismatch = by_id("dim-mismatch");
    assert_eq!(dimension_mismatch.dense_rank, None);
    assert!(dimension_mismatch.sparse_rank.is_some());
    // MMR 会把它与同文本的 text-hit 视为重复，因此重复文本不会紧跟在它的副本后面。
    assert_eq!(result.hits[0].id, "dim-mismatch");
    assert_eq!(result.hits[1].id, "vector-hit");
    assert_eq!(result.hits[2].id, "text-hit");
    result.validate().unwrap();

    // 请求要求 dense，但没有任何条目给出同维正分向量：必须写明降级原因，稀疏通道仍继续工作。
    let degraded = rank_retrieval(
        &RetrievalProfile::unified(),
        &request,
        &[item_with_vector(
            "only-mismatch",
            "checkout",
            1,
            vec![1.0, 0.0, 0.0],
        )],
        3,
    )
    .unwrap();
    assert_eq!(
        degraded.degraded_reasons,
        vec!["dense_unavailable_or_dimension_mismatch".to_owned()]
    );
    assert_eq!(degraded.hits.len(), 1);
    assert_eq!(degraded.hits[0].dense_rank, None);
    assert!(degraded.hits[0].sparse_rank.is_some());
}

#[test]
/// MMR 在第一轮相关性之后优先选择更不重复的条目，而不是把同文本副本排在前面。
fn mmr_prefers_a_distinct_hit_over_a_near_duplicate() {
    let result = rank_retrieval(
        &RetrievalProfile::unified(),
        &RetrievalRequest::new("checkout deployment", digest('s'), None).unwrap(),
        &[
            item("dup-a", "checkout deployment", true, 10),
            item("dup-b", "checkout deployment", true, 10),
            item("distinct", "checkout", true, 5),
        ],
        3,
    )
    .unwrap();
    let ids = result
        .hits
        .iter()
        .map(|hit| hit.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["dup-a", "distinct", "dup-b"]);
    assert!(result.hits[1].mmr_score > result.hits[2].mmr_score);
    result.validate().unwrap();
}

#[test]
/// scope 摘要由调用方标签派生：顺序无关、去重，标签集合变化即摘要变化，形状是 `sha256:` 加 64 hex。
fn retrieval_scope_digest_is_order_insensitive_and_dedupes() {
    let first = retrieval_scope_digest(["project:/a", "project:/b"]);
    assert_eq!(first, retrieval_scope_digest(["project:/b", "project:/a"]));
    assert_eq!(
        first,
        retrieval_scope_digest(["project:/a", "project:/b", "project:/a"])
    );
    assert_ne!(first, retrieval_scope_digest(["project:/a"]));
    assert!(first.starts_with("sha256:"));
    assert_eq!(first.len(), "sha256:".len() + 64);
}
