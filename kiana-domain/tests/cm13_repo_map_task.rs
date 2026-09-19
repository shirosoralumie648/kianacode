use kiana_domain::*;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

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
fn heuristic_symbol_is_not_reported_as_compiler_fact() {
    let symbol = RepoMapSymbol::heuristic("fn run").unwrap();
    assert_eq!(symbol.evidence, RepoMapEvidenceLevel::Heuristic);
    assert!(!serde_json::to_string(&symbol).unwrap().contains("compiler"));
}
