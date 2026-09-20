use kiana_domain::{
    ExtensionCleanupReceipt, ExtensionLifecycleChangeSet, ExtensionLifecycleControlAction,
    ExtensionRollbackCandidate,
};

fn raw_digest(value: char) -> String {
    value.to_string().repeat(64)
}

fn sha(value: char) -> String {
    format!("sha256:{}", raw_digest(value))
}

#[test]
fn upgrade_and_revoke_require_old_snapshot_invalidation() {
    let upgrade = ExtensionLifecycleChangeSet::new(
        ExtensionLifecycleControlAction::Upgrade,
        "plugin.alpha",
        4,
        5,
        true,
        true,
        true,
        true,
        None,
        None,
    )
    .expect("upgrade");
    upgrade.validate().expect("valid upgrade");

    let revoke = ExtensionLifecycleChangeSet::new(
        ExtensionLifecycleControlAction::Revoke,
        "plugin.alpha",
        5,
        6,
        true,
        true,
        true,
        true,
        None,
        None,
    )
    .expect("revoke");
    revoke.validate().expect("valid revoke");

    let unsafe_upgrade = ExtensionLifecycleChangeSet::new(
        ExtensionLifecycleControlAction::Upgrade,
        "plugin.alpha",
        4,
        5,
        true,
        false,
        true,
        true,
        None,
        None,
    )
    .expect_err("upgrade without approval invalidation must fail closed");
    assert_eq!(unsafe_upgrade, "extension_upgrade_invalidation_required");
}

#[test]
fn rollback_requires_verified_unrevoked_policy_allowed_candidate() {
    let candidate =
        ExtensionRollbackCandidate::new(raw_digest('a'), sha('b'), true, true, false, 7, 4)
            .expect("rollback candidate");
    let rollback = ExtensionLifecycleChangeSet::new(
        ExtensionLifecycleControlAction::Rollback,
        "plugin.alpha",
        7,
        8,
        true,
        false,
        true,
        true,
        Some(candidate),
        None,
    )
    .expect("rollback");
    rollback.validate().expect("valid rollback");

    let revoked =
        ExtensionRollbackCandidate::new(raw_digest('a'), sha('b'), true, true, true, 7, 4)
            .expect_err("revoked rollback target must fail closed");
    assert_eq!(revoked, "extension_rollback_candidate_not_eligible");
}

#[test]
fn uninstall_requires_cleanup_receipt_and_preserves_receipt_refs() {
    let cleanup = ExtensionCleanupReceipt::new(raw_digest('c'), true, true, true, "retained")
        .expect("cleanup receipt");
    let uninstall = ExtensionLifecycleChangeSet::new(
        ExtensionLifecycleControlAction::Uninstall,
        "plugin.alpha",
        8,
        9,
        true,
        true,
        true,
        true,
        None,
        Some(cleanup),
    )
    .expect("uninstall");
    uninstall.validate().expect("valid uninstall");

    let missing_cleanup = ExtensionLifecycleChangeSet::new(
        ExtensionLifecycleControlAction::Uninstall,
        "plugin.alpha",
        8,
        9,
        true,
        true,
        true,
        true,
        None,
        None,
    )
    .expect_err("uninstall without cleanup receipt must fail closed");
    assert_eq!(missing_cleanup, "extension_uninstall_cleanup_required");
}
