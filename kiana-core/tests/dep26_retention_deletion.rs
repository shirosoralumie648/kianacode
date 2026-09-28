//! DEP-26: connect retention, deletion and revocation to the lifecycles they protect.
//!
//! Deny-first, with the success paths last. The card names four failures — a deletion under a
//! legal hold, a derived layer deleted without its facts, a dangling artifact or backup reference,
//! and a revocation that does not propagate — and each has its own test here.

use kiana_core::{
    commit_retention_deletion, plan_retention_deletion, DeletionCommitReceipt, DeletionTarget,
    ProtectedTarget, RebuildLayerVerification, RetainedReference, RetentionDeletionPlan,
    RetentionDeletionRequest, RevocationLayer, RevocationLayerObservation, RevocationLayerState,
};
use kiana_domain::{BackupLegalHold, DeletionMode};

const CURRENT_EPOCH: u64 = 7;
const PREVIOUS_EPOCH: u64 = 6;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn acknowledged(layer: RevocationLayer) -> RevocationLayerObservation {
    RevocationLayerObservation::new(
        layer,
        RevocationLayerState::Tombstoned,
        CURRENT_EPOCH,
        41u64,
        Some(digest('a')),
        1_000,
    )
    .expect("acknowledged")
}

fn target(layer: RevocationLayer, id: &str) -> DeletionTarget {
    DeletionTarget::new(layer, id, digest('b'))
}

/// A request that is admissible: facts and one derived layer, both acknowledged, nothing held.
fn admissible() -> RetentionDeletionRequest {
    RetentionDeletionRequest::new(
        "plan-1",
        "operator",
        DeletionMode::Bounded,
        vec![
            target(RevocationLayer::Facts, "obj-1"),
            target(RevocationLayer::Artifact, "obj-1"),
        ],
        vec![],
        Vec::new(),
        vec![
            acknowledged(RevocationLayer::Facts),
            acknowledged(RevocationLayer::Artifact),
        ],
        Vec::new(),
        CURRENT_EPOCH,
        digest('a'),
    )
}

#[test]
fn deleting_something_a_legal_hold_covers_is_refused() {
    let hold = BackupLegalHold::new("hold-1", vec!["obj-1".to_owned()], "counsel", "litigation", 1)
        .expect("hold");
    let mut request = admissible();
    request.demanded = vec![target(RevocationLayer::Facts, "obj-1")];
    request.legal_holds = vec![hold];
    request.request_digest = request.digest();
    assert_eq!(
        plan_retention_deletion(&request).unwrap_err(),
        "retention_deletion_legal_hold"
    );
}

#[test]
fn a_derived_layer_cannot_be_deleted_without_its_facts() {
    // "只删 projection 不留事实". If the tombstone goes first, the layer that was supposed to stop
    // serving the payload has nothing left to be tombstoned against.
    let request = RetentionDeletionRequest::new(
        "plan-2",
        "operator",
        DeletionMode::Bounded,
        vec![target(RevocationLayer::Artifact, "obj-1")],
        vec![],
        Vec::new(),
        vec![acknowledged(RevocationLayer::Artifact)],
        Vec::new(),
        CURRENT_EPOCH,
        digest('a'),
    );
    assert_eq!(
        plan_retention_deletion(&request).unwrap_err(),
        "retention_deletion_facts_missing"
    );
}

#[test]
fn deleting_an_object_a_retained_thing_still_points_at_is_refused() {
    let mut request = admissible();
    request.demanded = vec![target(RevocationLayer::Artifact, "obj-1")];
    request.retained_references = vec![RetainedReference {
        retained_object_id: "ledger-1".to_owned(),
        retained_layer: RevocationLayer::Facts,
        references_layer: RevocationLayer::Artifact,
        references_object_id: "obj-1".to_owned(),
    }];
    request.request_digest = request.digest();
    assert_eq!(
        plan_retention_deletion(&request).unwrap_err(),
        "retention_deletion_dangling_reference"
    );
}

#[test]
fn a_revocation_that_has_not_propagated_blocks_the_deletion() {
    // Four different ways to not have propagated, four different reasons.
    let cases = [
        (Vec::new(), "retention_revocation_missing"),
        (
            vec![RevocationLayerObservation::new(
                RevocationLayer::Facts,
                RevocationLayerState::StillServing,
                CURRENT_EPOCH,
                41u64,
                None,
                1_000,
            )
            .expect("still serving")],
            "retention_revocation_incomplete",
        ),
        (
            vec![RevocationLayerObservation::new(
                RevocationLayer::Facts,
                RevocationLayerState::Tombstoned,
                PREVIOUS_EPOCH,
                41u64,
                Some(digest('a')),
                1_000,
            )
            .expect("stale epoch")],
            "retention_revocation_epoch_stale",
        ),
        (
            vec![RevocationLayerObservation::new(
                RevocationLayer::Facts,
                RevocationLayerState::Tombstoned,
                CURRENT_EPOCH,
                41u64,
                None,
                1_000,
            )
            .expect("no receipt")],
            "retention_revocation_receipt_missing",
        ),
    ];
    for (observations, expected) in cases {
        let request = RetentionDeletionRequest::new(
            "plan-3",
            "operator",
            DeletionMode::Bounded,
            vec![target(RevocationLayer::Facts, "obj-1")],
            vec![],
            Vec::new(),
            observations,
            Vec::new(),
            CURRENT_EPOCH,
        digest('a'),
        );
        assert_eq!(
            plan_retention_deletion(&request).unwrap_err(),
            expected
        );
    }
}

#[test]
fn a_dry_run_names_no_deletions_at_all() {
    let request = RetentionDeletionRequest::new(
        "plan-4",
        "operator",
        DeletionMode::DryRun,
        vec![
            target(RevocationLayer::Facts, "obj-1"),
            target(RevocationLayer::Artifact, "obj-1"),
        ],
        vec![],
        Vec::new(),
        vec![
            acknowledged(RevocationLayer::Facts),
            acknowledged(RevocationLayer::Artifact),
        ],
        Vec::new(),
        CURRENT_EPOCH,
        digest('a'),
    );
    let plan = plan_retention_deletion(&request).expect("dry run");
    assert!(plan.deletable.is_empty(), "a dry run must delete nothing");
    assert_eq!(plan.protected.len(), 2);
    assert!(
        plan.protected
            .iter()
            .all(|ProtectedTarget { reason, .. }| reason == "dry_run")
    );
    // And the order is still stated, so a reader can see what a real run would do.
    assert_eq!(plan.deletion_order.len(), 2);
}

#[test]
fn a_plan_cannot_be_edited_after_the_fact() {
    let request = admissible();
    let plan = plan_retention_deletion(&request).expect("plan");
    plan.validate_against(&request).expect("un edited");

    let mut promoted = plan.clone();
    promoted.deletable.push(target(RevocationLayer::Memory, "obj-1"));
    assert_eq!(
        promoted.validate_against(&request).unwrap_err(),
        "retention_deletion_plan_digest_mismatch"
    );

    let mut relabelled = plan.clone();
    relabelled.rebuild_required = false;
    assert_eq!(
        relabelled.validate_against(&request).unwrap_err(),
        "retention_deletion_plan_digest_mismatch"
    );

    let mut rebound = plan;
    rebound.plan_id = "other-plan".to_owned();
    assert_eq!(
        rebound.validate_against(&request).unwrap_err(),
        "retention_deletion_plan_binding_invalid"
    );
}

#[test]
fn a_bounded_plan_deletes_upstream_first_and_says_a_rebuild_is_needed() {
    let request = RetentionDeletionRequest::new(
        "plan-5",
        "operator",
        DeletionMode::Bounded,
        // Deliberately downstream first, to prove the order is imposed rather than inherited.
        vec![
            target(RevocationLayer::Index, "obj-1"),
            target(RevocationLayer::Artifact, "obj-1"),
            target(RevocationLayer::Facts, "obj-1"),
        ],
        vec![],
        Vec::new(),
        vec![
            acknowledged(RevocationLayer::Facts),
            acknowledged(RevocationLayer::Artifact),
            acknowledged(RevocationLayer::Index),
        ],
        Vec::new(),
        CURRENT_EPOCH,
        digest('a'),
    );
    let plan: RetentionDeletionPlan = plan_retention_deletion(&request).expect("bounded plan");
    assert_eq!(plan.deletion_order, vec![
        RevocationLayer::Facts,
        RevocationLayer::Artifact,
        RevocationLayer::Index,
    ]);
    assert_eq!(plan.deletable.len(), 3);
    assert!(plan.rebuild_required, "a derived layer was touched");
    plan.validate_against(&request).expect("re-derives");

    // Facts alone need no rebuild: there is nothing downstream to rebuild.
    let facts_only = RetentionDeletionRequest::new(
        "plan-6",
        "operator",
        DeletionMode::Bounded,
        vec![target(RevocationLayer::Facts, "obj-2")],
        vec![],
        Vec::new(),
        vec![acknowledged(RevocationLayer::Facts)],
        Vec::new(),
        CURRENT_EPOCH,
        digest('a'),
    );
    assert!(!plan_retention_deletion(&facts_only)
        .expect("facts only")
        .rebuild_required);
}

#[test]
fn a_held_object_is_kept_and_the_reason_is_recorded() {
    // The same hold that refuses a *demanded* deletion simply protects the object in an ordinary
    // plan. Refusing and protecting are the same rule seen from two directions.
    let hold = BackupLegalHold::new("hold-2", vec!["obj-1".to_owned()], "counsel", "litigation", 1)
        .expect("hold");
    let mut request = admissible();
    request.legal_holds = vec![hold];
    request.request_digest = request.digest();
    let plan = plan_retention_deletion(&request).expect("plan");
    assert!(plan.deletable.is_empty());
    assert_eq!(plan.protected.len(), 2);
    assert!(plan
        .protected
        .iter()
        .all(|entry| entry.reason == "legal_hold"));
}

// ---------------------------------------------------------------------------
// 卡片要求的另一半：commit receipt 与重建验证。
// 计划说「打算删」，receipt 说「删了，而且剩下的东西核过了」——这是两个不同的事实。
// ---------------------------------------------------------------------------

fn verified(layer: RevocationLayer) -> RebuildLayerVerification {
    RebuildLayerVerification::new(layer, true, 2, false, 1_800).expect("rebuilt")
}

fn commit_for(plan: &RetentionDeletionPlan) -> DeletionCommitReceipt {
    DeletionCommitReceipt::new(
        plan.plan_id.clone(),
        plan.plan_digest.clone(),
        plan.deletable.clone(),
        vec!["exec-1".to_owned(), "exec-2".to_owned()],
        vec![verified(RevocationLayer::Facts), verified(RevocationLayer::Artifact)],
        500,
        digest('a'),
        1_700,
    )
}

#[test]
fn a_plan_without_its_rebuild_verification_cannot_be_committed() {
    // 删掉派生层却不核验，删掉的字节没了而索引还指着它们——那不是完成，是制造了新的不一致。
    let request = admissible();
    let plan = plan_retention_deletion(&request).expect("plan");
    let mut commit = commit_for(&plan);
    commit.rebuild_verifications.clear();
    commit.commit_digest = commit.digest();
    assert_eq!(
        commit_retention_deletion(&plan, &request, &commit).unwrap_err(),
        "retention_rebuild_verification_required"
    );
}

#[test]
fn a_rebuild_that_still_serves_deleted_data_is_refused_by_the_verifier() {
    // 字节数证明不了任何事；「这一层还在提供被删掉的东西」才是要断言的那件事。
    assert_eq!(
        RebuildLayerVerification::new(RevocationLayer::Index, true, 2, true, 1_800).unwrap_err(),
        "retention_rebuild_still_serves_deleted"
    );
    assert_eq!(
        RebuildLayerVerification::new(RevocationLayer::Index, false, 2, false, 1_800).unwrap_err(),
        "retention_rebuild_not_performed"
    );
}

#[test]
fn a_target_without_an_execution_receipt_cannot_be_committed() {
    // 「我们删了」而没有「存储那边说了什么」，只是一个断言。
    let request = admissible();
    let plan = plan_retention_deletion(&request).expect("plan");
    let mut commit = commit_for(&plan);
    commit.execution_receipts.truncate(1);
    commit.commit_digest = commit.digest();
    assert_eq!(
        commit_retention_deletion(&plan, &request, &commit).unwrap_err(),
        "retention_delete_receipt_missing"
    );
}

#[test]
fn a_commit_that_moves_a_ledger_fact_is_refused_at_the_moment_it_can_break() {
    // 「保留/归档不改账本事实」这条不变量，在这里第一次有了它真正能被检验的时刻。
    let request = admissible();
    let plan = plan_retention_deletion(&request).expect("plan");
    let mut commit = commit_for(&plan);
    commit.ledger_fact_digest_after = digest('b');
    commit.commit_digest = commit.digest();
    assert_eq!(
        commit_retention_deletion(&plan, &request, &commit).unwrap_err(),
        "retention_commit_ledger_fact_changed"
    );
}

#[test]
fn a_dry_run_plan_cannot_be_committed_and_an_unplanned_target_cannot_be_carried() {
    let dry = RetentionDeletionRequest::new(
        "plan-dry",
        "operator",
        DeletionMode::DryRun,
        vec![
            target(RevocationLayer::Facts, "obj-1"),
            target(RevocationLayer::Artifact, "obj-1"),
        ],
        vec![],
        Vec::new(),
        vec![
            acknowledged(RevocationLayer::Facts),
            acknowledged(RevocationLayer::Artifact),
        ],
        Vec::new(),
        CURRENT_EPOCH,
        digest('a'),
    );
    let plan = plan_retention_deletion(&dry).expect("dry plan");
    let mut commit = commit_for(&plan);
    let commit = DeletionCommitReceipt::new(
        plan.plan_id.clone(),
        plan.plan_digest.clone(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        500,
        digest('a'),
        1_700,
    );
    assert_eq!(
        commit_retention_deletion(&plan, &dry, &commit).unwrap_err(),
        "retention_dry_run_not_committed"
    );

    // A commit may carry less than the plan, never more.
    let request = admissible();
    let plan = plan_retention_deletion(&request).expect("plan");
    let mut smuggled = commit_for(&plan);
    smuggled.committed_targets.push(target(RevocationLayer::Facts, "obj-999"));
    smuggled.execution_receipts.push("exec-3".to_owned());
    smuggled.commit_digest = smuggled.digest();
    assert_eq!(
        commit_retention_deletion(&plan, &request, &smuggled).unwrap_err(),
        "retention_commit_target_not_planned"
    );

    // And it may not drop one and call the pass finished.
    let mut dropped = commit_for(&plan);
    dropped.committed_targets.pop();
    dropped.execution_receipts.pop();
    dropped.commit_digest = dropped.digest();
    assert_eq!(
        commit_retention_deletion(&plan, &request, &dropped).unwrap_err(),
        "retention_commit_target_missing"
    );
}

#[test]
fn a_fully_evidenced_commit_is_recorded() {
    let request = admissible();
    let plan = plan_retention_deletion(&request).expect("plan");
    let commit = commit_for(&plan);
    let recorded = commit_retention_deletion(&plan, &request, &commit).expect("commit");
    assert_eq!(recorded.commit_digest, commit.commit_digest);
    assert_eq!(recorded.watermark_after, 500);
}
