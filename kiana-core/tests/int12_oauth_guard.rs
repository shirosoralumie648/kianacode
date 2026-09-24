#[test]
fn int12_account_store_keeps_provider_secret_boundary_and_cas() {
    let domain = include_str!("../../kiana-domain/src/oauth_accounts.rs");
    let oauth_domain = include_str!("../../kiana-domain/src/oauth.rs");
    let ports = include_str!("../../kiana-ports/src/oauth_accounts.rs");
    let daemon = include_str!("../../kiana-daemon/src/oauth_accounts.rs");
    let query = include_str!("../../kiana-query/src/oauth_accounts.rs");
    let provider = include_str!("../../kiana-provider/src/oauth.rs");
    let provider_lib = include_str!("../../kiana-provider/src/lib.rs");

    for marker in [
        "OAuthAccountRecord",
        "OAuthAccountStatus",
        "secret_ref_digest",
        "redirect_uri_valid",
        "credential_generation",
        "oauth_account_generation_conflict",
        "oauth_account_status_fenced",
        "scope_downgrade",
        "require_reauth",
        "pub fn revoke",
        "pub fn reauth",
        "#[serde(deny_unknown_fields)]",
    ] {
        assert!(domain.contains(marker), "domain marker missing: {marker}");
    }
    for marker in [
        "pub trait OAuthAccountStore",
        "upsert_oauth_account",
        "rotate_oauth_account",
        "require_oauth_reauth",
        "revoke_oauth_account",
        "generation",
        "never returns access/refresh token material",
    ] {
        assert!(ports.contains(marker), "port marker missing: {marker}");
    }
    for marker in [
        "InMemoryOAuthAccountStore",
        "RwLock",
        "oauth_account_revision_conflict",
        "current.rotate(",
        "current.require_reauth(",
        "current.revoke(",
    ] {
        assert!(daemon.contains(marker), "daemon marker missing: {marker}");
    }
    for marker in [
        "OAuthAccountQueryPage",
        "project_oauth_accounts",
        "raw access/refresh material",
        "source_cursor",
        "stale",
    ] {
        assert!(query.contains(marker), "query marker missing: {marker}");
    }
    for marker in [
        "canonical_redirect_uri",
        "oauth_redirect_origin_not_loopback",
        "oauth_state_mismatch",
        "oauth_redirect_mismatch",
        "oauth_flow_replay",
        "consumed_code_digests",
        "oauth_code_replay",
        "refreshing: bool",
        "refresh_notify",
        "RefreshFlightGuard",
        "GenerationConflict",
        "RefreshFailure::Transient",
        "RefreshFailure::Permanent",
        "RefreshFailure::Revoked",
        "OAuthSubject::Workload",
    ] {
        assert!(
            provider.contains(marker),
            "provider marker missing: {marker}"
        );
    }
    assert!(provider_lib.contains("mod oauth;"));
    assert!(oauth_domain.contains("OAuthTokenMetadata"));

    for source in [domain, ports, daemon, query] {
        for forbidden in [
            "access_token: String",
            "refresh_token: Option<String>",
            "Authorization: Bearer",
        ] {
            assert!(
                !source.contains(forbidden),
                "raw credential marker crossed account boundary: {forbidden}"
            );
        }
    }
    assert!(!query.contains("SecretRef {"));
    assert!(!query.contains("access_token"));
    assert!(!query.contains("refresh_token"));
}
