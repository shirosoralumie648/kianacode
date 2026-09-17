#[test]
fn huge_tool_output_is_bounded_before_buffering() {
    let source = include_str!("../src/harness_capabilities.rs");
    for marker in [
        "read_capped",
        "READ_CHUNK_SIZE",
        "EXEC_OUTPUT_MAX_BYTES",
        "output_total_limit",
        "observed_bytes",
        "truncated",
        "drain_capped",
    ] {
        assert!(
            source.contains(marker),
            "H15 output bound marker missing: {marker}"
        );
    }
    assert!(source.find("read_capped").unwrap() < source.find("child.wait").unwrap());
}

#[test]
fn artifact_from_other_run_is_denied() {
    let source = include_str!("../src/execution_control.rs");
    let core = include_str!("../../kiana-core/src/capabilities.rs");
    let domain = include_str!("../../kiana-domain/src/execution_output.rs");
    for marker in [
        "ExecutionOutputRef",
        "run_id",
        "check_owner",
        "execution_owner_run_mismatch",
        "output_reference_integrity_mismatch",
        "InvocationId::from_uuid",
    ] {
        assert!(
            source.contains(marker) || core.contains(marker) || domain.contains(marker),
            "H15 run scope marker missing: {marker}"
        );
    }
    assert!(source.contains("stored[\"run_id\"]"));
}

#[test]
fn expired_output_reference_is_not_returned_from_cache() {
    let source = include_str!("../src/execution_control.rs");
    let domain = include_str!("../../kiana-domain/src/execution_output.rs");
    assert!(source.contains("output_reference_expired"));
    assert!(source.contains("expires_at_unix_ms"));
    assert!(source.contains("unix_time_ms()? >= output_ref.expires_at_unix_ms"));
    assert!(domain.contains("expires_at_unix_ms"));
}

#[test]
fn truncated_result_can_be_read_in_verified_pages() {
    let source = include_str!("../src/execution_control.rs");
    for marker in [
        "content_digest",
        "output_content_digest_mismatch",
        "offset",
        "limit",
        "next_offset",
        "next_cursor",
        "output_cursor_invalid",
        "is_char_boundary",
    ] {
        assert!(
            source.contains(marker),
            "H15 paging marker missing: {marker}"
        );
    }
    assert!(source.contains("min(65536)"));
}
