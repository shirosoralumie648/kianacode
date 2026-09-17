use kiana_domain::{ExecutionOutputRef, InvocationId, RequestId, RunId};

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

#[test]
fn output_reference_round_trips_with_invocation_binding() {
    let output_id = RequestId::new();
    let reference = ExecutionOutputRef::new(
        output_id,
        Some(RunId::new()),
        InvocationId::from_uuid(output_id.as_uuid()),
        digest('c'),
        512,
        digest('s'),
        9_999,
    )
    .unwrap();
    reference.validate().unwrap();
    let encoded = serde_json::to_value(&reference).unwrap();
    assert_eq!(
        serde_json::from_value::<ExecutionOutputRef>(encoded).unwrap(),
        reference
    );
}

#[test]
fn output_reference_tampering_is_rejected() {
    let output_id = RequestId::new();
    let mut reference = ExecutionOutputRef::new(
        output_id,
        Some(RunId::new()),
        InvocationId::from_uuid(output_id.as_uuid()),
        digest('c'),
        512,
        digest('s'),
        9_999,
    )
    .unwrap();
    reference.size_bytes = 513;
    assert_eq!(
        reference.validate().unwrap_err(),
        "execution_output_reference_digest_mismatch"
    );
}
