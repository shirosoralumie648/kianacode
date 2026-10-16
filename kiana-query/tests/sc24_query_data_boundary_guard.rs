//! SC-24 Query 数据边界的静态源码守卫。
//!
//! 通过 `include_str!` 检查 Query 边界、Daemon memory 和 cache policy 源码中存在若干合同标记。
//! 它不执行撤销、过期或缓存读取，也不证明被检查的路径已接入生产入口。

#[test]
fn query_data_boundary_guard_prevents_revoked_derived_views() {
    let boundary = include_str!("../src/data_boundary.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let cache = include_str!("../src/cache_policy.rs");
    // marker 可出现在三个被检查源文件任意一个中；该 OR 条件不证明这些标记在同一调用链上。
    for marker in [
        "QueryDataBoundary",
        "scope_digest",
        "data_epoch",
        "query_data_boundary_revoked_data_allowed",
        "QueryDataDisposition::Denied",
    ] {
        assert!(boundary.contains(marker) || memory.contains(marker) || cache.contains(marker));
    }
}
