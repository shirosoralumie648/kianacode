use kiana_domain::{
    normalize_capability_result, CapabilityErrorCode, CapabilityResult, CapabilityResultReceipt,
    ExecutionId, InvocationId, RequestId,
};
use serde_json::json;

#[test]
fn result_receipt_separates_success_from_unknown_effect_and_binds_digest() {
    let request_id = RequestId::new();
    let success = CapabilityResult::success(request_id, json!({"value":"ok"}));
    let receipt = CapabilityResultReceipt::from_result(
        &success,
        Some(ExecutionId::new()),
        Some(InvocationId::new()),
        1,
        true,
    )
    .unwrap();
    assert!(receipt.validate().is_ok());
    assert!(receipt.success);
    assert!(receipt.effect_known);
    assert!(!receipt.zero_effect);

    let unknown = normalize_capability_result(
        request_id,
        CapabilityResult::failure_with_code(request_id, CapabilityErrorCode::ResultUnknown, None),
    );
    let unknown_receipt =
        CapabilityResultReceipt::from_result(&unknown, None, None, 1, true).unwrap();
    assert!(!unknown_receipt.success);
    assert!(!unknown_receipt.effect_known);
    assert!(unknown_receipt.fenced);
}

#[test]
fn result_receipt_rejects_inconsistent_flags_and_unknown_fields() {
    let request_id = RequestId::new();
    let result = CapabilityResult::success(request_id, json!({"value":"ok"}));
    let receipt = CapabilityResultReceipt::from_result(&result, None, None, 1, true).unwrap();
    let mut unknown = serde_json::to_value(&receipt).unwrap();
    unknown["secret_payload"] = json!("must-not-be-carried");
    assert!(CapabilityResultReceipt::from_json(&unknown).is_err());

    let mut inconsistent = receipt.clone();
    inconsistent.zero_effect = true;
    inconsistent.receipt_digest = inconsistent.digest();
    assert!(inconsistent.validate().is_err());
}
