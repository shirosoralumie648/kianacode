#[test]
fn swarm_queue_keeps_ready_query_separate_from_effect_execution() {
    let domain = include_str!("../../kiana-domain/src/swarm_queue.rs");
    let core = include_str!("../src/swarm_queue.rs");
    for marker in [
        "SwarmQueueLedger",
        "SwarmQueueEntry",
        "capacity",
        "session_key",
        "worker_epoch",
        "not_before_unix_ms",
        "swarm_queue_stale_fence",
        "swarm_queue_backoff_invalid",
        "claim_swarm_entry",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-06 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "KianaHarness",
        "std::process::Command",
        "tokio::spawn",
        "EventStore",
        "execute_effect",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-06 queue boundary must not execute effects: {forbidden}"
        );
    }
}
