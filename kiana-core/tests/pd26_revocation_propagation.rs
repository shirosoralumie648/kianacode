//! PD-26 failure-first fixture: every rejected-first column of the card, one test per rejection.

use kiana_core::{
    admit_derived_read_after_recovery, admit_derived_write, merge_layer_observation,
    propagation_scope, RevocationKind, RevocationLayer, RevocationLayerObservation,
    RevocationLayerState, RevocationPropagationReport, RevocationPropagationRequest,
    RevocationPropagationStatus,
};
use kiana_domain::RequestId;

const CURRENT_EPOCH: u64 = 7;
const PREVIOUS_EPOCH: u64 = 6;
const TOMBSTONE_CURSOR: u64 = 41;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn acknowledged(layer: RevocationLayer) -> RevocationLayerObservation {
    RevocationLayerObservation::new(
        layer,
        RevocationLayerState::Tombstoned,
        CURRENT_EPOCH,
        TOMBSTONE_CURSOR,
        Some(digest('a')),
        1_000,
    )
    .unwrap()
}

fn still_serving(layer: RevocationLayer) -> RevocationLayerObservation {
    RevocationLayerObservation::new(
        layer,
        RevocationLayerState::StillServing,
        CURRENT_EPOCH,
        TOMBSTONE_CURSOR,
        None,
        1_000,
    )
    .unwrap()
}

fn unreachable(layer: RevocationLayer) -> RevocationLayerObservation {
    RevocationLayerObservation::new(
        layer,
        RevocationLayerState::Unreachable,
        CURRENT_EPOCH,
        TOMBSTONE_CURSOR,
        None,
        1_000,
    )
    .unwrap()
}

fn pending(layer: RevocationLayer) -> RevocationLayerObservation {
    RevocationLayerObservation::new(
        layer,
        RevocationLayerState::Pending,
        CURRENT_EPOCH,
        TOMBSTONE_CURSOR,
        None,
        1_000,
    )
    .unwrap()
}

fn observations() -> Vec<RevocationLayerObservation> {
    RevocationLayer::ALL.into_iter().map(acknowledged).collect()
}

fn request(observations: Vec<RevocationLayerObservation>) -> RevocationPropagationRequest {
    RevocationPropagationRequest::new(
        RequestId::new(),
        RevocationKind::Delete,
        "principal:operator",
        "/repo",
        "src/input.txt",
        digest('b'),
        PREVIOUS_EPOCH,
        CURRENT_EPOCH,
        TOMBSTONE_CURSOR,
        observations,
    )
    .unwrap()
}

fn report(observations: Vec<RevocationLayerObservation>) -> RevocationPropagationReport {
    RevocationPropagationReport::evaluate(&request(observations)).unwrap()
}

#[test]
fn every_layer_acknowledged_in_order_completes_the_propagation() {
    let request = request(observations());
    let report = RevocationPropagationReport::evaluate(&request).unwrap();
    assert_eq!(report.status, RevocationPropagationStatus::Complete);
    assert!(report.complete());
    assert_eq!(report.acknowledged_layers, RevocationLayer::ALL.to_vec());
    assert!(report.pending_layers.is_empty());
    assert!(report.reason.is_empty());
    assert!(request.pending_layers().unwrap().is_empty());
    report.validate_against(&request).unwrap();
}

#[test]
fn the_propagation_scope_is_facts_artifact_memory_index_cache_checkpoint() {
    assert_eq!(
        propagation_scope(),
        [
            "facts",
            "artifact",
            "memory",
            "index",
            "cache",
            "checkpoint"
        ]
        .into_iter()
        .collect()
    );
    // A derive/delete/expire decision is the same propagation with a different reason.
    for kind in [
        RevocationKind::Revoke,
        RevocationKind::Delete,
        RevocationKind::Expire,
    ] {
        let request = RevocationPropagationRequest::new(
            RequestId::new(),
            kind,
            "principal:operator",
            "/repo",
            "src/input.txt",
            digest('b'),
            PREVIOUS_EPOCH,
            CURRENT_EPOCH,
            TOMBSTONE_CURSOR,
            observations(),
        )
        .unwrap();
        let report = RevocationPropagationReport::evaluate(&request).unwrap();
        assert_eq!(report.kind, kind);
        assert!(report.complete());
    }
}

// --- 先拒绝: 任一派生层继续返回 revoked data ---------------------------------------------

#[test]
fn a_derived_layer_that_still_serves_revoked_data_is_never_a_completion() {
    for (index, layer) in RevocationLayer::ALL.into_iter().enumerate() {
        let mut observations = observations();
        observations[index] = still_serving(layer);
        let report = report(observations);
        assert_eq!(
            report.status,
            RevocationPropagationStatus::RevokedDataServed
        );
        assert!(!report.complete());
        assert_eq!(
            report.reason,
            format!(
                "revocation_derived_layer_still_serves_data:{}",
                layer.as_str()
            )
        );
    }
}

#[test]
fn a_serving_layer_outranks_every_other_denial_in_the_fixed_order() {
    // Same attempt reports a stale epoch, an unreachable layer and a serving layer. The positive
    // observation of a leak is the reason, because the others are absences of proof.
    let mut observations = observations();
    observations[0] = still_serving(RevocationLayer::Facts);
    observations[2] = unreachable(RevocationLayer::Memory);
    let mut stale = acknowledged(RevocationLayer::Index);
    stale.data_epoch = PREVIOUS_EPOCH;
    stale.observation_digest = stale.digest();
    observations[3] = stale;
    let report = report(observations);
    assert_eq!(
        report.status,
        RevocationPropagationStatus::RevokedDataServed
    );
    assert_eq!(
        report.reason,
        "revocation_derived_layer_still_serves_data:facts"
    );
}

// --- 先拒绝: 旧 data_epoch 可写 ------------------------------------------------------------

#[test]
fn a_write_under_a_superseded_data_epoch_is_refused() {
    assert_eq!(
        admit_derived_write(PREVIOUS_EPOCH, CURRENT_EPOCH).unwrap_err(),
        "revocation_stale_data_epoch_write"
    );
    // A writer whose epoch is ahead of the authority has diverged from it in the other direction.
    assert_eq!(
        admit_derived_write(CURRENT_EPOCH + 1, CURRENT_EPOCH).unwrap_err(),
        "revocation_write_epoch_ahead"
    );
    assert_eq!(
        admit_derived_write(0, CURRENT_EPOCH).unwrap_err(),
        "revocation_write_epoch_required"
    );
    assert!(admit_derived_write(CURRENT_EPOCH, CURRENT_EPOCH).is_ok());
}

#[test]
fn a_layer_restored_at_an_older_epoch_blocks_propagation_completion() {
    let mut observations = observations();
    let mut restored = acknowledged(RevocationLayer::Cache);
    restored.data_epoch = PREVIOUS_EPOCH;
    restored.observation_digest = restored.digest();
    observations[4] = restored;
    let report = report(observations);
    assert_eq!(report.status, RevocationPropagationStatus::Incomplete);
    assert_eq!(report.reason, "revocation_layer_epoch_stale:cache");
    assert!(report.pending_layers.contains(&RevocationLayer::Cache));
}

// --- 先拒绝: 删除无 receipt -----------------------------------------------------------------

#[test]
fn an_unreachable_layer_is_not_an_acknowledgement() {
    let mut observations = observations();
    observations[5] = unreachable(RevocationLayer::Checkpoint);
    let report = report(observations);
    assert_eq!(report.status, RevocationPropagationStatus::Incomplete);
    assert_eq!(report.reason, "revocation_layer_state_unknown:checkpoint");
}

#[test]
fn a_tombstone_claim_without_a_receipt_is_rejected_at_the_source() {
    assert_eq!(
        RevocationLayerObservation::new(
            RevocationLayer::Index,
            RevocationLayerState::Tombstoned,
            CURRENT_EPOCH,
            TOMBSTONE_CURSOR,
            None,
            1_000,
        )
        .unwrap_err(),
        "revocation_observation_receipt_required"
    );
    // An unreachable layer cannot produce one either; a receipt from a store nobody could read
    // is a fabricated acknowledgement.
    assert_eq!(
        RevocationLayerObservation::new(
            RevocationLayer::Index,
            RevocationLayerState::Unreachable,
            CURRENT_EPOCH,
            TOMBSTONE_CURSOR,
            Some(digest('a')),
            1_000,
        )
        .unwrap_err(),
        "revocation_observation_unreachable_with_receipt"
    );
}

#[test]
fn a_replayed_receipt_is_idempotent_and_a_second_receipt_is_a_conflict() {
    let existing = acknowledged(RevocationLayer::Memory);
    assert_eq!(
        merge_layer_observation(&existing, &existing).unwrap(),
        existing
    );

    // A retried adapter call that has not advanced must not regress the recorded state.
    let regressed = RevocationLayerObservation::new(
        RevocationLayer::Memory,
        RevocationLayerState::StillServing,
        CURRENT_EPOCH,
        TOMBSTONE_CURSOR,
        None,
        2_000,
    )
    .unwrap();
    assert_eq!(
        merge_layer_observation(&existing, &regressed).unwrap_err(),
        "revocation_receipt_regression"
    );

    // Two different receipts for the same tombstone mean one of them is fabricated.
    let conflicting = RevocationLayerObservation::new(
        RevocationLayer::Memory,
        RevocationLayerState::Tombstoned,
        CURRENT_EPOCH,
        TOMBSTONE_CURSOR,
        Some(digest('c')),
        2_000,
    )
    .unwrap();
    assert_eq!(
        merge_layer_observation(&existing, &conflicting).unwrap_err(),
        "revocation_receipt_conflict"
    );
    assert_eq!(
        merge_layer_observation(&existing, &acknowledged(RevocationLayer::Index)).unwrap_err(),
        "revocation_receipt_layer_mismatch"
    );
}

#[test]
fn a_partially_propagated_tombstone_stays_incomplete_with_the_missing_layer_named() {
    let mut observations = observations();
    observations[3] = pending(RevocationLayer::Index);
    observations[4] = unreachable(RevocationLayer::Cache);
    observations[5] = unreachable(RevocationLayer::Checkpoint);
    let report = report(observations);
    assert_eq!(report.status, RevocationPropagationStatus::Incomplete);
    assert_eq!(
        report.reason,
        "revocation_layer_tombstone_not_applied:index"
    );
    assert_eq!(
        report.pending_layers,
        vec![
            RevocationLayer::Index,
            RevocationLayer::Cache,
            RevocationLayer::Checkpoint
        ]
    );
    assert_eq!(
        report.acknowledged_layers,
        vec![
            RevocationLayer::Facts,
            RevocationLayer::Artifact,
            RevocationLayer::Memory
        ]
    );
}

#[test]
fn a_layer_that_acknowledged_past_an_unreached_one_is_an_order_violation() {
    // Facts and artifact have not taken the tombstone, yet memory/index/cache/checkpoint all
    // acknowledged. Those acknowledgements are real but unearned: the layers before them are
    // still reachable, so the report names the gap rather than crediting the later layers.
    let mut observations = observations();
    observations[0] = pending(RevocationLayer::Facts);
    observations[1] = pending(RevocationLayer::Artifact);
    let report = report(observations);
    assert_eq!(report.status, RevocationPropagationStatus::Incomplete);
    assert_eq!(report.reason, "revocation_layer_order_violation:facts");
    // Credit is only the contiguous prefix; an out-of-order acknowledgement never shortens the gap.
    assert!(report.acknowledged_layers.is_empty());
    assert_eq!(
        report.pending_layers,
        vec![RevocationLayer::Facts, RevocationLayer::Artifact]
    );
}

#[test]
fn a_layer_that_has_not_taken_the_tombstone_cannot_have_receipted_it() {
    assert_eq!(
        RevocationLayerObservation::new(
            RevocationLayer::Memory,
            RevocationLayerState::Pending,
            CURRENT_EPOCH,
            TOMBSTONE_CURSOR,
            Some(digest('a')),
            1_000,
        )
        .unwrap_err(),
        "revocation_observation_pending_with_receipt"
    );
}

// --- 成功: 恢复后仍不复活 --------------------------------------------------------------------

#[test]
fn a_rebuilt_store_that_serves_revoked_data_is_refused_after_recovery() {
    let revived = RevocationLayerObservation::new(
        RevocationLayer::Index,
        RevocationLayerState::StillServing,
        CURRENT_EPOCH,
        TOMBSTONE_CURSOR,
        None,
        1_000,
    )
    .unwrap();
    assert_eq!(
        admit_derived_read_after_recovery(&revived, CURRENT_EPOCH).unwrap_err(),
        "revocation_tombstone_revival_denied"
    );

    // A layer restored from a pre-revocation backup is refused even when it reports no leak,
    // because the restore itself did not carry the tombstone.
    let restored = RevocationLayerObservation::new(
        RevocationLayer::Index,
        RevocationLayerState::Tombstoned,
        PREVIOUS_EPOCH,
        TOMBSTONE_CURSOR,
        Some(digest('a')),
        1_000,
    )
    .unwrap();
    assert_eq!(
        admit_derived_read_after_recovery(&restored, CURRENT_EPOCH).unwrap_err(),
        "revocation_recovered_epoch_superseded"
    );
    assert!(admit_derived_read_after_recovery(&revived, PREVIOUS_EPOCH).is_err());
    assert!(admit_derived_read_after_recovery(
        &acknowledged(RevocationLayer::Index),
        CURRENT_EPOCH
    )
    .is_ok());
}

#[test]
fn a_tampered_report_or_request_is_rejected_rather_than_trusted() {
    let request = request(observations());
    let report = RevocationPropagationReport::evaluate(&request).unwrap();

    let mut tampered = report.clone();
    tampered.status = RevocationPropagationStatus::Complete;
    tampered.pending_layers = vec![RevocationLayer::Checkpoint];
    assert!(tampered.validate_against(&request).is_err());

    let mut reclassified = report.clone();
    reclassified.reason = "revocation_propagation_incomplete:facts".to_owned();
    reclassified.report_digest = reclassified.digest();
    assert_eq!(
        reclassified.validate_against(&request).unwrap_err(),
        "revocation_report_binding_invalid"
    );

    let mut retargeted = request.clone();
    retargeted.data_epoch = CURRENT_EPOCH + 1;
    retargeted.request_digest = retargeted.digest();
    assert_eq!(
        report.validate_against(&retargeted).unwrap_err(),
        "revocation_report_binding_invalid"
    );
}
