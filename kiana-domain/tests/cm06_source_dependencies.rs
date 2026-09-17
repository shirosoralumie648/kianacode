use kiana_domain::{
    DependencyEdge, DependencyNode, DependencyNodeKind, SourceDependencyGraph,
    SOURCE_DEPENDENCY_GRAPH_SCHEMA, SOURCE_INVALIDATION_SCHEMA,
};
use serde_json::json;
use std::collections::BTreeSet;

fn node(id: &str, kind: DependencyNodeKind, source_id: Option<&str>) -> DependencyNode {
    DependencyNode {
        id: id.to_owned(),
        kind,
        source_id: source_id.map(str::to_owned),
    }
}

fn edge(from: &str, to: &str, relation: &str) -> DependencyEdge {
    DependencyEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        relation: relation.to_owned(),
    }
}

fn graph() -> SourceDependencyGraph {
    SourceDependencyGraph::new(
        1,
        vec![
            node("source:file", DependencyNodeKind::File, Some("source:doc")),
            node("event:1", DependencyNodeKind::Event, None),
            node("evidence:1", DependencyNodeKind::Evidence, None),
            node("memory:1", DependencyNodeKind::Memory, None),
            node("context:1", DependencyNodeKind::ContextPlan, None),
            node("index:1", DependencyNodeKind::Index, None),
            node("summary:1", DependencyNodeKind::Summary, None),
            node("plan:1", DependencyNodeKind::Plan, None),
            node("unrelated", DependencyNodeKind::File, Some("source:other")),
        ],
        vec![
            edge("event:1", "source:file", "reads"),
            edge("evidence:1", "event:1", "quotes"),
            edge("memory:1", "evidence:1", "supports"),
            edge("context:1", "memory:1", "selects"),
            edge("index:1", "memory:1", "indexes"),
            edge("summary:1", "plan:1", "summarizes"),
            edge("plan:1", "memory:1", "uses"),
        ],
    )
    .expect("fixture graph")
}

#[test]
fn revoking_source_invalidates_all_derived_context() {
    let mut first = graph();
    let mut reversed = graph();
    reversed.edges.reverse();
    assert_eq!(first, reversed);
    assert_eq!(first.schema, SOURCE_DEPENDENCY_GRAPH_SCHEMA);

    let affected = first.affected_by_source("source:doc").expect("closure");
    let expected = BTreeSet::from([
        "context:1".to_owned(),
        "evidence:1".to_owned(),
        "event:1".to_owned(),
        "index:1".to_owned(),
        "memory:1".to_owned(),
        "plan:1".to_owned(),
        "source:file".to_owned(),
    ]);
    assert_eq!(affected.into_iter().collect::<BTreeSet<_>>(), expected);
    assert!(!first
        .affected_by_source("source:doc")
        .unwrap()
        .contains(&"unrelated".to_owned()));

    let invalidation = first.revoke_source("source:doc", 2).expect("revoke");
    assert_eq!(invalidation.schema, SOURCE_INVALIDATION_SCHEMA);
    assert_eq!(invalidation.previous_epoch, 1);
    assert_eq!(invalidation.data_epoch, 2);
    assert_eq!(first.data_epoch, 2);
    first.validate().expect("epoch update validates");
    assert_eq!(
        first.revoke_source("source:doc", 2).unwrap_err(),
        "source_dependency_epoch_rollback"
    );
}

#[test]
fn source_dependency_graph_rejects_drift_and_unknown_edges() {
    let mut encoded = serde_json::to_value(graph()).expect("graph serializes");
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SourceDependencyGraph>(encoded).is_err());

    let mut missing = graph();
    missing
        .edges
        .push(edge("memory:missing", "source:file", "reads"));
    assert_eq!(
        missing.validate().unwrap_err(),
        "source_dependency_edge_invalid"
    );

    let mut duplicate = graph();
    duplicate.edges.push(duplicate.edges[0].clone());
    assert_eq!(
        SourceDependencyGraph::new(1, duplicate.nodes.into_values().collect(), duplicate.edges)
            .unwrap_err(),
        "source_dependency_edge_duplicate"
    );
}
