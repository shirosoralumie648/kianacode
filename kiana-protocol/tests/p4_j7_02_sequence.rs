use kiana_protocol::{
    ExecutionStatus, RequestId, ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent,
    UiCursor, PROTOCOL_SCHEMA,
};

#[test]
fn run_stream_sequence_is_monotonic() {
    let run_id = RunId::new();
    let mut cursor = UiCursor::default();
    let first = RunStreamEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        epoch: "epoch-one".to_owned(),
        sequence: 1,
        ui_cursor: 1,
        event: RunStreamEvent::Delta {
            run_id,
            text: "one".to_owned(),
        },
    };
    let second = RunStreamEnvelope {
        sequence: 2,
        event: RunStreamEvent::Usage {
            run_id,
            data: serde_json::json!({"input_tokens": 1}),
        },
        ..first.clone()
    };
    assert!(first.advance_cursor(&mut cursor).unwrap());
    assert_eq!(cursor.sequence, 1);
    assert!(second.advance_cursor(&mut cursor).unwrap());
    assert_eq!(cursor.sequence, 2);
    assert!(!second.advance_cursor(&mut cursor).unwrap());

    let terminal = RunStreamEnvelope {
        sequence: 3,
        event: RunStreamEvent::Terminal {
            run_id,
            response: ResponseEnvelope {
                schema: PROTOCOL_SCHEMA.to_owned(),
                request_id: RequestId::new(),
                status: ExecutionStatus::Completed,
                output: serde_json::json!({}),
                error: None,
            },
        },
        ..second.clone()
    };
    assert!(terminal.advance_cursor(&mut cursor).unwrap());
    assert_eq!(cursor.sequence, 3);

    let gap = RunStreamEnvelope {
        sequence: 5,
        ..terminal.clone()
    };
    assert_eq!(
        gap.advance_cursor(&mut cursor).unwrap_err(),
        "stream_sequence_gap"
    );
    let epoch_change = RunStreamEnvelope {
        epoch: "epoch-two".to_owned(),
        sequence: 4,
        ..terminal
    };
    assert_eq!(
        epoch_change.advance_cursor(&mut cursor).unwrap_err(),
        "stream_epoch_changed"
    );
}
