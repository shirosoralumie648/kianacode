use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn route(provider: &str, model: &str, connection: &str) -> ModelRoute {
    ModelRoute {
        provider_id: provider.to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: connection.to_owned(),
        model_id: model.to_owned(),
        profile: connection.to_owned(),
        configuration_revision: "config.v1".to_owned(),
        streaming: true,
    }
}

fn capabilities() -> ModelCapabilities {
    ModelCapabilities {
        tools: CapabilitySupport::Supported,
        streaming: CapabilitySupport::Supported,
        structured_output: CapabilitySupport::Supported,
        images: CapabilitySupport::Supported,
        reasoning_replay: CapabilitySupport::Supported,
        context_window: 128_000,
        max_output: 4_096,
        source: "fixture".to_owned(),
        revision: "catalog.v1".to_owned(),
    }
}

fn card() -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider-b",
        "model-b",
        "USD",
        Some(2),
        Some(3),
        1_000,
        Some(10_000),
        2,
        "bq18-fixture",
    )
    .expect("card");
    card.request_price = Some(5);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid card");
    card
}

fn requirements() -> FallbackCapabilityRequirements {
    FallbackCapabilityRequirements {
        require_tools: true,
        require_streaming: true,
        require_structured_output: true,
        require_images: true,
        require_reasoning_replay: false,
        min_context_window: 64_000,
        min_output_tokens: 1_024,
    }
}

fn request(run_id: RunId, original: &ModelRoute) -> FallbackAdmissionRequest {
    FallbackAdmissionRequest::new(
        run_id,
        AttemptId::new(),
        original,
        digest('a'),
        7,
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        requirements(),
        2_000,
    )
    .expect("request")
}

fn candidate() -> FallbackAdmissionCandidate {
    FallbackAdmissionCandidate::new(
        route("provider-b", "model-b", "backup"),
        capabilities(),
        digest('a'),
        7,
        digest('b'),
        Some(digest('g')),
        digest('c'),
        Some(digest('f')),
        Some(card()),
        digest('h'),
    )
}

fn admission() -> FallbackAttemptAdmission {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = FallbackRouteAllowlist::from_routes(std::slice::from_ref(&candidate.route))
        .expect("allowlist");
    let request = request(RunId::new(), &original);
    admit_fallback_attempt(&request, &candidate, &allowlist, AttemptId::new()).expect("admit")
}

fn usage(admission: &FallbackAttemptAdmission, confidence: UsageConfidence) -> NormalizedUsage {
    let unknown = confidence != UsageConfidence::Known;
    NormalizedUsage::new(
        UsageId::new(),
        admission.attempt_id,
        admission.run_id,
        Some(admission.route.provider_id.clone()),
        admission.route.model_id.clone(),
        admission.route.connection_id.clone(),
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(10),
            output_tokens: Some(4),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        unknown.then_some(BillingUnknownReason::ResultUnknown),
        "provider fallback fixture",
        digest('u'),
    )
    .expect("usage")
}

#[test]
fn fallback_rechecks_route_authority_data_budget_credential_price_and_permit() {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = FallbackRouteAllowlist::from_routes(std::slice::from_ref(&candidate.route))
        .expect("allowlist");
    let request = request(RunId::new(), &original);
    let admitted = admit_fallback_attempt(&request, &candidate, &allowlist, AttemptId::new())
        .expect("fresh admission");
    assert_ne!(admitted.route_digest, request.original_route_digest);
    assert_ne!(admitted.permit_digest, request.original_permit_digest);
    assert_ne!(
        admitted.budget_reservation_digest,
        request.original_budget_reservation_digest
    );

    let mut capability = candidate.clone();
    capability.capabilities.tools = CapabilitySupport::Unsupported;
    assert_eq!(
        admit_fallback_attempt(&request, &capability, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut authority = candidate.clone();
    authority.authority_digest = digest('x');
    assert_eq!(
        admit_fallback_attempt(&request, &authority, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_authority_recheck_failed"
    );

    let mut data = candidate.clone();
    data.data_boundary_digest = digest('x');
    assert_eq!(
        admit_fallback_attempt(&request, &data, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_data_boundary_mismatch"
    );

    let mut no_budget = candidate.clone();
    no_budget.budget_reservation_digest = None;
    assert_eq!(
        admit_fallback_attempt(&request, &no_budget, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_budget_reservation_missing"
    );

    let mut reused_permit = candidate.clone();
    reused_permit.permit_digest = request.original_permit_digest.clone();
    assert_eq!(
        admit_fallback_attempt(&request, &reused_permit, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_cannot_reuse_original_permit"
    );

    let mut no_price = candidate;
    no_price.rate_card = None;
    assert_eq!(
        admit_fallback_attempt(&request, &no_price, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_rate_card_missing"
    );
}

#[test]
fn allowlist_pins_exact_route_and_model_identity() {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = FallbackRouteAllowlist::from_routes(std::slice::from_ref(&candidate.route))
        .expect("allowlist");
    let request = request(RunId::new(), &original);
    let mut drifted = candidate;
    drifted.route.model_id = "model-c".to_owned();
    assert_eq!(
        admit_fallback_attempt(&request, &drifted, &allowlist, AttemptId::new()).unwrap_err(),
        "fallback_route_not_allowlisted"
    );
}

#[test]
fn every_fallback_attempt_has_independent_usage_rate_card_and_receipt() {
    let first = admission();
    let first_usage = usage(&first, UsageConfidence::Known);
    let first_cost =
        CostBreakdown::from_usage(&first_usage, Some(&card()), 1, 2_000).expect("estimate");
    let first_receipt =
        FallbackAttemptReceipt::from_admission(&first, first_usage, first_cost, None)
            .expect("estimate receipt");
    assert!(first_receipt.is_success());

    let mut second = first.clone();
    second.attempt_id = AttemptId::new();
    second.permit_digest = digest('i');
    second.budget_reservation_digest = digest('j');
    second.admission_digest = second.digest();
    second.validate().expect("fresh second admission");
    let second_usage = usage(&second, UsageConfidence::Known);
    let measured_receipt = ProviderReceiptRef::new("provider-receipt-bq18").expect("receipt");
    let second_cost = CostBreakdown::from_provider_receipt(
        &second_usage,
        Some(&card()),
        Money::new("USD", 31).expect("money"),
        measured_receipt.clone(),
        2_000,
    )
    .expect("measured cost");
    let second_receipt = FallbackAttemptReceipt::from_admission(
        &second,
        second_usage,
        second_cost,
        Some(measured_receipt),
    )
    .expect("measured receipt");
    assert_ne!(first_receipt.attempt_id, second_receipt.attempt_id);
    assert_ne!(first_receipt.receipt_digest, second_receipt.receipt_digest);
    assert_eq!(second_receipt.rate_card_version, 2);

    let unknown_admission = admission();
    let unknown_usage = usage(&unknown_admission, UsageConfidence::Unknown);
    let unknown_cost =
        CostBreakdown::from_usage(&unknown_usage, Some(&card()), 1, 2_000).expect("unknown cost");
    let unknown = FallbackAttemptReceipt::from_admission(
        &unknown_admission,
        unknown_usage,
        unknown_cost,
        None,
    )
    .expect("unknown receipt");
    assert!(!unknown.is_success());
}

#[test]
fn fallback_receipt_ledger_rejects_cross_attempt_reuse_and_conflict() {
    let first = admission();
    let usage = usage(&first, UsageConfidence::Known);
    let cost = CostBreakdown::from_usage(&usage, Some(&card()), 1, 2_000).expect("cost");
    let receipt =
        FallbackAttemptReceipt::from_admission(&first, usage, cost, None).expect("receipt");
    let mut ledger = FallbackReceiptLedger::default();
    assert_eq!(
        ledger.apply(receipt.clone()).expect("apply"),
        FallbackReceiptApplyOutcome::Applied
    );
    assert_eq!(
        ledger.apply(receipt.clone()).expect("replay"),
        FallbackReceiptApplyOutcome::Duplicate
    );
    let mut conflict = receipt;
    conflict.route_digest = digest('z');
    conflict.receipt_digest = conflict.digest();
    assert_eq!(
        ledger.apply(conflict).unwrap_err(),
        "fallback_attempt_receipt_conflict"
    );
}
