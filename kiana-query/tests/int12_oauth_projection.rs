use kiana_domain::*;
use kiana_query::project_oauth_accounts;

fn record_with(
    account_id: ProviderAccountId,
    access: &str,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> OAuthAccountRecord {
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
        issued_at_unix_ms,
        expires_at_unix_ms,
        json_digest(&serde_json::json!(access)),
        Some(json_digest(&serde_json::json!("refresh"))),
    )
    .expect("metadata");
    OAuthAccountRecord::new(
        account_id,
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

fn record() -> OAuthAccountRecord {
    record_with(ProviderAccountId::new(), "access", 1_000, 60_000)
}

#[test]
fn query_projection_is_deterministic_and_rejects_invalid_cursor() {
    let record = record();
    let page = project_oauth_accounts(vec![record.clone(), record], 1_001, 1).expect("page");
    assert_eq!(page.accounts.len(), 1);
    assert!(!page.stale);
    assert_eq!(page.accounts[0].proof_level, "source");
    assert!(project_oauth_accounts(Vec::new(), 1_001, 0).is_err());
}

#[test]
fn query_projection_rejects_conflicting_snapshots_and_keeps_expired_rows_redacted() {
    let account_id = ProviderAccountId::new();
    let first = record_with(account_id, "access-one", 1_000, 60_000);
    let conflicting = record_with(account_id, "access-two", 1_000, 60_000);
    assert_eq!(
        project_oauth_accounts(vec![first.clone(), conflicting], 1_001, 7).unwrap_err(),
        "oauth_account_projection_conflict"
    );

    let page = project_oauth_accounts(vec![first.clone(), first], 60_000, 9).expect("stable page");
    assert_eq!(page.accounts.len(), 1);
    assert_eq!(page.accounts[0].status, OAuthAccountStatus::Expired);
    assert_eq!(page.source_cursor, 9);
    assert!(page
        .limitations
        .iter()
        .any(|limitation| limitation == "source_cursor_is_adapter_supplied"));
    let encoded = serde_json::to_string(&page).expect("page json");
    for forbidden in [
        "access-one",
        "access_token",
        "refresh_token",
        "oauth/int12",
        "keyring",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "projection leaked {forbidden}: {encoded}"
        );
    }
}
