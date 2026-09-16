use kiana_domain::*;
use serde_json::json;

fn principal() -> Principal {
    let principal_id = PrincipalId::new();
    let mut authentication = AuthenticatedPrincipalRef::local();
    authentication.principal_id = principal_id.to_string();
    authentication.principal_digest = authentication.digest();
    Principal::new(principal_id, PrincipalKind::Human, authentication, 1).unwrap()
}

fn digest(value: &str) -> String {
    json_digest(&json!(value))
}

#[test]
fn ci02_domain_contracts_are_versioned_and_secret_free() {
    let principal = principal();
    principal.validate().unwrap();
    assert!(principal.active_at(2));

    let membership = Membership::new(
        MembershipId::new(),
        principal.principal_id,
        OrganizationId::new(),
        1,
        3,
    )
    .unwrap();
    membership.validate().unwrap();

    let secret = SecretRef::new("env", "OPENAI_API_KEY", "provider.request", "openai", 1).unwrap();
    secret.validate().unwrap();
    let encoded = serde_json::to_string(&secret).unwrap();
    assert!(!encoded.contains("sk-"));
    let mut forged = serde_json::to_value(&secret).unwrap();
    forged["raw_value"] = json!("sk-live-secret");
    assert!(serde_json::from_value::<SecretRef>(forged).is_err());

    let account =
        ProviderAccount::new(ProviderAccountId::new(), "openai", "project:project-1").unwrap();
    account.validate().unwrap();
    let config = ConfigSnapshot::new(
        vec!["builtin:provider".to_owned()],
        json!({"provider":"openai","api_key_env":"OPENAI_API_KEY"}),
        digest("config"),
        digest("trust"),
    )
    .unwrap();
    config.validate().unwrap();

    let authority = AuthoritySnapshot::new(
        principal.principal_id,
        membership.organization_id,
        ProjectId::new(),
        "local-user",
        ROLE_BUILDER,
        DEPARTMENT_EXECUTING,
        "default",
        "project:project-1",
        3,
        vec![AssignmentId::new()],
        digest("trust"),
    )
    .unwrap();
    authority.validate().unwrap();
    assert_eq!(
        authority.validate_current_epoch(2).unwrap_err(),
        "authority_epoch_rollback"
    );
    assert_eq!(
        authority.validate_current_epoch(4).unwrap_err(),
        "authority_epoch_stale"
    );
}

#[test]
fn ci02_snapshot_contracts_reject_raw_secret_fields_and_epoch_drift() {
    let config = ConfigSnapshot::new(
        Vec::new(),
        json!({"token":"bearer live-secret"}),
        digest("config"),
        digest("trust"),
    );
    assert_eq!(
        config.unwrap_err(),
        "config_snapshot_secret_or_size_invalid"
    );
}
