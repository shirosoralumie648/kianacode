#[test]
fn provider_configuration_is_snapshotted_and_cassette_mode_is_explicit() {
    let provider = include_str!("../../kiana-provider/src/config.rs");
    let gateway = include_str!("../../kiana-provider/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/model_client.rs");
    let domain = include_str!("../../kiana-domain/src/provider_config.rs");
    for marker in [
        "configuration_revision",
        "model_profile_unknown",
        "model_streaming_policy_invalid",
        "inherit_default",
        "credential_revision",
    ] {
        assert!(
            provider.contains(marker),
            "provider config marker missing: {marker}"
        );
    }
    for marker in [
        "configuration_snapshot",
        "ProviderConfigSnapshot",
        "credential_ref",
    ] {
        assert!(
            gateway.contains(marker) || domain.contains(marker),
            "snapshot marker missing: {marker}"
        );
    }
    for marker in [
        "selection_mode_from_env",
        "model_selection_conflict",
        "cassette_required",
        "validate_streaming_environment",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon selection marker missing: {marker}"
        );
    }
}
