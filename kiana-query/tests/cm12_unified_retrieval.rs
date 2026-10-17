//! CM-12 Query 适配器 fixture：`UnifiedRetrievalPort` 与便捷函数、CLI/memory 适配器共享的唯一实现。
//!
//! 这些用例只驱动 `kiana-query` 的 port adapter 与 Domain 入口，不启动 CLI 进程或 daemon；它们证明
//! adapter 不做自己的排名，并保留 ACL-first（被拒绝候选不进入排名、不改变允许候选的分数）。

use kiana_domain::{RetrievalItem, RetrievalRequest};
use kiana_ports::RetrievalPort;
use kiana_query::{rank_unified, UnifiedRetrievalPort};

fn digest(value: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (value as u32) & 0xffff).repeat(16)
    )
}

fn items() -> Vec<RetrievalItem> {
    vec![
        RetrievalItem {
            id: "a".to_owned(),
            text: "checkout deployment archive notes".to_owned(),
            source_digest: digest('a'),
            permission_scope_digest: digest('s'),
            allowed: true,
            created_at_ms: 1,
            dense_vector: None,
        },
        RetrievalItem {
            id: "b".to_owned(),
            text: "checkout deployment".to_owned(),
            source_digest: digest('b'),
            permission_scope_digest: digest('s'),
            allowed: true,
            created_at_ms: 2,
            dense_vector: None,
        },
    ]
}

#[test]
/// CLI 与 memory tool 适配器读同一个 `RetrievalPort` 实现：便捷函数与 port adapter 在同一请求下必须
/// 逐字段相等（顺序、通道分数、算法版本），而不是各自维护一套排序。
fn cli_and_memory_tool_have_identical_rankings() {
    let request = RetrievalRequest::new("checkout", digest('s'), None).unwrap();
    let direct = rank_unified(&request, &items(), 2).unwrap();
    let port = UnifiedRetrievalPort::default()
        .retrieve(&request, &items(), 2)
        .unwrap();
    assert_eq!(direct, port);
    assert_eq!(direct.hits[0].id, "b");
    assert_eq!(direct.hits.len(), 2);
    // adapter 报告的是 Domain profile 记录的算法版本，不是 adapter 自己的标签。
    assert_eq!(
        port.algorithm_version,
        project_port_profile_algorithm_version()
    );
}

/// 读取 adapter 默认 profile 的算法版本标签，用于与排名结果里的标签对照。
fn project_port_profile_algorithm_version() -> String {
    UnifiedRetrievalPort::default()
        .profile()
        .algorithm_version
        .clone()
}

#[test]
/// 同一 port 实现保持 ACL-first：被拒绝的高分候选只增加过滤计数，不改变允许候选的排名或通道分数。
fn port_keeps_acl_first_before_any_scoring() {
    let request = RetrievalRequest::new("checkout", digest('s'), None).unwrap();
    let mut denied = RetrievalItem {
        id: "denied".to_owned(),
        text: "checkout checkout checkout".to_owned(),
        source_digest: digest('d'),
        permission_scope_digest: digest('s'),
        allowed: false,
        created_at_ms: 9,
        dense_vector: None,
    };
    let allowed = items();
    let mut mixed = vec![denied.clone()];
    mixed.extend(allowed.iter().cloned());

    let baseline = UnifiedRetrievalPort::default()
        .retrieve(&request, &allowed, 2)
        .unwrap();
    let filtered = UnifiedRetrievalPort::default()
        .retrieve(&request, &mixed, 2)
        .unwrap();
    assert_eq!(filtered.acl_filtered_count, 1);
    assert_eq!(baseline.hits.len(), filtered.hits.len());
    for (expected, actual) in baseline.hits.iter().zip(filtered.hits.iter()) {
        assert_eq!(expected.id, actual.id);
        assert_eq!(expected.bm25_score, actual.bm25_score);
        assert_eq!(expected.exact_score, actual.exact_score);
        assert_eq!(expected.rrf_score, actual.rrf_score);
        assert_eq!(expected.mmr_score, actual.mmr_score);
    }

    // 没有 ACL 过滤声明的请求必须在 adapter 层就失败，不会算出一份“未过滤”的排名。
    denied.allowed = true; // 让失败只可能来自 acl_filtered 标志，而不是条目形状
    let mut unfiltered = request.clone();
    unfiltered.acl_filtered = false;
    let error = UnifiedRetrievalPort::default()
        .retrieve(&unfiltered, &items(), 2)
        .expect_err("unfiltered request must be rejected");
    assert!(format!("{error}").contains("retrieval_request_invalid"));
}
