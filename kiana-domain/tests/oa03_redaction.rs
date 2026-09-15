use kiana_domain::{
    encode_bounded_text, encode_bounded_value, redact_text_with_profile, redact_with_profile,
    DataClass, RedactionProfile, RedactionSignal, REDACTION_PROFILE_SCHEMA,
};
use serde_json::json;

#[test]
fn profiles_are_versioned_digest_bound_and_closed() {
    for signal in [
        RedactionSignal::Log,
        RedactionSignal::Metric,
        RedactionSignal::Trace,
        RedactionSignal::Audit,
        RedactionSignal::Export,
    ] {
        let profile = RedactionProfile::for_signal(signal);
        assert_eq!(profile.schema, REDACTION_PROFILE_SCHEMA);
        assert_eq!(profile.data_class, DataClass::Internal);
        profile.validate().unwrap();
        let encoded = serde_json::to_string(&profile).unwrap();
        assert_eq!(
            serde_json::from_str::<RedactionProfile>(&encoded).unwrap(),
            profile
        );
        let mut forged = profile.clone();
        forged.max_bytes = forged.max_bytes.saturating_sub(1);
        assert_eq!(
            forged.validate().unwrap_err(),
            "redaction_profile_digest_mismatch"
        );
        assert!(serde_json::from_str::<RedactionProfile>(
            &encoded.replace('}', ",\"untrusted\":true}")
        )
        .is_err());
    }
}

#[test]
fn every_signal_boundary_redacts_nested_secret_sentinels_before_encoding() {
    let input = json!({
        "message": "ordinary",
        "authorization": "Bearer raw-bearer-sentinel",
        "nested": [{"api_key": "raw-api-key-sentinel", "path": "src/lib.rs"}],
        "secret_ref": "vault://provider/anthropic",
        "tokens_used": 42,
    });
    for signal in [
        RedactionSignal::Log,
        RedactionSignal::Metric,
        RedactionSignal::Trace,
        RedactionSignal::Audit,
        RedactionSignal::Export,
    ] {
        let profile = RedactionProfile::for_signal(signal);
        let encoded = encode_bounded_value(&profile, &input).unwrap();
        let text = encoded.value.to_string();
        assert!(!text.contains("raw-bearer-sentinel"), "{text}");
        assert!(!text.contains("raw-api-key-sentinel"), "{text}");
        assert!(text.contains("vault://provider/anthropic"), "{text}");
        assert_eq!(encoded.profile_digest, profile.profile_digest);
        assert_eq!(
            redact_with_profile(&profile, &input).unwrap(),
            encoded.value
        );
    }

    let text_profile = RedactionProfile::for_signal(RedactionSignal::Trace);
    let redacted = encode_bounded_text(
        &text_profile,
        "provider failed Authorization: Bearer raw-stream-sentinel, retryable=true",
    )
    .unwrap();
    assert!(!redacted.text.contains("raw-stream-sentinel"));
    assert_eq!(
        redact_text_with_profile(&text_profile, &redacted.text).unwrap(),
        redacted.text
    );
}

#[test]
fn encoder_errors_are_fail_closed_without_returning_original_payload() {
    let tiny = RedactionProfile::new(RedactionSignal::Audit, DataClass::Restricted, 8, 2).unwrap();
    let oversized = json!({"message":"this is larger than eight bytes"});
    assert_eq!(
        encode_bounded_value(&tiny, &oversized).unwrap_err(),
        "redaction_value_too_large"
    );
    assert_eq!(
        encode_bounded_text(&tiny, "contains\0nul").unwrap_err(),
        "redaction_nul_forbidden"
    );

    let mut nested = json!("leaf");
    for _ in 0..4 {
        nested = json!([nested]);
    }
    assert_eq!(
        encode_bounded_value(&tiny, &nested).unwrap_err(),
        "redaction_value_depth_exceeded"
    );

    let malformed = json!({"secret": "Authorization: Bearer raw-unbounded-sentinel"});
    let profile =
        RedactionProfile::new(RedactionSignal::Log, DataClass::Internal, 4096, 8).unwrap();
    let encoded = encode_bounded_value(&profile, &malformed).unwrap();
    assert!(!encoded.value.to_string().contains("raw-unbounded-sentinel"));
}
