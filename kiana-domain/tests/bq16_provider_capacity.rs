use kiana_domain::*;

fn clock() -> ClockObservation {
    ClockObservation::observe("bq16-fixture", 60_000, 60_000, None, 1).expect("clock")
}

fn clock_at(wall_unix_ms: u128) -> ClockObservation {
    ClockObservation::observe("bq16-fixture", wall_unix_ms, wall_unix_ms, None, 1).expect("clock")
}

fn window() -> QuotaWindow {
    QuotaWindow::from_clock(&clock(), 60_000).expect("window")
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

fn entry(session: &str, tokens: u64, queued_at: u64, deadline: u64) -> ProviderCapacityQueueEntry {
    ProviderCapacityQueueEntry::new(
        session,
        RunId::new(),
        AttemptId::new(),
        tokens,
        queued_at,
        deadline,
    )
    .expect("entry")
}

/// Re-sign a typed outcome after a deliberate field mutation so the kind-specific fence (not the
/// digest fence) is the assertion target.
fn resign(outcome: &mut ProviderCapacityOutcome) {
    outcome.outcome_digest = outcome.digest();
}

#[test]
fn outcomes_are_typed_and_queue_is_session_fair_and_bounded() {
    let policy = policy(1, 2, 100, 10_000);
    let mut controller = ProviderCapacityController::new(policy.clone(), window()).expect("ctl");
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
    let mut controller = ProviderCapacityController::new(policy.clone(), window()).expect("ctl");
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
fn queued_request_deadline_is_not_extended_at_dispatch() {
    let policy = policy(1, 2, 100, 10_000);
    let mut controller = ProviderCapacityController::new(policy.clone(), window()).expect("ctl");
    let active = ProviderCapacityRequest::new(
        "active",
        RunId::new(),
        AttemptId::new(),
        policy.group.clone(),
        window(),
        100,
        60_001,
        70_000,
    )
    .expect("active request");
    let active_decision = controller.admit(active).expect("active");
    let queued = ProviderCapacityRequest::new(
        "queued",
        RunId::new(),
        AttemptId::new(),
        policy.group.clone(),
        window(),
        100,
        60_002,
        60_003,
    )
    .expect("queued request");
    let queued_decision = controller.admit(queued).expect("queue");
    assert_eq!(
        queued_decision.outcome.kind,
        ProviderCapacityOutcomeKind::Queue
    );

    controller
        .release(
            active_decision.lease.expect("active lease").lease_id,
            "active",
        )
        .expect("release active");
    let expired = controller
        .dispatch_next(60_100)
        .expect("dispatch")
        .expect("expired decision");
    assert_eq!(expired.outcome.kind, ProviderCapacityOutcomeKind::Reject);
    assert_eq!(
        expired.outcome.reason.as_deref(),
        Some("provider_capacity_queue_wait_expired")
    );
    assert!(expired.lease.is_none());
}

#[test]
fn quota_delay_does_not_consume_active_slot_and_group_alias_cannot_bypass() {
    let policy = policy(2, 2, 1, 10_000);
    let mut controller = ProviderCapacityController::new(policy.clone(), window()).expect("ctl");
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
        window(),
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

#[test]
fn accept_queue_delay_and_reject_outcomes_are_typed_and_observable() {
    let policy = policy(1, 3, 2, 1_000);
    let mut controller =
        ProviderCapacityController::new(policy.clone(), window()).expect("controller");
    let first = controller
        .admit(request(&policy, "session-a", 60_001))
        .expect("accept");
    let first_outcome = first.outcome.clone();
    assert_eq!(first.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    assert!(first.outcome.kind.is_dispatchable());
    assert!(first.outcome.queue_ticket.is_none());
    assert!(first.outcome.queue_wait_ms.is_none());
    assert!(first.outcome.retry_after_ms.is_none());
    assert!(first.outcome.reason.is_none());
    assert_eq!(first.outcome.policy_digest, policy.policy_digest);
    assert_eq!(first.outcome.outcome_digest, first.outcome.digest());
    assert!(first.lease.is_some());

    let second = controller
        .admit(request(&policy, "session-b", 60_002))
        .expect("queue");
    assert_eq!(second.outcome.kind, ProviderCapacityOutcomeKind::Queue);
    assert!(!second.outcome.kind.is_dispatchable());
    assert_eq!(second.outcome.queue_ticket, Some(1));
    assert!(second.outcome.retry_after_ms.is_none());
    assert_eq!(
        second.outcome.reason.as_deref(),
        Some("provider_capacity_concurrency_full")
    );
    assert!(second.lease.is_none());

    let third = controller
        .admit(request(&policy, "session-c", 60_003))
        .expect("queue");
    assert_eq!(third.outcome.queue_ticket, Some(2));
    let fourth = controller
        .admit(request(&policy, "session-d", 60_004))
        .expect("queue");
    assert_eq!(fourth.outcome.queue_ticket, Some(3));
    let fifth = controller
        .admit(request(&policy, "session-e", 60_005))
        .expect("reject");
    assert_eq!(fifth.outcome.kind, ProviderCapacityOutcomeKind::Reject);
    assert_eq!(
        fifth.outcome.reason.as_deref(),
        Some("provider_capacity_queue_full")
    );
    assert_eq!(fifth.outcome.retry_after_ms, Some(120_000 - 60_005));
    assert!(fifth.outcome.queue_ticket.is_none());
    assert!(fifth.lease.is_none());
    assert_eq!(controller.queue_len(), 3);

    controller
        .release(first.lease.expect("lease").lease_id, "session-a")
        .expect("owner release");
    let dispatched = controller
        .dispatch_next(60_006)
        .expect("dispatch")
        .expect("queued decision");
    assert_eq!(dispatched.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    assert_eq!(dispatched.outcome.attempt_id, second.outcome.attempt_id);
    assert_eq!(dispatched.outcome.queue_wait_ms, Some(4));
    controller
        .release(dispatched.lease.expect("lease").lease_id, "session-b")
        .expect("release dispatched");

    // RPM 2/2 is spent: further work is backpressure (Delay), never an unbounded queue.
    let delayed = controller
        .admit(request(&policy, "session-f", 60_007))
        .expect("delay");
    assert_eq!(delayed.outcome.kind, ProviderCapacityOutcomeKind::Delay);
    assert_eq!(
        delayed.outcome.reason.as_deref(),
        Some("provider_capacity_quota_window_exhausted")
    );
    assert_eq!(delayed.outcome.retry_after_ms, Some(120_000 - 60_007));
    assert!(delayed.lease.is_none());
    assert_eq!(controller.active_len(), 0);
    assert_eq!(controller.queue_len(), 2);

    // The four typed outcomes are digest-bound; forged shapes are refused, not silently mapped.
    let mut forged_accept = first_outcome.clone();
    forged_accept.queue_ticket = Some(1);
    resign(&mut forged_accept);
    assert_eq!(
        forged_accept.validate().unwrap_err(),
        "provider_capacity_accept_fields_invalid"
    );
    let mut forged_queue = second.outcome.clone();
    forged_queue.retry_after_ms = Some(10);
    resign(&mut forged_queue);
    assert_eq!(
        forged_queue.validate().unwrap_err(),
        "provider_capacity_queue_fields_invalid"
    );
    let mut forged_delay = delayed.outcome.clone();
    forged_delay.retry_after_ms = None;
    resign(&mut forged_delay);
    assert_eq!(
        forged_delay.validate().unwrap_err(),
        "provider_capacity_retry_after_missing"
    );
    let mut zero_ticket = second.outcome.clone();
    zero_ticket.queue_ticket = Some(0);
    resign(&mut zero_ticket);
    assert_eq!(
        zero_ticket.validate().unwrap_err(),
        "provider_capacity_outcome_invalid"
    );
    let mut digest_drift = first_outcome.clone();
    digest_drift.outcome_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        digest_drift.validate().unwrap_err(),
        "provider_capacity_outcome_invalid"
    );
}

#[test]
fn request_scope_drift_and_window_mismatch_are_refused_before_queueing() {
    let policy = policy(1, 2, 10, 1_000);
    let mut controller =
        ProviderCapacityController::new(policy.clone(), window()).expect("controller");

    let alias_group = QuotaGroupKey::new(
        "provider-a",
        "credential-a",
        Some("model-a".to_owned()),
        Some("alias-that-must-not-bypass".to_owned()),
    )
    .expect("alias group");
    let foreign_group = ProviderCapacityRequest::new(
        "session-a",
        RunId::new(),
        AttemptId::new(),
        alias_group,
        window(),
        100,
        60_001,
        70_000,
    )
    .expect("foreign group request");
    assert_eq!(
        controller.admit(foreign_group).unwrap_err(),
        "provider_capacity_quota_group_mismatch"
    );

    let next_window = QuotaWindow::from_clock(&clock_at(120_050), 60_000).expect("next window");
    assert_eq!(next_window.start_unix_ms, 120_000);
    let foreign_window = ProviderCapacityRequest::new(
        "session-a",
        RunId::new(),
        AttemptId::new(),
        policy.group.clone(),
        next_window,
        100,
        120_050,
        130_000,
    )
    .expect("foreign window request");
    assert_eq!(
        controller.admit(foreign_window).unwrap_err(),
        "provider_capacity_window_mismatch"
    );

    // A request sampled before the bound UTC window is a clock rollback, not an admission.
    assert_eq!(
        ProviderCapacityRequest::new(
            "session-a",
            RunId::new(),
            AttemptId::new(),
            policy.group.clone(),
            window(),
            100,
            59_999,
            70_000,
        )
        .unwrap_err(),
        "provider_capacity_clock_rollback"
    );

    // Malformed policies and requests are refused before any queue/usage state forms.
    assert_eq!(
        ProviderCapacityPolicy::new(policy.group.clone(), 0, 2, 10, 1_000, 30_000, "cfg.v1")
            .unwrap_err(),
        "provider_capacity_policy_invalid"
    );
    assert_eq!(
        ProviderCapacityPolicy::new(policy.group.clone(), 1, 2, 0, 1_000, 30_000, "cfg.v1")
            .unwrap_err(),
        "provider_capacity_policy_invalid"
    );
    for (tokens, deadline) in [(0_u64, 70_000_u64), (100, 60_001)] {
        assert_eq!(
            ProviderCapacityRequest::new(
                "session-a",
                RunId::new(),
                AttemptId::new(),
                policy.group.clone(),
                window(),
                tokens,
                60_001,
                deadline,
            )
            .unwrap_err(),
            "provider_capacity_request_invalid"
        );
    }
    let mut forged = ProviderCapacityRequest::new(
        "session-a",
        RunId::new(),
        AttemptId::new(),
        policy.group.clone(),
        window(),
        100,
        60_001,
        70_000,
    )
    .expect("request");
    forged.requested_tokens = 5_000;
    assert_eq!(
        controller.admit(forged).unwrap_err(),
        "provider_capacity_request_invalid"
    );
    assert_eq!(controller.queue_len(), 0);
    assert_eq!(controller.active_len(), 0);
    assert_eq!(controller.usage().admitted_requests, 0);
}

#[test]
fn queue_is_bounded_fair_and_refuses_duplicate_or_unknown_entries() {
    let policy = policy(1, 2, 10, 1_000);
    let mut queue = ProviderCapacityQueue::new(policy.clone()).expect("queue");
    let first = entry("session-a", 100, 60_001, 70_000);
    let first_attempt = first.attempt_id;
    assert_eq!(queue.enqueue(first.clone()).expect("ticket"), 1);
    assert_eq!(
        queue.enqueue(first.clone()).unwrap_err(),
        "provider_capacity_queue_duplicate_attempt"
    );
    let second = entry("session-b", 100, 60_002, 70_000);
    let second_attempt = second.attempt_id;
    assert_eq!(queue.enqueue(second).expect("ticket"), 2);
    assert_eq!(
        queue
            .enqueue(entry("session-c", 100, 60_003, 70_000))
            .unwrap_err(),
        "provider_capacity_queue_full"
    );
    // Unknown cancellation is a no-op and the bounded queue keeps no phantom entries.
    assert!(!queue.cancel(AttemptId::new()));
    assert_eq!(queue.len(), 2);
    // Round-robin: one entry per session, so each session takes exactly one turn.
    let dequeued_first = queue.dequeue().expect("first");
    assert_eq!(dequeued_first.attempt_id, first_attempt);
    let dequeued_second = queue.dequeue().expect("second");
    assert_eq!(dequeued_second.attempt_id, second_attempt);
    assert!(queue.dequeue().is_none());
    assert!(queue.is_empty());

    // Entries are deny-first: zero tokens, an already-elapsed deadline and a forged digest cannot
    // enter the queue.
    assert_eq!(
        ProviderCapacityQueueEntry::new(
            "session-a",
            RunId::new(),
            AttemptId::new(),
            0,
            60_001,
            70_000
        )
        .unwrap_err(),
        "provider_capacity_queue_entry_invalid"
    );
    assert_eq!(
        ProviderCapacityQueueEntry::new(
            "session-a",
            RunId::new(),
            AttemptId::new(),
            100,
            60_001,
            60_001
        )
        .unwrap_err(),
        "provider_capacity_queue_entry_invalid"
    );
    let mut forged = entry("session-a", 100, 60_001, 70_000);
    forged.requested_tokens = 0;
    assert_eq!(
        forged.validate(false).unwrap_err(),
        "provider_capacity_queue_entry_invalid"
    );
}

#[test]
fn cancelled_entry_is_never_dispatched_and_active_lease_is_not_cancellable() {
    let policy = policy(1, 4, 10, 1_000);
    let mut controller =
        ProviderCapacityController::new(policy.clone(), window()).expect("controller");
    let active = controller
        .admit(request(&policy, "owner", 60_001))
        .expect("accept");
    let active_lease = active.lease.clone().expect("lease");
    let queued_b = controller
        .admit(request(&policy, "session-b", 60_002))
        .expect("queue");
    let queued_c = controller
        .admit(request(&policy, "session-c", 60_003))
        .expect("queue");
    assert_eq!(controller.queue_len(), 2);

    // The active lease is not a queue entry: cancellation goes through the ControlPlane path and
    // must not silently recycle an occupied slot here.
    assert!(!controller.cancel(active_lease.attempt_id));
    assert_eq!(controller.active_len(), 1);
    assert!(controller.cancel(queued_b.outcome.attempt_id));
    assert!(!controller.cancel(queued_b.outcome.attempt_id));
    assert_eq!(controller.queue_len(), 1);

    controller
        .release(active_lease.lease_id, "owner")
        .expect("release active");
    let dispatched = controller
        .dispatch_next(60_010)
        .expect("dispatch")
        .expect("queued decision");
    assert_eq!(dispatched.outcome.attempt_id, queued_c.outcome.attempt_id);
    assert_ne!(dispatched.outcome.attempt_id, queued_b.outcome.attempt_id);
    controller
        .release(dispatched.lease.expect("lease").lease_id, "session-c")
        .expect("release dispatched");
    assert!(controller
        .dispatch_next(60_011)
        .expect("dispatch")
        .is_none());
}

#[test]
fn dispatch_backpressure_delays_without_holding_a_slot_or_losing_the_entry() {
    let policy = policy(1, 2, 1, 1_000);
    let mut controller =
        ProviderCapacityController::new(policy.clone(), window()).expect("controller");
    let accepted = controller
        .admit(request(&policy, "session-a", 60_001))
        .expect("accept");
    let queued = controller
        .admit(request(&policy, "session-b", 60_002))
        .expect("queue");
    assert_eq!(controller.usage().admitted_requests, 1);
    controller
        .release(accepted.lease.expect("lease").lease_id, "session-a")
        .expect("release");

    let delayed = controller
        .dispatch_next(60_004)
        .expect("dispatch")
        .expect("delayed decision");
    assert_eq!(delayed.outcome.kind, ProviderCapacityOutcomeKind::Delay);
    assert_eq!(
        delayed.outcome.reason.as_deref(),
        Some("provider_capacity_quota_window_exhausted")
    );
    assert_eq!(delayed.outcome.retry_after_ms, Some(120_000 - 60_004));
    assert!(delayed.lease.is_none());
    // Backpressure occupies no active slot and spends no quota; the entry stays queued.
    assert_eq!(controller.active_len(), 0);
    assert_eq!(controller.queue_len(), 1);
    assert_eq!(controller.usage().admitted_requests, 1);
    assert_eq!(controller.usage().admitted_tokens, 100);
    assert_eq!(controller.usage().active_concurrency, 0);

    // After the UTC window rolls, the same attempt is admitted against the next window; the delay
    // did not mutate the original policy/window binding or extend the closed window.
    let next_window = QuotaWindow::from_clock(&clock_at(120_050), 60_000).expect("next window");
    let mut next =
        ProviderCapacityController::new(policy.clone(), next_window.clone()).expect("next");
    let resumed = ProviderCapacityRequest::new(
        "session-b",
        RunId::new(),
        queued.outcome.attempt_id,
        policy.group.clone(),
        next_window,
        100,
        120_050,
        130_000,
    )
    .expect("resumed request");
    let decision = next.admit(resumed).expect("accept after roll");
    assert_eq!(decision.outcome.kind, ProviderCapacityOutcomeKind::Accept);
    assert_eq!(decision.outcome.attempt_id, queued.outcome.attempt_id);
}

#[test]
fn queue_wait_and_tickets_stay_observable_per_session() {
    let policy = policy(1, 4, 10, 1_000);
    let mut controller =
        ProviderCapacityController::new(policy.clone(), window()).expect("controller");
    let active = controller
        .admit(request(&policy, "session-a", 60_001))
        .expect("accept");
    let b = controller
        .admit(request(&policy, "session-b", 60_002))
        .expect("queue");
    let c = controller
        .admit(request(&policy, "session-c", 60_003))
        .expect("queue");
    assert_eq!(b.outcome.queue_ticket, Some(1));
    assert_eq!(c.outcome.queue_ticket, Some(2));
    assert!(b.outcome.queue_wait_ms.is_none());
    assert!(c.outcome.queue_wait_ms.is_none());

    controller
        .release(active.lease.expect("lease").lease_id, "session-a")
        .expect("release");
    let b_dispatched = controller
        .dispatch_next(60_010)
        .expect("dispatch")
        .expect("b decision");
    assert_eq!(b_dispatched.outcome.attempt_id, b.outcome.attempt_id);
    assert_eq!(b_dispatched.outcome.queue_wait_ms, Some(8));
    controller
        .release(b_dispatched.lease.expect("lease").lease_id, "session-b")
        .expect("release b");
    let c_dispatched = controller
        .dispatch_next(60_020)
        .expect("dispatch")
        .expect("c decision");
    assert_eq!(c_dispatched.outcome.attempt_id, c.outcome.attempt_id);
    assert_eq!(c_dispatched.outcome.queue_wait_ms, Some(17));
    controller
        .release(c_dispatched.lease.expect("lease").lease_id, "session-c")
        .expect("release c");
    assert!(controller
        .dispatch_next(60_021)
        .expect("dispatch")
        .is_none());
    assert_eq!(controller.usage().admitted_requests, 3);
}

#[test]
fn lease_release_is_owner_scoped_and_usage_rejects_leaseless_release() {
    let policy = policy(1, 2, 10, 1_000);
    let mut controller =
        ProviderCapacityController::new(policy.clone(), window()).expect("controller");
    let decision = controller
        .admit(request(&policy, "session-a", 60_001))
        .expect("accept");
    let lease = decision.lease.clone().expect("lease");
    assert_eq!(
        controller
            .release(lease.lease_id + 1, "session-a")
            .unwrap_err(),
        "provider_capacity_lease_unknown"
    );
    assert_eq!(
        controller.release(lease.lease_id, "session-b").unwrap_err(),
        "provider_capacity_lease_owner_mismatch"
    );
    assert_eq!(controller.active_len(), 1);
    assert_eq!(controller.usage().active_concurrency, 1);
    controller
        .release(lease.lease_id, "session-a")
        .expect("owner release");
    assert_eq!(controller.active_len(), 0);
    assert_eq!(controller.usage().active_concurrency, 0);
    assert_eq!(
        controller.release(lease.lease_id, "session-a").unwrap_err(),
        "provider_capacity_lease_unknown"
    );

    // A replayed/forged lease cannot release anything, even when its own digest is recomputed.
    let mut forged = lease.clone();
    forged.owner = "session-b".to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "provider_capacity_lease_invalid"
    );
    forged.lease_digest = forged.digest();
    assert_eq!(forged.validate(), Ok(()));
    assert_eq!(
        controller
            .release(forged.lease_id, "session-b")
            .unwrap_err(),
        "provider_capacity_lease_unknown"
    );
    let mut zero_id = lease.clone();
    zero_id.lease_id = 0;
    zero_id.lease_digest = zero_id.digest();
    assert_eq!(
        zero_id.validate().unwrap_err(),
        "provider_capacity_lease_invalid"
    );

    // Usage counters refuse admission without tokens and release without a permit.
    let mut usage = controller.usage().clone();
    assert_eq!(
        usage.can_admit(&policy, 0).unwrap_err(),
        "provider_capacity_tokens_invalid"
    );
    assert_eq!(
        usage.release().unwrap_err(),
        "provider_capacity_release_without_permit"
    );
}
