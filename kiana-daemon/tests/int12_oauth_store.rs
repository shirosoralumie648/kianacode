use kiana_daemon::InMemoryOAuthAccountStore;
use kiana_domain::*;
use kiana_ports::{OAuthAccountStore, PortError};

fn account() -> OAuthAccountRecord {
    let scopes = vec!["account.read".to_owned(), "model.use".to_owned()];
    let secret_ref = SecretRef::new(
        "keyring",
        "oauth/int12-daemon",
        OAUTH_ACCOUNT_SECRET_PURPOSE,
        &oauth_account_audience("provider-demo"),
        1,
    )
    .expect("secret ref");
    let metadata = OAuthTokenMetadata::new(
        "account-1",
        OAuthSubject::Workload,
        1,
        scopes.clone(),
        1_000,
        61_000,
        json_digest(&serde_json::json!("access")),
        Some(json_digest(&serde_json::json!("refresh"))),
    )
    .expect("metadata");
    OAuthAccountRecord::new(
        ProviderAccountId::new(),
        "provider-demo",
        "account-1",
        OAuthSubject::Workload,
        "http://127.0.0.1:43993/callback",
        scopes,
        &secret_ref,
        metadata,
    )
    .expect("account")
}

fn next_metadata() -> OAuthTokenMetadata {
    OAuthTokenMetadata::new(
        "account-1",
        OAuthSubject::Workload,
        2,
        vec!["account.read".to_owned(), "model.use".to_owned()],
        1_001,
        62_000,
        json_digest(&serde_json::json!("next-access")),
        Some(json_digest(&serde_json::json!("next-refresh"))),
    )
    .expect("metadata")
}

#[tokio::test]
async fn store_cas_fences_old_refresh_and_reauth() {
    let store = InMemoryOAuthAccountStore::new();
    let initial = account();
    let id = initial.account_id;
    let initial = store
        .upsert_oauth_account(initial, None)
        .await
        .expect("insert");
    let rotated = store
        .rotate_oauth_account(
            id,
            initial.revision,
            initial.credential_generation,
            next_metadata(),
            1_001,
        )
        .await
        .expect("rotate");
    assert_eq!(rotated.credential_generation, 2);
    assert!(matches!(
        store
            .rotate_oauth_account(
                id,
                initial.revision,
                initial.credential_generation,
                rotated.token_metadata.clone(),
                1_002,
            )
            .await,
        Err(PortError::Conflict(_))
    ));
    let reauth = store
        .require_oauth_reauth(id, rotated.revision, rotated.credential_generation, 1_003)
        .await
        .expect("reauth");
    assert_eq!(reauth.status, OAuthAccountStatus::ReauthRequired);
    assert!(reauth.credential_generation > rotated.credential_generation);
    let revoked = store
        .revoke_oauth_account(id, reauth.revision, reauth.credential_generation, 1_004)
        .await
        .expect("revoke");
    assert_eq!(revoked.status, OAuthAccountStatus::Revoked);
}
