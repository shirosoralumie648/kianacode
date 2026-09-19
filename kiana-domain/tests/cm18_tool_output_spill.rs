use kiana_domain::{
    journal_sha256, json_digest, InvocationId, RequestId, RunId, ToolOutputSpill, TurnId,
    MAX_TOOL_OUTPUT_PAGE_BYTES, MAX_TOOL_OUTPUT_PREVIEW_BYTES,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn spill(content: &[u8], binary: bool, page_size: u32) -> ToolOutputSpill {
    let output_id = RequestId::new();
    ToolOutputSpill::new(
        output_id,
        RunId::new(),
        TurnId::new(),
        InvocationId::from_uuid(output_id.as_uuid()),
        "shell.exec",
        digest("s"),
        format!("sha256:{}", journal_sha256(content)),
        content.len() as u64,
        "bounded preview",
        true,
        binary,
        9_999,
        page_size,
    )
    .unwrap()
}

#[test]
fn huge_tool_output_is_bounded_before_buffering() {
    let output_id = RequestId::new();
    let spill = ToolOutputSpill::new(
        output_id,
        RunId::new(),
        TurnId::new(),
        InvocationId::from_uuid(output_id.as_uuid()),
        "process.start",
        digest("s"),
        digest("complete-output"),
        8 * 1024 * 1024,
        "p".repeat(MAX_TOOL_OUTPUT_PREVIEW_BYTES),
        true,
        false,
        9_999,
        MAX_TOOL_OUTPUT_PAGE_BYTES as u32,
    )
    .unwrap();
    spill.validate().unwrap();
    assert!(spill.preview_truncated);
    assert_eq!(spill.preview.len(), MAX_TOOL_OUTPUT_PREVIEW_BYTES);
}

#[test]
fn foreign_run_reference_is_denied() {
    let content = b"bounded output";
    let spill = spill(content, false, 8);
    assert_eq!(
        spill
            .page(
                content,
                RunId::new(),
                spill.turn_id,
                &spill.capability_id,
                &spill.source_scope_digest,
                0,
                8,
                None,
            )
            .unwrap_err(),
        "tool_output_spill_run_mismatch"
    );
    assert_eq!(
        spill
            .page(
                content,
                spill.output_ref.run_id.unwrap(),
                spill.turn_id,
                &spill.capability_id,
                &digest("foreign-scope"),
                0,
                8,
                None,
            )
            .unwrap_err(),
        "tool_output_spill_scope_mismatch"
    );
}

#[test]
fn verified_page_retrieval_preserves_digest() {
    let content = b"abcdefghijklmnop";
    let spill = spill(content, false, 5);
    let run_id = spill.output_ref.run_id.unwrap();
    let first = spill
        .page(
            content,
            run_id,
            spill.turn_id,
            &spill.capability_id,
            &spill.source_scope_digest,
            0,
            5,
            None,
        )
        .unwrap();
    first.validate().unwrap();
    assert_eq!(first.data, b"abcde");
    let second = spill
        .page(
            content,
            run_id,
            spill.turn_id,
            &spill.capability_id,
            &spill.source_scope_digest,
            first.next_offset,
            5,
            first.next_cursor.as_deref(),
        )
        .unwrap();
    assert_eq!(second.data, b"fghij");
    assert_eq!(second.content_digest, spill.output_ref.content_hash);
    assert_eq!(
        spill
            .page(
                b"tampered",
                run_id,
                spill.turn_id,
                &spill.capability_id,
                &spill.source_scope_digest,
                0,
                5,
                None,
            )
            .unwrap_err(),
        "tool_output_spill_content_mismatch"
    );
}
