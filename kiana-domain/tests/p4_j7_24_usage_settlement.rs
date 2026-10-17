use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}
fn usage(provider: Option<&str>) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        provider.map(str::to_owned),
        "model-a",
        "connection-a",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(3),
            output_tokens: Some(2),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        UsageConfidence::Known,
        None,
        "provider fixture",
        digest('a'),
    )
    .expect("fixture usage")
}

fn usage_with(
    attempt_id: AttemptId,
    run_id: RunId,
    provider: &str,
    vector: UsageVector,
    confidence: UsageConfidence,
    unknown_reason: Option<BillingUnknownReason>,
) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        attempt_id,
        run_id,
        Some(provider.to_owned()),
        "model-a",
        "connection-a",
        vector,
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        unknown_reason,
        "provider fixture",
        digest('b'),
    )
    .expect("fixture usage")
}

fn tokens(input: Option<u64>, output: Option<u64>) -> UsageVector {
    UsageVector {
        schema: USAGE_VECTOR_SCHEMA.to_owned(),
        input_tokens: input,
        output_tokens: output,
        ..UsageVector::zero()
    }
}

fn card_with(provider: &str, model: &str, input: Option<u64>, output: Option<u64>) -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        provider,
        model,
        "USD",
        input,
        output,
        1,
        None,
        1,
        "fixture-rate-card",
    )
    .expect("rate card");
    card.request_price = Some(0);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid rate card");
    card
}

fn attempt_reservation(
    attempt_id: AttemptId,
    run_id: RunId,
    protocol: ModelProtocol,
    retry_ordinal: u32,
    reserved_tokens: u64,
    rate_card: Option<&RateCard>,
) -> ProviderAttemptReservation {
    ProviderAttemptReservation::new(
        attempt_id,
        run_id,
        None,
        protocol,
        retry_ordinal,
        reserved_tokens,
        rate_card.map(RateCardSnapshot::of),
        1_000,
    )
    .expect("fixture reservation")
}

fn settlement_for(
    attempt_id: AttemptId,
    run_id: RunId,
    protocol: ModelProtocol,
    vector: UsageVector,
    confidence: UsageConfidence,
    unknown_reason: Option<BillingUnknownReason>,
) -> ProviderUsageSettlement {
    ProviderUsageSettlement::from_usage_with_protocol(
        &usage_with(
            attempt_id,
            run_id,
            "provider-a",
            vector,
            confidence,
            unknown_reason,
        ),
        protocol,
        None,
        1,
        100,
    )
    .expect("fixture settlement")
}

#[test]
fn missing_provider_or_rate_card_never_becomes_zero_cost() {
    assert!(ProviderUsageSettlement::from_usage(&usage(None), None, 1, 100).is_err());
    let settlement = ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
        .expect("unknown settlement");
    assert!(matches!(settlement.cost, SettlementCost::Unknown { .. }));
    assert_eq!(settlement.state, BillingState::Unknown);
}

#[test]
fn settlement_binds_only_an_explicit_non_nil_execution_identity() {
    let usage = usage(Some("provider-a"));
    let execution_id = ExecutionId::new();
    let settlement =
        ProviderUsageSettlement::from_usage_with_execution_id(&usage, execution_id, None, 1, 100)
            .expect("server-bound execution settlement");
    assert_eq!(settlement.execution_id, Some(execution_id));
    settlement.validate().expect("valid execution binding");
    let unbound = ProviderUsageSettlement::from_usage(&usage, None, 1, 100)
        .expect("compatible run/attempt settlement");
    assert_ne!(settlement.settlement_digest, unbound.settlement_digest);

    assert!(ProviderUsageSettlement::from_usage_with_execution_id(
        &usage,
        ExecutionId::from_uuid(uuid::Uuid::nil()),
        None,
        1,
        100,
    )
    .is_err());
}

#[test]
fn measured_receipt_is_explicit_and_attempt_replay_is_idempotent() {
    let mut settlement =
        ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
            .expect("unknown settlement");
    settlement
        .attach_measured(
            Money::new("USD", 17).expect("money"),
            ProviderReceiptRef::new("invoice-1").expect("receipt"),
        )
        .expect("measured settlement");
    let mut ledger = ProviderUsageLedger::default();
    assert_eq!(
        ledger.apply(settlement.clone()).expect("first apply"),
        SettlementApplyOutcome::Applied
    );
    assert_eq!(
        ledger.apply(settlement).expect("duplicate apply"),
        SettlementApplyOutcome::Duplicate
    );
}

#[test]
fn unpriced_nonzero_usage_dimension_keeps_known_observation_unknown() {
    let mut usage = usage(Some("provider-a"));
    usage.vector.tool_calls = 1;
    usage.usage_digest = usage.digest();
    usage.validate().expect("usage with tool call");

    let mut card = RateCard::new(
        RateCardId::new(),
        "provider-a",
        "model-a",
        "USD",
        Some(2),
        Some(3),
        1,
        None,
        1,
        "fixture-rate-card",
    )
    .expect("rate card");
    card.request_price = Some(5);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid rate card");

    let settlement =
        ProviderUsageSettlement::from_usage(&usage, Some(&card), 1, 100).expect("settlement");
    assert!(matches!(settlement.cost, SettlementCost::Unknown { .. }));
    assert_eq!(settlement.state, BillingState::Unknown);
}

#[test]
fn unknown_cost_transitions_fail_without_mutating_the_settlement() {
    let mut settlement =
        ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
            .expect("unknown settlement");
    let before = settlement.clone();

    assert_eq!(
        settlement.transition(BillingState::Corrected).unwrap_err(),
        "provider_usage_unknown_cannot_correct"
    );
    assert_eq!(settlement, before);

    assert_eq!(
        settlement.transition(BillingState::Settled).unwrap_err(),
        "provider_usage_unknown_cannot_settle"
    );
    assert_eq!(settlement, before);
}

#[test]
fn known_cost_cannot_be_downgraded_to_unknown() {
    let mut settlement =
        ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
            .expect("unknown settlement");
    settlement
        .attach_measured(
            Money::new("USD", 17).expect("money"),
            ProviderReceiptRef::new("invoice-2").expect("receipt"),
        )
        .expect("measured settlement");
    let before = settlement.clone();

    assert_eq!(
        settlement.transition(BillingState::Unknown).unwrap_err(),
        "billing_state_transition_invalid"
    );
    assert_eq!(settlement, before);
}

#[test]
fn invalid_measured_provider_receipt_is_rejected_after_deserialization() {
    let settlement = ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
        .expect("unknown settlement");
    let mut value = serde_json::to_value(&settlement).expect("serialize settlement");
    value["cost"] = serde_json::json!({
        "Measured": {
            "amount": {
                "schema": "kiana.money.v1",
                "currency": "USD",
                "micros": 17
            },
            "provider_receipt": "provider-receipt:secret"
        }
    });
    let mut injected: ProviderUsageSettlement =
        serde_json::from_value(value).expect("deserialize injected settlement");
    injected.settlement_digest = injected.digest();
    assert_eq!(injected.validate().unwrap_err(), "provider_receipt_invalid");
}

#[test]
fn missing_usage_and_price_remain_unknown() {
    let run_id = RunId::new();
    let first = AttemptId::new();
    let mut ledger = ProviderUsageAttemptLedger::new(100).expect("ledger");
    ledger
        .reserve(attempt_reservation(
            first,
            run_id,
            ModelProtocol::OpenAiChat,
            1,
            30,
            None,
        ))
        .expect("reserve");
    ledger.dispatch(first).expect("dispatch");

    // The provider never reported usage: neither a priced zero nor a free call may be produced.
    let settlement = settlement_for(
        first,
        run_id,
        ModelProtocol::OpenAiChat,
        tokens(None, None),
        UsageConfidence::Unknown,
        Some(BillingUnknownReason::ProviderUnreported),
    );
    assert!(matches!(
        settlement.cost,
        SettlementCost::Unknown {
            reason: BillingUnknownReason::ProviderUnreported
        }
    ));
    assert_eq!(settlement.state, BillingState::Unknown);
    assert_eq!(
        ledger.settle(settlement).expect("settle"),
        SettlementApplyOutcome::Applied
    );
    let entry = ledger.entry(first).expect("entry");
    assert_eq!(entry.state, BillingState::Unknown);
    assert_eq!(entry.charged_tokens, None);
    assert_eq!(entry.charge_basis, ProviderChargeBasis::ReservedUpperBound);
    assert_eq!(
        entry.unknown_cost_reason,
        Some(BillingUnknownReason::ProviderUnreported)
    );
    // Unknown consumption keeps the reserved upper bound charged instead of refunding it.
    assert_eq!(entry.conservative_charge(), 30);
    assert_eq!(ledger.charged_tokens().expect("charged"), 30);
    assert_eq!(ledger.open_reserved_tokens().expect("open"), 0);

    // Known usage without a rate card keeps the token charge and only the cost unknown.
    let unpriced = settlement_for(
        AttemptId::new(),
        run_id,
        ModelProtocol::OpenAiChat,
        tokens(Some(7), Some(3)),
        UsageConfidence::Known,
        None,
    );
    assert!(matches!(
        unpriced.cost,
        SettlementCost::Unknown {
            reason: BillingUnknownReason::RateCardMissing
        }
    ));
    assert_eq!(unpriced.state, BillingState::Unknown);
    assert_eq!(
        ProviderUsageContainment::declared(ModelProtocol::OpenAiChat)
            .chargeable(&unpriced.usage.vector)
            .expect("chargeable")
            .total_units,
        10
    );
}

#[test]
fn usage_overflow_is_rejected() {
    let run_id = RunId::new();
    // Two holds that cannot both fit in u64 are not admitted as a wrapped sum.
    let mut ledger = ProviderUsageAttemptLedger::new(u64::MAX).expect("ledger");
    let half = u64::MAX / 2 + 1;
    let first = AttemptId::new();
    ledger
        .reserve(attempt_reservation(
            first,
            run_id,
            ModelProtocol::OpenAiChat,
            1,
            half,
            None,
        ))
        .expect("first hold");
    let second = AttemptId::new();
    assert_eq!(
        ledger
            .reserve(attempt_reservation(
                second,
                run_id,
                ModelProtocol::OpenAiChat,
                2,
                half,
                None,
            ))
            .unwrap_err(),
        "provider_usage_overflow"
    );
    assert_eq!(ledger.len(), 1);
    assert!(ledger.entry(second).is_none());

    // Net token units are summed with checked arithmetic; wrapping would silently undercharge.
    let overflow_attempt = AttemptId::new();
    let mut ledger = ProviderUsageAttemptLedger::new(u64::MAX).expect("ledger");
    ledger
        .reserve(attempt_reservation(
            overflow_attempt,
            run_id,
            ModelProtocol::OpenAiChat,
            1,
            u64::MAX,
            None,
        ))
        .expect("hold");
    ledger.dispatch(overflow_attempt).expect("dispatch");
    let before = ledger
        .entry(overflow_attempt)
        .cloned()
        .expect("entry before settle");
    let settlement = settlement_for(
        overflow_attempt,
        run_id,
        ModelProtocol::OpenAiChat,
        tokens(Some(u64::MAX), Some(1)),
        UsageConfidence::Known,
        None,
    );
    assert_eq!(
        ledger.settle(settlement).unwrap_err(),
        "provider_usage_overflow"
    );
    assert_eq!(ledger.entry(overflow_attempt), Some(&before));
}

#[test]
fn concurrent_model_calls_cannot_overspend_reserved_budget() {
    let run_id = RunId::new();
    let mut ledger = ProviderUsageAttemptLedger::new(100).expect("ledger");
    let first = AttemptId::new();
    let second = AttemptId::new();
    ledger
        .reserve(attempt_reservation(
            first,
            run_id,
            ModelProtocol::AnthropicMessages,
            1,
            60,
            None,
        ))
        .expect("first hold");
    // A second call admitted against the same stale balance is rejected before dispatch.
    assert_eq!(
        ledger
            .reserve(attempt_reservation(
                second,
                run_id,
                ModelProtocol::AnthropicMessages,
                1,
                60,
                None,
            ))
            .unwrap_err(),
        "provider_usage_budget_exceeded"
    );
    assert!(ledger.entry(second).is_none());

    // Unknown consumption of the first call stays charged at the reserved upper bound.
    ledger.dispatch(first).expect("dispatch");
    assert_eq!(
        ledger
            .settle(settlement_for(
                first,
                run_id,
                ModelProtocol::AnthropicMessages,
                tokens(None, None),
                UsageConfidence::Unknown,
                Some(BillingUnknownReason::ResultUnknown),
            ))
            .expect("unknown settle"),
        SettlementApplyOutcome::Applied
    );
    assert_eq!(ledger.charged_tokens().expect("charged"), 60);
    assert_eq!(
        ledger
            .reserve(attempt_reservation(
                second,
                run_id,
                ModelProtocol::AnthropicMessages,
                1,
                50,
                None,
            ))
            .unwrap_err(),
        "provider_usage_budget_exceeded"
    );
    ledger
        .reserve(attempt_reservation(
            second,
            run_id,
            ModelProtocol::AnthropicMessages,
            2,
            40,
            None,
        ))
        .expect("retry fits the remaining reservation");
    assert_eq!(ledger.open_reserved_tokens().expect("open"), 40);

    // Known usage replaces the hold with the measured charge and frees the difference.
    ledger.dispatch(second).expect("dispatch");
    assert_eq!(
        ledger
            .settle(settlement_for(
                second,
                run_id,
                ModelProtocol::AnthropicMessages,
                tokens(Some(6), Some(4)),
                UsageConfidence::Known,
                None,
            ))
            .expect("known settle"),
        SettlementApplyOutcome::Applied
    );
    let entry = ledger.entry(second).expect("entry");
    assert_eq!(entry.charged_tokens, Some(10));
    assert_eq!(entry.charge_basis, ProviderChargeBasis::KnownUsage);
    assert_eq!(ledger.charged_tokens().expect("charged"), 70);
    assert_eq!(ledger.open_reserved_tokens().expect("open"), 0);

    // A cancelled-but-never-dispatched call releases its hold.
    let cancelled = AttemptId::new();
    ledger
        .reserve(attempt_reservation(
            cancelled,
            run_id,
            ModelProtocol::AnthropicMessages,
            1,
            30,
            None,
        ))
        .expect("hold");
    ledger.release(cancelled).expect("release before dispatch");
    assert_eq!(
        ledger.entry(cancelled).expect("entry").charge_basis,
        ProviderChargeBasis::Released
    );
    assert_eq!(
        ledger.entry(cancelled).expect("entry").charged_tokens,
        Some(0)
    );
    assert_eq!(ledger.charged_tokens().expect("charged"), 70);
    assert_eq!(ledger.open_reserved_tokens().expect("open"), 0);
    assert_eq!(
        ledger.dispatch(cancelled).unwrap_err(),
        "provider_usage_released_attempt_cannot_dispatch"
    );
}

#[test]
fn cumulative_cache_and_reasoning_usage_is_not_double_counted() {
    let mut vector = UsageVector::zero();
    // OpenAI chat reports prompt_tokens inclusive of cached tokens and completion_tokens
    // inclusive of reasoning tokens.
    vector.input_tokens = Some(10);
    vector.cache_read_tokens = Some(4);
    vector.output_tokens = Some(5);
    vector.reasoning_output_tokens = Some(2);

    let openai = ProviderUsageContainment::declared(ModelProtocol::OpenAiChat);
    assert_eq!(openai.cache_read_contained_in_input, Some(true));
    assert_eq!(openai.reasoning_contained_in_output, Some(true));
    let chargeable = openai.chargeable(&vector).expect("chargeable");
    assert_eq!(chargeable.input_units, 6);
    assert_eq!(chargeable.cache_read_units, 4);
    assert_eq!(chargeable.output_units, 3);
    assert_eq!(chargeable.reasoning_units, 2);
    // 10 + 5, never the naive 10 + 4 + 5 + 2 = 21.
    assert_eq!(chargeable.total_units, 15);

    // Anthropic reports cached tokens outside input_tokens, so its cache dimensions are disjoint
    // and the very same numbers are honestly additive.  Its wire shape never reports a reasoning
    // line, so that pairing stays undeclared instead of being guessed.
    let anthropic = ProviderUsageContainment::declared(ModelProtocol::AnthropicMessages);
    assert_eq!(anthropic.cache_read_contained_in_input, Some(false));
    assert_eq!(anthropic.cache_write_contained_in_input, Some(false));
    assert_eq!(anthropic.reasoning_contained_in_output, None);
    let mut anthropic_vector = vector.clone();
    anthropic_vector.reasoning_output_tokens = Some(0);
    assert_eq!(
        anthropic
            .chargeable(&anthropic_vector)
            .expect("chargeable")
            .total_units,
        19
    );

    // An undeclared relationship is never silently resolved: the token count stays additive
    // (conservative for a budget) while the cost estimate refuses to price the overlap at all.
    let legacy = ProviderUsageContainment::declared(ModelProtocol::Legacy);
    assert!(legacy.has_undeclared_overlap(&vector));
    assert_eq!(
        legacy.chargeable(&vector).expect("chargeable").total_units,
        21
    );

    // A card that prices cached tokens charges them once at the cache rate and subtracts them
    // from the input line, so the estimate never pays for them twice.
    let mut priced = card_with("provider-a", "model-a", Some(3), Some(5));
    priced.cache_read_price_per_unit = Some(1);
    priced.reasoning_price_per_unit = Some(5);
    priced.rate_card_digest = priced.digest();
    priced.validate().expect("valid card");
    let estimate = priced
        .estimate_with_containment(&vector, 1, &openai)
        .expect("estimate");
    assert_eq!(
        estimate.amount.expect("priced estimate").micros,
        6 * 3 + 4 * 1 + 3 * 5 + 2 * 5
    );
    assert_eq!(estimate.unknown_reason, None);

    // A card that does not price the contained dimensions needs no separate line for them: they
    // stay inside the container rate instead of becoming an unknown dimension.
    let container_only = card_with("provider-a", "model-a", Some(3), Some(5));
    let estimate = container_only
        .estimate_with_containment(&vector, 1, &openai)
        .expect("estimate");
    assert_eq!(
        estimate.amount.expect("priced estimate").micros,
        10 * 3 + 5 * 5
    );
    assert_eq!(estimate.unknown_reason, None);

    // A protocol with an undeclared overlap keeps the conservative refusal; the table only
    // narrows the guard to the pairs the protocol really documents.
    let undeclared = container_only
        .estimate_with_containment(&vector, 1, &legacy)
        .expect("estimate");
    assert_eq!(
        undeclared.unknown_reason,
        Some(BillingUnknownReason::Unsupported)
    );

    // Without a declared protocol the conservative overlap guard still refuses to price the
    // observation at all, exactly as before this table existed.
    let conservative = container_only.estimate(&vector, 1).expect("estimate");
    assert_eq!(
        conservative.unknown_reason,
        Some(BillingUnknownReason::Unsupported)
    );

    // The declaration is validated, not trusted.
    let mut violated = vector.clone();
    violated.cache_read_tokens = Some(12);
    assert_eq!(
        openai.chargeable(&violated).unwrap_err(),
        "provider_usage_containment_violated:input_tokens"
    );
    let mut unprovable = vector.clone();
    unprovable.input_tokens = None;
    assert_eq!(
        openai.chargeable(&unprovable).unwrap_err(),
        "provider_usage_containment_unprovable:input_tokens"
    );
}

#[test]
fn delta_observation_is_not_settled_as_cumulative() {
    let attempt_id = AttemptId::new();
    let run_id = RunId::new();
    // A delta reports only the increment since the previous observation.  Billing it as the
    // attempt's total would undercharge, so the settlement contract only accepts the
    // accumulator's cumulative observation.
    let delta = NormalizedUsage::new(
        UsageId::new(),
        attempt_id,
        run_id,
        Some("provider-a".to_owned()),
        "model-a",
        "connection-a",
        tokens(Some(4), Some(2)),
        UsageSource::Provider,
        UsageObservation::Delta,
        Some(1),
        UsageConfidence::Known,
        None,
        "provider fixture",
        digest('d'),
    )
    .expect("delta observation");
    assert_eq!(
        ProviderUsageSettlement::from_usage_with_protocol(
            &delta,
            ModelProtocol::OpenAiChat,
            None,
            1,
            100,
        )
        .unwrap_err(),
        "provider_usage_delta_observation_not_settleable"
    );
    assert_eq!(
        ProviderUsageCorrection::new(
            &delta,
            SettlementCost::Unknown {
                reason: BillingUnknownReason::ReconciliationRequired,
            },
            digest('e'),
            "late provider usage",
            200,
        )
        .unwrap_err(),
        "provider_usage_delta_observation_not_settleable"
    );

    // The validator, not only the constructor, refuses a delta that arrives with consistent
    // digests: a deserialized fact cannot smuggle an unaccumulated increment into billing.
    let mut settlement = ProviderUsageSettlement::from_usage_with_protocol(
        &usage_with(
            attempt_id,
            run_id,
            "provider-a",
            tokens(Some(10), Some(5)),
            UsageConfidence::Known,
            None,
        ),
        ModelProtocol::OpenAiChat,
        None,
        1,
        100,
    )
    .expect("final settlement");
    let mut smuggled = settlement.usage.clone();
    smuggled.observation = UsageObservation::Delta;
    smuggled.sequence = Some(2);
    smuggled.usage_digest = smuggled.digest();
    settlement.usage = smuggled;
    settlement.settlement_digest = settlement.digest();
    assert_eq!(
        settlement.validate().unwrap_err(),
        "provider_usage_delta_observation_not_settleable"
    );

    // A cumulative snapshot is a terminal observation of the same shape and stays settleable;
    // only the delta shape is refused.
    let mut cumulative = settlement.usage.clone();
    cumulative.observation = UsageObservation::Snapshot;
    cumulative.usage_digest = cumulative.digest();
    settlement.usage = cumulative;
    settlement.settlement_digest = settlement.digest();
    settlement.validate().expect("cumulative snapshot settles");
}

#[test]
fn self_hosted_route_does_not_default_external_cost_to_zero() {
    let usage = usage_with(
        AttemptId::new(),
        RunId::new(),
        "local-provider",
        tokens(Some(120), Some(80)),
        UsageConfidence::Known,
        None,
    );
    // A zero-priced card for a locally served model does not establish an external price.
    let zero_card = card_with("local-provider", "model-a", Some(0), Some(0));
    let settlement = ProviderUsageSettlement::from_usage_with_protocol(
        &usage,
        ModelProtocol::OllamaChat,
        Some(&zero_card),
        1,
        100,
    )
    .expect("settlement");
    assert!(matches!(
        settlement.cost,
        SettlementCost::Unknown {
            reason: BillingUnknownReason::Unsupported
        }
    ));
    assert_eq!(settlement.state, BillingState::Unknown);

    // An explicit non-zero card is an estimate, so a locally priced route is still costable.
    let priced_card = card_with("local-provider", "model-a", Some(1), Some(2));
    let priced = ProviderUsageSettlement::from_usage_with_protocol(
        &usage,
        ModelProtocol::OllamaChat,
        Some(&priced_card),
        1,
        100,
    )
    .expect("settlement");
    assert!(matches!(priced.cost, SettlementCost::Estimated { .. }));

    // A remote protocol may explicitly declare a zero-priced card; that is a priced statement.
    let remote_zero = ProviderUsageSettlement::from_usage_with_protocol(
        &usage,
        ModelProtocol::OpenAiChat,
        Some(&zero_card),
        1,
        100,
    )
    .expect("settlement");
    assert!(matches!(remote_zero.cost, SettlementCost::Estimated { .. }));
}

#[test]
fn failed_attempt_usage_is_retained() {
    let run_id = RunId::new();
    let mut ledger = ProviderUsageAttemptLedger::new(1_000).expect("ledger");
    let first = AttemptId::new();
    ledger
        .reserve(attempt_reservation(
            first,
            run_id,
            ModelProtocol::OpenAiChat,
            1,
            300,
            None,
        ))
        .expect("reserve");
    ledger.dispatch(first).expect("dispatch");

    // The attempt was sent and then failed without a usable observation: the fact is retained and
    // the reserved upper bound stays charged instead of vanishing with the failure.
    let failed = settlement_for(
        first,
        run_id,
        ModelProtocol::OpenAiChat,
        tokens(None, None),
        UsageConfidence::Partial,
        Some(BillingUnknownReason::Partial),
    );
    let replaced = failed.settlement_digest.clone();
    assert_eq!(
        ledger.settle(failed).expect("settle"),
        SettlementApplyOutcome::Applied
    );
    let entry = ledger.entry(first).expect("entry");
    assert_eq!(entry.state, BillingState::Unknown);
    assert_eq!(entry.conservative_charge(), 300);
    assert_eq!(
        entry
            .settlement
            .as_ref()
            .expect("settlement retained")
            .attempt_id,
        first
    );
    assert!(matches!(
        entry.effective_cost(),
        Some(SettlementCost::Unknown { .. })
    ));

    // Late usage for the failed attempt is appended as a correction, never a second settlement.
    let late = usage_with(
        first,
        run_id,
        "provider-a",
        tokens(Some(40), Some(20)),
        UsageConfidence::Known,
        None,
    );
    let correction = ProviderUsageCorrection::new(
        &late,
        SettlementCost::Unknown {
            reason: BillingUnknownReason::ReconciliationRequired,
        },
        replaced.clone(),
        "late provider usage",
        200,
    )
    .expect("correction");
    assert_eq!(
        ledger.correct(correction.clone()).expect("correction"),
        SettlementApplyOutcome::Applied
    );
    let entry = ledger.entry(first).expect("entry");
    assert_eq!(entry.state, BillingState::Corrected);
    assert_eq!(entry.charged_tokens, Some(60));
    assert_eq!(entry.conservative_charge(), 60);
    assert_eq!(ledger.charged_tokens().expect("charged"), 60);

    // The correction is idempotent by its own digest, and a different correction for the same
    // replaced settlement is a conflict rather than a silent overwrite.
    assert_eq!(
        ledger.correct(correction).expect("replay"),
        SettlementApplyOutcome::Duplicate
    );
    let conflicting = ProviderUsageCorrection::new(
        &usage_with(
            first,
            run_id,
            "provider-a",
            tokens(Some(50), Some(25)),
            UsageConfidence::Known,
            None,
        ),
        SettlementCost::Unknown {
            reason: BillingUnknownReason::ReconciliationRequired,
        },
        replaced.clone(),
        "late provider usage",
        201,
    )
    .expect("correction");
    assert_eq!(
        ledger.correct(conflicting).unwrap_err(),
        "provider_usage_correction_conflict"
    );
    let wrong_target = ProviderUsageCorrection::new(
        &late,
        SettlementCost::Unknown {
            reason: BillingUnknownReason::ReconciliationRequired,
        },
        digest('c'),
        "late provider usage",
        202,
    )
    .expect("correction");
    assert_eq!(
        ledger.correct(wrong_target).unwrap_err(),
        "provider_usage_correction_target_mismatch"
    );
    assert_eq!(ledger.charged_tokens().expect("charged"), 60);

    // A retry is a separate attempt with its own explainable charge.
    let retry = AttemptId::new();
    ledger
        .reserve(attempt_reservation(
            retry,
            run_id,
            ModelProtocol::OpenAiChat,
            2,
            300,
            None,
        ))
        .expect("retry hold");
    ledger.dispatch(retry).expect("dispatch");
    assert_eq!(
        ledger
            .settle(settlement_for(
                retry,
                run_id,
                ModelProtocol::OpenAiChat,
                tokens(Some(10), Some(5)),
                UsageConfidence::Known,
                None,
            ))
            .expect("retry settle"),
        SettlementApplyOutcome::Applied
    );
    assert_eq!(ledger.entry(first).expect("first").retry_ordinal, 1);
    assert_eq!(ledger.entry(retry).expect("retry").retry_ordinal, 2);
    assert_eq!(
        ledger.entry(first).expect("first").conservative_charge(),
        60
    );
    assert_eq!(
        ledger.entry(retry).expect("retry").conservative_charge(),
        15
    );
    assert_eq!(ledger.charged_tokens().expect("charged"), 75);
}

#[test]
fn replayed_usage_is_settled_once() {
    let run_id = RunId::new();
    let attempt_id = AttemptId::new();
    let mut ledger = ProviderUsageAttemptLedger::new(100).expect("ledger");
    ledger
        .reserve(attempt_reservation(
            attempt_id,
            run_id,
            ModelProtocol::OpenAiChat,
            1,
            50,
            None,
        ))
        .expect("reserve");
    ledger.dispatch(attempt_id).expect("dispatch");
    let settlement = settlement_for(
        attempt_id,
        run_id,
        ModelProtocol::OpenAiChat,
        tokens(Some(6), Some(4)),
        UsageConfidence::Known,
        None,
    );
    assert_eq!(
        ledger.settle(settlement.clone()).expect("first settlement"),
        SettlementApplyOutcome::Applied
    );
    assert_eq!(ledger.charged_tokens().expect("charged"), 10);

    // The exact same attempt observation is a replay, not a second charge.
    assert_eq!(
        ledger.settle(settlement.clone()).expect("replay"),
        SettlementApplyOutcome::Duplicate
    );
    assert_eq!(ledger.charged_tokens().expect("charged"), 10);
    assert_eq!(ledger.len(), 1);

    // A different settlement for the same attempt is a conflict.
    let conflicting = settlement_for(
        attempt_id,
        run_id,
        ModelProtocol::OpenAiChat,
        tokens(Some(60), Some(40)),
        UsageConfidence::Known,
        None,
    );
    assert_eq!(
        ledger.settle(conflicting).unwrap_err(),
        "provider_usage_attempt_conflict"
    );
    assert_eq!(ledger.charged_tokens().expect("charged"), 10);

    // A settlement for an attempt that was never reserved is an orphan.
    let orphan_attempt = AttemptId::new();
    assert_eq!(
        ledger
            .settle(settlement_for(
                orphan_attempt,
                run_id,
                ModelProtocol::OpenAiChat,
                tokens(Some(1), Some(1)),
                UsageConfidence::Known,
                None,
            ))
            .unwrap_err(),
        "provider_usage_settlement_orphan"
    );
    assert_eq!(ledger.len(), 1);
}

#[test]
fn settlement_identity_release_and_rate_card_scope_are_fail_closed() {
    let run_id = RunId::new();
    let attempt_id = AttemptId::new();
    let mut ledger = ProviderUsageAttemptLedger::new(1_000).expect("ledger");
    let execution_id = ExecutionId::new();
    let bound = ProviderAttemptReservation::new(
        attempt_id,
        run_id,
        Some(execution_id),
        ModelProtocol::AnthropicMessages,
        1,
        200,
        None,
        1_000,
    )
    .expect("bound reservation");
    ledger
        .reserve(bound)
        .expect("reserve with execution binding");
    ledger.dispatch(attempt_id).expect("dispatch");
    assert_eq!(
        ledger.entry(attempt_id).expect("entry").execution_id,
        Some(execution_id)
    );

    // The settlement of a bound attempt must carry the same server-owned execution identity.
    let unbound = ProviderUsageSettlement::from_usage_with_protocol(
        &usage_with(
            attempt_id,
            run_id,
            "provider-a",
            tokens(Some(1), Some(1)),
            UsageConfidence::Known,
            None,
        ),
        ModelProtocol::AnthropicMessages,
        None,
        1,
        100,
    )
    .expect("settlement");
    assert_eq!(
        ledger.settle(unbound.clone()).unwrap_err(),
        "provider_usage_execution_id_mismatch"
    );
    let other_execution = ProviderUsageSettlement::from_usage_with_execution_id(
        &usage_with(
            attempt_id,
            run_id,
            "provider-a",
            tokens(Some(1), Some(1)),
            UsageConfidence::Known,
            None,
        ),
        ExecutionId::new(),
        None,
        1,
        100,
    )
    .expect("settlement");
    assert_eq!(
        ledger.settle(other_execution).unwrap_err(),
        "provider_usage_execution_id_mismatch"
    );

    // ... while the identical binding is accepted: the rejections above are about identity, not
    // about the settlement being unusable.
    let bound_ok = AttemptId::new();
    let bound_ok_execution = ExecutionId::new();
    ledger
        .reserve(
            ProviderAttemptReservation::new(
                bound_ok,
                run_id,
                Some(bound_ok_execution),
                ModelProtocol::AnthropicMessages,
                1,
                50,
                None,
                1_000,
            )
            .expect("bound reservation"),
        )
        .expect("reserve");
    ledger.dispatch(bound_ok).expect("dispatch");
    let matching = ProviderUsageSettlement::from_usage_with_execution_id(
        &usage_with(
            bound_ok,
            run_id,
            "provider-a",
            tokens(Some(1), Some(1)),
            UsageConfidence::Known,
            None,
        ),
        bound_ok_execution,
        None,
        1,
        100,
    )
    .expect("settlement");
    assert_eq!(
        ledger
            .settle(matching)
            .expect("matching execution binding accepted"),
        SettlementApplyOutcome::Applied
    );

    // Facts must belong to the same run as the reservation; a foreign attempt is an orphan, and
    // the same attempt under a different run is an identity mismatch.
    let foreign_attempt = settlement_for(
        AttemptId::new(),
        RunId::new(),
        ModelProtocol::AnthropicMessages,
        tokens(Some(1), Some(1)),
        UsageConfidence::Known,
        None,
    );
    assert_eq!(
        ledger.settle(foreign_attempt).unwrap_err(),
        "provider_usage_settlement_orphan"
    );
    assert!(ledger
        .entry(attempt_id)
        .expect("entry")
        .settlement
        .is_none());
    let retry_attempt = AttemptId::new();
    let retry_run = RunId::new();
    ledger
        .reserve(attempt_reservation(
            retry_attempt,
            retry_run,
            ModelProtocol::AnthropicMessages,
            2,
            100,
            None,
        ))
        .expect("reserve");
    // A settlement that never got dispatched cannot be recorded at all.
    assert_eq!(
        ledger
            .settle(settlement_for(
                retry_attempt,
                retry_run,
                ModelProtocol::AnthropicMessages,
                tokens(Some(1), Some(1)),
                UsageConfidence::Known,
                None,
            ))
            .unwrap_err(),
        "provider_usage_attempt_not_dispatched"
    );
    let cross_run = ProviderUsageSettlement::from_usage_with_protocol(
        &usage_with(
            retry_attempt,
            run_id,
            "provider-a",
            tokens(Some(1), Some(1)),
            UsageConfidence::Known,
            None,
        ),
        ModelProtocol::AnthropicMessages,
        None,
        1,
        100,
    )
    .expect("settlement");
    ledger.dispatch(retry_attempt).expect("dispatch");
    assert_eq!(
        ledger.settle(cross_run).unwrap_err(),
        "provider_usage_settlement_identity_mismatch"
    );
    assert_eq!(
        ledger
            .settle(settlement_for(
                retry_attempt,
                retry_run,
                ModelProtocol::AnthropicMessages,
                tokens(Some(1), Some(1)),
                UsageConfidence::Known,
                None,
            ))
            .expect("same-run settle"),
        SettlementApplyOutcome::Applied
    );

    // A dispatched attempt can never be released; a released one can never settle afterwards.
    assert_eq!(
        ledger.release(attempt_id).unwrap_err(),
        "provider_usage_dispatched_attempt_cannot_release"
    );
    let released = AttemptId::new();
    ledger
        .reserve(attempt_reservation(
            released,
            run_id,
            ModelProtocol::AnthropicMessages,
            1,
            200,
            None,
        ))
        .expect("reserve");
    ledger.release(released).expect("release");
    assert_eq!(
        ledger
            .settle(settlement_for(
                released,
                run_id,
                ModelProtocol::AnthropicMessages,
                tokens(Some(1), Some(1)),
                UsageConfidence::Known,
                None,
            ))
            .unwrap_err(),
        "provider_usage_released_attempt_cannot_settle"
    );

    // A rate card outside the provider/model/validity scope may not be used at all.
    let other_provider_card = card_with("provider-b", "model-a", Some(1), Some(1));
    assert_eq!(
        ProviderUsageSettlement::from_usage_with_protocol(
            &usage_with(
                AttemptId::new(),
                run_id,
                "provider-a",
                tokens(Some(1), Some(1)),
                UsageConfidence::Known,
                None,
            ),
            ModelProtocol::AnthropicMessages,
            Some(&other_provider_card),
            1,
            100,
        )
        .unwrap_err(),
        "provider_rate_card_scope_mismatch"
    );
    let expired_card = RateCard::new(
        RateCardId::new(),
        "provider-a",
        "model-a",
        "USD",
        Some(1),
        Some(1),
        1,
        Some(50),
        1,
        "fixture-rate-card",
    )
    .expect("expired rate card");
    assert_eq!(
        ProviderUsageSettlement::from_usage_with_protocol(
            &usage_with(
                AttemptId::new(),
                run_id,
                "provider-a",
                tokens(Some(1), Some(1)),
                UsageConfidence::Known,
                None,
            ),
            ModelProtocol::AnthropicMessages,
            Some(&expired_card),
            1,
            100,
        )
        .unwrap_err(),
        "provider_rate_card_scope_mismatch"
    );
    let other_model_card = card_with("provider-a", "model-b", Some(1), Some(1));
    assert_eq!(
        ProviderUsageSettlement::from_usage_with_protocol(
            &usage_with(
                AttemptId::new(),
                run_id,
                "provider-a",
                tokens(Some(1), Some(1)),
                UsageConfidence::Known,
                None,
            ),
            ModelProtocol::AnthropicMessages,
            Some(&other_model_card),
            1,
            100,
        )
        .unwrap_err(),
        "provider_rate_card_scope_mismatch"
    );
}

#[test]
fn settlement_from_another_rate_card_than_the_reservation_is_rejected() {
    let run_id = RunId::new();
    let attempt_id = AttemptId::new();
    let reserved_card = card_with("provider-a", "model-a", Some(1), Some(1));
    let mut ledger = ProviderUsageAttemptLedger::new(1_000).expect("ledger");
    ledger
        .reserve(attempt_reservation(
            attempt_id,
            run_id,
            ModelProtocol::AnthropicMessages,
            1,
            200,
            Some(&reserved_card),
        ))
        .expect("reserve");
    ledger.dispatch(attempt_id).expect("dispatch");

    let other_card = card_with("provider-a", "model-a", Some(2), Some(2));
    let settlement = ProviderUsageSettlement::from_usage_with_protocol(
        &usage_with(
            attempt_id,
            run_id,
            "provider-a",
            tokens(Some(1), Some(1)),
            UsageConfidence::Known,
            None,
        ),
        ModelProtocol::AnthropicMessages,
        Some(&other_card),
        1,
        100,
    )
    .expect("settlement");
    assert_eq!(
        ledger.settle(settlement).unwrap_err(),
        "provider_usage_rate_card_mismatch"
    );
    assert!(ledger
        .entry(attempt_id)
        .expect("entry")
        .settlement
        .is_none());
}

#[test]
fn charge_above_the_hold_is_recorded_before_the_error() {
    let run_id = RunId::new();
    let attempt_id = AttemptId::new();
    // Budget of exactly the measured charge: the hold was smaller than what the provider reported.
    let mut ledger = ProviderUsageAttemptLedger::new(100).expect("ledger");
    ledger
        .reserve(attempt_reservation(
            attempt_id,
            run_id,
            ModelProtocol::OpenAiChat,
            1,
            20,
            None,
        ))
        .expect("reserve");
    ledger.dispatch(attempt_id).expect("dispatch");
    let settlement = settlement_for(
        attempt_id,
        run_id,
        ModelProtocol::OpenAiChat,
        tokens(Some(50), Some(50)),
        UsageConfidence::Known,
        None,
    );
    // The overspend fact is committed before the error, exactly like the ControlPlane budget
    // ledger: a provider already spent the tokens and dropping the fact would hide the overspend.
    assert_eq!(
        ledger.settle(settlement).unwrap_err(),
        "provider_usage_charge_exceeds_reservation"
    );
    let entry = ledger.entry(attempt_id).expect("entry");
    assert!(entry.over_reservation);
    assert_eq!(entry.charged_tokens, Some(100));
    assert_eq!(ledger.charged_tokens().expect("charged"), 100);
    assert_eq!(ledger.open_reserved_tokens().expect("open"), 0);
    // Further holds are rejected while the charge exceeds what is left of the budget.
    assert_eq!(
        ledger
            .reserve(attempt_reservation(
                AttemptId::new(),
                run_id,
                ModelProtocol::OpenAiChat,
                1,
                1,
                None,
            ))
            .unwrap_err(),
        "provider_usage_budget_exceeded"
    );
}
