use kiana_domain::{
    json_digest, AuditDeliveryReceipt, AuditDeliveryState, AuditExportFormat, AuditExportManifest,
    EventId,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!(value))
}

fn manifest() -> AuditExportManifest {
    AuditExportManifest::new(
        "export:one",
        digest("query"),
        7,
        1,
        1,
        AuditExportFormat::Jsonl,
        "security review",
        "principal:operator",
        "audit",
        digest("redacted-content"),
        vec![EventId::new()],
    )
    .unwrap()
}

#[test]
fn manifest_binds_query_source_artifact_and_format_without_raw_event_fields() {
    let manifest = manifest();
    manifest.validate().unwrap();
    let encoded = serde_json::to_string(&manifest).unwrap();
    assert!(!encoded.contains("redacted-content"));
    assert_eq!(
        serde_json::from_str::<AuditExportManifest>(&encoded).unwrap(),
        manifest
    );
    let mut forged = manifest.clone();
    forged.artifact_digest = digest("changed");
    assert_eq!(
        forged.validate().unwrap_err(),
        "audit_export_manifest_digest_mismatch"
    );
    let mut unknown = serde_json::to_value(&manifest).unwrap();
    unknown["raw_events"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AuditExportManifest>(unknown).is_err());
}

#[test]
fn delivery_receipt_never_claims_delivered_without_server_confirmation() {
    let manifest = manifest();
    let unknown = AuditDeliveryReceipt::new(
        "delivery:one",
        &manifest.export_id,
        &manifest.manifest_digest,
        &manifest.artifact_digest,
        &manifest.recipient,
        AuditDeliveryState::Unknown,
        None,
        manifest.source_cursor,
    )
    .unwrap();
    unknown.validate().unwrap();
    assert!(AuditDeliveryReceipt::new(
        "delivery:two",
        &manifest.export_id,
        &manifest.manifest_digest,
        &manifest.artifact_digest,
        &manifest.recipient,
        AuditDeliveryState::Delivered,
        None,
        manifest.source_cursor,
    )
    .is_err());
    let delivered = AuditDeliveryReceipt::new(
        "delivery:three",
        &manifest.export_id,
        &manifest.manifest_digest,
        &manifest.artifact_digest,
        &manifest.recipient,
        AuditDeliveryState::Delivered,
        Some(digest("server-confirmation")),
        manifest.source_cursor,
    )
    .unwrap();
    delivered.validate().unwrap();
    assert!(AuditDeliveryReceipt::new(
        "delivery:four",
        &manifest.export_id,
        &manifest.manifest_digest,
        &manifest.artifact_digest,
        &manifest.recipient,
        AuditDeliveryState::Failed,
        Some(digest("forged")),
        manifest.source_cursor,
    )
    .is_err());
}
