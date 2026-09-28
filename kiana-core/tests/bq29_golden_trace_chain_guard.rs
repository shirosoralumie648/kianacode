//! BQ-29 source guard: the three named failures are refusals in the verification, and the module
//! replays nothing.

#[test]
fn bq29_pins_the_three_named_failures() {
    let source = include_str!("../src/golden_trace_chain.rs");

    for marker in [
        "golden_chain_stage_route_bypass",
        "golden_chain_call_count_mismatch",
        "golden_chain_reservation_mismatch",
        "golden_chain_business_outcome_without_runtime",
        "golden_chain_business_outcome_evidence_missing",
        "golden_chain_business_outcome_empty",
        "golden_chain_business_evidence_without_outcome",
        "golden_chain_stage_order_invalid",
        "golden_chain_receipt_missing",
        "golden_chain_digest_mismatch",
        "golden_chain_report_binding_invalid",
        "golden_chain_report_digest_mismatch",
        // The binding to a real GoldenTrace. Without these, the chain carries a digest that
        // nothing ever compares -- a string, not evidence.
        "bind_golden_trace",
        "GoldenTrace",
        "golden_chain_trace_digest_mismatch",
        "golden_chain_trace_empty",
        "golden_chain_trace_cursor_invalid",
        "golden_chain_trace_source_missing",
        "golden_chain_trace_expired",
        "golden_chain_trace_not_accepted",
        "golden_chain_trace_receipt_missing",
        "golden_chain_binding_time_required",
    ] {
        assert!(source.contains(marker), "BQ-29 module lost {marker}");
    }
}

#[test]
fn bq29_reuses_the_existing_route_and_trace_vocabulary() {
    let source = include_str!("../src/golden_trace_chain.rs");

    for marker in [
        "ENTRYPOINT_ROUTE",
        "use crate::entrypoint_parity::ENTRYPOINT_ROUTE",
        "golden_trace_digest",
        "ChainStage::ALL",
        "ModelCall",
        "ToolDispatch",
        "EventAppend",
        "Receipt",
        "InvoiceCorrection",
    ] {
        assert!(source.contains(marker), "BQ-29 module lost {marker}");
    }
    // A second route string would be a second answer to "how does an entry reach the control plane".
    for forbidden in [
        "const GOLDEN_CHAIN_ROUTE",
        "pub enum GoldenStage",
    ] {
        assert!(
            !source.contains(forbidden),
            "BQ-29 invented a parallel vocabulary: {forbidden}"
        );
    }
}

#[test]
fn bq29_verifies_and_does_not_replay() {
    let source = include_str!("../src/golden_trace_chain.rs");

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
        "ProviderClient",
    ] {
        assert!(
            !source.contains(forbidden),
            "BQ-29 module gained a token a verifier must not have: {forbidden}"
        );
    }
}
