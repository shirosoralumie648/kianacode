use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const STRICT_ALLOWED: &[(&str, &[&str])] = &[
    ("kiana-domain", &[]),
    ("kiana-protocol", &["kiana-domain"]),
    ("kiana-client", &["kiana-protocol"]),
    (
        "kiana-core",
        &[
            "kiana-domain",
            "kiana-gates",
            "kiana-policy",
            "kiana-ports",
            "kiana-runner-protocol",
            "kiana-workflow",
        ],
    ),
    (
        "kiana-runner",
        &["kiana-domain", "kiana-ports", "kiana-runner-protocol"],
    ),
    ("kiana-runner-protocol", &["kiana-domain"]),
    ("kiana-policy", &["kiana-domain"]),
    ("kiana-gates", &["kiana-domain"]),
    ("kiana-workflow", &["kiana-domain"]),
    ("kiana-ports", &["kiana-domain", "kiana-runner-protocol"]),
    ("kiana-eventlog", &["kiana-domain", "kiana-ports"]),
    ("kiana-capability-broker", &["kiana-domain", "kiana-ports"]),
    (
        "kiana-daemon",
        &[
            "kiana-capability-broker",
            "kiana-core",
            "kiana-domain",
            "kiana-eventlog",
            "kiana-gates",
            "kiana-policy",
            "kiana-ports",
            "kiana-protocol",
            "kiana-query",
            "kiana-runner",
            "kiana-runner-protocol",
            "kiana-services",
        ],
    ),
];

const LEGACY_EDGES: &[(&str, &str)] = &[
    ("kiana-entrypoints", "kiana-commands"),
    ("kiana-entrypoints", "kiana-query"),
    ("kiana-entrypoints", "kiana-services"),
    ("kiana-entrypoints", "kiana-tools"),
    ("kiana-commands", "kiana-services"),
    ("kiana-commands", "kiana-tasks"),
    ("kiana-commands", "kiana-tools"),
    ("kiana-tools", "kiana-query"),
    ("kiana-tools", "kiana-services"),
];

fn workspace_dependencies() -> BTreeMap<String, BTreeSet<String>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .expect("cargo metadata should start");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("valid cargo metadata");
    let packages = metadata["packages"]
        .as_array()
        .expect("metadata packages should be an array");
    let workspace_names = packages
        .iter()
        .filter_map(|package| package["name"].as_str())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    packages
        .iter()
        .map(|package| {
            let name = package["name"].as_str().unwrap().to_owned();
            let dependencies = package["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|dependency| dependency["kind"].is_null())
                .filter_map(|dependency| dependency["name"].as_str())
                .filter(|dependency| workspace_names.contains(*dependency))
                .map(str::to_owned)
                .collect();
            (name, dependencies)
        })
        .collect()
}

#[test]
fn strict_control_plane_crates_only_use_allowed_internal_dependencies() {
    let dependencies = workspace_dependencies();
    for (package, allowed) in STRICT_ALLOWED {
        let actual = dependencies
            .get(*package)
            .unwrap_or_else(|| panic!("missing strict workspace package {package}"));
        let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
        let unexpected = actual
            .iter()
            .filter(|dependency| !allowed.contains(dependency.as_str()))
            .collect::<Vec<_>>();
        assert!(
            unexpected.is_empty(),
            "{package} has forbidden internal dependencies: {unexpected:?}"
        );
    }
}

#[test]
fn legacy_implementation_edges_can_only_decrease() {
    let dependencies = workspace_dependencies();
    let legacy = LEGACY_EDGES.iter().copied().collect::<BTreeSet<_>>();
    let debt_sources = ["kiana-entrypoints", "kiana-commands", "kiana-tools"];
    let implementations = [
        "kiana-commands",
        "kiana-query",
        "kiana-services",
        "kiana-tasks",
        "kiana-tools",
    ];
    let mut present = BTreeSet::new();

    for source in debt_sources {
        for target in dependencies.get(source).into_iter().flatten() {
            if implementations.contains(&target.as_str()) {
                let edge = (source, target.as_str());
                assert!(
                    legacy.contains(&edge),
                    "new legacy edge is forbidden: {edge:?}"
                );
                present.insert(edge);
            }
        }
    }

    assert!(present.len() <= LEGACY_EDGES.len());
    eprintln!("legacy_edges_remaining={}", present.len());
}
