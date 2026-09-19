//! CAP-10 source guard for isolated write layers, exact new files and controlled publication.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-10 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn staged_workspace_and_patch_use_one_controlled_publish_boundary() {
    let workspace = include_str!("../../kiana-daemon/src/execution_workspace.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let control = include_str!("../../kiana-daemon/src/execution_control.rs");
    let capabilities = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let checkpoints = include_str!("../src/workspace_checkpoints.rs");

    require(
        workspace,
        &[
            "ExecutionWorkspace",
            "isolated_staged",
            "baseline",
            "PublishedFile",
            "path_allow",
            "publish_workspace_files",
            "changeset_outside_grant:not_published",
            "No script changes are published after failure, timeout or cancellation",
            "host_effect",
            "retained",
        ],
        "staged write layer",
    );
    require(
        patch,
        &[
            "publish_workspace_files",
            "PathSnapshot",
            "PlannedPatch",
            "commit_planned",
            "rollback_patch_guarded",
            "workspace_journal",
            "pending_patch_transactions",
            "confined_new_file",
            "create_new_file_at",
            "open_commit_directories",
            "same_snapshot",
            "workspace_publish_revision_conflict",
        ],
        "controlled patch publish",
    );
    require(
        control,
        &[
            "ExecutionWorkspace::prepare",
            "workspace",
            "publish",
            "process_publication_join_failed",
            "result_unknown",
            "stop_confirmed",
        ],
        "process publication",
    );
    require(
        capabilities,
        &[
            "ExecutionWorkspace::prepare",
            "workspace_publication_join_failed",
            "apply_patch_requires_workspace_write",
        ],
        "capability publication",
    );
    require(
        checkpoints,
        &[
            "prepare_checkpoint_restore",
            "checkpoint_data_epoch_changed",
            "workspace_revision",
        ],
        "checkpoint publication",
    );
}

#[test]
fn new_file_scope_and_publish_failure_never_overwrite_unrelated_host_state() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let workspace = include_str!("../../kiana-daemon/src/execution_workspace.rs");
    let control = include_str!("../../kiana-daemon/src/execution_control.rs");
    let cap08 = include_str!("cap08_path_resolver_guard.rs");
    let cap09 = include_str!("cap09_linux_file_view_guard.rs");

    require(
        patch,
        &[
            "new_file",
            "apply_patch_path_outside_project",
            "apply_patch_path_hardlink",
            "workspace_publish_revision_conflict",
            "workspace_rollback_revision_conflict",
            "result_unknown:apply_patch_rollback_failed",
            "workspace_transaction_reconciliation_required",
        ],
        "new-file/rollback fence",
    );
    require(
        workspace,
        &[
            "write_scope_required",
            "write_scope_empty",
            "path_allow",
            "file_identity_changed_or_hardlink",
            "changeset_outside_grant:not_published",
            "host_effect\":\"none\"",
        ],
        "write scope fence",
    );
    require(
        control,
        &[
            "path_allow",
            "workspace-write",
            "result_unknown:process_publication_join_failed",
        ],
        "control write fence",
    );
    require(
        cap08,
        &["PathResolverPort", "reject_hardlink", "renameat("],
        "CAP-08 identity",
    );
    require(
        cap09,
        &["isolated_staged", "workspace-write", "path_allow"],
        "CAP-09 view",
    );
    for source in [patch, workspace, control] {
        for forbidden in [
            "reset --hard",
            "git checkout --",
            "publish_out_of_scope",
            "overwrite_unrelated_host_file",
            "clean_workspace_on_unknown",
        ] {
            assert!(
                !source.contains(forbidden),
                "CAP-10 publish bypass marker present: {forbidden}"
            );
        }
    }
}
