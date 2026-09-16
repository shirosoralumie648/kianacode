#[test]
fn provider_content_boundary_is_validated_before_wire_compilation() {
    let request = include_str!("../src/request.rs");
    let domain = include_str!("../../kiana-domain/src/model.rs");
    let response = include_str!("../src/response.rs");
    for marker in [
        "normalize_structured_request",
        "opaque_item_cannot_cross_provider",
        "unsupported_content_block_fails_before_request",
        "provider_continuation_unsupported",
        "content_blocks",
    ] {
        assert!(
            request.contains(marker) || domain.contains(marker),
            "missing H04 content boundary marker {marker}"
        );
    }
    assert!(request.contains("anthropic_body"));
    assert!(request.contains("responses_body"));
    assert!(request.contains("gemini_body"));
    assert!(response.contains("ModelOutput"));
    assert!(!request.contains("raw_response"));
}
