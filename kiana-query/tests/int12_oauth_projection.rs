use kiana_domain::*;
use kiana_query::project_oauth_accounts;

fn record() -> OAuthAccountRecord {
    let scopes = vec!["account.read".to_owned()];
    let secret_ref = SecretRef::new(
        "keyring",
        "oauth/int12",
        "connector.oauth",
        "provider:provider",
        1,
    )
    .expect("secret ref");
    let metadata = OAuthTokenMetadata::new(
        "account-1",
        OAuthSubject::Workload,
        1,
        scopes.clone(),
        1_000,
        60_000,
        json_digest(&serde_json::json!("access")),
        Some(json_digest(&serde_json::json!("refresh"))),
    )
    .expect("metadata");
    OAuthAccountRecord::new(
        ProviderAccountId::new(),
        "provider",
        "account-1",
        OAuthSubject::Workload,
        "http://localhost:43992/callback",
        scopes,
        &secret_ref,
        metadata,
    )
    .expect("record")
}

#[test]
fn query_projection_is_deterministic_and_rejects_invalid_cursor() {
    let record = record();
    let page = project_oauth_accounts(vec![record.clone(), record], 1_001, 1).expect("page");
    assert_eq!(page.accounts.len(), 1);
    assert!(!page.stale);
    assert!(project_oauth_accounts(Vec::new(), 1_001, 0).is_err());
}
