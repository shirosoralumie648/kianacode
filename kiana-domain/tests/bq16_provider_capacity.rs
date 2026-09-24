use kiana_domain::*;

fn clock() -> ClockObservation {
    ClockObservation::observe("bq16-fixture", 60_000, 60_000, None, 1).expect("clock")
}

fn policy(max_concurrency: u32, max_queue: u32, rpm: u64, tpm: u64) -> ProviderCapacityPolicy {
    let group = QuotaGroupKey::new(
        "provider-a",
        "credential-a",
        Some("model-a".to_owned()),
        Some("primary".to_owned()),
    )
    .expect("group");
    ProviderCapacityPolicy::new(
        group,
        max_concurrency,
        max_queue,
        rpm,
        tpm,
        30_000,
        "cfg.v1",
    )
    .expect("policy")
}

fn request(policy: &ProviderCapacityPolicy, session: &str, now: u64) -> ProviderCapacityRequest {
    let window = QuotaWindow::from_clock(&clock(), 60_000).expect("window");
    ProviderCapacityRequest::new(
        session,
        RunId::new(),
        AttemptId::new(),
        policy.group.clone(),
        window,
        100,
        now,
        now + 10_000,
    )
    .expect("request")
}

#[test]
fn outcomes_are_typed_and_queue_is_session_fair_and_bounded() {
    let policy = policy(1, 2, 100, 10_000);
    let window = QuotaWindow::from_clock(&clock(), 60_000).expect("window");
    let mut controller = ProviderCapacityController::new(policy.clone(), window).expect("ctl");
    let first = controller
        .admit(request(&policy, "session-a", 60_001))
        .expect("accept");
    assert_eq!(first.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    let second = controller
        .admit(request(&policy, "session-a", 60_002))
        .expect("queue");
    assert_eq!(second.outcome.kind, ProviderCapacityOutcomeKind::Queue);
    let third = controller
        .admit(request(&policy, "session-b", 60_003))
        .expect("queue");
    assert_eq!(third.outcome.kind, ProviderCapacityOutcomeKind::Queue);
    let rejected = controller
        .admit(request(&policy, "session-c", 60_004))
        .expect("bounded reject");
    assert_eq!(rejected.outcome.kind, ProviderCapacityOutcomeKind::Reject);
    assert_eq!(controller.queue_len(), 2);
    assert!(controller
        .dispatch_next(60_004)
        .expect("active slot blocks dispatch")
        .is_none());
    assert!(controller.cancel(third.outcome.attempt_id));
    assert_eq!(controller.queue_len(), 1);
    let lease = first.lease.expect("lease");
    assert_eq!(
        controller.release(lease.lease_id, "session-b").unwrap_err(),
        "provider_capacity_lease_owner_mismatch"
    );
    controller
        .release(lease.lease_id, "session-a")
        .expect("owner release");
    assert_eq!(controller.active_len(), 0);
    let successor = controller
        .dispatch_next(60_005)
        .expect("queued successor")
        .expect("queued successor decision");
    assert_eq!(successor.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    assert!(successor.outcome.queue_wait_ms.unwrap_or_default() > 0);
    let successor_lease = successor.lease.expect("successor lease");
    controller
        .release(successor_lease.lease_id, "session-a")
        .expect("successor release");
}

#[test]
fn queued_sessions_rotate_fairly_after_the_active_slot_is_released() {
    let policy = policy(1, 4, 100, 10_000);
    let window = QuotaWindow::from_clock(&clock(), 60_000).expect("window");
    let mut controller = ProviderCapacityController::new(policy.clone(), window).expect("ctl");
    let active = controller
        .admit(request(&policy, "session-a", 60_001))
        .expect("active");
    let a = controller
        .admit(request(&policy, "session-a", 60_002))
        .expect("queue a");
    let b = controller
        .admit(request(&policy, "session-b", 60_003))
        .expect("queue b");
    controller
        .release(active.lease.expect("active lease").lease_id, "session-a")
        .expect("release");
    let first = controller
        .dispatch_next(60_004)
        .expect("dispatch a")
        .expect("a decision");
    assert_eq!(first.outcome.attempt_id, a.outcome.attempt_id);
    controller
        .release(first.lease.expect("a lease").lease_id, "session-a")
        .expect("a release");
    let second = controller
        .dispatch_next(60_005)
        .expect("dispatch b")
        .expect("b decision");
    assert_eq!(second.outcome.attempt_id, b.outcome.attempt_id);
    controller
        .release(second.lease.expect("b lease").lease_id, "session-b")
        .expect("b release");
}

#[test]
fn quota_delay_does_not_consume_active_slot_and_group_alias_cannot_bypass() {
    let policy = policy(2, 2, 1, 10_000);
    let window = QuotaWindow::from_clock(&clock(), 60_000).expect("window");
    let mut controller = ProviderCapacityController::new(policy.clone(), window).expect("ctl");
    let accepted = controller
        .admit(request(&policy, "owner", 60_001))
        .expect("accept");
    assert_eq!(accepted.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    let delayed = controller
        .admit(request(&policy, "other", 60_002))
        .expect("delay");
    assert_eq!(delayed.outcome.kind, ProviderCapacityOutcomeKind::Delay);
    assert_eq!(controller.active_len(), 1);

    let alias_group = QuotaGroupKey::new(
        "provider-a",
        "credential-a",
        Some("model-a".to_owned()),
        Some("alias-that-must-not-bypass".to_owned()),
    )
    .expect("alias group");
    let alias_request = ProviderCapacityRequest::new(
        "alias",
        RunId::new(),
        AttemptId::new(),
        alias_group,
        QuotaWindow::from_clock(&clock(), 60_000).expect("window"),
        100,
        60_003,
        70_000,
    )
    .expect("request");
    assert_eq!(
        controller.admit(alias_request).unwrap_err(),
        "provider_capacity_quota_group_mismatch"
    );
}
