#[test]
fn h06_harness_uses_one_attempt_stream_accumulator() {
    let harness = include_str!("../src/harness.rs");
    let normalizer = include_str!("../src/stream_normalizer.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    for marker in [
        "ModelStreamAccumulator",
        "ModelDelta::ToolArguments",
        "ModelDelta::Usage",
        "ModelDelta::Stop",
        "STREAM_NORMALIZER_MAX_DELTAS",
        "STREAM_NORMALIZER_MAX_TEXT_BYTES",
        "STREAM_NORMALIZER_MAX_TOOL_BYTES",
        "split_invalid_tool_json_has_zero_dispatches",
        "eof_without_stop_never_completes",
        "late_delta_after_cancel_is_discarded",
        "model_stream_normalization_failed",
    ] {
        assert!(
            harness.contains(marker) || normalizer.contains(marker) || model.contains(marker),
            "H06 marker missing: {marker}"
        );
    }
    let create = harness
        .find("ModelStreamAccumulator::new(model_attempt_id)")
        .expect("each attempt must create one accumulator");
    let finish = harness
        .find("stream.finish(reply.output)")
        .expect("final output must pass through the same accumulator");
    assert!(create < finish);
    assert!(harness.contains("stream.push(delta)?"));
    assert!(normalizer.contains("validate_model_calls"));
    assert!(normalizer.contains("parse_bounded_json"));
    for forbidden in [
        "dispatch_tools_from_partial_json",
        "complete_on_eof_without_stop",
        "publish_late_delta_after_cancel",
        "second_stream_accumulator",
    ] {
        assert!(
            !harness.contains(forbidden) && !normalizer.contains(forbidden),
            "forbidden H06 path: {forbidden}"
        );
    }
}
