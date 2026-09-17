use kiana_domain::{ApprovalExecutionMaterial, ApprovalMaterialState, APPROVAL_MATERIAL_SCHEMA};
use serde_json::json;

#[test]
fn approval_material_binds_payload_preview_state_and_expiry_without_raw_content() {
    let payload = json!({"operation":"secret.read","arguments":{"secret_ref":"opaque"}});
    let preview = json!({"operation":"secret.read","arguments":{"preview":"[REDACTED]"}});
    let material = ApprovalExecutionMaterial::from_payloads(
        &payload,
        &preview,
        ApprovalMaterialState::VolatileProtected,
        500,
    )
    .unwrap();
    assert_eq!(material.schema, APPROVAL_MATERIAL_SCHEMA);
    assert!(material.validate().is_ok());
    assert!(material.matches_payloads(&payload, &preview).unwrap());
    assert!(!material
        .matches_payloads(&payload, &json!({"changed":true}))
        .unwrap());
    let value = serde_json::to_value(&material).unwrap();
    assert!(value.get("payload").is_none());
    assert!(value.get("preview").is_none());
    assert_eq!(
        ApprovalExecutionMaterial::from_json(&value).unwrap(),
        material
    );
}

#[test]
fn approval_material_rejects_unknown_or_bad_digest_fields() {
    let payload = json!({"operation":"read"});
    let material = ApprovalExecutionMaterial::from_payloads(
        &payload,
        &payload,
        ApprovalMaterialState::InlineRedacted,
        500,
    )
    .unwrap();
    let mut unknown = serde_json::to_value(&material).unwrap();
    unknown["raw_secret"] = json!("sentinel");
    assert!(ApprovalExecutionMaterial::from_json(&unknown).is_err());
    let mut bad = serde_json::to_value(&material).unwrap();
    bad["payload_digest"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");
    assert!(ApprovalExecutionMaterial::from_json(&bad).is_err());
}
