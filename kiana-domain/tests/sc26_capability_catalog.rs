use kiana_domain::{
    extension_manifest_digest, CapabilityCatalog, CapabilityCatalogDecision, ExtensionEffect,
    ExtensionManifest, ExtensionNetworkPolicy, ExtensionRequires, ExtensionSignature,
    ExtensionType, EXTENSION_MANIFEST_SCHEMA,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn manifest(id: &str, effect: ExtensionEffect, required: &[&str]) -> ExtensionManifest {
    ExtensionManifest {
        schema: EXTENSION_MANIFEST_SCHEMA.to_owned(),
        extension_id: id.to_owned(),
        version: "1.0.0".to_owned(),
        publisher: "fixture-publisher".to_owned(),
        license: "MIT".to_owned(),
        content_hash: "a".repeat(64),
        signature: ExtensionSignature {
            algorithm: "ed25519".to_owned(),
            key_id: "fixture-key".to_owned(),
            value: "b".repeat(128),
        },
        extension_type: ExtensionType::Skill,
        effect,
        provided_capabilities: BTreeSet::from(["shell.exec".to_owned()]),
        required_capabilities: required.iter().map(|value| (*value).to_owned()).collect(),
        supported_roles: BTreeSet::from(["builder".to_owned()]),
        data_classes: BTreeSet::new(),
        network_policy: ExtensionNetworkPolicy::Deny,
        secret_refs: BTreeSet::new(),
        configuration_schema: json!({"type":"object"}),
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

fn capabilities(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn catalog_binds_manifest_digest_and_intersects_all_authority_sets() {
    let read_only = manifest("reader", ExtensionEffect::ReadOnly, &["memory.search"]);
    let write = manifest("writer", ExtensionEffect::ReadWrite, &["shell.exec"]);
    let catalog = CapabilityCatalog::from_manifests(
        1,
        &[read_only.clone(), write],
        &capabilities(&["memory.search", "shell.exec"]),
        &capabilities(&["memory.search"]),
        &capabilities(&["memory.search"]),
    )
    .unwrap();
    let reader = &catalog.entries[0];
    assert_eq!(reader.extension_id, "reader");
    assert_eq!(reader.decision, CapabilityCatalogDecision::Allowed);
    assert_eq!(
        reader.effective_capabilities,
        capabilities(&["memory.search"])
    );
    assert_eq!(
        reader.manifest_digest,
        extension_manifest_digest(&read_only).unwrap()
    );
    let writer = &catalog.entries[1];
    assert_eq!(writer.decision, CapabilityCatalogDecision::Denied);
    assert!(writer
        .reason
        .starts_with("capability_intersection_missing:"));
    catalog.validate().unwrap();
}

#[test]
fn read_only_manifest_cannot_request_write_and_provided_tools_do_not_authorize() {
    let read_only_write = manifest("bad-reader", ExtensionEffect::ReadOnly, &["memory.write"]);
    let catalog = CapabilityCatalog::from_manifests(
        2,
        &[read_only_write],
        &capabilities(&["memory.write"]),
        &capabilities(&["memory.write"]),
        &capabilities(&["memory.write"]),
    )
    .unwrap();
    let entry = &catalog.entries[0];
    assert_eq!(entry.decision, CapabilityCatalogDecision::Denied);
    assert_eq!(entry.reason, "extension_read_only_write_denied");
    assert!(entry.effective_capabilities.is_empty());
}

#[test]
fn widened_or_tampered_catalog_entry_is_rejected() {
    let manifest = manifest("reader", ExtensionEffect::ReadOnly, &["memory.search"]);
    let mut catalog = CapabilityCatalog::from_manifests(
        3,
        &[manifest],
        &capabilities(&["memory.search"]),
        &capabilities(&["memory.search"]),
        &capabilities(&["memory.search"]),
    )
    .unwrap();
    catalog.entries[0]
        .effective_capabilities
        .insert("shell.exec".to_owned());
    assert_eq!(
        catalog.validate().unwrap_err(),
        "capability_effective_set_widened"
    );
}
