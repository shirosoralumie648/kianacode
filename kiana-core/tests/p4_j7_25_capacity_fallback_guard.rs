#[test]
fn provider_capacity_path_is_bounded_and_fallback_is_re_admitted() {
    let config = include_str!("../../kiana-provider/src/config.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let domain = include_str!("../../kiana-domain/src/provider_capacity.rs");
    for marker in [
        "KIANA_MODEL_QUEUE_LIMIT",
        "try_acquire_owned",
        "provider_capacity_queue_full",
        "deadline_unix_ms",
        "provider_capacity_queue_wait_expired",
        "ProviderCircuitBreaker",
        "provider_circuit_open",
        "HalfOpenProbeGuard",
        "abandon_probe",
        "provider_circuit_probe_not_in_flight",
        "dropping_half_open_probe_guard_reopens_breaker_and_clears_busy_fence",
        "FallbackRoutePlan",
        "admit_fallback",
        // P4-J7-25 追加：有界公平队列、许可释放与白名单 fallback 的稳定拒绝码。
        "ProviderCapacityQueue",
        "cancel_entry",
        "provider_capacity_quota_window_exhausted",
        "provider_capacity_lease_unknown",
        "provider_capacity_release_without_permit",
        "route_reason",
        "allowlisted_fallback",
        "fallback_route_not_allowlisted",
        "fallback_cannot_send_private_context_to_new_provider",
        "fallback_cannot_reuse_original_permit",
        "fallback_cannot_reduce_required_capabilities",
        // P4-J7-25 完成切片：配置更新重置派生健康与别名归并的唯一入口。
        "reset_for_config",
        "share_capacity_gates",
    ] {
        assert!(
            config.contains(marker) || transport.contains(marker) || domain.contains(marker),
            "P4-J7-25 marker missing: {marker}"
        );
    }
    for forbidden in [
        "fallback.unwrap()",
        "tokio::spawn",
        "CapabilityBroker",
        "EventLog",
    ] {
        assert!(
            !transport.contains(forbidden),
            "capacity transport boundary widened: {forbidden}"
        );
    }
    // 断路器状态是派生健康信息，只参与放行判定，不能自己授权执行。
    for forbidden in ["CapabilityRequested", "EventStore", "approval"] {
        assert!(
            !domain.contains(forbidden),
            "circuit state must not become authority: {forbidden}"
        );
    }
}

#[test]
fn p4_j7_25_deny_first_fixtures_and_lane_are_wired() {
    let domain_fixtures = include_str!("../../kiana-domain/tests/p4_j7_25_capacity_fallback.rs");
    let provider_fixtures =
        include_str!("../../kiana-provider/tests/p4_j7_25_capacity_fallback.rs");
    let provider_config = include_str!("../../kiana-provider/src/config.rs");
    for fixture in [
        "fallback_cannot_send_private_context_to_new_provider",
        "fallback_cannot_reduce_required_capabilities",
        "cancelled_queue_entry_never_sends",
        "rate_limit_queue_is_bounded_and_fair",
        "failed_attempt_releases_provider_permit",
        "allowed_fallback_has_its_own_route_and_usage_receipt",
        // 完成切片：配置更新重置语义与「多个别名共享一份有界闸门」的行为夹具。
        "config_update_resets_derived_health_before_any_new_admission",
        "connection_aliases_roll_up_to_one_bounded_gate_per_upstream_identity",
    ] {
        assert!(
            domain_fixtures.contains(fixture)
                || provider_fixtures.contains(fixture)
                || provider_config.contains(fixture),
            "P4-J7-25 fixture missing: {fixture}"
        );
    }
    let lane = include_str!("../../.github/workflows/p4-j7-25-capacity-fallback.yml");
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test p4_j7_25_capacity_fallback --locked -- --test-threads=1",
        "cargo test -p kiana-provider --test p4_j7_25_capacity_fallback --locked -- --test-threads=1",
        "cargo test -p kiana-provider --lib circuit_tests --locked -- --test-threads=1",
        "cargo test -p kiana-provider --lib capacity_alias_tests --locked -- --test-threads=1",
        "cargo test -p kiana-core --test p4_j7_25_capacity_fallback_guard --locked -- --test-threads=1",
    ] {
        assert!(
            lane.contains(command),
            "P4-J7-25 lane command missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "schedule:"] {
        assert!(
            !lane.contains(forbidden),
            "P4-J7-25 lane must stay manual: {forbidden}"
        );
    }
}
