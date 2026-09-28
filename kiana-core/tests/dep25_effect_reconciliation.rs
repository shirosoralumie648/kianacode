//! DEP-25: what may be done about an external effect whose real result is not known.
//!
//! Deny-first, with the four success paths last. The three failures the card names come first:
//! an unknown that is retried automatically, a success asserted without anything external behind
//! it, and an approval or epoch that is reused.

use kiana_core::{
    check_idempotent_replay, reconcile_effect, EffectReconciliationRequest, EffectResolution,
    ExternalReceiptRef,
};
use kiana_domain::{EffectObservation, EffectObservationState, ExecutionId, InvocationId};

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn observation(
    state: EffectObservationState,
    provider_receipt_id: Option<&str>,
    evidence: Vec<String>,
) -> EffectObservation {
    EffectObservation::new(
        ExecutionId::new(),
        InvocationId::new(),
        1,
        digest('a'),
        digest('b'),
        digest('c'),
        provider_receipt_id.map(str::to_owned),
        Some("pending".to_owned()),
        1_700_000_000_000,
        Some(digest('d')),
        evidence,
        state,
    )
    .expect("observation")
}

fn external_receipt() -> ExternalReceiptRef {
    ExternalReceiptRef::new("ext-1", digest('e'), 1_700_000_000_000)
}

#[allow(clippy::too_many_arguments)]
fn request(
    observation: EffectObservation,
    resolution: EffectResolution,
    external: Option<ExternalReceiptRef>,
    compensation: Option<&str>,
    abandon: Option<&str>,
    consumed: Vec<&str>,
) -> Result<EffectReconciliationRequest, String> {
    EffectReconciliationRequest::new(
        observation,
        resolution,
        external,
        compensation.map(str::to_owned),
        abandon.map(str::to_owned),
        "approval-1",
        4,
        "fence-1",
        consumed.into_iter().map(str::to_owned).collect(),
        "idem-1",
    )
}

// ---------------------------------------------------------------------------
// The three failures the card names.
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_outcome_is_never_resolved_by_retrying_it() {
    // The rule the card leads with. `NoEffect` is somebody having proved nothing happened;
    // `Unknown` is nobody having proved anything. Retrying the second is how a timeout becomes a
    // double charge, so it is refused at the point of decision.
    for state in [
        EffectObservationState::Unknown,
        EffectObservationState::ConfirmedSuccess,
        EffectObservationState::ConfirmedFailure,
    ] {
        let error = request(
            observation(state, None, vec![digest('f')]),
            EffectResolution::RetryWithoutEffect,
            None,
            None,
            None,
            vec![],
        )
        .unwrap_err();
        assert_eq!(error, "effect_reconcile_unknown_auto_retry", "state {state:?}");
    }
}

#[test]
fn success_cannot_be_asserted_without_something_external_behind_it() {
    // Two independent ways to fake it, both refused: no external reference at all, and an external
    // reference over an observation the provider never receipted.
    assert_eq!(
        request(
            observation(EffectObservationState::ConfirmedSuccess, Some("receipt-1"), vec![]),
            EffectResolution::Reconciled,
            None,
            None,
            None,
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_success_without_external_ref"
    );
    assert_eq!(
        request(
            observation(EffectObservationState::ConfirmedSuccess, None, vec![]),
            EffectResolution::Reconciled,
            Some(external_receipt()),
            None,
            None,
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_observation_receipt_missing"
    );
    // And an unknown is never "reconciled" just because somebody supplied a reference.
    assert_eq!(
        request(
            observation(EffectObservationState::Unknown, None, vec![]),
            EffectResolution::Reconciled,
            Some(external_receipt()),
            None,
            None,
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_success_without_external_ref"
    );
}

#[test]
fn an_approval_or_a_zero_epoch_cannot_be_reused() {
    assert_eq!(
        request(
            observation(EffectObservationState::NoEffect, None, vec![]),
            EffectResolution::Abandoned,
            None,
            None,
            Some("no longer reachable"),
            vec!["approval-1"],
        )
        .unwrap_err(),
        "effect_approval_reused"
    );
    // An epoch of zero means "no authority", which is not a weaker claim, it is no claim.
    let error = EffectReconciliationRequest::new(
        observation(EffectObservationState::NoEffect, None, vec![]),
        EffectResolution::Abandoned,
        None,
        None,
        Some("no longer reachable".to_owned()),
        "approval-2",
        0,
        "fence-1",
        Vec::new(),
        "idem-2",
    )
    .unwrap_err();
    assert_eq!(error, "effect_approval_epoch_required");
}

#[test]
fn a_replay_under_the_same_key_with_different_content_is_refused() {
    let first = request(
        observation(EffectObservationState::NoEffect, None, vec![]),
        EffectResolution::Abandoned,
        None,
        None,
        Some("no longer reachable"),
        vec![],
    )
    .expect("first");
    // Same key, different payload: not a retry, a second decision wearing a retry's name.
    let second = EffectReconciliationRequest::new(
        observation(EffectObservationState::NoEffect, None, vec![]),
        EffectResolution::Abandoned,
        None,
        None,
        Some("a different reason entirely".to_owned()),
        "approval-1",
        4,
        "fence-1",
        Vec::new(),
        "idem-1",
    )
    .expect("second");
    assert_eq!(
        check_idempotent_replay(&first, &second).unwrap_err(),
        "effect_reconcile_idempotency_conflict"
    );
    // An exact replay is accepted, which is what makes the key worth having.
    assert!(check_idempotent_replay(&first, &first).is_ok());
    // A different key is a different decision and must say so.
    let other = EffectReconciliationRequest::new(
        observation(EffectObservationState::NoEffect, None, vec![]),
        EffectResolution::Abandoned,
        None,
        None,
        Some("no longer reachable".to_owned()),
        "approval-9",
        4,
        "fence-1",
        Vec::new(),
        "idem-9",
    )
    .expect("other key");
    assert_eq!(
        check_idempotent_replay(&first, &other).unwrap_err(),
        "effect_reconcile_idempotency_key_mismatch"
    );
}

// ---------------------------------------------------------------------------
// Each of the four resolutions refuses the states it must refuse.
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_can_neither_be_abandoned_nor_compensated_without_evidence() {
    // Abandoning an unknown is how an unknown quietly becomes a success.
    assert_eq!(
        request(
            observation(EffectObservationState::Unknown, None, vec![]),
            EffectResolution::Abandoned,
            None,
            None,
            Some("giving up"),
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_abandon_requires_no_effect"
    );
    // A compensation with nothing behind it is a phantom action in the audit trail.
    assert_eq!(
        request(
            observation(EffectObservationState::NoEffect, None, vec![]),
            EffectResolution::Compensated,
            None,
            Some("comp-1"),
            None,
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_compensation_without_effect"
    );
    // And one without a reference or without evidence is not a compensation either.
    assert_eq!(
        request(
            observation(EffectObservationState::Unknown, None, vec![digest('f')]),
            EffectResolution::Compensated,
            None,
            None,
            None,
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_compensation_ref_required"
    );
    assert_eq!(
        request(
            observation(EffectObservationState::Unknown, None, vec![]),
            EffectResolution::Compensated,
            None,
            Some("comp-1"),
            None,
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_compensation_evidence_required"
    );
}

#[test]
fn abandoning_requires_a_reason_and_refuses_conflicting_evidence() {
    assert_eq!(
        request(
            observation(EffectObservationState::NoEffect, None, vec![]),
            EffectResolution::Abandoned,
            None,
            None,
            None,
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_abandon_reason_required"
    );
    assert_eq!(
        request(
            observation(EffectObservationState::NoEffect, None, vec![]),
            EffectResolution::Abandoned,
            Some(external_receipt()),
            None,
            Some("giving up"),
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_abandon_conflicting_evidence"
    );
}

#[test]
fn a_field_belonging_to_another_resolution_cannot_travel_with_this_one() {
    // A receipt that says two things at once is worse than one that says the wrong thing,
    // because the wrong thing is at least checkable.
    assert_eq!(
        request(
            observation(EffectObservationState::NoEffect, None, vec![]),
            EffectResolution::RetryWithoutEffect,
            Some(external_receipt()),
            None,
            None,
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_receipt_without_effect"
    );
    assert_eq!(
        request(
            observation(EffectObservationState::Unknown, None, vec![digest('f')]),
            EffectResolution::Compensated,
            None,
            Some("comp-1"),
            Some("and also abandoned"),
            vec![],
        )
        .unwrap_err(),
        "effect_reconcile_abandon_reason_not_allowed"
    );
}

// ---------------------------------------------------------------------------
// A receipt cannot be edited after the fact.
// ---------------------------------------------------------------------------

#[test]
fn an_edited_receipt_cannot_publish_itself() {
    let request = request(
        observation(EffectObservationState::Unknown, None, vec![digest('f')]),
        EffectResolution::Compensated,
        None,
        Some("comp-1"),
        None,
        vec![],
    )
    .expect("request");
    let receipt = reconcile_effect(&request).expect("compensated");
    receipt.validate_against(&request).expect("un edited");

    let mut promoted = receipt.clone();
    promoted.resolution = EffectResolution::Reconciled;
    assert_eq!(
        promoted.validate_against(&request).unwrap_err(),
        "effect_reconcile_receipt_binding_invalid"
    );

    let mut unsealed = receipt;
    unsealed.receipt_digest = digest('9');
    assert_eq!(
        unsealed.validate_against(&request).unwrap_err(),
        "effect_reconcile_receipt_digest_mismatch"
    );
}

// ---------------------------------------------------------------------------
// The four success paths, last.
// ---------------------------------------------------------------------------

#[test]
fn each_of_the_four_resolutions_produces_its_own_receipt() {
    let cases = [
        (
            EffectResolution::Reconciled,
            observation(EffectObservationState::ConfirmedSuccess, Some("receipt-1"), vec![]),
            Some(external_receipt()),
            None,
            None,
        ),
        (
            EffectResolution::RetryWithoutEffect,
            observation(EffectObservationState::NoEffect, None, vec![]),
            None,
            None,
            None,
        ),
        (
            EffectResolution::Abandoned,
            observation(EffectObservationState::NoEffect, None, vec![]),
            None,
            None,
            Some("endpoint no longer exists"),
        ),
        (
            EffectResolution::Compensated,
            observation(EffectObservationState::Unknown, None, vec![digest('f')]),
            None,
            Some("comp-1"),
            None,
        ),
    ];
    for (resolution, observation, external, compensation, abandon) in cases {
        let request = request(
            observation,
            resolution,
            external,
            compensation,
            abandon,
            vec![],
        )
        .unwrap_or_else(|error| panic!("{resolution:?} should be admissible: {error}"));
        let receipt = reconcile_effect(&request).expect("receipt");
        receipt.validate_against(&request).expect("re-derives");
        assert_eq!(receipt.resolution, resolution);
        assert!(receipt.admitted(), "{resolution:?} carries no reason");
        // Each resolution says what it was reasoning from, not only what it concluded.
        assert_eq!(receipt.observed_state, request.observation.state);
        assert_eq!(receipt.receipt_digest, receipt.digest());
    }
}
