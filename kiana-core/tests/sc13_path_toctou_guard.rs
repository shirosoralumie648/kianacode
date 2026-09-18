#[test]
fn sc13_path_mutations_are_root_relative_and_fenced() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let workspace = include_str!("../../kiana-daemon/src/execution_workspace.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let sessions = include_str!("../src/sessions.rs");
    let fence = include_str!("../src/security_fence.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");

    for marker in [
        "project_root.canonicalize()",
        "reject_symlink_components",
        "reject_hardlink",
        "metadata_fingerprint",
        "device: metadata.dev()",
        "inode: metadata.ino()",
        "capture_preconditions",
        "verify_preconditions",
        "rollback_patch_guarded",
        "openat(",
        "O_NOFOLLOW",
        "renameat(",
    ] {
        assert!(
            patch.contains(marker),
            "SC-13 patch marker missing: {marker}"
        );
    }

    for marker in [
        "read_regular",
        "file_identity_changed_or_hardlink",
        "O_NOFOLLOW",
        "openat(",
        "directory_identity_changed",
        "special_file_requires_explicit_adapter",
    ] {
        assert!(
            workspace.contains(marker),
            "SC-13 workspace marker missing: {marker}"
        );
    }

    for marker in [
        "canonicalize_dir",
        "sandbox_read_deny_symlink_requires_staging",
        "sandbox_external_symlink_requires_staging",
        "starts_with(&project_root)",
    ] {
        assert!(
            sandbox.contains(marker),
            "SC-13 sandbox marker missing: {marker}"
        );
    }

    for marker in [
        "acquire_durable_path_locks",
        "path_lock_conflict",
        "canonical_resource_set",
        "O_NOFOLLOW",
    ] {
        assert!(
            sessions.contains(marker),
            "SC-13 path-lock marker missing: {marker}"
        );
    }

    for marker in [
        "session_generation",
        "validate_current",
        "FACT_FENCE_MISMATCH",
        "authority_epoch",
    ] {
        assert!(
            fence.contains(marker),
            "SC-13 fence marker missing: {marker}"
        );
    }

    for marker in [
        "validate_action",
        "capability_action_not_prepared",
        "execution_scope_required",
        "execution_permit_verifier_required",
        "validate_for_request",
    ] {
        assert!(
            broker.contains(marker),
            "SC-13 broker recheck marker missing: {marker}"
        );
    }

    for (name, source) in [
        ("patch", patch),
        ("workspace", workspace),
        ("sandbox", sandbox),
        ("sessions", sessions),
        ("fence", fence),
        ("broker", broker),
    ] {
        for forbidden in ["TeamCreate", "SendMessage"] {
            assert!(
                !source.contains(forbidden),
                "SC-13 {name} source contains forbidden free-message path: {forbidden}"
            );
        }
    }
}
