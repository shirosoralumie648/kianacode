use kiana_domain::{ModelAttemptId, ModelDelta, ModelOutput, ModelToolCall};
use kiana_runner::ModelStreamAccumulator;
use serde_json::json;

#[test]
fn interleaved_tool_deltas_equal_nonstream_response() {
    let mut accumulator = ModelStreamAccumulator::new(ModelAttemptId::new());
    accumulator
        .push(ModelDelta::Text {
            text: "before".to_owned(),
        })
        .unwrap();
    accumulator
        .push(ModelDelta::ToolArguments {
            index: 0,
            id: "call-1".to_owned(),
            name: "shell".to_owned(),
            partial_json: "{\"command\":\"ls\"}".to_owned(),
        })
        .unwrap();
    accumulator
        .push(ModelDelta::Stop {
            reason: "tool_use".to_owned(),
        })
        .unwrap();
    let output = accumulator
        .finish(ModelOutput {
            text: "before".to_owned(),
            tool_calls: vec![ModelToolCall {
                id: "call-1".to_owned(),
                name: "shell".to_owned(),
                arguments: json!({"command":"ls"}),
            }],
            stop_reason: Some("tool_use".to_owned()),
            ..ModelOutput::default()
        })
        .unwrap();
    assert_eq!(output.tool_calls.len(), 1);
    assert_eq!(output.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn split_invalid_tool_json_has_zero_dispatches() {
    let mut accumulator = ModelStreamAccumulator::new(ModelAttemptId::new());
    accumulator
        .push(ModelDelta::ToolArguments {
            index: 0,
            id: "call-1".to_owned(),
            name: "shell".to_owned(),
            partial_json: "{\"command\":".to_owned(),
        })
        .unwrap();
    accumulator
        .push(ModelDelta::Stop {
            reason: "tool_use".to_owned(),
        })
        .unwrap();
    let error = accumulator
        .finish(ModelOutput {
            stop_reason: Some("tool_use".to_owned()),
            ..ModelOutput::default()
        })
        .unwrap_err();
    assert!(error.starts_with("split_invalid_tool_json_has_zero_dispatches"));
}

#[test]
fn eof_and_late_cancel_deltas_never_complete() {
    let mut eof = ModelStreamAccumulator::new(ModelAttemptId::new());
    eof.push(ModelDelta::Text {
        text: "partial".to_owned(),
    })
    .unwrap();
    assert_eq!(
        eof.finish(ModelOutput {
            text: "partial".to_owned(),
            ..ModelOutput::default()
        })
        .unwrap_err(),
        "eof_without_stop_never_completes"
    );

    let mut cancelled = ModelStreamAccumulator::new(ModelAttemptId::new());
    cancelled.cancel();
    assert_eq!(
        cancelled
            .push(ModelDelta::Text {
                text: "late".to_owned(),
            })
            .unwrap_err(),
        "late_delta_after_cancel_is_discarded"
    );
}
