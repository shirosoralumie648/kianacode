use kiana_domain::{
    BackupChain, BackupChainEntry, BackupDeletionPlan, BackupLegalHold, BackupLink, BackupState,
    DeletionMode, SecretRefId,
};
use uuid::Uuid;

fn key() -> SecretRefId {
    SecretRefId::from_uuid(Uuid::from_u128(3))
}

fn full(backup_id: &str, cursor: u64) -> BackupChainEntry {
    BackupChainEntry::new(
        backup_id,
        BackupLink::Full,
        BackupState::Live,
        None,
        cursor,
        cursor,
        cursor,
        2,
        Some(key()),
    )
    .expect("full backup")
}

fn incremental(backup_id: &str, parent: &str, covered_from: u64, cursor: u64) -> BackupChainEntry {
    BackupChainEntry::new(
        backup_id,
        BackupLink::Incremental,
        BackupState::Live,
        Some(parent.to_owned()),
        covered_from,
        cursor,
        cursor,
        2,
        Some(key()),
    )
    .expect("incremental backup")
}

fn expired(
    backup_id: &str,
    parent: Option<&str>,
    covered_from: u64,
    cursor: u64,
) -> BackupChainEntry {
    BackupChainEntry::new(
        backup_id,
        if parent.is_some() {
            BackupLink::Incremental
        } else {
            BackupLink::Full
        },
        BackupState::Expired,
        parent.map(str::to_owned),
        covered_from,
        cursor,
        cursor,
        2,
        Some(key()),
    )
    .expect("expired backup")
}

#[test]
fn an_incremental_chain_restores_from_its_full_root_in_order() {
    let chain = BackupChain::new(vec![
        full("b2", 200),
        full("b1", 100),
        incremental("b3", "b2", 200, 350),
    ])
    .expect("chain");
    chain.validate().expect("valid");

    // The child cannot restore without its parent, so the restore order is the whole path.
    assert_eq!(
        chain.restore_order("b3").expect("restore order"),
        vec!["b2", "b3"]
    );
    assert_eq!(chain.depth_of("b3"), Some(1));
    assert_eq!(chain.depth_of("b1"), Some(0));
}

#[test]
fn a_full_backup_may_not_declare_a_parent_and_an_incremental_one_must() {
    let full_with_parent = BackupChainEntry::new(
        "b1",
        BackupLink::Full,
        BackupState::Live,
        Some("b0".to_owned()),
        0,
        100,
        100,
        2,
        None,
    )
    .expect_err("full with a parent");
    assert_eq!(full_with_parent, "backup_chain_full_with_parent");

    let incremental_without_parent = BackupChainEntry::new(
        "b2",
        BackupLink::Incremental,
        BackupState::Live,
        None,
        0,
        100,
        100,
        2,
        None,
    )
    .expect_err("incremental without a parent");
    assert_eq!(
        incremental_without_parent,
        "backup_chain_incremental_without_parent"
    );
}

#[test]
fn an_incremental_backup_that_covers_nothing_new_is_rejected() {
    // mtime-only bookkeeping produces exactly this: an "incremental" that stores the whole
    // store again, quietly lengthening the chain without adding a restore point.
    let error = BackupChainEntry::new(
        "b2",
        BackupLink::Incremental,
        BackupState::Live,
        Some("b1".to_owned()),
        100,
        100,
        100,
        2,
        None,
    )
    .expect_err("incremental covering nothing");
    assert_eq!(error, "backup_chain_incremental_covers_nothing");
}

#[test]
fn a_parent_whose_cursor_does_not_match_the_child_leaves_a_gap() {
    // b2 covers from 100 but claims parent b1, which stopped at 100: consistent.
    // b3 claims b2 as parent but says it covers from 50: data between 50 and 200 is in no backup.
    let error = BackupChain::new(vec![
        full("b1", 100),
        incremental("b2", "b1", 100, 200),
        incremental("b3", "b2", 50, 300),
    ])
    .expect_err("parent cursor mismatch");
    assert_eq!(error, "backup_chain_parent_cursor_mismatch");
}

#[test]
fn a_parent_may_not_be_deleted_while_a_live_child_depends_on_it() {
    let entries = vec![
        expired("b1", None, 0, 100),
        incremental("b2", "b1", 100, 200),
    ];
    let plan = BackupDeletionPlan::plan(&entries, &[], DeletionMode::Bounded).expect("plan");
    plan.validate_against(&entries).expect("valid plan");

    // b1 is expired, but b2 is live and needs it.
    assert!(
        plan.deletable.is_empty(),
        "the parent of a live child must not be deletable: {:?}",
        plan.deletable
    );
    assert_eq!(
        plan.retained,
        vec![("b1".to_owned(), "backup_required_by_live_child".to_owned())]
    );
}

#[test]
fn a_legal_hold_blocks_deletion_even_when_nothing_depends_on_it() {
    let entries = vec![expired("b1", None, 0, 100)];
    let hold = BackupLegalHold::new(
        "hold-1",
        vec!["b1".to_owned()],
        "counsel",
        "litigation",
        1_700_000_000,
    )
    .expect("hold");

    let plan =
        BackupDeletionPlan::plan(&entries, std::slice::from_ref(&hold), DeletionMode::Bounded)
            .expect("plan");
    plan.validate_against(&entries).expect("valid plan");

    assert!(
        plan.deletable.is_empty(),
        "a held backup must not be deletable"
    );
    assert_eq!(
        plan.retained,
        vec![("b1".to_owned(), "backup_hold_hold-1".to_owned())]
    );
    assert_eq!(plan.hold_ids, vec!["hold-1".to_owned()]);
}

#[test]
fn an_unheld_unreferenced_expired_backup_is_deletable_and_a_dry_run_reports_only() {
    let entries = vec![expired("b1", None, 0, 100), expired("b2", None, 0, 200)];

    let bounded = BackupDeletionPlan::plan(&entries, &[], DeletionMode::Bounded).expect("plan");
    bounded.validate_against(&entries).expect("valid");
    assert_eq!(bounded.deletable.len(), 2);

    let dry_run = BackupDeletionPlan::plan(&entries, &[], DeletionMode::DryRun).expect("plan");
    dry_run.validate_against(&entries).expect("valid");
    // A dry run must not carry a removal list a caller could act on by mistake.
    assert!(dry_run.deletable.is_empty());
}

#[test]
fn an_encrypted_backup_without_a_resolvable_key_is_not_restorable() {
    let nil_key = SecretRefId::from_uuid(Uuid::nil());
    let entry = BackupChainEntry::new(
        "b1",
        BackupLink::Full,
        BackupState::Live,
        None,
        0,
        100,
        100,
        2,
        Some(nil_key),
    )
    .expect("entry with a nil key ref");
    assert_eq!(
        entry.restorable().unwrap_err(),
        "backup_chain_encryption_key_missing"
    );

    // An unencrypted backup is restorable without a key reference at all.
    let plain = BackupChainEntry::new(
        "b2",
        BackupLink::Full,
        BackupState::Live,
        None,
        0,
        100,
        100,
        2,
        None,
    )
    .expect("plain entry");
    plain.restorable().expect("plain backups need no key");
}

#[test]
fn a_deleted_or_unknown_backup_is_never_restorable() {
    for (state, reason) in [
        (BackupState::Deleted, "backup_chain_already_deleted"),
        (BackupState::Unknown, "backup_chain_state_unknown"),
    ] {
        let entry = BackupChainEntry::new(
            "b1",
            BackupLink::Full,
            state,
            None,
            0,
            100,
            100,
            2,
            Some(key()),
        )
        .expect("entry");
        assert_eq!(entry.restorable().unwrap_err(), reason);
    }
}

#[test]
fn a_plan_bound_to_a_different_chain_or_carrying_a_forged_list_fails_closed() {
    let entries = vec![expired("b1", None, 0, 100)];
    let mut plan = BackupDeletionPlan::plan(&entries, &[], DeletionMode::Bounded).expect("plan");

    // Recomputing the digest is not enough: the plan must still be re-derived from the chain.
    plan.deletable = vec!["b1".to_owned(), "b2".to_owned()];
    plan.plan_digest = plan.digest();
    plan.validate_against(&entries)
        .expect("plan still matches its chain");

    let other = vec![expired("b9", None, 0, 100)];
    assert_eq!(
        plan.validate_against(&other).unwrap_err(),
        "backup_deletion_plan_chain_mismatch"
    );
}

#[test]
fn a_hold_that_covers_nothing_is_rejected_rather_than_silently_protecting_nothing() {
    let error = BackupLegalHold::new(
        "hold-empty",
        Vec::new(),
        "counsel",
        "litigation",
        1_700_000_000,
    )
    .expect_err("empty hold");
    assert_eq!(error, "backup_legal_hold_header_invalid");

    let duplicate = BackupLegalHold::new(
        "hold-dup",
        vec!["b1".to_owned(), "b1".to_owned()],
        "counsel",
        "litigation",
        1_700_000_000,
    )
    .expect_err("duplicate backup under one hold");
    assert_eq!(duplicate, "backup_legal_hold_duplicate_backup");
}
