use kiana_domain::{
    parse_security_object, schema_contract, upcast_security_object, upcast_security_registry,
    SecurityEventId, SecurityObjectEnvelope, SecurityObjectKind, SecuritySchemaRegistry,
    SECURITY_OBJECT_SCHEMA, SECURITY_SCHEMA_REGISTRY_SCHEMA,
};
use serde_json::json;

#[test]
fn security_registry_is_generated_sorted_and_round_trips() {
    let id = SecurityEventId::new();
    assert_eq!(SecurityEventId::parse_str(&format!("  {id}  ")), Some(id));
    assert!(SecurityEventId::parse_str("not-a-uuid").is_none());

    let registry = SecuritySchemaRegistry::current().expect("genesis registry");
    assert_eq!(registry.schema, SECURITY_SCHEMA_REGISTRY_SCHEMA);
    assert_eq!(registry.registry_revision, 1);
    assert_eq!(registry.authority_epoch, 1);
    assert_eq!(registry.data_epoch, 1);
    assert_eq!(registry.source_sequence, 1);
    assert!(registry.parent_digest.is_none());
    assert!(registry
        .entries
        .windows(2)
        .all(|pair| pair[0].schema < pair[1].schema));
    assert!(schema_contract(SECURITY_SCHEMA_REGISTRY_SCHEMA).is_some());

    let encoded = registry.to_json().expect("registry json");
    let decoded = SecuritySchemaRegistry::from_json(&encoded).expect("registry round trip");
    assert_eq!(decoded, registry);
    assert_eq!(decoded.digest(), registry.registry_digest);
}

#[test]
fn security_registry_rejects_unknown_major_duplicate_ids_and_rollback() {
    let registry = SecuritySchemaRegistry::current().expect("genesis registry");

    let mut unknown_major = registry.to_json().expect("registry json");
    unknown_major["version"]["major"] = json!(2);
    assert_eq!(
        SecuritySchemaRegistry::from_json(&unknown_major).unwrap_err(),
        "security_schema_registry_unknown_major"
    );

    let mut duplicate_id = registry.to_json().expect("registry json");
    let first_id = duplicate_id["entries"][0]["registry_id"].clone();
    duplicate_id["entries"][1]["registry_id"] = first_id;
    assert_eq!(
        SecuritySchemaRegistry::from_json(&duplicate_id).unwrap_err(),
        "security_schema_registry_duplicate_id"
    );

    assert_eq!(
        SecuritySchemaRegistry::successor(&registry, 0, 1, 2).unwrap_err(),
        "security_schema_registry_authority_epoch_rollback"
    );
    assert_eq!(
        SecuritySchemaRegistry::successor(&registry, 1, 0, 2).unwrap_err(),
        "security_schema_registry_data_epoch_rollback"
    );
    assert_eq!(
        SecuritySchemaRegistry::successor(&registry, 1, 1, 1).unwrap_err(),
        "security_schema_registry_sequence_rollback"
    );

    let successor = SecuritySchemaRegistry::successor(&registry, 2, 1, 2).expect("successor");
    assert!(successor.validate_successor(&registry).is_ok());
    let unrelated = SecuritySchemaRegistry::current().expect("second genesis");
    assert_eq!(
        successor.validate_successor(&unrelated).unwrap_err(),
        "security_schema_registry_digest_rollback"
    );
}

#[test]
fn security_registry_rejects_secret_fields_and_unknown_upcasts() {
    let registry = SecuritySchemaRegistry::current().expect("genesis registry");
    let mut secret = registry.to_json().expect("registry json");
    secret["secret"] = json!("sentinel-secret");
    assert!(SecuritySchemaRegistry::from_json(&secret).is_err());

    let mut unknown = registry.to_json().expect("registry json");
    unknown["schema"] = json!("kiana.security-schema-registry.v9");
    assert_eq!(
        upcast_security_registry(unknown, SECURITY_SCHEMA_REGISTRY_SCHEMA).unwrap_err(),
        "security_schema_unknown_major"
    );
}

#[test]
fn security_object_is_canonical_chain_and_secret_safe() {
    let object = SecurityObjectEnvelope::new(
        SecurityEventId::new(),
        SecurityObjectKind::Decision,
        1,
        1,
        json!({"decision":"deny", "reason_code":"AUTH_UNTRUSTED"}),
    )
    .expect("security object");
    assert_eq!(object.schema, SECURITY_OBJECT_SCHEMA);
    assert_eq!(object.sequence, 1);
    assert!(object.previous_digest.is_none());
    assert_eq!(
        parse_security_object(&object.to_json().unwrap()).unwrap(),
        object
    );

    let successor = SecurityObjectEnvelope::successor(
        &object,
        1,
        2,
        json!({"decision":"unknown", "evidence_ref":"event:1"}),
    )
    .expect("security object successor");
    assert!(successor.validate_successor(&object).is_ok());
    assert_eq!(successor.sequence, 2);
    assert_eq!(
        successor.previous_digest.as_deref(),
        Some(object.envelope_digest.as_str())
    );

    assert_eq!(
        SecurityObjectEnvelope::successor(&object, 0, 1, json!({})).unwrap_err(),
        "security_object_authority_epoch_rollback"
    );
    assert_eq!(
        SecurityObjectEnvelope::successor(&object, 1, 0, json!({})).unwrap_err(),
        "security_object_data_epoch_rollback"
    );

    let secret = SecurityObjectEnvelope::new(
        SecurityEventId::new(),
        SecurityObjectKind::SecretReference,
        1,
        1,
        json!({"secret_value":"sentinel-secret"}),
    );
    assert_eq!(secret.unwrap_err(), "security_object_secret_field");
}

#[test]
fn security_object_rejects_unknown_major_and_implicit_migration() {
    let object = SecurityObjectEnvelope::new(
        SecurityEventId::new(),
        SecurityObjectKind::Context,
        1,
        1,
        json!({"context_id":"opaque"}),
    )
    .expect("security object");
    let mut value = object.to_json().unwrap();
    value["version"]["major"] = json!(2);
    assert_eq!(
        SecurityObjectEnvelope::from_json(&value).unwrap_err(),
        "security_object_unknown_major"
    );

    value["version"]["major"] = json!(1);
    value["schema"] = json!("kiana.security-object.v0");
    assert_eq!(
        upcast_security_object(value, SECURITY_OBJECT_SCHEMA).unwrap_err(),
        "security_schema_unknown_major"
    );
}
