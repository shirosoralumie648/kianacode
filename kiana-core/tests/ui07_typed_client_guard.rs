#[test]
fn ui07_typed_client_keeps_control_plane_as_effect_authority() {
    let client = include_str!("../../kiana-client/src/typed.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "QueryClient",
        "FeedClient",
        "ActionClient",
        "ArtifactClient",
        "ClientRequestOptions",
        "deadline_unix_ms",
        "CancellationToken",
        "UI_ACTION_STATUS_OPERATION",
        "CommandRetryForbidden",
        "LateResponse",
        "ui_history_instance_mismatch",
        "ui_history_epoch_mismatch",
        "Accepted is intentionally returned as Accepted",
        "RequestEnvelope::command",
    ] {
        assert!(
            client.contains(marker) || protocol.contains(marker),
            "UI-07 typed-client marker missing: {marker}"
        );
    }
    assert!(!client.contains("CapabilityBroker"));
    assert!(!client.contains("ModelClient"));
    assert!(!client.contains("tokio::spawn"));
}
