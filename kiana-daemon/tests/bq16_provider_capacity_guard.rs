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
}
