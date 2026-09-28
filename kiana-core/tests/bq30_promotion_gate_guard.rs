//! BQ-30 source guard: the gate consumes the evidence contract rather than defining a second one,
//! and it keeps offline durable and opt-in live as separate levels.

#[test]
fn bq30_pins_the_three_named_failures() {
    let source = include_str!("../src/promotion_gate.rs");

    for marker in [
        "promotion_above_evidence_ceiling",
        "promotion_live_requires_provider_receipt",
        "promotion_live_requires_opt_in",
        "promotion_measured_requires_provider_receipt",
        "promotion_limitations_unrecorded",
        "promotion_durable_requires_restart_replay",
        "promotion_request_digest_mismatch",
        "promotion_decision_binding_invalid",
        "promotion_decision_digest_mismatch",
        "promotion_request_header_invalid",
        "promotion_claim_id_required",
    ] {
        assert!(source.contains(marker), "BQ-30 module lost {marker}");
    }
}

#[test]
fn bq30_keeps_offline_durable_and_opt_in_live_separate() {
    let source = include_str!("../src/promotion_gate.rs");

    for marker in [
        "OfflineDurable",
        "OptInLive",
        "TypeOnly",
        "UnitTest",
        "Estimated",
        "restart_replay_ref",
        "opt_in_ref",
        "provider_receipt_ref",
        // There is deliberately no single "production" rung.
        "\"production\"",
    ] {
        if marker == "\"production\"" {
            assert!(
                !source.contains(marker),
                "BQ-30 must not collapse durable and live into one production level"
            );
            continue;
        }
        assert!(source.contains(marker), "BQ-30 module lost {marker}");
    }
}

#[test]
fn bq30_consumes_the_evidence_contract_instead_of_defining_one() {
    let source = include_str!("../src/promotion_gate.rs");

    for marker in [
        "use crate::evidence_manifest::{EvidenceManifest, EvidenceProofLevel}",
        "EvidenceProofLevel::Source",
        "EvidenceProofLevel::LocalBehavior",
        "EvidenceProofLevel::Durable",
        "EvidenceProofLevel::Live",
        "request.evidence.validate()",
    ] {
        assert!(source.contains(marker), "BQ-30 module lost {marker}");
    }
    // A second evidence contract or a second proof ladder would be a second answer to the question
    // this gate exists to ask.
    for forbidden in [
        "pub enum Bq30ProofLevel",
        "pub struct Bq30Evidence",
        "const BQ30_PROOF_CEILING",
    ] {
        assert!(
            !source.contains(forbidden),
            "BQ-30 invented a parallel evidence vocabulary: {forbidden}"
        );
    }
}

#[test]
fn bq30_decides_and_does_not_promote() {
    let source = include_str!("../src/promotion_gate.rs");

    for forbidden in [
        "std::fs",
        "File::",
        "Command::",
        "std::process",
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
            "BQ-30 module gained a token a gate must not have: {forbidden}"
        );
    }
}
