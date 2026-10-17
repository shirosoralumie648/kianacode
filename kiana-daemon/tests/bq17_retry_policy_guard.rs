//! BQ-17 daemon guard: retry classification, Retry-After handling and per-attempt reservations
//! stay in the Runner; the daemon only composes `DaemonHost` and must not own a second retry
//! policy, retry loop or reservation ledger.

const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq17-retry-policy.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn daemon_does_not_own_retry_policy_or_attempt_reservations() {
    let daemon = include_str!("../src/lib.rs");
    let model_client = include_str!("../src/model_client.rs");
    for marker in ["DaemonHost", "ControlPlane", "ProviderGateway"] {
        assert!(
            daemon.contains(marker) || model_client.contains(marker),
            "BQ-17 daemon boundary marker missing: {marker}"
        );
    }
    for forbidden in [
        "RetryPolicy::new",
        "classify_retry",
        "RetryAttemptReservation",
        "retry_attempt_cancel_race_terminal",
        "retry_delay(",
        "tokio::spawn(retry",
    ] {
        assert!(
            !daemon.contains(forbidden) && !model_client.contains(forbidden),
            "daemon must not own retry policy/dispatch: {forbidden}"
        );
    }
}

#[test]
fn daemon_retry_guard_stays_wired_to_the_manual_lane_and_the_whole_crate_shard() {
    assert!(
        MANUAL_LANE.contains(
            "cargo test -p kiana-daemon --test bq17_retry_policy_guard --locked -- --test-threads=1"
        ),
        "BQ-17 manual lane must run the daemon guard"
    );
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-17 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    assert!(
        shards["kiana-daemon"]["targets"].is_null(),
        "daemon stays a whole-crate shard so the guard is discovered automatically"
    );
}
