//! NM-20 source guard: the notification-chain fault harness must stay a driving, effect-free
//! classifier over existing seams, and both fixture targets plus the manual lane must be bound.
//!
//! This guard is a source check: it fails if the harness grows a second execution path, an
//! effectful call, a protected-record shed, an "unavailable as empty" query, or a case list that
//! no longer matches the card.

#[test]
fn notification_fault_harness_stays_effect_free_and_card_complete() {
    let harness = include_str!("../src/notification_fault_harness.rs");
    let support = include_str!("support/nm20_chain_seam.rs");
    let daemon = include_str!("../../kiana-daemon/tests/nm20_notification_channel_faults.rs");

    for marker in [
        "Nm20FaultFamily::Duplicate",
        "Nm20FaultFamily::OutOfOrder",
        "Nm20FaultFamily::CursorGap",
        "Nm20FaultFamily::CrashWindow",
        "Nm20FaultFamily::SlowConsumer",
        "Nm20FaultFamily::QueueFull",
        "Nm20FaultFamily::DiskFull",
        "Nm20FaultFamily::SecretSentinel",
        "Nm20FaultFamily::ProjectionLoss",
        "nm20_adapter_seam_required",
        "evaluate_with_seam",
        "evaluate_with_default",
        "missing_cases",
        "from_observations",
    ] {
        assert!(
            harness.contains(marker),
            "NM-20 harness marker missing: {marker}"
        );
    }

    for case in [
        "DuplicateReplay",
        "CursorRegression",
        "CursorGap",
        "LeaseReclaimFence",
        "DiskFullWriteRefusal",
        "SecretSentinel",
        "ProjectionLoss",
        "CriticalQueryFallback",
        "CrashAfterClaim",
        "BoundedQueueShed",
        "SlowSubscriber",
        "MultiSubscriberIsolation",
    ] {
        assert!(
            harness.contains(case),
            "NM-20 card case missing from the harness: {case}"
        );
    }

    // Every refusal/invariant code the card relies on is named in source, so a silent rename
    // breaks this guard instead of weakening a fixture.
    for refusal in [
        "notification_projection_duplicate_suppressed",
        "projection_checkpoint_cursor_not_advanced",
        "projection_checkpoint_cursor_gap",
        "notification_outbox_lease_fence_mismatch",
        "notification_outbox_lease_expired",
        "platform_disk_full",
        "secret_sentinel_quarantined",
        "notification_projection_rebuild_equivalent",
        "notification_query_fallback_served",
        "notification_outbox_queue_full_shed_counted",
        "notification_outbox_capacity_exceeded",
    ] {
        assert!(
            harness.contains(refusal) || support.contains(refusal) || daemon.contains(refusal),
            "NM-20 refusal code missing: {refusal}"
        );
    }

    // No effect, no second execution loop, no direct broker/provider/network/file side effect.
    for forbidden in [
        "tokio::spawn",
        "std::process::Command",
        "reqwest::Client",
        "std::fs::write",
        "std::net::TcpStream",
        "CapabilityBroker",
        "KianaHarness",
        "send(",
        "kill(",
        "write_event(",
        ".await",
    ] {
        assert!(
            !harness.contains(forbidden),
            "NM-20 harness widened the execution surface: {forbidden}"
        );
    }

    // A bounded queue may never shed a protected record and an unavailable projection may never
    // be reported as an empty inbox.
    assert!(harness.contains("critical_preserved"));
    assert!(harness.contains("effect_started: false"));
    assert!(harness.contains("secret_free: true"));
    assert!(
        support.contains("protected_shed") || support.contains(".protected()"),
        "NM-20 shed fixture must assert protected records are never shed"
    );
    assert!(harness.contains("NotificationStoreError::ProjectionUnavailable"));

    // The daemon target drives the two channel/UI cases that this crate cannot reach.
    for marker in [
        "SlowSubscriber",
        "MultiSubscriberIsolation",
        "NotificationStreamBridge",
        "terminal_query_required",
    ] {
        assert!(
            daemon.contains(marker),
            "NM-20 daemon channel fixture missing: {marker}"
        );
    }
}

#[test]
fn notification_fault_harness_targets_are_bound_to_ci() {
    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "nm20_notification_faults",
        "nm20_notification_faults_guard",
        "nm20_notification_fault_harness",
        "nm20_notification_fault_harness_guard",
    ] {
        assert!(
            shards.contains(target),
            "NM-20 target not bound to a CI shard: {target}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm20-notification-faults.yml");
    for marker in [
        "workflow_dispatch",
        "permissions:\n  contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test nm20_notification_fault_harness --locked -- --test-threads=1",
        "cargo test -p kiana-core --test nm20_notification_fault_harness_guard --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test nm20_notification_channel_faults --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test nm20_notification_channel_faults_guard --locked -- --test-threads=1",
    ] {
        assert!(
            lane.contains(marker),
            "NM-20 manual lane marker missing: {marker}"
        );
    }
}
