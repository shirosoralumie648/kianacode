//! SC-24 Query 数据边界派生规则的合成输入 fixture。
//!
//! 测试只调用纯 DTO 构造器，验证有效治理输入和撤销/禁止保留输入映射到的 disposition。
//! 它不连接治理账本、读取 Memory/Index/Cache，也不验证真实产品入口采用了这个边界。

use kiana_query::{QueryDataBoundary, QueryDataDisposition};

/// 生成仅用于格式校验的 sha256 文本；重复字符不代表实际 scope 内容。
fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

/// 验证一般读取条件通过且显式请求导出时，四类边界字段都允许。
///
/// 允许保留且未撤销时，memory/index/cache 可用；本例还将 `export_requested` 设为 true，
/// 所以 export 也应 Allowed。
#[test]
fn allowed_boundary_covers_memory_index_cache_and_explicit_export() {
    // 重复字符只用于构成格式正确的摘要字符串，不代表实际 scope 内容。
    let boundary = QueryDataBoundary::derive("/repo", digest('a'), 4, false, true, true).unwrap();
    assert_eq!(boundary.memory, QueryDataDisposition::Allowed);
    assert_eq!(boundary.index, QueryDataDisposition::Allowed);
    assert_eq!(boundary.cache, QueryDataDisposition::Allowed);
    assert_eq!(boundary.export, QueryDataDisposition::Allowed);
}

/// 撤销来源或禁止保留中的任意一项都会拒绝所有派生视图。
///
/// 循环中的两组输入分别隔离 revoked 与 retention 条件；即使请求导出，也不能越过主拒绝条件。
#[test]
fn revoked_or_retention_blocked_boundary_denies_all_derived_views() {
    for (revoked, retention) in [(true, true), (false, false)] {
        let boundary =
            QueryDataBoundary::derive("/repo", digest('a'), 4, revoked, retention, true).unwrap();
        assert_eq!(boundary.memory, QueryDataDisposition::Denied);
        assert_eq!(boundary.index, QueryDataDisposition::Denied);
        assert_eq!(boundary.cache, QueryDataDisposition::Denied);
        assert_eq!(boundary.export, QueryDataDisposition::Denied);
        boundary.validate().unwrap();
    }
}
