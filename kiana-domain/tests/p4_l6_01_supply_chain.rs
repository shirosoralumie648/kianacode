use kiana_domain::{
    ExtensionEffect, ExtensionManifest, ExtensionNetworkPolicy, ExtensionPackage,
    ExtensionRequires, ExtensionSignature, ExtensionType, EXTENSION_MANIFEST_SCHEMA,
    EXTENSION_PACKAGE_SCHEMA,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn manifest(version: &str, required: &str) -> ExtensionManifest {
    ExtensionManifest {
        schema: EXTENSION_MANIFEST_SCHEMA.to_owned(),
        extension_id: "review-skill".to_owned(),
        version: version.to_owned(),
        publisher: "fixture-publisher".to_owned(),
        license: "MIT".to_owned(),
        content_hash: "a".repeat(64),
        signature: ExtensionSignature {
            algorithm: "ed25519".to_owned(),
            key_id: "fixture-key".to_owned(),
            value: "b".repeat(128),
        },
        extension_type: ExtensionType::Skill,
        effect: ExtensionEffect::ReadOnly,
        provided_capabilities: BTreeSet::new(),
        required_capabilities: BTreeSet::from([required.to_owned()]),
        supported_roles: BTreeSet::from(["builder".to_owned()]),
        data_classes: BTreeSet::new(),
        network_policy: ExtensionNetworkPolicy::Deny,
        secret_refs: BTreeSet::new(),
        configuration_schema: json!({"type": "object"}),
        migration_ref: Some("migrations/v1.json".to_owned()),
        rollback_ref: Some("rollback/v1.json".to_owned()),
        requires: ExtensionRequires {
            kiana_version: env!("CARGO_PKG_VERSION").to_owned(),
            protocol_version: "kiana.protocol.v1".to_owned(),
            capability_versions: BTreeMap::new(),
            policy_features: BTreeSet::from(["control_plane".to_owned()]),
            memory_collections: BTreeSet::new(),
            supported_platforms: BTreeSet::new(),
            extensions: BTreeMap::new(),
        },
    }
}

#[test]
fn extension_signature_is_verified_before_install() {
    let current = manifest("1.0.0", "memory.search");
    current.validate().expect("manifest metadata validates");
    let next = manifest("1.1.0", "memory.write");
    next.validate().expect("next metadata validates");
    let diff = next.capability_diff(Some(&current));
    assert_eq!(diff["added"], json!(["memory.write"]));
    assert_eq!(diff["removed"], json!(["memory.search"]));
    assert_eq!(diff["effect_before"], json!(ExtensionEffect::ReadOnly));
    assert_eq!(diff["license_after"], json!("MIT"));

    let package = ExtensionPackage {
        schema: EXTENSION_PACKAGE_SCHEMA.to_owned(),
        manifest: current.clone(),
        files: BTreeMap::from([
            ("SKILL.md".to_owned(), "23".to_owned()),
            ("migrations/v1.json".to_owned(), "7b7d".to_owned()),
            ("rollback/v1.json".to_owned(), "7b7d".to_owned()),
        ]),
    };
    let encoded = serde_json::to_value(&package).expect("strict package serializes");
    let decoded: ExtensionPackage = serde_json::from_value(encoded.clone()).expect("round trip");
    assert_eq!(decoded, package);
    let mut unknown = encoded;
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ExtensionPackage>(unknown).is_err());

    let mut bad = current;
    bad.signature.algorithm = "none".to_owned();
    assert_eq!(bad.validate(), Err("extension_signature_invalid"));
    let mut traversal = next;
    traversal.migration_ref = Some("../install.sh".to_owned());
    assert_eq!(traversal.validate(), Err("extension_reference_invalid"));
}
