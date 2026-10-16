use kiana_domain::*;

#[test]
fn normalization_is_deterministic_for_unicode_and_cjk() {
    let profile = TextNormalizationProfile::default();
    let first = NormalizedText::normalize(&profile, "\u{feff}ＦooBar\r\n中文\r").unwrap();
    let second = NormalizedText::normalize(&profile, "\u{feff}ＦooBar\r\n中文\r").unwrap();
    assert_eq!(first, second);
    assert_eq!(first.text.as_deref(), Some("FooBar\n中文\n"));
    assert_eq!(first.language, "cjk");
    assert!(first.tokens.iter().any(|token| token == "foo"));
    assert!(first.tokens.iter().any(|token| token == "bar"));
    assert!(first.tokens.iter().any(|token| token == "中文"));
    first.validate().unwrap();
}

#[test]
fn secret_never_enters_index_or_embedding() {
    let reject = TextNormalizationProfile::new(SensitiveHandling::Reject).unwrap();
    assert_eq!(
        NormalizedText::normalize(&reject, "api_key=super-secret").unwrap_err(),
        "normalization_sensitive_input_rejected"
    );
    for source in [
        "api_key:super-secret",
        "password:super-secret",
        "secret:super-secret",
        "access_key:super-secret",
        "private_key:super-secret",
        "access_token=CM09_RAW",
        "refresh_token:CM09_RAW",
        "id_token=CM09_RAW",
        "api-key:CM09_RAW",
        "client_secret=CM09_RAW",
        "proxy-authorization: CM09_RAW",
    ] {
        assert_eq!(
            NormalizedText::normalize(&reject, source).unwrap_err(),
            "normalization_sensitive_input_rejected",
            "colon-form marker was not rejected: {source}"
        );
    }

    let redact = TextNormalizationProfile::new(SensitiveHandling::Redact).unwrap();
    let redacted = NormalizedText::normalize(
        &redact,
        "Authorization: Bearer super-secret user@example.com",
    )
    .unwrap();
    assert_eq!(redacted.disposition, SensitiveDisposition::Redacted);
    let text = redacted.text.as_deref().unwrap();
    assert!(!text.contains("super-secret"));
    assert!(!text.contains("user@example.com"));
    assert!(redacted.embedding.secret_free);
    assert!(redacted.llm.secret_free);
    redacted.validate().unwrap();
    assert_eq!(
        NormalizedText::normalize(&reject, "Authorization: Bearer [REDACTED]suffix").unwrap_err(),
        "normalization_sensitive_input_rejected"
    );

    let reference = TextNormalizationProfile::new(SensitiveHandling::ReferenceOnly).unwrap();
    let reference_only = NormalizedText::normalize(&reference, "password=do-not-store").unwrap();
    assert_eq!(
        reference_only.disposition,
        SensitiveDisposition::ReferenceOnly
    );
    assert!(reference_only.text.is_none());
    assert!(reference_only.tokens.is_empty());
    reference_only.validate().unwrap();
}

#[test]
fn malformed_json_like_secret_key_is_rejected_without_original_fallback() {
    let reject = TextNormalizationProfile::new(SensitiveHandling::Reject).unwrap();
    assert_eq!(
        NormalizedText::normalize(&reject, r#"{"access_token":"CM09_RAW",}"#,).unwrap_err(),
        "normalization_sensitive_input_rejected"
    );
}

#[test]
fn valid_json_secret_is_rejected_and_redacted_before_tokens() {
    let source = r#"{"api_key":"CM09_RAW","note":"safe"}"#;
    let reject = TextNormalizationProfile::new(SensitiveHandling::Reject).unwrap();
    assert_eq!(
        NormalizedText::normalize(&reject, source).unwrap_err(),
        "normalization_sensitive_input_rejected"
    );

    let redacted = NormalizedText::normalize(
        &TextNormalizationProfile::new(SensitiveHandling::Redact).unwrap(),
        source,
    )
    .unwrap();
    let text = redacted.text.as_deref().expect("redacted text");
    assert!(!text.contains("CM09_RAW"));
    assert!(text.contains("[REDACTED]"));
    assert!(redacted.tokens.iter().all(|token| token != "CM09_RAW"));
    redacted.validate().unwrap();

    let reference = NormalizedText::normalize(
        &TextNormalizationProfile::new(SensitiveHandling::ReferenceOnly).unwrap(),
        source,
    )
    .unwrap();
    assert!(reference.text.is_none());
    assert!(reference.tokens.is_empty());
    reference.validate().unwrap();
}

#[test]
fn formatted_phone_is_rejected_and_redacted_before_tokens() {
    let source = "Call 138 0013 8000 today";
    let reject = TextNormalizationProfile::new(SensitiveHandling::Reject).unwrap();
    assert_eq!(
        NormalizedText::normalize(&reject, source).unwrap_err(),
        "normalization_sensitive_input_rejected"
    );

    let redacted = NormalizedText::normalize(
        &TextNormalizationProfile::new(SensitiveHandling::Redact).unwrap(),
        source,
    )
    .unwrap();
    let text = redacted.text.as_deref().expect("redacted text");
    assert!(!text.contains("138"));
    assert!(text.contains("[PII_REDACTED]"));
    redacted.validate().unwrap();

    let reference = NormalizedText::normalize(
        &TextNormalizationProfile::new(SensitiveHandling::ReferenceOnly).unwrap(),
        source,
    )
    .unwrap();
    assert!(reference.text.is_none());
    assert!(reference.tokens.is_empty());
}

#[test]
fn embedding_and_llm_metadata_are_separate_versioned_contracts() {
    let result = NormalizedText::normalize(&TextNormalizationProfile::default(), "hello").unwrap();
    assert_eq!(result.embedding.schema, EMBEDDING_METADATA_SCHEMA);
    assert_eq!(result.llm.schema, LLM_TEXT_METADATA_SCHEMA);
    assert_ne!(result.embedding.metadata_digest, result.llm.metadata_digest);
    assert_eq!(
        schema_contract(TEXT_NORMALIZATION_PROFILE_SCHEMA)
            .unwrap()
            .owner_crate,
        "kiana-domain"
    );
}
