use kiana_domain::{
    CapabilityExecutionState, EventId, ExecutionReceipt, ExecutionStatus, RequestId, RunId,
    RunReceipt, SchemaVersion, SessionId, EXECUTION_RECEIPT_SCHEMA, RECEIPT_CONTRACT_VERSION,
    RUN_RECEIPT_SCHEMA,
};
use serde_json::json;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

#[test]
fn run_receipt_binds_owner_source_and_redaction_metadata() {
    let run_id = RunId::new();
    let event_id = EventId::new();
    let receipt = RunReceipt::new(
        run_id,
        SessionId::new("session-1"),
        Some("actor-1".to_owned()),
        digest('a'),
        ExecutionStatus::Completed,
        None,
        8,
        vec![event_id],
        digest('b'),
        "implemented",
        "source",
        digest('c'),
        vec![digest('d')],
    )
    .unwrap();
    assert_eq!(receipt.schema, RUN_RECEIPT_SCHEMA);
    assert_eq!(receipt.version, RECEIPT_CONTRACT_VERSION);
    assert!(receipt.validate().is_ok());
    assert_eq!(
        RunReceipt::from_json(&receipt.to_json().unwrap()).unwrap(),
        receipt
    );
}

#[test]
fn execution_receipt_keeps_unknown_effect_fenced() {
    let request_id = RequestId::new();
    let receipt = ExecutionReceipt::new(
        request_id,
        None,
        None,
        1,
        digest('a'),
        CapabilityExecutionState::Unknown,
        false,
        Some(false),
        true,
        None,
        3,
        vec![EventId::new()],
        digest('b'),
        "implemented",
        "source",
    )
    .unwrap();
    assert_eq!(receipt.schema, EXECUTION_RECEIPT_SCHEMA);
    assert!(receipt.validate().is_ok());

    let mut success = receipt.clone();
    success.status = CapabilityExecutionState::Succeeded;
    success.effect_known = false;
    success.receipt_digest = success.digest();
    assert_eq!(
        success.validate().unwrap_err(),
        "execution_receipt_header_invalid"
    );
}

#[test]
fn receipt_contracts_reject_unknown_fields_and_tampered_digest() {
    let receipt = RunReceipt::new(
        RunId::new(),
        SessionId::new("session-1"),
        None,
        digest('a'),
        ExecutionStatus::Running,
        None,
        1,
        vec![EventId::new()],
        digest('b'),
        "implemented",
        "source",
        digest('c'),
        Vec::new(),
    )
    .unwrap();
    let mut unknown = receipt.to_json().unwrap();
    unknown["raw_output"] = json!("must-not-cross-receipt-boundary");
    assert_eq!(
        RunReceipt::from_json(&unknown).unwrap_err(),
        "run_receipt_decode_failed"
    );
    let mut tampered = receipt;
    tampered.result_digest = digest('z');
    assert_eq!(
        tampered.validate().unwrap_err(),
        "run_receipt_digest_mismatch"
    );
    let mut version = tampered;
    version.version = SchemaVersion::new(2, 0);
    assert_eq!(
        version.validate().unwrap_err(),
        "run_receipt_header_invalid"
    );
}
