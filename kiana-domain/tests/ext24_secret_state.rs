use kiana_domain::{
    ExtensionConfigurationSnapshot, ExtensionSecretBinding, ExtensionStateMigrationPlan,
    ExtensionStateMigrationReceipt, ExtensionStateMigrationStatus, ExtensionStateScope, SecretRef,
    EXTENSION_SECRET_BINDING_SCHEMA,
};
use serde_json::json;
use std::collections::BTreeMap;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn secret_ref() -> SecretRef {
    SecretRef::new(
        "fixture-store",
        "plugin/alpha/api",
        "extension",
        "fixture",
        1,
    )
    .expect("secret ref")
}

fn binding(scope: &str) -> ExtensionSecretBinding {
    ExtensionSecretBinding::new(
        "plugin.alpha",
        scope,
        "api_key_ref",
        secret_ref(),
        "provider.example",
        100,
    )
    .expect("binding")
}

fn scope() -> ExtensionStateScope {
    ExtensionStateScope::new(
        "fixture-publisher",
        "plugin.alpha",
        digest('a'),
        "fixture-publisher/plugin.alpha/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/state",
        "fixture-publisher/plugin.alpha/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/cache",
    )
    .expect("scope")
}

fn migration_plan() -> ExtensionStateMigrationPlan {
    ExtensionStateMigrationPlan::new(
        "plugin.alpha",
        "fixture-publisher",
        digest('a'),
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        "backup-alpha-1",
        1024,
        4,
        5,
    )
    .expect("migration plan")
}

#[test]
fn secret_binding_is_handle_only_and_rechecked_at_destination() {
    let scope = digest('a');
    let binding = binding(&scope);
    assert_eq!(binding.schema, EXTENSION_SECRET_BINDING_SCHEMA);
    binding
        .validate_at(99, "plugin.alpha", "api_key_ref", "provider.example")
        .expect("valid effect-time binding");
    assert_eq!(
        binding
            .validate_at(99, "plugin.alpha", "api_key_ref", "other.example")
            .expect_err("destination drift must fail closed"),
        "extension_secret_destination_mismatch"
    );
    assert_eq!(
        binding
            .validate_at(100, "plugin.alpha", "api_key_ref", "provider.example")
            .expect_err("expired binding must fail closed"),
        "extension_secret_binding_expired"
    );
    let encoded = serde_json::to_string(&binding).expect("binding json");
    assert!(!encoded.contains("secret_value"));
    assert!(!encoded.contains("api_key_secret"));
}

#[test]
fn configuration_snapshot_rejects_raw_secret_and_binds_opaque_handles() {
    let scope = scope();
    let mut bindings = BTreeMap::new();
    bindings.insert("api_key_ref".to_owned(), binding(&scope.scope_digest));
    let snapshot = ExtensionConfigurationSnapshot::new(
        "plugin.alpha",
        scope.scope_digest.clone(),
        json!({"endpoint": "provider.example", "timeout_ms": 1000}),
        bindings,
    )
    .expect("handle-only configuration");
    snapshot.validate().expect("valid configuration");

    let raw = ExtensionConfigurationSnapshot::new(
        "plugin.alpha",
        scope.scope_digest,
        json!({"api_key": "sk-live-secret"}),
        BTreeMap::new(),
    )
    .expect_err("raw secret must fail closed");
    assert_eq!(raw, "extension_configuration_raw_secret_forbidden");
}

#[test]
fn state_scope_separates_mutable_state_from_read_only_cache() {
    let scope = scope();
    assert_ne!(scope.state_namespace, scope.cache_namespace);
    assert!(scope.cache_read_only);

    let mut mutable_cache = scope.clone();
    mutable_cache.cache_read_only = false;
    assert_eq!(
        mutable_cache
            .validate()
            .expect_err("mutable package cache must fail closed"),
        "extension_state_scope_header_invalid"
    );
}

#[test]
fn migration_requires_backup_hashes_bounded_bytes_and_explicit_unknown() {
    let plan = migration_plan();
    let unknown = ExtensionStateMigrationReceipt::unknown(&plan, "migration_effect_not_confirmed")
        .expect("unknown migration receipt");
    assert_eq!(unknown.status, ExtensionStateMigrationStatus::Unknown);
    assert!(unknown.old_state_retained);
    assert!(unknown.backup_verified);
    assert_eq!(unknown.committed_generation, 0);
    assert!(unknown.new_state_digest.is_none());

    let committed =
        ExtensionStateMigrationReceipt::committed(&plan, digest('f')).expect("commit receipt");
    assert_eq!(committed.status, ExtensionStateMigrationStatus::Committed);
    assert_eq!(committed.committed_generation, 5);
    assert!(committed.new_state_digest.is_some());

    let oversized = ExtensionStateMigrationPlan::new(
        "plugin.alpha",
        "fixture-publisher",
        digest('a'),
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        "backup-alpha-1",
        4 * 1024 * 1024 + 1,
        4,
        5,
    )
    .expect_err("unbounded migration must fail closed");
    assert_eq!(oversized, "extension_state_migration_plan_header_invalid");
}

#[test]
fn unknown_fields_are_rejected_on_secret_binding() {
    let mut value = serde_json::to_value(binding(&digest('a'))).expect("binding value");
    value["secret_value"] = json!("must-not-cross-boundary");
    assert!(
        serde_json::from_value::<ExtensionSecretBinding>(value).is_err(),
        "raw secret fields must not be accepted by the contract"
    );
}
