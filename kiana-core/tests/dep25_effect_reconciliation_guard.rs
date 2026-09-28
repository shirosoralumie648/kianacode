//! DEP-25 source guard: the card's three named failures have to be refusals in the decision, and
//! the four resolutions have to stay four.
//!
//! The absence check is load-bearing. This module decides; the moment it can look something up, the
//! decision stops being re-derivable from the request alone, which is the property the whole
//! receipt design rests on.

#[test]
fn dep25_pins_the_four_resolutions_and_reuses_the_domain_observation() {
    let source = include_str!("../src/effect_reconciliation.rs");

    for marker in [
        "EffectResolution",
        "Reconciled",
        "RetryWithoutEffect",
        "Abandoned",
        "Compensated",
        "EffectReconciliationRequest",
        "EffectReconciliationReceipt",
        "ExternalReceiptRef",
        "reconcile_effect",
        "check_idempotent_replay",
        "validate_against",
        "EFFECT_RECONCILIATION_SCHEMA",
        "EFFECT_RECONCILIATION_RECEIPT_SCHEMA",
        // The domain vocabulary is reused rather than forked. A second effect-state enum would be
        // a second fact source, which the constitution forbids.
        "EffectObservation",
        "EffectObservationState",
        "ConfirmedSuccess",
        "ConfirmedFailure",
        "NoEffect",
        "Unknown",
    ] {
        assert!(source.contains(marker), "DEP-25 module lost {marker}");
    }
}

#[test]
fn dep25_pins_the_three_named_failures() {
    let source = include_str!("../src/effect_reconciliation.rs");

    for marker in [
        // An unknown is never retried.
        "effect_reconcile_unknown_auto_retry",
        // Success is never asserted locally.
        "effect_reconcile_success_without_external_ref",
        "effect_reconcile_observation_receipt_missing",
        // An approval is single-use, and an epoch of zero is no claim at all.
        "effect_reconcile_approval_reused",
        "effect_approval_epoch_required",
        // And a key that is reused with different content is a second decision, not a retry.
        "effect_reconcile_idempotency_conflict",
        "effect_reconcile_idempotency_key_mismatch",
    ] {
        assert!(source.contains(marker), "DEP-25 module lost {marker}");
    }
}

#[test]
fn dep25_pins_the_compensation_and_abandon_gates() {
    let source = include_str!("../src/effect_reconciliation.rs");

    for marker in [
        "effect_reconcile_compensation_without_effect",
        "effect_reconcile_compensation_ref_required",
        "effect_reconcile_compensation_evidence_required",
        "effect_reconcile_abandon_requires_no_effect",
        "effect_reconcile_abandon_reason_required",
        "effect_reconcile_abandon_conflicting_evidence",
        "effect_reconcile_receipt_without_effect",
        "effect_reconcile_external_ref_not_allowed",
        "effect_reconcile_compensation_ref_not_allowed",
        "effect_reconcile_abandon_reason_not_allowed",
    ] {
        assert!(source.contains(marker), "DEP-25 module lost {marker}");
    }
}

#[test]
fn dep25_receipt_is_sealed_and_re_derived() {
    let source = include_str!("../src/effect_reconciliation.rs");

    for marker in [
        "effect_reconcile_request_digest_mismatch",
        "effect_reconcile_receipt_binding_invalid",
        "effect_reconcile_receipt_digest_mismatch",
        "effect_reconcile_header_invalid",
        "request_digest_seal",
        "fn digest(&self) -> String",
    ] {
        assert!(source.contains(marker), "DEP-25 module lost {marker}");
    }
}

#[test]
fn dep25_decides_and_does_not_act() {
    let source = include_str!("../src/effect_reconciliation.rs");

    for forbidden in [
        "std::fs",
        "File::",
        "Command::",
        "std::process",
        "TcpStream",
        "reqwest",
        "tokio",
        "spawn",
        "thread::sleep",
        "SystemTime",
        "Instant::now",
        "EventStore",
        "append_event",
        "ControlPlane",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-25 module gained a token it must not have: {forbidden}"
        );
    }
}
