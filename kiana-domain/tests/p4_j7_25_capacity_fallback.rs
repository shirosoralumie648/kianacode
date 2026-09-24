use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn policy() -> ProviderCapacityPolicy {
    let group = QuotaGroupKey::new(
        "provider-a",
        "credential-a",
        Some("model-a".to_owned()),
        Some("primary".to_owned()),
    )
    .expect("quota group");
    ProviderCapacityPolicy::new(group, 2, 4, 100, 100_000, 30_000, "config.v1")
        .expect("policy")
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
    assert_eq!(
        breaker.allow(50).unwrap_err(),
        "provider_circuit_open"
    );
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
