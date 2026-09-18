use kiana_domain::{
    estimate_admission, AdmissionEstimateInput, RateCard, RateCardId, TokenEstimateBasis,
};

fn input(basis: TokenEstimateBasis, hard_cost: bool) -> AdmissionEstimateInput {
    AdmissionEstimateInput {
        schema: "kiana.admission-estimate.v1".to_owned(),
        version: kiana_domain::ADMISSION_SCHEMA_VERSION,
        wire_request_digest:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        wire_request_bytes: 400,
        token_basis: basis,
        exact_input_tokens: (basis == TokenEstimateBasis::ExactTokenizer).then_some(100),
        max_output_tokens: 50,
        retry_allowance: 2,
        tool_calls_upper: 3,
        effects_upper: 1,
        storage_bytes_upper: 1024,
        cost_hard_limit_enforced: hard_cost,
    }
}

fn card() -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider",
        "model",
        "USD",
        Some(1),
        Some(2),
        1,
        None,
        3,
        "fixture",
    )
    .unwrap();
    card.request_price = Some(5);
    card.tool_price = Some(1);
    card.effect_price = Some(1);
    card.rate_card_digest = card.digest();
    card
}

#[test]
fn admission_estimate_is_an_upper_bound_with_retry_and_pinned_price() {
    let estimate = estimate_admission(
        &input(TokenEstimateBasis::BytesUpperBound, false),
        Some(&card()),
    )
    .unwrap();
    assert_eq!(estimate.requests_upper, 3);
    assert_eq!(estimate.input_tokens_upper, Some(100));
    assert_eq!(estimate.total_tokens_upper, Some(150));
    assert!(!estimate.token_estimate_exact);
    assert!(estimate.estimated_cost.is_some());
    assert_eq!(estimate.rate_card_version, Some(3));
    estimate.validate().unwrap();
}

#[test]
fn unknown_tokenizer_or_price_cannot_claim_exact_hard_cost() {
    assert!(estimate_admission(&input(TokenEstimateBasis::ExactTokenizer, true), None).is_err());
    let estimate = estimate_admission(&input(TokenEstimateBasis::Unknown, false), None).unwrap();
    assert_eq!(estimate.input_tokens_upper, None);
    assert!(!estimate.token_estimate_exact);
    assert_eq!(
        estimate.unknown_cost_reason.as_deref(),
        Some("rate_card_missing")
    );
}

#[test]
fn malformed_input_and_retry_overflow_fail_closed() {
    let mut bad = input(TokenEstimateBasis::ExactTokenizer, false);
    bad.exact_input_tokens = None;
    assert_eq!(
        estimate_admission(&bad, None).unwrap_err(),
        "admission_estimate_input_invalid"
    );
    bad = input(TokenEstimateBasis::BytesUpperBound, false);
    bad.retry_allowance = 33;
    assert_eq!(
        estimate_admission(&bad, None).unwrap_err(),
        "admission_estimate_input_invalid"
    );
}
