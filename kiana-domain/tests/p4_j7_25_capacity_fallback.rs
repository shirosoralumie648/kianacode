use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn route() -> ProviderFallbackRoute {
    ProviderFallbackRoute {
        profile: "backup".to_owned(),
        provider_id: "provider-b".to_owned(),
        model_id: "model-b".to_owned(),
        route_digest: digest('a'),
        capabilities_digest: digest('b'),
        data_scope_digest: digest('c'),
        budget_digest: digest('d'),
    }
}

#[test]
fn circuit_opens_after_typed_failures_and_allows_one_half_open_probe() {
    let policy = ProviderCapacityPolicy::new(2, 4, 2, 100).expect("policy");
    let mut breaker = ProviderCircuitBreaker::new(policy).expect("breaker");
    assert!(breaker.allow(1).expect("closed allow"));
    breaker.record_failure(10).expect("first failure");
    assert!(breaker.allow(11).expect("still closed"));
    breaker.record_failure(12).expect("open failure");
    assert!(!breaker.allow(50).expect("cooldown deny"));
    assert!(breaker.allow(112).expect("half open probe"));
    assert!(!breaker.allow(113).expect("second probe denied"));
    breaker.record_success().expect("close");
    assert!(breaker.allow(114).expect("closed again"));
}

#[test]
fn fallback_rechecks_capability_data_and_budget_digests() {
    let candidate = route();
    assert!(admit_fallback(&candidate, &digest('b'), &digest('c'), &digest('d')).is_ok());
    assert_eq!(
        admit_fallback(&candidate, &digest('x'), &digest('c'), &digest('d')).unwrap_err(),
        "provider_fallback_capability_mismatch"
    );
    assert_eq!(
        admit_fallback(&candidate, &digest('b'), &digest('x'), &digest('d')).unwrap_err(),
        "provider_fallback_data_scope_mismatch"
    );
    assert_eq!(
        admit_fallback(&candidate, &digest('b'), &digest('c'), &digest('x')).unwrap_err(),
        "provider_fallback_budget_mismatch"
    );
}

