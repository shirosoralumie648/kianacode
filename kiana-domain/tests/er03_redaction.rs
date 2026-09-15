use kiana_domain::{
    encode_bounded_value, RedactionProfile, RedactionSignal, RequestId, RuntimeEvent,
    MAX_REDACTION_DEPTH,
};
use serde_json::json;

#[test]
fn secret_never_enters_event_or_receipt() {
    let profile = RedactionProfile::new(
        RedactionSignal::Audit,
        kiana_domain::DataClass::Restricted,
        64 * 1024,
        MAX_REDACTION_DEPTH,
    )
    .unwrap();
    let encoded = encode_bounded_value(
        &profile,
        &json!({
            "api_key": "er03-secret",
            "nested": ["Authorization: Bearer er03-bearer"],
            "secret_ref": "vault://er03/reference",
            "artifact_refs": ["artifact:one"]
        }),
    )
    .unwrap();
    let text = encoded.value.to_string();
    assert!(!text.contains("er03-secret"));
    assert!(!text.contains("er03-bearer"));
    assert_eq!(encoded.value["secret_ref"], "vault://er03/reference");
    assert_eq!(encoded.value["artifact_refs"][0], "artifact:one");
}

#[test]
fn redaction_changes_snapshot_marks_non_resumable() {
    let profile = RedactionProfile::for_signal(RedactionSignal::Audit);
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "run.completed",
        json!({"run_id":"run-1"}),
    )
    .unwrap()
    .with_redaction_metadata(
        profile.profile_digest.clone(),
        false,
        Some(7),
        vec!["artifact:er03".to_owned()],
    );
    event.validate_redaction_metadata().unwrap();
    assert_eq!(event.payload_recoverable, Some(false));
    assert_eq!(event.data_epoch, Some(7));
    assert_eq!(event.artifact_refs, vec!["artifact:er03"]);
    let mut forged = event.clone();
    forged.payload_recoverable = Some(true);
    assert_eq!(
        forged.validate_redaction_metadata().unwrap_err(),
        "event_redacted_payload_recoverable"
    );
}

#[test]
fn oversize_payload_is_rejected() {
    let small = RedactionProfile::new(
        RedactionSignal::Audit,
        kiana_domain::DataClass::Internal,
        16,
        2,
    )
    .unwrap();
    assert_eq!(
        encode_bounded_value(&small, &json!({"text":"01234567890123456789"})).unwrap_err(),
        "redaction_value_too_large"
    );
    let deep = json!({"a":{"b":{"c":true}}});
    assert_eq!(
        encode_bounded_value(&small, &deep).unwrap_err(),
        "redaction_value_depth_exceeded"
    );
}
