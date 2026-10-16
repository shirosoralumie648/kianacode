//! CM-13 的静态源码边界守卫。
//!
//! `include_str!` 把 Domain、RepoMap scanner 和 task-lens adapter 当作普通文本读取；断言只
//! 检查关键 marker 存在、adapter 没有直接连接执行组件，不运行筛选函数或文件扫描。

#[test]
/// 防止 task-lens 被误写成编译器证明或带副作用的执行路径。
fn repo_map_task_lens_keeps_heuristic_and_budget_boundaries() {
    let domain = include_str!("../../kiana-domain/src/repo_map_task.rs");
    let repo_map = include_str!("../../kiana-query/src/repo_map.rs");
    let adapter = include_str!("../../kiana-query/src/repo_map_task.rs");
    for marker in [
        "RepoMapEvidenceLevel",
        "Heuristic",
        "RepoMapDependencyEdge",
        "task_score",
        "estimated_tokens",
        "omitted_paths",
        "token_budget",
        "compiler",
        "selection_digest",
    ] {
        assert!(
            domain.contains(marker) || adapter.contains(marker) || repo_map.contains(marker),
            "CM-13 marker missing: {marker}"
        );
    }
    assert!(!adapter.contains("ModelClient"));
    assert!(!adapter.contains("CapabilityBroker"));
}
