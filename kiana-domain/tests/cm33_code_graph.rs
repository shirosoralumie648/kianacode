use kiana_domain::{
    json_digest, CodeGraphEdge, CodeGraphNodeKind, CodeGraphRebuildPlan, CodeGraphRelation,
    CodeGraphTemporalState, EvidenceStatus, SourceKind, SourceRef,
};
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

fn source(id: &str) -> SourceRef {
    SourceRef::new(
        id,
        SourceKind::WorkspaceFile,
        format!("src/{id}.rs"),
        "git:cm33",
        digest(id),
        Some(1),
        EvidenceStatus::Attributed,
    )
    .unwrap()
}

fn edge(id: &str, source_id: &str) -> CodeGraphEdge {
    CodeGraphEdge::new(
        id,
        format!("symbol:{id}:from"),
        CodeGraphNodeKind::Symbol,
        format!("symbol:{id}:to"),
        CodeGraphNodeKind::Symbol,
        CodeGraphRelation::References,
        source(source_id),
        digest("scope:cm33"),
        10,
        Some(200),
    )
    .unwrap()
}

#[test]
fn graph_edge_has_source_and_scope() {
    let edge = edge("edge-a", "file-a");
    edge.validate().unwrap();
    assert_eq!(edge.source.kind, SourceKind::WorkspaceFile);
    assert_eq!(edge.state_at(100).unwrap(), CodeGraphTemporalState::Valid);
    assert_eq!(edge.state_at(250).unwrap(), CodeGraphTemporalState::Expired);
}

#[test]
fn graph_delete_rebuilds_affected_edges_only() {
    let affected = edge("edge-a", "file-a");
    let retained = edge("edge-b", "file-b");
    let plan =
        CodeGraphRebuildPlan::for_source(&[affected, retained], "file-a", digest("file-a"), 4)
            .unwrap();
    plan.validate().unwrap();
    assert_eq!(plan.affected_edge_ids, vec!["edge-a"]);
    assert_eq!(plan.retained_edge_ids, vec!["edge-b"]);
}
