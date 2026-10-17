use kiana_domain::*;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn policy() -> ProviderCapacityPolicy {
    let group = QuotaGroupKey::new(
        "provider-a",
        "credential-a",
        Some("model-a".to_owned()),
        Some("primary".to_owned()),
    )
    .expect("quota group");
    ProviderCapacityPolicy::new(group, 2, 4, 100, 100_000, 30_000, "config.v1").expect("policy")
}

fn capacity_policy(
    max_concurrency: u32,
    max_queue: u32,
    requests_per_minute: u64,
    tokens_per_minute: u64,
) -> ProviderCapacityPolicy {
    let group = QuotaGroupKey::new(
        "provider-a",
        "credential-a",
        Some("model-a".to_owned()),
        Some("primary".to_owned()),
    )
    .expect("quota group");
    ProviderCapacityPolicy::new(
        group,
        max_concurrency,
        max_queue,
        requests_per_minute,
        tokens_per_minute,
        30_000,
        "config.v1",
    )
    .expect("policy")
}

fn capacity_window() -> QuotaWindow {
    let clock =
        ClockObservation::observe("p4-j7-25-fixture", 60_000, 60_000, None, 1).expect("clock");
    QuotaWindow::from_clock(&clock, 60_000).expect("window")
}

fn capacity_request(
    policy: &ProviderCapacityPolicy,
    session: &str,
    requested_at_unix_ms: u64,
) -> ProviderCapacityRequest {
    ProviderCapacityRequest::new(
        session,
        RunId::new(),
        AttemptId::new(),
        policy.group.clone(),
        capacity_window(),
        100,
        requested_at_unix_ms,
        requested_at_unix_ms + 10_000,
    )
    .expect("request")
}

#[test]
fn circuit_opens_after_typed_failures_and_allows_one_half_open_probe() {
    let capacity_policy = policy();
    assert_eq!(capacity_policy.max_concurrency, 2);
    let mut breaker = ProviderCircuitBreaker::new("config.v1", 2, 30_000).expect("breaker");
    assert_eq!(
        breaker.allow(1).expect("closed allow"),
        CircuitAdmission::Allowed
    );
    breaker.observe_failure(10).expect("first failure");
    assert_eq!(
        breaker.allow(11).expect("still closed"),
        CircuitAdmission::Allowed
    );
    breaker.observe_failure(12).expect("open failure");
    assert_eq!(breaker.allow(50).unwrap_err(), "provider_circuit_open");
    assert_eq!(
        breaker.allow(30_012).expect("half open probe"),
        CircuitAdmission::HalfOpenProbe
    );
    assert_eq!(
        breaker.allow(30_013).unwrap_err(),
        "provider_circuit_probe_busy"
    );
    breaker.observe_success().expect("close");
    assert_eq!(
        breaker.allow(30_014).expect("closed again"),
        CircuitAdmission::Allowed
    );
}

#[test]
fn abandoned_half_open_probe_reopens_for_a_fresh_cooldown() {
    let mut closed = ProviderCircuitBreaker::new("config.v1", 1, 100).expect("closed breaker");
    assert_eq!(
        closed.abandon_probe(1).unwrap_err(),
        "provider_circuit_probe_not_in_flight"
    );

    let mut breaker = ProviderCircuitBreaker::new("config.v1", 1, 100).expect("breaker");
    breaker
        .observe_failure(1)
        .expect("open after typed failure");
    assert_eq!(
        breaker.allow(101).expect("half-open probe"),
        CircuitAdmission::HalfOpenProbe
    );

    breaker.abandon_probe(150).expect("abandon probe");
    assert_eq!(breaker.state, ProviderCircuitState::Open);
    assert!(!breaker.half_open_probe_in_flight);
    assert_eq!(breaker.open_until_unix_ms, Some(250));
    assert_eq!(breaker.allow(249).unwrap_err(), "provider_circuit_open");
    assert_eq!(
        breaker.allow(250).expect("new half-open probe"),
        CircuitAdmission::HalfOpenProbe
    );
    assert_eq!(
        breaker.abandon_probe(u64::MAX).unwrap_err(),
        "provider_circuit_open_until_overflow"
    );
}

#[test]
fn observe_failure_overflow_does_not_leave_a_half_updated_breaker() {
    let mut breaker = ProviderCircuitBreaker::new("config.v1", 1, 100).expect("breaker");
    let before = breaker.clone();

    assert_eq!(
        breaker.observe_failure(u64::MAX).unwrap_err(),
        "provider_circuit_open_until_overflow"
    );
    assert_eq!(breaker, before);
    breaker
        .validate()
        .expect("overflow must preserve a valid state");
}

fn original_route() -> ModelRoute {
    ModelRoute {
        provider_id: "provider-a".to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: "primary".to_owned(),
        model_id: "model-a".to_owned(),
        profile: "primary".to_owned(),
        configuration_revision: "config.v1".to_owned(),
        streaming: true,
    }
}

fn fallback_candidate() -> FallbackCandidate {
    FallbackCandidate {
        route: ModelRoute {
            provider_id: "provider-b".to_owned(),
            protocol: ModelProtocol::OpenAiChat,
            connection_id: "backup".to_owned(),
            model_id: "model-b".to_owned(),
            profile: "backup".to_owned(),
            configuration_revision: "config.v1".to_owned(),
            streaming: true,
        },
        capabilities: ModelCapabilities {
            tools: CapabilitySupport::Supported,
            streaming: CapabilitySupport::Supported,
            structured_output: CapabilitySupport::Supported,
            images: CapabilitySupport::Supported,
            reasoning_replay: CapabilitySupport::Supported,
            context_window: 128_000,
            max_output: 4096,
            source: "fixture".to_owned(),
            revision: "fixture.v1".to_owned(),
        },
        credential_revision: Some(digest('e')),
        context_scope_digest: digest('a'),
        data_boundary_digest: digest('b'),
        budget_digest: digest('c'),
        permit_digest: digest('f'),
    }
}

fn requirements() -> FallbackRequirements {
    FallbackRequirements {
        context_scope_digest: digest('a'),
        data_boundary_digest: digest('b'),
        budget_digest: digest('c'),
        original_permit_digest: digest('d'),
        require_tools: true,
        require_images: true,
        require_structured_output: true,
        min_context_window: 64_000,
        min_output_tokens: 1024,
    }
}

fn admission_route(provider_id: &str, model_id: &str, connection_id: &str) -> ModelRoute {
    ModelRoute {
        provider_id: provider_id.to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: connection_id.to_owned(),
        model_id: model_id.to_owned(),
        profile: connection_id.to_owned(),
        configuration_revision: "config.v1".to_owned(),
        streaming: true,
    }
}

fn admission_capabilities() -> ModelCapabilities {
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

fn admission_card(rate_card_id: RateCardId) -> RateCard {
    let mut card = RateCard::new(
        rate_card_id,
        "provider-b",
        "model-b",
        "USD",
        Some(2),
        Some(3),
        1_000,
        Some(10_000),
        2,
        "p4-j7-25-fixture",
    )
    .expect("card");
    card.request_price = Some(5);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid card");
    card
}

fn admission_requirements() -> FallbackCapabilityRequirements {
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

fn admission_request(run_id: RunId, original: &ModelRoute) -> FallbackAdmissionRequest {
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
        admission_requirements(),
        2_000,
    )
    .expect("request")
}

fn admission_candidate() -> FallbackAdmissionCandidate {
    FallbackAdmissionCandidate::new(
        admission_route("provider-b", "model-b", "backup"),
        admission_capabilities(),
        digest('a'),
        7,
        digest('b'),
        Some(digest('g')),
        digest('c'),
        Some(digest('f')),
        Some(admission_card(RateCardId::new())),
        digest('h'),
    )
}

fn admission_usage(admission: &FallbackAttemptAdmission) -> NormalizedUsage {
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
        UsageConfidence::Known,
        None,
        "p4-j7-25 fallback fixture",
        digest('u'),
    )
    .expect("usage")
}

#[test]
fn fallback_rechecks_capability_data_and_budget_digests() {
    let original = original_route();
    let candidate = fallback_candidate();
    let requirements = requirements();
    let allowlist = vec![candidate.route.digest()];
    assert!(admit_fallback(
        &original,
        AttemptId::new(),
        &requirements,
        &candidate,
        &allowlist,
        AttemptId::new(),
    )
    .is_ok());

    let mut capability_mismatch = candidate.clone();
    capability_mismatch.capabilities.tools = CapabilitySupport::Unsupported;
    assert_eq!(
        admit_fallback(
            &original,
            AttemptId::new(),
            &requirements,
            &capability_mismatch,
            &allowlist,
            AttemptId::new(),
        )
        .unwrap_err(),
        "fallback_cannot_reduce_required_capabilities"
    );

    let mut data_mismatch = candidate.clone();
    data_mismatch.data_boundary_digest = digest('x');
    assert_eq!(
        admit_fallback(
            &original,
            AttemptId::new(),
            &requirements,
            &data_mismatch,
            &allowlist,
            AttemptId::new(),
        )
        .unwrap_err(),
        "fallback_data_boundary_mismatch"
    );

    let mut budget_mismatch = candidate;
    budget_mismatch.budget_digest = digest('x');
    assert_eq!(
        admit_fallback(
            &original,
            AttemptId::new(),
            &requirements,
            &budget_mismatch,
            &allowlist,
            AttemptId::new(),
        )
        .unwrap_err(),
        "fallback_budget_recheck_required"
    );
}

#[test]
fn fallback_cannot_send_private_context_to_new_provider() {
    let original = original_route();
    let candidate = fallback_candidate();
    let requirements = requirements();
    let allowlist = vec![candidate.route.digest()];

    let mut private_context = candidate.clone();
    private_context.context_scope_digest = digest('x');
    assert_eq!(
        admit_fallback(
            &original,
            AttemptId::new(),
            &requirements,
            &private_context,
            &allowlist,
            AttemptId::new(),
        )
        .unwrap_err(),
        "fallback_cannot_send_private_context_to_new_provider"
    );

    // A destination that is not in the configured whitelist never becomes a fallback, even when
    // every digest matches.
    assert_eq!(
        admit_fallback(
            &original,
            AttemptId::new(),
            &requirements,
            &candidate,
            &[digest('9')],
            AttemptId::new(),
        )
        .unwrap_err(),
        "fallback_route_not_allowlisted"
    );
    assert_eq!(
        admit_fallback(
            &original,
            AttemptId::new(),
            &requirements,
            &candidate,
            &[],
            AttemptId::new(),
        )
        .unwrap_err(),
        "fallback_allowlist_invalid"
    );

    // The primary attempt's permit and identity can never be recycled for the fallback.
    let mut reused_permit = candidate.clone();
    reused_permit.permit_digest = requirements.original_permit_digest.clone();
    assert_eq!(
        admit_fallback(
            &original,
            AttemptId::new(),
            &requirements,
            &reused_permit,
            &allowlist,
            AttemptId::new(),
        )
        .unwrap_err(),
        "fallback_cannot_reuse_original_permit"
    );
    let same_attempt = AttemptId::new();
    assert_eq!(
        admit_fallback(
            &original,
            same_attempt,
            &requirements,
            &candidate,
            &allowlist,
            same_attempt,
        )
        .unwrap_err(),
        "fallback_attempt_identity_invalid"
    );
}

#[test]
fn cancelled_queue_entry_never_sends() {
    let capacity_policy = capacity_policy(1, 4, 100, 10_000);
    let mut controller =
        ProviderCapacityController::new(capacity_policy.clone(), capacity_window()).expect("ctl");
    let active = controller
        .admit(capacity_request(&capacity_policy, "session-a", 60_001))
        .expect("accept");
    assert_eq!(active.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    let queued = controller
        .admit(capacity_request(&capacity_policy, "session-a", 60_002))
        .expect("queue");
    assert_eq!(queued.outcome.kind, ProviderCapacityOutcomeKind::Queue);
    let queued_attempt_id = queued.outcome.attempt_id;
    assert_eq!(controller.queue_len(), 1);

    assert!(controller.cancel(queued_attempt_id));
    assert_eq!(controller.queue_len(), 0);
    assert!(!controller.cancel(queued_attempt_id));

    controller
        .release(active.lease.expect("active lease").lease_id, "session-a")
        .expect("release active");
    assert!(
        controller
            .dispatch_next(60_003)
            .expect("dispatch")
            .is_none(),
        "a cancelled queue entry must never be handed to a provider"
    );
    assert_eq!(controller.active_len(), 0);
    // The cancelled attempt never consumed request/token quota.
    assert_eq!(controller.usage().admitted_requests, 1);
    assert_eq!(controller.usage().admitted_tokens, 100);

    let fresh = controller
        .admit(capacity_request(&capacity_policy, "session-a", 60_004))
        .expect("fresh accept");
    assert_eq!(fresh.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    assert_ne!(fresh.outcome.attempt_id, queued_attempt_id);
}

#[test]
fn rate_limit_queue_is_bounded_and_fair() {
    let capacity_policy = capacity_policy(1, 3, 2, 10_000);
    let mut controller =
        ProviderCapacityController::new(capacity_policy.clone(), capacity_window()).expect("ctl");

    let active = controller
        .admit(capacity_request(&capacity_policy, "session-a", 60_001))
        .expect("accept");
    assert_eq!(active.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    let a2 = controller
        .admit(capacity_request(&capacity_policy, "session-a", 60_002))
        .expect("queue a2");
    let a3 = controller
        .admit(capacity_request(&capacity_policy, "session-a", 60_003))
        .expect("queue a3");
    let b1 = controller
        .admit(capacity_request(&capacity_policy, "session-b", 60_004))
        .expect("queue b1");
    assert_eq!(controller.queue_len(), 3);

    let overflow = controller
        .admit(capacity_request(&capacity_policy, "session-b", 60_005))
        .expect("bounded reject");
    assert_eq!(overflow.outcome.kind, ProviderCapacityOutcomeKind::Reject);
    assert_eq!(
        overflow.outcome.reason.as_deref(),
        Some("provider_capacity_queue_full")
    );
    assert_eq!(controller.queue_len(), 3);

    controller
        .release(active.lease.expect("active lease").lease_id, "session-a")
        .expect("release active");
    let first = controller
        .dispatch_next(60_006)
        .expect("dispatch a2")
        .expect("a2 decision");
    assert_eq!(first.outcome.attempt_id, a2.outcome.attempt_id);
    controller
        .release(first.lease.expect("a2 lease").lease_id, "session-a")
        .expect("release a2");

    // Session fairness: the second session is served before the first session's third entry.
    let second = controller
        .dispatch_next(60_007)
        .expect("dispatch b1")
        .expect("b1 decision");
    assert_eq!(second.outcome.attempt_id, b1.outcome.attempt_id);
    controller
        .release(second.lease.expect("b1 lease").lease_id, "session-b")
        .expect("release b1");

    // The UTC request window is exhausted; the delay is typed, keeps the entry queued and never
    // consumes an active slot (backoff does not occupy the network slot).
    let delayed = controller
        .dispatch_next(60_008)
        .expect("dispatch delay")
        .expect("delay decision");
    assert_eq!(delayed.outcome.kind, ProviderCapacityOutcomeKind::Delay);
    assert_eq!(
        delayed.outcome.reason.as_deref(),
        Some("provider_capacity_quota_window_exhausted")
    );
    assert!(delayed.lease.is_none());
    assert_eq!(controller.active_len(), 0);
    assert_eq!(controller.queue_len(), 1);
    assert_eq!(controller.usage().admitted_requests, 2);

    let delayed_again = controller
        .dispatch_next(60_009)
        .expect("dispatch delay")
        .expect("second delay decision");
    assert_eq!(
        delayed_again.outcome.kind,
        ProviderCapacityOutcomeKind::Delay
    );
    assert_eq!(delayed_again.outcome.attempt_id, a3.outcome.attempt_id);
    assert_eq!(controller.queue_len(), 1);
    assert_eq!(controller.active_len(), 0);
}

#[test]
fn failed_attempt_releases_provider_permit() {
    let capacity_policy = capacity_policy(1, 4, 100, 10_000);
    let mut controller =
        ProviderCapacityController::new(capacity_policy.clone(), capacity_window()).expect("ctl");
    let failed = controller
        .admit(capacity_request(&capacity_policy, "session-a", 60_001))
        .expect("accept");
    let failed_lease_id = failed.lease.expect("failed lease").lease_id;
    let failed_attempt_id = failed.outcome.attempt_id;
    assert_eq!(controller.active_len(), 1);
    assert_eq!(controller.usage().active_concurrency, 1);

    // The failed attempt returns its permit exactly once.
    controller
        .release(failed_lease_id, "session-a")
        .expect("failure release");
    assert_eq!(controller.active_len(), 0);
    assert_eq!(controller.usage().active_concurrency, 0);
    // Failure still records admitted usage; a release never refunds the request.
    assert_eq!(controller.usage().admitted_requests, 1);
    assert_eq!(
        controller
            .release(failed_lease_id, "session-a")
            .unwrap_err(),
        "provider_capacity_lease_unknown"
    );

    let recovered = controller
        .admit(capacity_request(&capacity_policy, "session-a", 60_002))
        .expect("recovered accept");
    assert_eq!(recovered.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    let recovered_lease_id = recovered.lease.expect("recovered lease").lease_id;
    assert_ne!(recovered_lease_id, failed_lease_id);
    // Cancellation only removes queued work: an active permit is released by ControlPlane.
    assert!(!controller.cancel(recovered.outcome.attempt_id));
    assert_eq!(controller.active_len(), 1);
    controller
        .release(recovered_lease_id, "session-a")
        .expect("release recovered");
    assert!(!controller.cancel(failed_attempt_id));
}

#[test]
fn allowed_fallback_has_its_own_route_and_usage_receipt() {
    let original = admission_route("provider-a", "model-a", "primary");
    let request = admission_request(RunId::new(), &original);
    let candidate = admission_candidate();
    let allowlist = FallbackRouteAllowlist::from_routes(std::slice::from_ref(&candidate.route))
        .expect("allowlist");
    let new_attempt_id = AttemptId::new();
    let admission = admit_fallback_attempt(&request, &candidate, &allowlist, new_attempt_id)
        .expect("admitted fallback");

    // The fallback owns a fresh attempt, route, budget reservation and permit binding.
    assert!(admission.is_fresh_attempt());
    assert_eq!(admission.attempt_id, new_attempt_id);
    assert_ne!(admission.attempt_id, request.original_attempt_id);
    assert_ne!(admission.route_digest, request.original_route_digest);
    assert_ne!(admission.permit_digest, request.original_permit_digest);
    assert_ne!(
        admission.budget_reservation_digest,
        request.original_budget_reservation_digest
    );

    // The capacity contract records the route reason on the new attempt rather than merging the
    // primary attempt's partial output.
    let plan = admit_fallback(
        &original,
        request.original_attempt_id,
        &FallbackRequirements {
            context_scope_digest: digest('a'),
            data_boundary_digest: request.data_boundary_digest.clone(),
            budget_digest: request.budget_digest.clone(),
            original_permit_digest: request.original_permit_digest.clone(),
            require_tools: true,
            require_images: true,
            require_structured_output: true,
            min_context_window: 64_000,
            min_output_tokens: 1_024,
        },
        &FallbackCandidate {
            route: candidate.route.clone(),
            capabilities: candidate.capabilities.clone(),
            credential_revision: candidate.credential_revision.clone(),
            context_scope_digest: digest('a'),
            data_boundary_digest: request.data_boundary_digest.clone(),
            budget_digest: request.budget_digest.clone(),
            permit_digest: candidate.permit_digest.clone(),
        },
        &[candidate.route.digest()],
        new_attempt_id,
    )
    .expect("capacity fallback plan");
    assert_eq!(plan.route_reason, "allowlisted_fallback");
    assert_eq!(plan.attempt_id, new_attempt_id);
    assert_ne!(plan.original_route_digest, plan.fallback_route_digest);
    plan.validate().expect("valid plan");

    // Usage, cost and the receipt stay bound to the fallback route/attempt.
    let card = admission_card(admission.rate_card_id);
    let usage = admission_usage(&admission);
    let cost = CostBreakdown::from_usage(&usage, Some(&card), 2, 2_000).expect("cost");
    let receipt =
        FallbackAttemptReceipt::from_admission(&admission, usage, cost, None).expect("receipt");
    assert!(receipt.is_success());
    assert_eq!(receipt.attempt_id, new_attempt_id);
    assert_eq!(receipt.route_digest, admission.route_digest);
    assert_ne!(receipt.route_digest, request.original_route_digest);
    assert_eq!(receipt.rate_card_id, admission.rate_card_id);
}
