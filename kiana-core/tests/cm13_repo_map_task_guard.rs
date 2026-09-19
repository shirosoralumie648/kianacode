#[test]
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
