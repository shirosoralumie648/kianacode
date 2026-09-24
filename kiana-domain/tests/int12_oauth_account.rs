use kiana_domain::*;

const NOW: u64 = 10_000;

fn scopes() -> Vec<String> {
    vec!["account.read".to_owned(), "model.use".to_owned()]
}

fn account() -> OAuthAccountRecord {
    let secret_ref = SecretRef::new(
        "keyring",
        "oauth/account-1",
        "connector.oauth",
        "provider:provider-demo",
        7,
    )
    .expect("secret ref");
    let metadata = OAuthTokenMetadata::new(
        "provider-account-1",
        OAuthSubject::User,
        7,
        scopes(),
        NOW,
        NOW + 60_000,
        json_digest(&serde_json::json!("access-sentinel")),
        Some(json_digest(&serde_json::json!("refresh-sentinel"))),
    )
    .expect("metadata");
    OAuthAccountRecord::new(
        ProviderAccountId::new(),
        "provider-demo",
        "provider-account-1",
        OAuthSubject::User,
        "http://127.0.0.1:43991/callback",
        scopes(),
        &secret_ref,
        metadata,
    )
    .expect("account")
}

#[test]
fn account_record_is_secret_free_and_projects_only_metadata() {
    let record = account();
    record.validate_at(NOW + 1).expect("valid account");
    let projection = OAuthAccountProjection::from_record(&record, NOW + 1).expect("projection");
    let encoded = serde_json::to_string(&projection).expect("projection json");
    assert!(!encoded.contains("access_token"));
    assert!(!encoded.contains("refresh_token"));
    assert!(!encoded.contains("oauth/account-1"));
    assert!(!encoded.contains("access-sentinel"));
    assert!(!encoded.contains("refresh-sentinel"));
    assert_eq!(projection.credential_generation, 7);
    assert_eq!(projection.status, OAuthAccountStatus::Active);
}

#[test]
fn rotation_rejects_scope_downgrade_and_stale_generation() {
    let record = account();
    let downgraded = OAuthTokenMetadata::new(
        "provider-account-1",
        OAuthSubject::User,
        8,
        vec!["account.read".to_owned()],
        NOW + 1,
        NOW + 60_001,
        json_digest(&serde_json::json!("next-access")),
        Some(json_digest(&serde_json::json!("next-refresh"))),
    )
    .expect("metadata");
    assert_eq!(
        record
            .rotate(
                record.revision,
                record.credential_generation,
                downgraded,
                NOW + 1
            )
            .unwrap_err(),
        "oauth_scope_downgrade"
    );

    let next_metadata = OAuthTokenMetadata::new(
        "provider-account-1",
        OAuthSubject::User,
        8,
        scopes(),
        NOW + 1,
        NOW + 60_001,
        json_digest(&serde_json::json!("next-access")),
        Some(json_digest(&serde_json::json!("next-refresh"))),
    )
    .expect("metadata");
    let next = record
        .rotate(
            record.revision,
            record.credential_generation,
            next_metadata,
            NOW + 1,
        )
        .expect("rotation");
    assert_eq!(next.revision, 2);
    assert_eq!(next.credential_generation, 8);
    assert_eq!(
        record
            .rotate(
                record.revision,
                next.credential_generation,
                next.token_metadata.clone(),
                NOW + 1
            )
            .unwrap_err(),
        "oauth_account_generation_conflict"
    );
}

#[test]
fn reauth_and_revoke_advance_fences_and_unknown_fields_deny() {
    let record = account();
    let reauth = record
        .require_reauth(record.revision, record.credential_generation, NOW + 1)
        .expect("reauth");
    assert_eq!(reauth.status, OAuthAccountStatus::ReauthRequired);
    assert!(reauth.credential_generation > record.credential_generation);
    assert_eq!(
        reauth
            .rotate(
                reauth.revision,
                reauth.credential_generation,
                record.token_metadata.clone(),
                NOW + 1,
            )
            .unwrap_err(),
        "oauth_account_status_fenced"
    );

    let revoked = record
        .revoke(record.revision, record.credential_generation, NOW + 1)
        .expect("revoked");
    assert_eq!(revoked.status, OAuthAccountStatus::Revoked);
    assert!(revoked.credential_generation > record.credential_generation);

    let mut unknown = serde_json::to_value(record).expect("record json");
    unknown["raw_token"] = serde_json::json!("INT12_RAW_TOKEN_SENTINEL");
    assert!(serde_json::from_value::<OAuthAccountRecord>(unknown).is_err());
}
