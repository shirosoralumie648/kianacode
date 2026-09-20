use kiana_domain::{
    ExtensionBinding, ExtensionBindingSnapshot, ExtensionDependency, ExtensionDependencyGraph,
    ExtensionGraphKind, ExtensionGraphNode, ExtensionNetworkPolicy, ScopeDimension, ScopeLimit,
    ScopeSet,
};
use std::collections::BTreeSet;

fn digest() -> String {
    "a".repeat(64)
}

fn scope(operations: &[&str]) -> ScopeSet {
    ScopeSet::new(
        ScopeDimension::Restricted(operations.iter().map(|value| (*value).to_owned()).collect()),
        ScopeDimension::Restricted(vec![".".to_owned()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::Restricted(16),
        ScopeLimit::Restricted(4),
    )
    .expect("fixture scope")
}

fn node(
    node_id: &str,
    version: &str,
    kind: ExtensionGraphKind,
    dependencies: Vec<ExtensionDependency>,
    supported_platforms: BTreeSet<String>,
    node_scope: ScopeSet,
) -> ExtensionGraphNode {
    ExtensionGraphNode {
        node_id: node_id.to_owned(),
        extension_id: node_id.to_owned(),
        version: version.to_owned(),
        kind,
        source_digest: digest(),
        dependencies,
        supported_platforms,
        scope: node_scope,
    }
}

#[test]
fn dependency_graph_rejects_cycles_and_missing_versions() {
    let cycle = ExtensionDependencyGraph::new(
        "linux",
        vec![
            node(
                "skill.alpha",
                "1",
                ExtensionGraphKind::Skill,
                vec![ExtensionDependency {
                    dependency_id: "skill.beta".to_owned(),
                    version: "1".to_owned(),
                    kind: ExtensionGraphKind::Skill,
                    optional: false,
                    required_scope: None,
                }],
                BTreeSet::new(),
                scope(&["shell"]),
            ),
            node(
                "skill.beta",
                "1",
                ExtensionGraphKind::Skill,
                vec![ExtensionDependency {
                    dependency_id: "skill.alpha".to_owned(),
                    version: "1".to_owned(),
                    kind: ExtensionGraphKind::Skill,
                    optional: false,
                    required_scope: None,
                }],
                BTreeSet::new(),
                scope(&["shell"]),
            ),
        ],
    )
    .expect_err("cycle must fail closed");
    assert_eq!(cycle, "extension_dependency_cycle");

    let missing_version = ExtensionDependencyGraph::new(
        "linux",
        vec![
            node(
                "skill.alpha",
                "1",
                ExtensionGraphKind::Skill,
                vec![ExtensionDependency {
                    dependency_id: "skill.beta".to_owned(),
                    version: "2".to_owned(),
                    kind: ExtensionGraphKind::Skill,
                    optional: false,
                    required_scope: None,
                }],
                BTreeSet::new(),
                scope(&["shell"]),
            ),
            node(
                "skill.beta",
                "1",
                ExtensionGraphKind::Skill,
                Vec::new(),
                BTreeSet::new(),
                scope(&["shell"]),
            ),
        ],
    )
    .expect_err("exact version mismatch must fail closed");
    assert_eq!(missing_version, "extension_dependency_version_unavailable");
}

#[test]
fn dependency_graph_rejects_scope_and_platform_mismatch() {
    let scope_mismatch = ExtensionDependencyGraph::new(
        "linux",
        vec![
            node(
                "skill.alpha",
                "1",
                ExtensionGraphKind::Skill,
                vec![ExtensionDependency {
                    dependency_id: "provider.beta".to_owned(),
                    version: "1".to_owned(),
                    kind: ExtensionGraphKind::Provider,
                    optional: false,
                    required_scope: None,
                }],
                BTreeSet::new(),
                scope(&["shell"]),
            ),
            node(
                "provider.beta",
                "1",
                ExtensionGraphKind::Provider,
                Vec::new(),
                BTreeSet::new(),
                scope(&["apply_patch"]),
            ),
        ],
    )
    .expect_err("scope disjointness must fail closed");
    assert_eq!(scope_mismatch, "extension_dependency_scope_disjoint");

    let mut windows = BTreeSet::new();
    windows.insert("windows".to_owned());
    let platform_mismatch = ExtensionDependencyGraph::new(
        "linux",
        vec![node(
            "provider.beta",
            "1",
            ExtensionGraphKind::Provider,
            Vec::new(),
            windows,
            scope(&["shell"]),
        )],
    )
    .expect_err("platform mismatch must fail closed");
    assert_eq!(platform_mismatch, "extension_dependency_platform_mismatch");
}

#[test]
fn binding_snapshot_is_invalidated_when_parent_scope_shrinks() {
    let parent = scope(&["shell", "apply_patch"]);
    let graph = ExtensionDependencyGraph::new(
        "linux",
        vec![node(
            "skill.alpha",
            "1",
            ExtensionGraphKind::Skill,
            Vec::new(),
            BTreeSet::new(),
            parent.clone(),
        )],
    )
    .expect("graph");
    let resolution = graph.resolve().expect("resolution");
    let binding = ExtensionBinding::new(
        "binding.alpha",
        "snapshot.alpha",
        "skill.alpha",
        "prompt",
        "packet.alpha",
        "builder",
        BTreeSet::from(["source".to_owned()]),
        ExtensionNetworkPolicy::Deny,
        BTreeSet::from(["secret.handle".to_owned()]),
        ScopeLimit::Restricted(8),
        100,
        &parent,
        scope(&["shell"]),
    )
    .expect("binding");
    let snapshot =
        ExtensionBindingSnapshot::new(&graph, &resolution, "snapshot.alpha", 1, vec![binding])
            .expect("snapshot");
    snapshot
        .revalidate(&graph, 1)
        .expect("initial snapshot valid");

    let narrowed_graph = ExtensionDependencyGraph::new(
        "linux",
        vec![node(
            "skill.alpha",
            "1",
            ExtensionGraphKind::Skill,
            Vec::new(),
            BTreeSet::new(),
            scope(&["apply_patch"]),
        )],
    )
    .expect("narrowed graph");
    assert_eq!(
        snapshot.bindings[0]
            .is_valid_against(&scope(&["apply_patch"]), 1)
            .expect_err("parent scope shrink must invalidate child binding"),
        "extension_binding_parent_scope_changed"
    );
    assert_eq!(
        snapshot
            .revalidate(&narrowed_graph, 1)
            .expect_err("graph shrink must invalidate the binding snapshot"),
        "extension_binding_graph_changed"
    );
}

#[test]
fn binding_snapshot_round_trips_with_stable_digests() {
    let scope = scope(&["shell"]);
    let graph = ExtensionDependencyGraph::new(
        "linux",
        vec![node(
            "skill.alpha",
            "1",
            ExtensionGraphKind::Skill,
            Vec::new(),
            BTreeSet::new(),
            scope.clone(),
        )],
    )
    .expect("graph");
    let resolution = graph.resolve().expect("resolution");
    let binding = ExtensionBinding::new(
        "binding.alpha",
        "snapshot.alpha",
        "skill.alpha",
        "prompt",
        "packet.alpha",
        "builder",
        BTreeSet::new(),
        ExtensionNetworkPolicy::Deny,
        BTreeSet::new(),
        ScopeLimit::Restricted(1),
        100,
        &scope,
        scope,
    )
    .expect("binding");
    let snapshot =
        ExtensionBindingSnapshot::new(&graph, &resolution, "snapshot.alpha", 1, vec![binding])
            .expect("snapshot");
    let encoded = serde_json::to_value(&snapshot).expect("json");
    let decoded: ExtensionBindingSnapshot = serde_json::from_value(encoded).expect("round trip");
    decoded.validate().expect("decoded snapshot valid");
    assert_eq!(decoded.snapshot_digest, snapshot.snapshot_digest);
}
