#[test]
fn notification_fault_matrix_is_replay_only_and_has_bounded_safety_invariants() {
    let core = include_str!("../src/notification_faults.rs");
    let domain = include_str!("../../kiana-domain/src/notification_faults.rs");
    for marker in [
        "notification_fault_matrix",
        "NotificationFaultScenario",
        "Duplicate",
        "CursorGap",
        "CrashAfterClaim",
        "SlowConsumer",
        "QueueFull",
        "DiskFull",
        "SecretSentinel",
        "ProjectionLoss",
        "critical_preserved",
        "effect_started",
        "secret_free",
    ] {
        assert!(
            core.contains(marker) || domain.contains(marker),
            "NM-20 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "NotificationDeliveryWorker",
        "tokio::spawn",
        "std::process::Command",
        "reqwest::Client",
        "send(",
        "kill(",
        "write_event(",
    ] {
        assert!(
            !core.contains(forbidden),
            "NM-20 core fault path widened: {forbidden}"
        );
        assert!(
            !domain.contains(forbidden),
            "NM-20 domain fault path widened: {forbidden}"
        );
    }
}
