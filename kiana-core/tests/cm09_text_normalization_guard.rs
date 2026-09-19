#[test]
fn normalization_keeps_unicode_sensitive_and_metadata_boundaries_explicit() {
    let source = include_str!("../../kiana-domain/src/text_normalization.rs");
    let redaction = include_str!("../../kiana-domain/src/redaction.rs");
    let memory = include_str!("../../kiana-domain/src/memory.rs");
    for marker in [
        "TextNormalizationProfile",
        "SensitiveHandling",
        "SensitiveDisposition",
        "canonical_text",
        "fullwidth_ascii",
        "identifier_tokens",
        "language_hint",
        "EmbeddingMetadata",
        "LlmTextMetadata",
        "ReferenceOnly",
        "normalization_sensitive_input_rejected",
        "secret_free",
        "MAX_NORMALIZED_BYTES",
        "MAX_NORMALIZED_TOKENS",
    ] {
        assert!(
            source.contains(marker) || redaction.contains(marker) || memory.contains(marker),
            "CM-09 marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("std::net"));
}
