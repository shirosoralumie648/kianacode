//! Daemon 到 Query 缓存源码边界的静态 marker 守卫。
//!
//! 本测试把三个源文件读成字符串，再确认 Query 决策字段、索引读取 helper 和 Daemon 查询
//! 入口提示都存在；它不运行请求，不验证缓存失败是否在每种运行场景下都被正确处理。

#[test]
fn daemon_cache_path_does_not_treat_cache_failure_as_empty_facts() {
    let query = include_str!("../../kiana-query/src/cache_policy.rs");
    let index = include_str!("../../kiana-query/src/index.rs");
    let daemon = include_str!("../src/context_query.rs");
    // 三处 marker 用于防止静态结构被整体移除；具体的数据流仍需从各模块实现确认。
    assert!(query.contains("business_result_from_cache"));
    assert!(index.contains("read_cached_index"));
    assert!(daemon.contains("ContextIndex") || daemon.contains("context index"));
}
