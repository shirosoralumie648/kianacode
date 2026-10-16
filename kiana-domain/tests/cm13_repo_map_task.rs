//! RepoMap 任务选择 Domain 合同的纯内存 fixture。
//!
//! 候选路径、分数、估算 token、符号和依赖边均由测试直接构造；测试检查排序与摘要稳定、
//! 预算遗漏和证据等级，不运行文件扫描器或证明依赖关系来自真实源代码。

use kiana_domain::*;

/// 生成固定形状的合成 SHA-256 文本，不对源文件求哈希。
fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

/// 创建带一个启发式符号和一条启发式依赖边的测试候选。
fn candidate(path: &str, score: u64, tokens: u64) -> RepoMapTaskCandidate {
    RepoMapTaskCandidate {
        path: path.to_owned(),
        language: Some("rust".to_owned()),
        content_digest: digest(if score > 1 { 'a' } else { 'b' }),
        symbols: vec![RepoMapSymbol::heuristic("fn run").unwrap()],
        dependencies: vec![RepoMapDependencyEdge::heuristic(
            path,
            "src/lib.rs",
            "heuristic_import",
        )
        .unwrap()],
        task_score: score,
        estimated_tokens: tokens,
    }
}

#[test]
/// 反转输入候选顺序后仍选择高分路径，并把超预算候选记录为 omitted。
fn repo_map_budget_is_stable() {
    let first = RepoMapTaskSelection::new(
        "run deployment",
        10,
        vec![candidate("src/z.rs", 1, 6), candidate("src/a.rs", 3, 6)],
    )
    .unwrap();
    let second = RepoMapTaskSelection::new(
        "run deployment",
        10,
        vec![candidate("src/a.rs", 3, 6), candidate("src/z.rs", 1, 6)],
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.selected[0].path, "src/a.rs");
    assert_eq!(first.omitted_paths, vec!["src/z.rs"]);
}

#[test]
/// 由启发式 helper 构造的符号保留 Heuristic 标签，不被伪装成编译器事实。
fn heuristic_symbol_is_not_reported_as_compiler_fact() {
    let symbol = RepoMapSymbol::heuristic("fn run").unwrap();
    assert_eq!(symbol.evidence, RepoMapEvidenceLevel::Heuristic);
    assert!(!serde_json::to_string(&symbol).unwrap().contains("compiler"));
}
