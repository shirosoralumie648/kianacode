//! Query task-lens adapter 的内存 fixture。
//!
//! 测试手工构造只有一个文件的 RepoMap，然后调用 task selector；它不启动 scanner，所以不
//! 覆盖目录遍历、实际源文件 digest 或 workspace token 截断。

use kiana_query::{select_repo_map_for_task, RepoMap, RepoMapFile};

#[test]
/// 确认单文件候选沿 Query adapter 进入 Domain 选择，并保留启发式证据和预算结果。
fn task_lens_reuses_repo_map_and_keeps_budget() {
    let map = RepoMap {
        root: "/repo".to_owned(),
        token_budget: 100,
        estimated_tokens: 10,
        truncated: false,
        omitted_files: 0,
        files: vec![RepoMapFile {
            content_hash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            path: "src/deploy.rs".to_owned(),
            language: Some("rust".to_owned()),
            bytes: 10,
            estimated_tokens: 6,
            symbols: vec!["fn deploy".to_owned()],
        }],
    };
    let selection = select_repo_map_for_task(&map, "deploy", 10).unwrap();
    assert_eq!(selection.selected[0].path, "src/deploy.rs");
    assert!(selection.selected[0]
        .symbols
        .iter()
        .all(|symbol| { symbol.evidence == kiana_domain::RepoMapEvidenceLevel::Heuristic }));
    selection.validate().unwrap();
}
