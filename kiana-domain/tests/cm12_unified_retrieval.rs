//! CM-12 Domain 统一检索算法的纯输入/输出 fixture。
//!
//! 测试直接构造 profile、request 和候选项，再调用 Domain reducer；它们不启动检索索引或产品入口，也不
//! 验证权限摘要的真实来源。当前覆盖重点是：拒绝候选不影响排名、相同输入稳定输出、ACL 标志为 false 时
//! 在打分前失败。dense 向量、RRF 两路融合、MMR 多样性取舍和资源上限边界没有在这些用例里分别展开。

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

#[test]
/// 一个分数很高但被标为 denied 的候选不能进入返回结果或改变允许候选的统计范围。
/// 此例验证输出过滤和计数；它不证明 `allowed` 是从真实授权服务得到的。
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
/// 固定 profile 与同一 request/snapshot 重复排序得到完全相同的 DTO，并保留算法版本标签。
/// 两个条目同分时结果还会走稳定 tie-break；本 fixture 未提供 dense 向量。
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
