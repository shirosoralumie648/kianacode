#[test]
fn side_effecting_boundaries_use_shared_path_containment() {
    let paths = include_str!("../../kiana-domain/src/paths.rs");
    let shell_patch = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let package = include_str!("../../kiana-daemon/src/execution_control.rs");
    let daemon_checkpoint = include_str!("../../kiana-daemon/src/workspace_checkpoints.rs");
    let workspace = include_str!("../../kiana-daemon/src/execution_workspace.rs");
    let core_checkpoint = include_str!("../src/workspace_checkpoints.rs");
    let cells = include_str!("../src/cell_registry.rs");
    let events = include_str!("../src/events.rs");
    for marker in [
        "pub fn enforce_path_containment",
        "pub fn enforce_root_containment",
        "path_not_relative",
        "path_outside_scope",
    ] {
        assert!(
            paths.contains(marker),
            "shared containment marker missing: {marker}"
        );
    }
    for (name, source, marker) in [
        ("shell/workdir", shell_patch, "enforce_root_containment"),
        ("shell/patch", shell_patch, "enforce_path_containment"),
        ("apply_patch", patch, "enforce_root_containment"),
        ("package", package, "enforce_path_containment"),
        (
            "daemon checkpoint",
            daemon_checkpoint,
            "enforce_path_containment",
        ),
        ("execution workspace", workspace, "enforce_path_containment"),
        (
            "core checkpoint",
            core_checkpoint,
            "enforce_path_containment",
        ),
        ("cell registry", cells, "enforce_path_containment"),
        ("event scope", events, "enforce_path_containment"),
    ] {
        assert!(
            source.contains(marker),
            "{name} bypasses shared containment"
        );
    }
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    assert!(mcp.contains("project_root"));
    assert!(memory.contains("confined_project_root"));
}
