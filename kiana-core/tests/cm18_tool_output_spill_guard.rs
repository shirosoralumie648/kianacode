#[test]
fn tool_output_spill_keeps_bounded_preview_and_scope_page_contract() {
    let source = include_str!("../../kiana-domain/src/tool_output_spill.rs");
    for marker in [
        "ToolOutputSpill",
        "ToolOutputPage",
        "preview_truncated",
        "capability_id",
        "source_scope_digest",
        "turn_id",
        "tool_output_spill_run_mismatch",
        "tool_output_page_cursor_invalid",
        "MAX_TOOL_OUTPUT_PAGE_BYTES",
        "journal_sha256",
    ] {
        assert!(
            source.contains(marker),
            "CM-18 source marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}
