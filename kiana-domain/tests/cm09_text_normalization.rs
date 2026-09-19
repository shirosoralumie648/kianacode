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
