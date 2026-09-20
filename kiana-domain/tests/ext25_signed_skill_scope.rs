use kiana_domain::{
    ExtensionEffect, ExtensionExecutionScope, ExtensionManifest, ExtensionNetworkPolicy,
    ExtensionRequires, ExtensionSignature, ExtensionType, EXTENSION_MANIFEST_SCHEMA,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn raw_hash(value: char) -> String {
    value.to_string().repeat(64)
}

fn manifest() -> ExtensionManifest {
    ExtensionManifest {
        schema: EXTENSION_MANIFEST_SCHEMA.to_owned(),
        extension_id: "review-skill".to_owned(),
        version: "1.0.0".to_owned(),
        publisher: "fixture-publisher".to_owned(),
        license: "MIT".to_owned(),
        content_hash: raw_hash('b'),
        signature: ExtensionSignature {
            algorithm: "ed25519".to_owned(),
            key_id: "fixture-key".to_owned(),
            value: raw_hash('c').repeat(2),
        },
        extension_type: ExtensionType::Skill,
        effect: ExtensionEffect::ReadOnly,
        provided_capabilities: BTreeSet::new(),
        required_capabilities: BTreeSet::from(["memory.search".to_owned()]),
        supported_roles: BTreeSet::from(["builder".to_owned()]),
        data_classes: BTreeSet::new(),
        network_policy: ExtensionNetworkPolicy::Deny,
        secret_refs: BTreeSet::new(),
        configuration_schema: json!({"type": "object"}),
        migration_ref: None,
        rollback_ref: None,
        requires: ExtensionRequires {
            kiana_version: env!("CARGO_PKG_VERSION").to_owned(),
            protocol_version: "kiana.protocol.v1".to_owned(),
            capability_versions: BTreeMap::new(),
            policy_features: BTreeSet::new(),
            memory_collections: BTreeSet::new(),
            supported_platforms: BTreeSet::new(),
            extensions: BTreeMap::new(),
        },
    }
}

#[test]
fn signed_skill_scope_binds_registry_role_policy_and_expiry() {
    let scope = ExtensionExecutionScope::new(&manifest(), raw_hash('a'), 7, "builder", 100, 200)
        .expect("scope");
    scope.validate().expect("valid scope");
    scope
        .validate_at(199, 7, "builder")
        .expect("current role and registry generation");
    assert_eq!(
        scope
            .validate_at(199, 8, "builder")
            .expect_err("stale registry generation must fail closed"),
        "extension_scope_generation_stale"
    );
    assert_eq!(
        scope
            .validate_at(200, 7, "builder")
            .expect_err("expired scope must fail closed"),
        "extension_scope_expired"
    );
    assert_eq!(
        scope
            .validate_at(199, 7, "architect")
            .expect_err("role drift must fail closed"),
        "extension_scope_role_mismatch"
    );
}

#[test]
fn signed_skill_scope_rejects_manifest_policy_drift_and_unknown_fields() {
    let manifest = manifest();
    let scope = ExtensionExecutionScope::new(&manifest, raw_hash('a'), 7, "builder", 100, 200)
        .expect("scope");
    scope.matches_manifest(&manifest).expect("same manifest");

    let mut changed = manifest.clone();
    changed
        .required_capabilities
        .insert("context.search".to_owned());
    assert_eq!(
        scope
            .matches_manifest(&changed)
            .expect_err("capability diff drift must fail closed"),
        "extension_scope_policy_digest_mismatch"
    );

    let mut encoded = serde_json::to_value(scope).expect("scope value");
    encoded["forged_by_model"] = json!(true);
    assert!(
        serde_json::from_value::<ExtensionExecutionScope>(encoded).is_err(),
        "scope contract must reject unknown fields"
    );
}
