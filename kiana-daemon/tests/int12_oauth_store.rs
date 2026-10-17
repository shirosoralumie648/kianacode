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

#[tokio::test]
async fn store_is_insert_or_idempotent_and_refuses_non_cas_replacement() {
    let store = InMemoryOAuthAccountStore::new();
    let initial = account();
    let id = initial.account_id;
    assert!(matches!(
        store.upsert_oauth_account(initial.clone(), Some(1)).await,
        Err(PortError::Conflict(reason)) if reason == "oauth_account_expected_missing"
    ));
    assert!(store.read_oauth_account(id).await.expect("read").is_none());
    let stored = store
        .upsert_oauth_account(initial.clone(), None)
        .await
        .expect("insert");
    assert_eq!(stored, initial);
    let replayed = store
        .upsert_oauth_account(initial.clone(), None)
        .await
        .expect("idempotent replay");
    assert_eq!(replayed, initial);

    let rotated = initial
        .rotate(
            initial.revision,
            initial.credential_generation,
            next_metadata(),
            1_001,
        )
        .expect("rotation fixture");
    assert!(matches!(
        store
            .upsert_oauth_account(rotated.clone(), Some(initial.revision))
            .await,
        Err(PortError::Conflict(reason)) if reason == "oauth_account_mutation_requires_cas"
    ));
    assert!(matches!(
        store
            .upsert_oauth_account(rotated, Some(2))
            .await,
        Err(PortError::Conflict(reason)) if reason == "oauth_account_revision_conflict"
    ));
    assert_eq!(
        store
            .read_oauth_account(id)
            .await
            .expect("read")
            .expect("record"),
        initial
    );
}

#[tokio::test]
async fn stale_lifecycle_fences_never_revive_an_old_generation() {
    let store = InMemoryOAuthAccountStore::new();
    let initial = store
        .upsert_oauth_account(account(), None)
        .await
        .expect("insert");
    let id = initial.account_id;
    let reauth = store
        .require_oauth_reauth(id, initial.revision, initial.credential_generation, 1_002)
        .await
        .expect("reauth");
    assert!(matches!(
        store
            .rotate_oauth_account(
                id,
                initial.revision,
                initial.credential_generation,
                next_metadata(),
                1_003,
            )
            .await,
        Err(PortError::Conflict(_))
    ));
    assert!(matches!(
        store
            .revoke_oauth_account(id, initial.revision, initial.credential_generation, 1_004)
            .await,
        Err(PortError::Conflict(_))
    ));
    assert_eq!(
        store
            .read_oauth_account(id)
            .await
            .expect("read")
            .expect("record"),
        reauth
    );
    assert!(matches!(
        store
            .rotate_oauth_account(
                id,
                reauth.revision,
                reauth.credential_generation,
                next_metadata(),
                1_005,
            )
            .await,
        Err(PortError::Conflict(reason)) if reason == "oauth_account_status_fenced"
    ));
    let revoked = store
        .revoke_oauth_account(id, reauth.revision, reauth.credential_generation, 1_006)
        .await
        .expect("revoke");
    assert_eq!(revoked.status, OAuthAccountStatus::Revoked);
    assert!(matches!(
        store
            .require_oauth_reauth(id, revoked.revision, revoked.credential_generation, 1_007)
            .await,
        Err(PortError::Conflict(reason)) if reason == "oauth_account_status_fenced"
    ));
    assert_eq!(
        store
            .read_oauth_account(id)
            .await
            .expect("read")
            .expect("record"),
        revoked
    );
}
