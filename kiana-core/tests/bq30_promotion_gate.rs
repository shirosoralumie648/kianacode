//! BQ-30: the gate a claim passes before it may be called something stronger than it is.
//!
//! Deny-first. The three failures the card names each get a test against a request that is
//! otherwise admissible, so a refusal is about the level and not about the fixture.

use kiana_core::{
    evaluate_promotion, ClaimedLevel, CommandInvocation, EnvironmentFact, EvidenceFeatureStatus,
    EvidenceManifest, EvidenceProofLevel, FixtureKind, FixtureRef, PromotionGateDecision,
    PromotionGateRequest,
};

const RECEIPT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A sealed evidence manifest at the requested proof level.
fn manifest(proof_level: EvidenceProofLevel) -> EvidenceManifest {
    EvidenceManifest::new(
        "bq30-evidence",
        "0be643aa",
        "recorder",
        "reviewer",
        CommandInvocation::new(
            vec!["cargo".to_owned(), "test".to_owned()],
            "/repo",
            Some(0),
        ),
        vec![EnvironmentFact::new("CI", "github-actions")],
        vec![FixtureRef::new(
            "kiana-core/tests/fixtures/one.json",
            FixtureKind::Fixture,
            RECEIPT,
        )],
        "",
        "row moved",
        EvidenceFeatureStatus::Partial,
        proof_level,
        vec!["no external system was exercised".to_owned()],
        None,
    )
    .expect("manifest")
}

#[test]
fn a_type_a_test_or_an_estimate_cannot_be_called_billing_live() {
    // The gap between "the code compiles" and "money moved" is the whole gap the ladder describes.
    for proof in [
        EvidenceProofLevel::Source,
        EvidenceProofLevel::LocalBehavior,
        EvidenceProofLevel::Durable,
    ] {
        let request = PromotionGateRequest::new(
            "bq30-claim",
            ClaimedLevel::OptInLive,
            manifest(proof),
            Some("receipt-1".to_owned()),
            Some("approval:opt-in".to_owned()),
            Some("replay-1".to_owned()),
        );
        assert_eq!(
            evaluate_promotion(&request).unwrap_err(),
            "promotion_above_evidence_ceiling",
            "proof {proof:?} must not reach live"
        );
    }
}

#[test]
fn nothing_may_be_called_measured_without_a_provider_receipt() {
    // An estimate computed locally is an estimate. Rendering it as a measurement is how a budget
    // page starts lying, and the receipt is what somebody outside this system left behind.
    let request = PromotionGateRequest::new(
        "bq30-claim",
        ClaimedLevel::OptInLive,
        manifest(EvidenceProofLevel::Live),
        None,
        Some("approval:opt-in".to_owned()),
        Some("replay-1".to_owned()),
    );
    assert_eq!(
        evaluate_promotion(&request).unwrap_err(),
        "promotion_live_requires_provider_receipt"
    );
}

#[test]
fn a_live_claim_without_a_typed_opt_in_is_refused() {
    let request = PromotionGateRequest::new(
        "bq30-claim",
        ClaimedLevel::OptInLive,
        manifest(EvidenceProofLevel::Live),
        Some("receipt-1".to_owned()),
        None,
        Some("replay-1".to_owned()),
    );
    assert_eq!(
        evaluate_promotion(&request).unwrap_err(),
        "promotion_live_requires_opt_in"
    );
}

#[test]
fn unrecorded_limitations_block_the_promotion() {
    // A review that produced no limitations did not happen. SC-35 refuses an empty list at
    // construction, so the way to reach this rule is a manifest that was tampered with afterwards.
    let mut evidence = manifest(EvidenceProofLevel::Durable);
    evidence.limitations.clear();
    evidence.manifest_digest = evidence.digest();
    let request = PromotionGateRequest::new(
        "bq30-claim",
        ClaimedLevel::UnitTest,
        evidence,
        None,
        None,
        Some("replay-1".to_owned()),
    );
    // The evidence contract refuses it first, which is the correct order: the gate does not get to
    // interpret a record the record layer already rejected.
    assert!(evaluate_promotion(&request).is_err());
}

#[test]
fn offline_durable_and_opt_in_live_are_two_separate_doors() {
    // Satisfying one has said nothing about the other. A durable claim with no restart replay is
    // refused even though a live claim with the same evidence would have needed a different ref.
    let request = PromotionGateRequest::new(
        "bq30-claim",
        ClaimedLevel::OfflineDurable,
        manifest(EvidenceProofLevel::Durable),
        None,
        None,
        None,
    );
    assert_eq!(
        evaluate_promotion(&request).unwrap_err(),
        "promotion_durable_requires_restart_replay"
    );
}

#[test]
fn a_tampered_request_or_decision_cannot_publish_itself() {
    let request = PromotionGateRequest::new(
        "bq30-claim",
        ClaimedLevel::OfflineDurable,
        manifest(EvidenceProofLevel::Durable),
        None,
        None,
        Some("replay-1".to_owned()),
    );
    let decision: PromotionGateDecision = evaluate_promotion(&request).expect("decision");
    decision.validate_against(&request).expect("un edited");

    let mut promoted = request.clone();
    promoted.claimed_level = ClaimedLevel::OptInLive;
    assert_eq!(
        evaluate_promotion(&promoted).unwrap_err(),
        "promotion_request_digest_mismatch"
    );

    let mut relabelled = decision.clone();
    relabelled.evidence_reaches = ClaimedLevel::OptInLive;
    assert_eq!(
        relabelled.validate_against(&request).unwrap_err(),
        "promotion_decision_digest_mismatch"
    );

    let mut rebound = decision;
    rebound.claim_id = "other-claim".to_owned();
    assert_eq!(
        rebound.validate_against(&request).unwrap_err(),
        "promotion_decision_binding_invalid"
    );
}

#[test]
fn a_claim_the_evidence_reaches_is_promoted_and_the_decision_says_so_plainly() {
    let request = PromotionGateRequest::new(
        "bq30-claim",
        ClaimedLevel::OfflineDurable,
        manifest(EvidenceProofLevel::Durable),
        None,
        None,
        Some("replay-1".to_owned()),
    );
    let decision = evaluate_promotion(&request).expect("decision");
    decision.validate_against(&request).expect("re-derives");
    assert!(decision.promoted);
    assert_eq!(decision.evidence_reaches, ClaimedLevel::OfflineDurable);
    assert_eq!(decision.claimed_level, ClaimedLevel::OfflineDurable);
    // The limitations travel with the decision, so a reader of the decision sees the caveats and
    // not only the verdict.
    assert_eq!(
        decision.limitations,
        vec!["no external system was exercised".to_owned()]
    );

    let live = PromotionGateRequest::new(
        "bq30-claim",
        ClaimedLevel::OptInLive,
        manifest(EvidenceProofLevel::Live),
        Some("receipt-1".to_owned()),
        Some("approval:opt-in".to_owned()),
        Some("replay-1".to_owned()),
    );
    assert!(evaluate_promotion(&live).expect("live").promoted);
}
