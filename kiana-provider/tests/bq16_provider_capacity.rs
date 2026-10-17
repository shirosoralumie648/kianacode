//! BQ-16 provider capacity fixtures: bounded waiter/active semaphores, fixed-window RPM/TPM
//! accounting and the alias fold at the provider boundary.  The source assertions below pin the
//! ordering and the absence of an unbounded or second provider loop; the behavioral fixtures live
//! in `kiana-provider/src/{capacity,transport}.rs` (`--lib`) so they drive the real values.

#[test]
fn transport_keeps_waiters_bounded_and_releases_queue_before_active_slot() {
    let config = include_str!("../src/config.rs");
    let capacity = include_str!("../src/capacity.rs");
    let transport = include_str!("../src/transport.rs");
    for marker in [
        "ProviderCapacityPolicy",
        "capacity_policy",
        "KIANA_MODEL_QUEUE_LIMIT",
        "KIANA_MODEL_MAX_CONCURRENCY",
        "provider_capacity_tpm_exceeded",
        "provider_capacity_closed",
        "try_acquire_owned",
        "acquire_owned",
        "drop(queue_slot)",
        "provider_capacity_queue_full",
        "requests_per_minute",
        "provider_capacity_clock_rollback",
        "provider_capacity_window_request_invalid",
        "provider_capacity_window_overflow",
        "provider_capacity_quota_exhausted",
        "capacity_scope",
        "share_capacity_gates",
    ] {
        assert!(
            config.contains(marker) || transport.contains(marker) || capacity.contains(marker),
            "BQ-16 provider marker missing: {marker}"
        );
    }
    for forbidden in [
        "queue_slots = tokio::sync::Semaphore::new(usize::MAX)",
        "Semaphore::new(usize::MAX)",
        "tokio::spawn",
    ] {
        assert!(
            !transport.contains(forbidden),
            "unbounded provider waiter: {forbidden}"
        );
    }

    // The waiter gate is taken first, the active slot second, and the waiter slot is returned as
    // soon as the active slot is held; the RPM/TPM window is only reserved after both.
    let waiter_gate = transport
        .find(".try_acquire_owned()")
        .expect("bounded waiter gate");
    let active_slot = transport
        .find("let _capacity = connection")
        .expect("active capacity slot");
    let waiter_release = transport.find("drop(queue_slot)").expect("waiter release");
    let window_reserve = transport
        .find(".capacity_window")
        .expect("rpm/tpm reservation");
    assert!(
        waiter_gate < active_slot
            && active_slot < waiter_release
            && waiter_release < window_reserve,
        "provider capacity gates must be taken and released in order"
    );
}

#[test]
fn provider_capacity_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    let own = include_str!("bq16_provider_capacity.rs");
    let config = include_str!("../src/config.rs");
    let capacity = include_str!("../src/capacity.rs");
    let transport = include_str!("../src/transport.rs");
    let lane = include_str!("../../.github/workflows/bq16-provider-capacity.yml");
    for name in [
        "transport_keeps_waiters_bounded_and_releases_queue_before_active_slot",
        "provider_capacity_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards",
        "capacity_permits_are_bounded_and_released_on_drop",
        "aliased_connections_cannot_double_spend_the_shared_capacity_budget",
        "reserve_keeps_rpm_and_tpm_in_the_fixed_utc_minute_and_rolls_the_window",
        "reserve_fails_closed_on_invalid_input_clock_rollback_and_overflow",
    ] {
        assert!(
            own.contains(name)
                || config.contains(name)
                || capacity.contains(name)
                || transport.contains(name),
            "BQ-16 provider fixture missing: {name}"
        );
    }
    for source in [own, config, capacity, transport] {
        assert!(
            !source.contains("#[ignore"),
            "BQ-16 provider fixtures must not be ignored"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-provider --test bq16_provider_capacity --locked -- --test-threads=1",
        "cargo test -p kiana-provider --lib --locked capacity -- --test-threads=1",
        "cargo test -p kiana-provider --test bq16_provider_capacity --no-run --locked",
        "cargo test -p kiana-provider --lib --no-run --locked",
    ] {
        assert!(
            lane.contains(command),
            "BQ-16 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !lane.contains(forbidden),
            "BQ-16 manual lane must stay dispatch-only: {forbidden}"
        );
    }
}
