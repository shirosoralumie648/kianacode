#[test]
fn extension_control_changes_require_invalidation_rollback_and_cleanup_fences() {
    let lifecycle = include_str!("../../kiana-domain/src/extension_lifecycle.rs");
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    for marker in [
        "ExtensionLifecycleChangeSet",
        "ExtensionRollbackCandidate",
        "ExtensionCleanupReceipt",
        "extension_upgrade_invalidation_required",
        "extension_revoke_invalidation_required",
        "extension_rollback_candidate_not_eligible",
        "extension_uninstall_cleanup_required",
    ] {
        assert!(
            lifecycle.contains(marker),
            "missing EXT-23 domain marker: {marker}"
        );
    }
    for marker in [
        "\"upgrade\"",
        "\"revoke\"",
        "\"rollback\"",
        "\"uninstall\"",
        "extension_version_content_conflict",
        "extension_package_revoked",
        "extension_rollback_snapshot_missing",
        "extension_activation_event_failed",
    ] {
        assert!(
            daemon.contains(marker),
            "missing EXT-23 daemon marker: {marker}"
        );
    }
    assert!(lifecycle.contains("receipt_refs_retained"));
}
