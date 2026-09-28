//! DEP-26 source guard: the four failures the card names have to be refusals in the plan, and the
//! plan has to stay a plan.
//!
//! The absence check is load-bearing. A module called `retention_deletion` that can reach a file
//! or a store would be doing the deletion, and the reviewable-decision property is the whole point.

#[test]
fn dep26_pins_the_four_named_failures() {
    let source = include_str!("../src/retention_deletion.rs");

    for marker in [
        "retention_deletion_legal_hold",
        "retention_deletion_facts_missing",
        "retention_deletion_dangling_reference",
        "retention_revocation_missing",
        "retention_revocation_incomplete",
        "retention_revocation_epoch_stale",
        "retention_revocation_receipt_missing",
    ] {
        assert!(source.contains(marker), "DEP-26 module lost {marker}");
    }

    // The card's other half: a plan is not a deletion, and a deletion is not finished until the
    // layers it touched have been verified. These are the codes that make that difference real.
    for marker in [
        "DeletionCommitReceipt",
        "RebuildLayerVerification",
        "commit_retention_deletion",
        "RETENTION_COMMIT_RECEIPT_SCHEMA",
        "retention_rebuild_verification_required",
        "retention_rebuild_still_serves_deleted",
        "retention_rebuild_not_performed",
        "retention_rebuild_digest_mismatch",
        "retention_delete_receipt_missing",
        "retention_dry_run_not_committed",
        "retention_commit_target_not_planned",
        "retention_commit_target_missing",
        "retention_commit_plan_mismatch",
        "retention_commit_ledger_fact_changed",
        "retention_ledger_fact",
    ] {
        assert!(source.contains(marker), "DEP-26 module lost {marker}");
    }
}

#[test]
fn dep26_reuses_the_existing_lifecycles_rather_than_forking_them() {
    let source = include_str!("../src/retention_deletion.rs");

    // A second layer enum or a second hold type would be a second answer to "what is downstream of
    // the facts" and to "what is under hold", which is exactly what this slice must not do.
    for marker in [
        "BackupLegalHold",
        "DeletionMode",
        "RevocationLayer",
        "RevocationLayerState",
        "RevocationLayerObservation",
        "Tombstoned",
    ] {
        assert!(source.contains(marker), "DEP-26 module lost {marker}");
    }
    // And it must not define its own.
    for forbidden in [
        "pub enum RetentionLayer",
        "pub struct RetentionHold",
        "pub enum RetentionDeletionMode",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-26 invented a parallel lifecycle vocabulary: {forbidden}"
        );
    }
}

#[test]
fn dep26_pins_the_order_and_the_dry_run_invariant() {
    let source = include_str!("../src/retention_deletion.rs");

    for marker in [
        "DELETION_ORDER",
        "RevocationLayer::ALL",
        "fn order(&self)",
        "sort_by_key",
        "DeletionMode::DryRun",
        "deletable.clear()",
        "rebuild_required",
        "retention_deletion_dry_run",
    ] {
        if marker == "retention_deletion_dry_run" {
            // There is no such refusal code: the dry-run rule is enforced by emptying the set and
            // recording why, not by rejecting the request. Assert the shape instead.
            assert!(
                source.contains("dry_run\".to_owned()"),
                "DEP-26 must record dry_run as the protection reason"
            );
            continue;
        }
        assert!(source.contains(marker), "DEP-26 module lost {marker}");
    }
}

#[test]
fn dep26_plans_and_does_not_delete() {
    let source = include_str!("../src/retention_deletion.rs");

    for forbidden in [
        "std::fs",
        "File::",
        "Command::",
        "std::process",
        "remove_dir",
        "remove_file",
        "TcpStream",
        "reqwest",
        "tokio",
        "spawn",
        "EventStore",
        "append_event",
        "ControlPlane",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-26 module gained a token a plan must not have: {forbidden}"
        );
    }
}
