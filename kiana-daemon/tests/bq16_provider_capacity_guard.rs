//! BQ-16 daemon guard: the daemon owns scheduling and delegation, never provider capacity
//! admission or provider dispatch.

const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq16-provider-capacity.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn daemon_does_not_create_a_provider_capacity_execution_loop() {
    let daemon = include_str!("../src/lib.rs");
    let model_client = include_str!("../src/model_client.rs");
    for marker in ["DaemonHost", "ControlPlane", "ProviderGateway"] {
        assert!(
            daemon.contains(marker) || model_client.contains(marker),
            "daemon capacity boundary marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn(provider",
        "ProviderCapacityController::new",
        "send_inner",
    ] {
        assert!(
            !daemon.contains(forbidden),
            "daemon must not own provider dispatch: {forbidden}"
        );
    }
    for forbidden in [
        "ProviderCapacityPolicy::new",
        "CapacityWindow",
        "try_acquire_owned",
        "provider_capacity_",
    ] {
        assert!(
            !daemon.contains(forbidden) && !model_client.contains(forbidden),
            "daemon must not own provider capacity: {forbidden}"
        );
    }
}

#[test]
fn daemon_capacity_guard_stays_wired_to_the_manual_lane_and_the_whole_crate_shard() {
    assert!(
        MANUAL_LANE.contains(
            "cargo test -p kiana-daemon --test bq16_provider_capacity_guard --locked -- --test-threads=1"
        ),
        "BQ-16 manual lane must run the daemon guard"
    );
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-16 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    assert!(
        shards["kiana-daemon"]["targets"].is_null(),
        "daemon stays a whole-crate shard so the guard is discovered automatically"
    );
}
