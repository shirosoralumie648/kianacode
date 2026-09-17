use kiana_domain::{
    canonical_scopes, json_digest, OAuthAuthorizationRequest, OAuthCallback, OAuthSubject,
    OAuthTokenMetadata, OAuthTokenStatus, RequestId,
};

fn scopes() -> Vec<String> {
    vec!["model.read".to_owned(), "model.use".to_owned()]
}

#[test]
fn oauth_flow_contract_binds_state_pkce_redirect_and_subject() {
    let flow = OAuthAuthorizationRequest::new(
        RequestId::new(),
        OAuthSubject::Workload,
        "ci09-client",
        "https://idp.invalid/authorize",
        "https://app.invalid/callback",
        scopes(),
        json_digest(&serde_json::json!("ci09-state")),
        "A".repeat(43),
        1_000,
        600_000,
    )
    .expect("flow");
    flow.validate_at(1_001).expect("flow validity");
    let callback = OAuthCallback::new(
        flow.flow_id,
        "ci09-state",
        "authorization-code",
        flow.redirect_uri.clone(),
    )
    .expect("callback");
    callback.validate().expect("callback validity");
    let encoded = serde_json::to_string(&flow).expect("flow json");
    assert!(!encoded.contains("access_token"));
    assert!(!encoded.contains("refresh_token"));

    let mut unknown = serde_json::to_value(callback).expect("callback value");
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<OAuthCallback>(unknown).is_err());
}

#[test]
fn token_metadata_uses_generation_cas_expiry_status_and_opaque_digests() {
    assert!(canonical_scopes(scopes()).is_ok());
    assert!(canonical_scopes(vec!["model.use".to_owned(), "model.use".to_owned()]).is_err());
    let access_digest = json_digest(&serde_json::json!("CI09_ACCESS_SENTINEL"));
    let refresh_digest = json_digest(&serde_json::json!("CI09_REFRESH_SENTINEL"));
    let metadata = OAuthTokenMetadata::new(
        "fake-account",
        OAuthSubject::User,
        1,
        scopes(),
        1_000,
        3_600_000,
        access_digest,
        Some(refresh_digest),
    )
    .expect("metadata");
    assert!(metadata.needs_refresh(1_000, 3_000_000));
    let next = metadata
        .rotate(
            1,
            2_000,
            3_602_000,
            json_digest(&serde_json::json!("new-access")),
            Some(json_digest(&serde_json::json!("new-refresh"))),
            scopes(),
        )
        .expect("rotation");
    assert_eq!(next.generation, 2);
    assert_eq!(
        metadata
            .rotate(
                0,
                2_000,
                3_602_000,
                json_digest(&serde_json::json!("stale")),
                None,
                scopes(),
            )
            .unwrap_err(),
        "oauth_generation_conflict"
    );
    let revoked = next.revoke().expect("revoke");
    assert_eq!(revoked.status, OAuthTokenStatus::Revoked);
    assert_eq!(revoked.generation, 3);
    let encoded = serde_json::to_string(&revoked).expect("metadata json");
    assert!(!encoded.contains("CI09_ACCESS_SENTINEL"));
    assert!(!encoded.contains("CI09_REFRESH_SENTINEL"));
}
