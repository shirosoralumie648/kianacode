#[test]
fn oauth_workload_identity_lifecycle_is_provider_owned_and_fail_closed() {
    let domain = include_str!("../../kiana-domain/src/oauth.rs");
    let provider = include_str!("../../kiana-provider/src/oauth.rs");
    let provider_lib = include_str!("../../kiana-provider/src/lib.rs");
    let provider_manifest = include_str!("../../kiana-provider/Cargo.toml");
    for marker in [
        "OAuthAuthorizationRequest",
        "OAuthCallback",
        "OAuthTokenMetadata",
        "OAuthSubject",
        "OAuthTokenStatus",
        "oauth_generation_conflict",
        "OAuthSubject::Workload",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "domain OAuth marker missing: {marker}"
        );
    }
    assert!(!domain.contains("access_token: String"));
    assert!(!domain.contains("refresh_token: Option<String>"));
    for marker in [
        "pkce_challenge",
        "code_challenge_method",
        "oauth_state_mismatch",
        "oauth_redirect_mismatch",
        "oauth_flow_replay",
        "decode_token_response",
        "oauth_token_response_too_large",
        "oauth_scope_insufficient",
        "refreshing: bool",
        "refresh_notify",
        "refresh_cooldown_until",
        "GenerationConflict",
        "RefreshFailure::Transient",
        "RefreshFailure::Permanent",
        "RefreshFailure::Revoked",
        "create_new(true)",
        "sync_all",
        "rename",
        "0o600",
        "0o700",
        "symlink_metadata",
        "OAuthSubject::Workload",
    ] {
        assert!(
            provider.contains(marker),
            "provider OAuth marker missing: {marker}"
        );
    }
    assert!(provider_lib.contains("mod oauth;"));
    assert!(provider_lib.contains("ProviderGateway"));
    assert!(!provider_manifest.contains("kiana-services"));
}
