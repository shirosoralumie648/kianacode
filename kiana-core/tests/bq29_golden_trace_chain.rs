//! BQ-29: the end-to-end chain, and the three ways it lies.
//!
//! Deny-first. A list of stage names is easy to produce and says almost nothing, so each test
//! here builds an otherwise-valid chain and breaks exactly one property that a stage list cannot
//! express.

use kiana_core::{
    verify_golden_trace_chain, ChainStage, ChainStageObservation, GoldenTraceChain,
    GoldenTraceChainReport, ENTRYPOINT_ROUTE,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn stage(kind: ChainStage) -> ChainStageObservation {
    ChainStageObservation::new(
        kind,
        ENTRYPOINT_ROUTE,
        1,
        1,
        1,
        true,
        None,
        Vec::new(),
        match kind {
            ChainStage::Receipt => Some(digest('c')),
            _ => None,
        },
    )
}

/// model(1 provider, 1 handler) → tool(1,1) → event(0,0) → receipt(0,0) → correction(0,0)
fn valid() -> GoldenTraceChain {
    GoldenTraceChain::new(
        "trace-1",
        digest('g'),
        2,
        2,
        vec![
            stage(ChainStage::ModelCall),
            stage(ChainStage::ToolDispatch),
            ChainStageObservation::new(
                ChainStage::EventAppend,
                ENTRYPOINT_ROUTE,
                0,
                0,
                0,
                true,
                None,
                Vec::new(),
                None,
            ),
            ChainStageObservation::new(
                ChainStage::Receipt,
                ENTRYPOINT_ROUTE,
                0,
                0,
                0,
                true,
                None,
                Vec::new(),
                Some(digest('c')),
            ),
            ChainStageObservation::new(
                ChainStage::InvoiceCorrection,
                ENTRYPOINT_ROUTE,
                0,
                0,
                0,
                true,
                None,
                Vec::new(),
                None,
            ),
        ],
    )
}

#[test]
fn an_entrypoint_that_bypassed_the_daemon_is_refused() {
    // The route is what makes a stage's contents trustworthy, so it is checked per stage and first.
    // A stage that arrived another way is not a slightly worse stage; it is an unverified one.
    for kind in ChainStage::ALL {
        let mut chain = valid();
        let index = chain
            .stages
            .iter()
            .position(|stage| stage.stage == kind)
            .expect("stage present");
        chain.stages[index].route = "workbench.direct_provider".to_owned();
        chain.chain_digest = chain.digest();
        assert_eq!(
            verify_golden_trace_chain(&chain).unwrap_err(),
            "golden_chain_stage_route_bypass",
            "stage {kind:?} should be refused"
        );
    }
}

#[test]
fn a_missing_or_reordered_stage_is_refused() {
    let mut short = valid();
    short.stages.pop();
    short.chain_digest = short.digest();
    assert_eq!(
        verify_golden_trace_chain(&short).unwrap_err(),
        "golden_chain_stage_order_invalid"
    );

    let mut swapped = valid();
    swapped.stages.swap(0, 1);
    swapped.chain_digest = swapped.digest();
    assert_eq!(
        verify_golden_trace_chain(&swapped).unwrap_err(),
        "golden_chain_stage_order_invalid"
    );
}

#[test]
fn call_counts_that_disagree_with_the_expectation_are_refused() {
    // A provider called twice against one reservation is a double charge wearing a receipt.
    let mut chain = valid();
    chain.stages[0].provider_calls = 3;
    chain.chain_digest = chain.digest();
    assert_eq!(
        verify_golden_trace_chain(&chain).unwrap_err(),
        "golden_chain_call_count_mismatch"
    );

    let mut handlers = valid();
    handlers.stages[1].handler_calls = 2;
    handlers.chain_digest = handlers.digest();
    assert_eq!(
        verify_golden_trace_chain(&handlers).unwrap_err(),
        "golden_chain_call_count_mismatch"
    );
}

#[test]
fn a_stage_that_ran_with_no_reservation_is_refused() {
    // An effect nobody charged for is as much an accounting failure as a charge nobody made.
    let mut chain = valid();
    chain.stages[0].reservations_held = 0;
    chain.chain_digest = chain.digest();
    assert_eq!(
        verify_golden_trace_chain(&chain).unwrap_err(),
        "golden_chain_reservation_mismatch"
    );
}

#[test]
fn a_receipt_stage_without_a_digest_is_refused() {
    let mut chain = valid();
    let index = chain
        .stages
        .iter()
        .position(|stage| stage.stage == ChainStage::Receipt)
        .expect("receipt stage");
    chain.stages[index].receipt_digest = None;
    chain.chain_digest = chain.digest();
    assert_eq!(
        verify_golden_trace_chain(&chain).unwrap_err(),
        "golden_chain_receipt_missing"
    );
}

#[test]
fn a_runtime_success_may_not_be_written_as_a_business_outcome() {
    // The card's third failure. A 200 is a fact about the transport; whether the business
    // succeeded is a separate claim that needs its own evidence.
    let mut no_evidence = valid();
    no_evidence.stages[0].business_outcome = Some("invoice_paid".to_owned());
    no_evidence.chain_digest = no_evidence.digest();
    assert_eq!(
        verify_golden_trace_chain(&no_evidence).unwrap_err(),
        "golden_chain_business_outcome_evidence_missing"
    );

    // A claim whose runtime did not succeed is refused even with evidence attached.
    let mut failed_runtime = valid();
    failed_runtime.stages[0].runtime_success = false;
    failed_runtime.stages[0].business_outcome = Some("invoice_paid".to_owned());
    failed_runtime.stages[0].business_evidence_refs = vec![digest('e')];
    failed_runtime.chain_digest = failed_runtime.digest();
    assert_eq!(
        verify_golden_trace_chain(&failed_runtime).unwrap_err(),
        "golden_chain_business_outcome_without_runtime"
    );

    // And an empty claim is not a claim.
    let mut empty = valid();
    empty.stages[0].business_outcome = Some("   ".to_owned());
    empty.chain_digest = empty.digest();
    assert_eq!(
        verify_golden_trace_chain(&empty).unwrap_err(),
        "golden_chain_business_outcome_empty"
    );
}

#[test]
fn evidence_travelling_without_a_claim_is_refused_too() {
    // The same confusion in reverse: evidence attached to a stage that claims nothing is a claim
    // that was meant and not written down.
    let mut chain = valid();
    chain.stages[0].business_evidence_refs = vec![digest('e')];
    chain.chain_digest = chain.digest();
    assert_eq!(
        verify_golden_trace_chain(&chain).unwrap_err(),
        "golden_chain_business_evidence_without_outcome"
    );
}

#[test]
fn an_edited_chain_or_report_cannot_publish_itself() {
    let chain = valid();
    let report: GoldenTraceChainReport = verify_golden_trace_chain(&chain).expect("report");
    report.validate_against(&chain).expect("un edited");

    let mut unsealed_chain = chain.clone();
    unsealed_chain.trace_id = "trace-2".to_owned();
    assert_eq!(
        verify_golden_trace_chain(&unsealed_chain).unwrap_err(),
        "golden_chain_digest_mismatch"
    );

    let mut shrunk = report.clone();
    shrunk.provider_calls = 99;
    assert_eq!(
        shrunk.validate_against(&chain).unwrap_err(),
        "golden_chain_report_binding_invalid"
    );

    let mut broken = report;
    broken.report_digest = digest('9');
    assert_eq!(
        broken.validate_against(&chain).unwrap_err(),
        "golden_chain_report_digest_mismatch"
    );
}

#[test]
fn a_complete_chain_verifies_and_says_it_replayed_nothing() {
    let chain = valid();
    let report = verify_golden_trace_chain(&chain).expect("chain");
    report.validate_against(&chain).expect("re-derives");
    assert_eq!(report.stages, ChainStage::ALL.to_vec());
    assert_eq!(report.route, ENTRYPOINT_ROUTE);
    assert!(!report.business_outcome_claimed);
    assert!(!report.limitations.is_empty());

    // A business outcome that is properly evidenced is admitted, and the report says it was claimed.
    let mut claimed = valid();
    claimed.stages[0].business_outcome = Some("invoice_paid".to_owned());
    claimed.stages[0].business_evidence_refs = vec![digest('e')];
    claimed.chain_digest = claimed.digest();
    let report = verify_golden_trace_chain(&claimed).expect("claimed chain");
    assert!(report.business_outcome_claimed);
}
