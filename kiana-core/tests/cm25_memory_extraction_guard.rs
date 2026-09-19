#[test]
fn turn_extraction_keeps_quote_and_idempotency_boundary() {
    let source = include_str!("../../kiana-domain/src/memory_extraction.rs");
    for marker in [
        "MemoryExtractionRequest",
        "MemoryEvidenceQuote",
        "idempotency_key",
        "MEMORY_EXTRACTION_MAX_TOTAL_QUOTE_BYTES",
        "validate_proposal",
        "memory_extraction_quote_not_found",
        "memory_extraction_quote_mismatch",
        "source_cursor_start",
    ] {
        assert!(
            source.contains(marker),
            "CM-25 source marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}
