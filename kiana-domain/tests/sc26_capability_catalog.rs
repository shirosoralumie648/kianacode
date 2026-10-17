use kiana_domain::{
    extension_manifest_digest, json_digest, CapabilityCatalog, CapabilityCatalogDecision,
    ComponentId, EvidenceStatus, ExtensionEffect, ExtensionManifest, ExtensionNetworkPolicy,
    ExtensionRequires, ExtensionSignature, ExtensionType, SkillDescriptor, SkillLifecycle,
    SnapshotId, SourceKind, SourceRef, EXTENSION_MANIFEST_SCHEMA, SKILL_DESCRIPTOR_SCHEMA,
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
        required_capabilities: capabilities(required),
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

fn authority(values: &[&str]) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let available = capabilities(values);
    (available.clone(), available.clone(), available)
}

fn skill_descriptor(id: &str, allowed: &[&str]) -> SkillDescriptor {
    SkillDescriptor {
        schema: SKILL_DESCRIPTOR_SCHEMA.to_owned(),
        component_id: ComponentId::new(),
        extension_id: None,
        name: "review-skill".to_owned(),
        version: "1.0.0".to_owned(),
        description: "SC-26 fixture skill".to_owned(),
        source: SourceRef::new(
            id,
            SourceKind::UserImport,
            format!("fixture://{id}"),
            "revision:1",
            json_digest(&json!({"source": id})),
            Some(1),
            EvidenceStatus::Attributed,
        )
        .unwrap(),
        content_digest: json_digest(&json!({"body": id})),
        lifecycle: SkillLifecycle::Eligible,
        allowed_tools: capabilities(allowed),
        activation_reason: Some("fixture".to_owned()),
        snapshot_id: SnapshotId::new(),
    }
}

#[test]
fn catalog_binds_manifest_digest_and_intersects_all_authority_sets() {
    let read_only = manifest("reader", ExtensionEffect::ReadOnly, &["memory.search"]);
    let write = manifest("writer", ExtensionEffect::ReadWrite, &["shell.exec"]);
    let available = capabilities(&["memory.search", "shell.exec", "network.fetch"]);
    let grant = capabilities(&["memory.search", "network.fetch"]);
    let approval = capabilities(&["memory.search"]);
    let catalog = CapabilityCatalog::from_manifests(
        1,
        &[read_only.clone(), write.clone()],
        &available,
        &grant,
        &approval,
    )
    .unwrap();

    let reader = &catalog.entries[0];
    assert_eq!(reader.extension_id, "reader");
    assert_eq!(reader.decision, CapabilityCatalogDecision::Allowed);
    assert_eq!(
        reader.requested_capabilities,
        capabilities(&["memory.search"])
    );
    assert_eq!(
        reader.effective_capabilities,
        capabilities(&["memory.search"])
    );
    assert_eq!(
        reader.manifest_digest,
        extension_manifest_digest(&read_only).unwrap()
    );
    catalog.validate().unwrap();

    // Available, granted or approved capabilities that the manifest never requested stay
    // outside the effective set and cannot be handed out.
    assert_eq!(
        catalog
            .authorize_manifest(&read_only, "network.fetch")
            .unwrap_err(),
        "capability_not_effective:network.fetch"
    );
    assert!(catalog
        .authorize_manifest(&read_only, "memory.search")
        .is_ok());

    let writer = &catalog.entries[1];
    assert_eq!(writer.decision, CapabilityCatalogDecision::Denied);
    assert!(writer
        .reason
        .starts_with("capability_intersection_missing:"));
    assert_eq!(writer.requested_capabilities, capabilities(&["shell.exec"]));
    assert!(writer.effective_capabilities.is_empty());

    // The catalog digest is order-independent and binds the full manifest, including signature
    // metadata, so a re-signed manifest is a different input.
    let reordered = CapabilityCatalog::from_manifests(
        1,
        &[write, read_only.clone()],
        &available,
        &grant,
        &approval,
    )
    .unwrap();
    assert_eq!(reordered.catalog_digest, catalog.catalog_digest);
    let round_tripped: ExtensionManifest =
        serde_json::from_value(serde_json::to_value(&read_only).unwrap()).unwrap();
    assert_eq!(
        extension_manifest_digest(&round_tripped).unwrap(),
        reader.manifest_digest
    );
    let mut resigned = read_only;
    resigned.signature.value = "c".repeat(128);
    assert_ne!(
        extension_manifest_digest(&resigned).unwrap(),
        reader.manifest_digest
    );
}

#[test]
fn read_only_manifest_cannot_request_write_and_provided_tools_do_not_authorize() {
    let read_only_write = manifest("bad-reader", ExtensionEffect::ReadOnly, &["memory.write"]);
    let authority = capabilities(&["memory.write", "shell.exec"]);
    let catalog = CapabilityCatalog::from_manifests(
        2,
        std::slice::from_ref(&read_only_write),
        &authority,
        &authority,
        &authority,
    )
    .unwrap();
    let entry = &catalog.entries[0];
    assert_eq!(entry.decision, CapabilityCatalogDecision::Denied);
    assert_eq!(entry.reason, "extension_read_only_write_denied");
    // The requested set stays visible for audit, the effective set stays inert.
    assert_eq!(
        entry.requested_capabilities,
        capabilities(&["memory.write"])
    );
    assert!(entry.effective_capabilities.is_empty());
    // A capability that is only declared as provided is not requested and never effective.
    assert!(read_only_write.provided_capabilities.contains("shell.exec"));
    assert!(!entry.requested_capabilities.contains("shell.exec"));
    assert_eq!(
        catalog
            .authorize_manifest(&read_only_write, "memory.write")
            .unwrap_err(),
        "capability_catalog_entry_denied:extension_read_only_write_denied"
    );
    assert_eq!(
        entry.authorize("shell.exec").unwrap_err(),
        "capability_catalog_entry_denied:extension_read_only_write_denied"
    );
}

#[test]
fn allowed_tools_declarations_are_descriptive_and_never_enter_the_grant_set() {
    let skill = skill_descriptor("review-skill", &["shell.exec", "network.fetch"]);
    skill.validate().unwrap();
    let restricted = manifest(
        "review-skill",
        ExtensionEffect::ReadWrite,
        &["memory.write"],
    );
    let (available, grant, approval) = authority(&["memory.write", "shell.exec", "network.fetch"]);
    let catalog = CapabilityCatalog::from_manifests(
        3,
        std::slice::from_ref(&restricted),
        &available,
        &grant,
        &approval,
    )
    .unwrap();
    let entry = catalog.entry_for_manifest(&restricted).unwrap();
    assert_eq!(
        entry.requested_capabilities,
        capabilities(&["memory.write"])
    );
    assert_eq!(
        entry.effective_capabilities,
        capabilities(&["memory.write"])
    );
    for declared in &skill.allowed_tools {
        assert!(!entry.effective_capabilities.contains(declared));
        assert_eq!(
            catalog
                .authorize_manifest(&restricted, declared)
                .unwrap_err(),
            format!("capability_not_effective:{declared}")
        );
    }
    assert!(catalog
        .authorize_manifest(&restricted, "memory.write")
        .is_ok());
}

#[test]
fn version_capability_and_digest_drift_cannot_reuse_a_catalog_entry() {
    let approved = manifest(
        "drift-reader",
        ExtensionEffect::ReadWrite,
        &["memory.search"],
    );
    let (available, grant, approval) = authority(&["memory.search", "memory.write"]);
    let catalog = CapabilityCatalog::from_manifests(
        4,
        std::slice::from_ref(&approved),
        &available,
        &grant,
        &approval,
    )
    .unwrap();
    assert!(catalog
        .authorize_manifest(&approved, "memory.search")
        .is_ok());
    let entry = catalog.entry_for_manifest(&approved).unwrap();

    let mut version_drift = approved.clone();
    version_drift.version = "2.0.0".to_owned();
    assert_eq!(
        catalog
            .authorize_manifest(&version_drift, "memory.search")
            .unwrap_err(),
        "capability_catalog_manifest_version_drift:2.0.0"
    );

    let mut capability_drift = approved.clone();
    capability_drift
        .required_capabilities
        .insert("memory.write".to_owned());
    assert_eq!(
        catalog
            .authorize_manifest(&capability_drift, "memory.write")
            .unwrap_err(),
        "capability_catalog_manifest_capability_drift:memory.write"
    );

    let mut digest_drift = approved.clone();
    digest_drift.publisher = "attacker-publisher".to_owned();
    assert_eq!(
        catalog
            .authorize_manifest(&digest_drift, "memory.search")
            .unwrap_err(),
        "capability_catalog_manifest_digest_drift"
    );

    let mut foreign = approved.clone();
    foreign.extension_id = "other-extension".to_owned();
    assert_eq!(
        entry.check_manifest_drift(&foreign).unwrap_err(),
        "capability_catalog_extension_identity_drift:other-extension"
    );
    assert_eq!(
        catalog
            .authorize_manifest(&foreign, "memory.search")
            .unwrap_err(),
        "capability_catalog_extension_unknown:other-extension"
    );
}

#[test]
fn denied_or_unrequested_capabilities_never_authorize() {
    let allowed = manifest("reader", ExtensionEffect::ReadOnly, &["memory.search"]);
    let denied = manifest("writer", ExtensionEffect::ReadWrite, &["shell.exec"]);
    let bad_read_only = manifest("bad-reader", ExtensionEffect::ReadOnly, &["memory.write"]);
    let catalog = CapabilityCatalog::from_manifests(
        5,
        &[allowed.clone(), denied.clone(), bad_read_only.clone()],
        &capabilities(&["memory.search", "shell.exec", "memory.write"]),
        &capabilities(&["memory.search"]),
        &capabilities(&["memory.search"]),
    )
    .unwrap();

    // An allowed entry hands out only capabilities it made effective.
    assert!(catalog
        .authorize_manifest(&allowed, "memory.search")
        .is_ok());
    assert_eq!(
        catalog
            .authorize_manifest(&allowed, "memory.write")
            .unwrap_err(),
        "capability_not_effective:memory.write"
    );

    // A missing intersection member denies the whole entry with a stable reason.
    assert_eq!(
        catalog
            .authorize_manifest(&denied, "shell.exec")
            .unwrap_err(),
        "capability_catalog_entry_denied:capability_intersection_missing:shell.exec"
    );

    // A read-only manifest requesting a write capability stays inert even when every authority
    // set offers that capability.
    let entry = catalog.entry_for_manifest(&bad_read_only).unwrap();
    assert!(entry.effective_capabilities.is_empty());
    assert_eq!(
        catalog
            .authorize_manifest(&bad_read_only, "memory.write")
            .unwrap_err(),
        "capability_catalog_entry_denied:extension_read_only_write_denied"
    );

    // Malformed capability requests are rejected before any set lookup.
    assert_eq!(
        entry.authorize("").unwrap_err(),
        "capability_catalog_request_invalid"
    );
    assert_eq!(
        entry.authorize(&"x".repeat(257)).unwrap_err(),
        "capability_catalog_request_invalid"
    );
}

#[test]
fn duplicate_identity_and_invalid_catalog_shapes_are_rejected() {
    let reader = manifest("dup-reader", ExtensionEffect::ReadOnly, &["memory.search"]);
    let mut newer = reader.clone();
    newer.version = "2.0.0".to_owned();
    let (available, grant, approval) = authority(&["memory.search"]);
    assert_eq!(
        CapabilityCatalog::from_manifests(
            1,
            &[reader.clone(), newer],
            &available,
            &grant,
            &approval
        )
        .unwrap_err(),
        "capability_catalog_extension_duplicate"
    );
    assert_eq!(
        CapabilityCatalog::from_manifests(
            0,
            std::slice::from_ref(&reader),
            &available,
            &grant,
            &approval,
        )
        .unwrap_err(),
        "capability_catalog_header_invalid"
    );
    let mut invalid = reader.clone();
    invalid.publisher = String::new();
    assert_eq!(
        CapabilityCatalog::from_manifests(1, &[invalid], &available, &grant, &approval)
            .unwrap_err(),
        "extension_manifest:extension_manifest_invalid"
    );
}

#[test]
fn widened_or_tampered_catalog_entry_is_rejected() {
    let reader_manifest = manifest("reader", ExtensionEffect::ReadOnly, &["memory.search"]);
    let authority = capabilities(&["memory.search"]);
    let catalog = CapabilityCatalog::from_manifests(
        3,
        std::slice::from_ref(&reader_manifest),
        &authority,
        &authority,
        &authority,
    )
    .unwrap();

    let mut widened = catalog.clone();
    widened.entries[0]
        .effective_capabilities
        .insert("shell.exec".to_owned());
    assert_eq!(
        widened.validate().unwrap_err(),
        "capability_effective_set_widened"
    );
    assert_eq!(
        widened.entries[0].authorize("shell.exec").unwrap_err(),
        "capability_effective_set_widened"
    );

    let mut resealed = catalog.clone();
    resealed.entries[0].manifest_digest = format!("sha256:{}", "d".repeat(64));
    assert_eq!(
        resealed.entries[0].validate().unwrap_err(),
        "capability_catalog_entry_digest_mismatch"
    );
    assert_eq!(
        resealed.validate().unwrap_err(),
        "capability_catalog_entry_digest_mismatch"
    );

    let mut regeneration = catalog.clone();
    regeneration.generation += 1;
    assert_eq!(
        regeneration.validate().unwrap_err(),
        "capability_catalog_digest_mismatch"
    );

    let mut allow_reason = catalog.clone();
    allow_reason.entries[0].reason = "capability_intersection_missing:memory.search".to_owned();
    assert_eq!(
        allow_reason.entries[0].validate().unwrap_err(),
        "capability_catalog_allow_reason_invalid"
    );

    let denied = manifest("writer", ExtensionEffect::ReadWrite, &["shell.exec"]);
    let mut denied_catalog = CapabilityCatalog::from_manifests(
        4,
        std::slice::from_ref(&denied),
        &capabilities(&["memory.write"]),
        &capabilities(&["memory.write"]),
        &capabilities(&["memory.write"]),
    )
    .unwrap();
    assert_eq!(
        denied_catalog.entries[0].decision,
        CapabilityCatalogDecision::Denied
    );
    denied_catalog.entries[0].reason = "capability_intersection_allowed".to_owned();
    assert_eq!(
        denied_catalog.entries[0].validate().unwrap_err(),
        "capability_catalog_deny_reason_invalid"
    );
}
