use kiana_domain::CredentialDisplayStatus;
use kiana_policy::{
    ProviderPolicyBundle, ProviderPolicyEffect, ProviderPolicyRule, PROVIDER_USE_OPERATION,
};

#[test]
fn provider_policy_is_default_deny_and_precedence_is_deterministic() {
    let allow = ProviderPolicyRule::new(
        "allow-openai",
        "openai",
        PROVIDER_USE_OPERATION,
        ProviderPolicyEffect::Allow,
        10,
    )
    .unwrap();
    let deny = ProviderPolicyRule::new(
        "deny-openai",
        "openai",
        PROVIDER_USE_OPERATION,
        ProviderPolicyEffect::Deny,
        20,
    )
    .unwrap();
    let bundle =
        ProviderPolicyBundle::new(7, ProviderPolicyEffect::Deny, vec![allow, deny]).unwrap();

    let decision = bundle.evaluate(
        "openai",
        PROVIDER_USE_OPERATION,
        CredentialDisplayStatus::Configured,
    );
    assert_eq!(decision.effect, ProviderPolicyEffect::Deny);
    assert_eq!(decision.matched_rule.as_deref(), Some("deny-openai"));
    assert!(!decision.allowed());

    let unknown_provider = bundle.evaluate(
        "unregistered",
        PROVIDER_USE_OPERATION,
        CredentialDisplayStatus::Configured,
    );
    assert_eq!(unknown_provider.reason, "provider_policy_denied");
    assert!(unknown_provider.matched_rule.is_none());

    let wrong_operation = bundle.evaluate(
        "openai",
        "provider.catalog",
        CredentialDisplayStatus::Configured,
    );
    assert_eq!(wrong_operation.reason, "provider_operation_denied");
    assert!(!wrong_operation.allowed());
}

#[test]
fn equal_precedence_uses_last_declaration_and_statuses_stay_distinct() {
    let first = ProviderPolicyRule::new(
        "first",
        "*",
        PROVIDER_USE_OPERATION,
        ProviderPolicyEffect::Deny,
        30,
    )
    .unwrap();
    let last = ProviderPolicyRule::new(
        "last",
        "openai",
        PROVIDER_USE_OPERATION,
        ProviderPolicyEffect::Allow,
        30,
    )
    .unwrap();
    let bundle =
        ProviderPolicyBundle::new(8, ProviderPolicyEffect::Deny, vec![first, last]).unwrap();

    let ready = bundle.evaluate(
        "openai",
        PROVIDER_USE_OPERATION,
        CredentialDisplayStatus::Configured,
    );
    assert_eq!(ready.effect, ProviderPolicyEffect::Allow);
    assert_eq!(ready.reason, "provider_ready");
    assert_eq!(ready.matched_rule.as_deref(), Some("last"));
    assert!(ready.allowed());

    let scope = bundle.evaluate(
        "openai",
        PROVIDER_USE_OPERATION,
        CredentialDisplayStatus::ScopeInsufficient,
    );
    assert_eq!(scope.reason, "provider_scope_insufficient");
    assert!(!scope.allowed());

    let missing = bundle.evaluate(
        "openai",
        PROVIDER_USE_OPERATION,
        CredentialDisplayStatus::Missing,
    );
    assert_eq!(missing.reason, "provider_credential_missing");
    assert!(!missing.allowed());
}

#[test]
fn provider_policy_rejects_unknown_fields_and_duplicate_rule_ids() {
    let rule = ProviderPolicyRule::new(
        "same",
        "openai",
        PROVIDER_USE_OPERATION,
        ProviderPolicyEffect::Allow,
        1,
    )
    .unwrap();
    let duplicate =
        ProviderPolicyBundle::new(1, ProviderPolicyEffect::Deny, vec![rule.clone(), rule]);
    assert_eq!(duplicate.unwrap_err(), "provider_policy_rule_duplicate");

    let mut encoded =
        serde_json::to_value(ProviderPolicyBundle::default_deny(1).expect("default deny policy"))
            .unwrap();
    encoded["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProviderPolicyBundle>(encoded).is_err());
}
