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

#[test]
fn binding_drift_and_expired_status_fail_closed() {
    let baseline = account();
    let secret_ref = SecretRef::new(
        "keyring",
        "oauth/account-1",
        "connector.oauth",
        "provider:provider-demo",
        7,
    )
    .expect("secret ref");
    let metadata = || {
        OAuthTokenMetadata::new(
            "provider-account-1",
            OAuthSubject::User,
            7,
            scopes(),
            NOW,
            NOW + 60_000,
            json_digest(&serde_json::json!("access-sentinel")),
            Some(json_digest(&serde_json::json!("refresh-sentinel"))),
        )
        .expect("metadata")
    };

    let wrong_purpose = SecretRef::new(
        "keyring",
        "oauth/account-1",
        "connector.other",
        "provider:provider-demo",
        7,
    )
    .expect("secret ref");
    assert_eq!(
        OAuthAccountRecord::new(
            ProviderAccountId::new(),
            "provider-demo",
            "provider-account-1",
            OAuthSubject::User,
            "http://127.0.0.1:43991/callback",
            scopes(),
            &wrong_purpose,
            metadata(),
        )
        .unwrap_err(),
        "oauth_account_secret_ref_binding_mismatch"
    );

    let wrong_audience = SecretRef::new(
        "keyring",
        "oauth/account-1",
        "connector.oauth",
        "provider:provider-other",
        7,
    )
    .expect("secret ref");
    assert_eq!(
        OAuthAccountRecord::new(
            ProviderAccountId::new(),
            "provider-demo",
            "provider-account-1",
            OAuthSubject::User,
            "http://127.0.0.1:43991/callback",
            scopes(),
            &wrong_audience,
            metadata(),
        )
        .unwrap_err(),
        "oauth_account_secret_ref_binding_mismatch"
    );

    let foreign_provider = OAuthTokenMetadata::new(
        "provider-account-1",
        OAuthSubject::User,
        7,
        scopes(),
        NOW,
        NOW + 60_000,
        json_digest(&serde_json::json!("access-sentinel")),
        None,
    )
    .expect("metadata");
    assert_eq!(
        OAuthAccountRecord::new(
            ProviderAccountId::new(),
            "provider-demo",
            "provider-account-other",
            OAuthSubject::User,
            "http://127.0.0.1:43991/callback",
            scopes(),
            &secret_ref,
            foreign_provider,
        )
        .unwrap_err(),
        "oauth_account_provider_binding_mismatch"
    );

    let stale_generation = OAuthTokenMetadata::new(
        "provider-account-1",
        OAuthSubject::User,
        8,
        scopes(),
        NOW,
        NOW + 60_000,
        json_digest(&serde_json::json!("access-sentinel")),
        None,
    )
    .expect("metadata");
    assert_eq!(
        OAuthAccountRecord::new(
            ProviderAccountId::new(),
            "provider-demo",
            "provider-account-1",
            OAuthSubject::User,
            "http://127.0.0.1:43991/callback",
            scopes(),
            &secret_ref,
            stale_generation,
        )
        .unwrap_err(),
        "oauth_account_invalid"
    );

    for redirect in [
        "http://127.0.0.1:43991/callback?state=forbidden",
        "http://127.0.0.1:43991/callback#fragment",
        "https://user:pass@example.invalid/callback",
        "http://example.invalid/callback",
    ] {
        assert_eq!(
            OAuthAccountRecord::new(
                ProviderAccountId::new(),
                "provider-demo",
                "provider-account-1",
                OAuthSubject::User,
                redirect,
                scopes(),
                &secret_ref,
                metadata(),
            )
            .unwrap_err(),
            "oauth_account_invalid",
            "unsafe redirect must fail closed: {redirect}"
        );
    }

    let mut tampered = serde_json::to_value(&baseline).expect("record json");
    tampered["scopes"] = serde_json::json!(["account.read"]);
    let tampered: OAuthAccountRecord = serde_json::from_value(tampered).expect("record shape");
    assert_eq!(
        tampered.validate_at(NOW + 1).unwrap_err(),
        "oauth_account_invalid"
    );

    assert_eq!(baseline.effective_status(NOW), OAuthAccountStatus::Active);
    assert_eq!(
        baseline.effective_status(NOW + 60_000),
        OAuthAccountStatus::Expired
    );
    let expired =
        OAuthAccountProjection::from_record(&baseline, NOW + 60_000).expect("expired projection");
    assert_eq!(expired.status, OAuthAccountStatus::Expired);

    assert_eq!(
        baseline
            .revoke(
                baseline.revision - 1,
                baseline.credential_generation,
                NOW + 1
            )
            .unwrap_err(),
        "oauth_account_generation_conflict"
    );
    let reauth = baseline
        .require_reauth(baseline.revision, baseline.credential_generation, NOW + 1)
        .expect("reauth");
    assert_eq!(
        reauth
            .revoke(baseline.revision, baseline.credential_generation, NOW + 2)
            .unwrap_err(),
        "oauth_account_generation_conflict"
    );
    assert_eq!(reauth.status, OAuthAccountStatus::ReauthRequired);
    assert_eq!(reauth.account_digest, reauth.digest());
}
