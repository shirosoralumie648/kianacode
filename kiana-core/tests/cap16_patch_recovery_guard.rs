//! CAP-16 source guard for patch transaction, bounded rollback and recovery boundaries.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-16 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn patch_commit_and_recovery_keep_one_journaled_boundary() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");
    require(
        patch,
        &[
            "PatchTransaction",
            "prepared.json",
            "resolved.json",
            "pending_patch_transactions",
            "workspace_transaction_reconciliation_required",
            "rollback_patch_guarded",
            "workspace_rollback_revision_conflict",
            "workspace_transaction_effect_unconfirmed",
            "result_unknown:apply_patch_rollback_failed",
            "verify_preconditions",
            "open_commit_directories",
            "sync_all",
            "renameat",
            "workspace_transaction_root_changed",
        ],
        "transaction boundary",
    );
    require(
        baseline,
        &[
            "patch_mid_commit_crash_is_reconciled_without_reexecution",
            "rollback_never_overwrites_a_concurrent_external_edit",
            "journal_write_failure_prevents_unrecorded_commit",
        ],
        "CAP-16 card",
    );
    for forbidden in [
        "reset --hard",
        "git checkout --",
        "clean_workspace_on_unknown",
    ] {
        assert!(
            !patch.contains(forbidden),
            "CAP-16 cleanup bypass marker present: {forbidden}"
        );
    }
}

#[test]
fn transaction_recovery_has_no_second_authority_or_automatic_reexecution() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    require(
        patch,
        &[
            "original_invocation_unchanged",
            "workspace_transaction_action_invalid",
            "workspace_transaction_resolution_required",
            "already_resolved",
            "reconciliation_required",
        ],
        "recovery semantics",
    );
}
