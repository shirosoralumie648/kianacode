use kiana_domain::{
    json_digest, CompactionArtifact, CompactionCommit, ContextCacheBinding, ContextCheckpoint,
    ContextInvalidationPlan, ContextInvalidationTarget, ContextResumeView, DependencyEdge,
    DependencyNode, DependencyNodeKind, EventId, RunId, SourceDependencyGraph,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn graph() -> SourceDependencyGraph {
    SourceDependencyGraph::new(
        1,
        vec![
            DependencyNode {
                id: "source:memory".to_owned(),
                kind: DependencyNodeKind::Memory,
                source_id: Some("source:memory".to_owned()),
            },
            DependencyNode {
                id: "summary:1".to_owned(),
                kind: DependencyNodeKind::Summary,
                source_id: None,
            },
            DependencyNode {
                id: "plan:1".to_owned(),
                kind: DependencyNodeKind::Plan,
                source_id: None,
            },
            DependencyNode {
                id: "context-plan:1".to_owned(),
                kind: DependencyNodeKind::ContextPlan,
                source_id: None,
            },
            DependencyNode {
                id: "index:1".to_owned(),
                kind: DependencyNodeKind::Index,
                source_id: None,
            },
        ],
        vec![
            DependencyEdge {
                from: "summary:1".to_owned(),
                to: "source:memory".to_owned(),
                relation: "summarizes".to_owned(),
            },
            DependencyEdge {
                from: "plan:1".to_owned(),
                to: "summary:1".to_owned(),
                relation: "uses".to_owned(),
            },
            DependencyEdge {
                from: "context-plan:1".to_owned(),
                to: "source:memory".to_owned(),
                relation: "selects".to_owned(),
            },
            DependencyEdge {
                from: "index:1".to_owned(),
                to: "source:memory".to_owned(),
                relation: "indexes".to_owned(),
            },
        ],
    )
    .unwrap()
}

fn committed_checkpoint() -> ContextCheckpoint {
    let checkpoint = ContextCheckpoint::new(RunId::new(), 4, 10, digest("inbox"), 1).unwrap();
    let artifact = CompactionArtifact::new(
        checkpoint.run_id,
        digest("summary"),
        digest("artifact"),
        10,
        12,
        256,
    )
    .unwrap();
    let commit = CompactionCommit::new(
        &artifact,
        4,
        5,
        vec![EventId::new()],
        digest("prompt"),
        "builder-default",
        digest("budget"),
        8,
        3,
    )
    .unwrap();
    checkpoint
        .commit(&artifact, &commit, 4, 12, 8, 3, digest("inbox"), 1)
        .unwrap()
}

#[test]
fn compacted_context_rebuilds_to_same_view_after_restart() {
    let checkpoint = committed_checkpoint();
    let prepared = digest("prepared-request");
    let first = ContextResumeView::new(&checkpoint, &prepared, 3).unwrap();
    let rebuilt = ContextResumeView::rebuild_after_restart(&checkpoint, &prepared, 3).unwrap();
    assert_eq!(first.view_digest, rebuilt.view_digest);
    assert_eq!(first.context_revision, 5);
    assert_eq!(first.source_cursor, 12);
}

#[test]
fn revoked_source_invalidates_summary_and_cache() {
    let mut graph = graph();
    let invalidation = graph.revoke_source("source:memory", 2).unwrap();
    let plan = ContextInvalidationPlan::from_source_invalidation(&graph, &invalidation).unwrap();
    plan.validate().unwrap();
    assert!(plan.invalidates(ContextInvalidationTarget::Summary));
    assert!(plan.invalidates(ContextInvalidationTarget::Selection));
    assert!(plan.invalidates(ContextInvalidationTarget::Cache));
    assert!(plan.invalidates(ContextInvalidationTarget::Checkpoint));

    let cache = ContextCacheBinding::new(digest("prepared"), digest("cache"), 1).unwrap();
    let invalidated = cache.invalidate(&plan).unwrap();
    assert!(!invalidated.valid);
    assert_eq!(invalidated.data_epoch, 2);
    assert_eq!(
        invalidated.invalidated_by.as_deref(),
        Some(plan.plan_digest.as_str())
    );
    invalidated.validate().unwrap();
}
