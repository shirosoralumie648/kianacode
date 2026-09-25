use kiana_entrypoints::workbench_render::{
    TimelineApply, TimelineItemKind, TimelineRenderer, MAX_TIMELINE_BYTES, MAX_TIMELINE_ITEMS,
    MAX_TIMELINE_ITEM_BYTES,
};
use kiana_protocol::{
    ExecutionStatus, RequestId, ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent,
    PROTOCOL_SCHEMA,
};
use serde_json::{json, Value};

const SNAPSHOT_FIXTURE: &str = include_str!("fixtures/ui13-render-snapshot.json");

fn run_id(value: &str) -> RunId {
    RunId::parse_str(value).expect("fixture uses a valid run id")
}

fn response(run_id: RunId, status: ExecutionStatus, output: Value) -> ResponseEnvelope {
    let mut output = output;
    output["run_id"] = Value::String(run_id.to_string());
    ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status,
        output,
        error: None,
    }
}

fn envelope(epoch: &str, sequence: u64, event: RunStreamEvent) -> RunStreamEnvelope {
    RunStreamEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        epoch: epoch.to_owned(),
        sequence,
        ui_cursor: sequence,
        event,
    }
}

fn kind_name(kind: TimelineItemKind) -> &'static str {
    match kind {
        TimelineItemKind::Delta => "Delta",
        TimelineItemKind::Terminal => "Terminal",
        TimelineItemKind::Usage => "Usage",
        TimelineItemKind::ToolCall => "ToolCall",
        TimelineItemKind::Approval => "Approval",
        TimelineItemKind::Artifact => "Artifact",
        TimelineItemKind::Error => "Error",
        TimelineItemKind::Unknown => "Unknown",
        TimelineItemKind::Gap => "Gap",
        TimelineItemKind::Limit => "Limit",
    }
}

#[test]
fn render_snapshot_maps_typed_events_and_terminal_artifacts() {
    let fixture: Value = serde_json::from_str(SNAPSHOT_FIXTURE).expect("valid UI-13 fixture");
    let run_id = run_id(fixture["run_id"].as_str().unwrap());
    let mut renderer = TimelineRenderer::new(run_id);
    let events = fixture["events"].as_array().unwrap();

    for value in events {
        let envelope: RunStreamEnvelope = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(renderer.apply(&envelope), Ok(TimelineApply::Applied));
    }

    let actual: Vec<_> = renderer
        .items()
        .iter()
        .map(|item| kind_name(item.kind))
        .collect();
    let expected: Vec<_> = fixture["expected_kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|kind| kind.as_str().unwrap())
        .collect();
    assert_eq!(actual, expected);
    assert!(renderer.terminal());
    assert_eq!(renderer.cursor().sequence, 7);
    assert!(renderer.items().iter().any(|item| {
        item.kind == TimelineItemKind::Artifact && item.body.contains("src/main.rs")
    }));
}

#[test]
fn deny_first_cursor_handling_rejects_duplicates_gaps_foreign_runs_and_late_events() {
    let run_id = run_id("00000000-0000-0000-0000-000000000013");
    let foreign = run_id("00000000-0000-0000-0000-000000000099");
    let mut renderer = TimelineRenderer::new(run_id);
    let first = envelope(
        "epoch-a",
        1,
        RunStreamEvent::Delta {
            run_id,
            text: "first".to_owned(),
        },
    );
    assert_eq!(renderer.apply(&first), Ok(TimelineApply::Applied));
    assert_eq!(renderer.apply(&first), Ok(TimelineApply::IgnoredDuplicate));
    assert_eq!(renderer.cursor().sequence, 1);

    let gap = envelope(
        "epoch-a",
        3,
        RunStreamEvent::Delta {
            run_id,
            text: "gap".to_owned(),
        },
    );
    assert_eq!(renderer.apply(&gap), Ok(TimelineApply::GapRequiresSnapshot));
    assert!(renderer.gap_detected());
    assert!(!renderer.terminal());
    let count_after_gap = renderer.items().len();
    let after_gap = envelope(
        "epoch-a",
        4,
        RunStreamEvent::Delta {
            run_id,
            text: "must wait for snapshot".to_owned(),
        },
    );
    assert_eq!(
        renderer.apply(&after_gap),
        Ok(TimelineApply::GapRequiresSnapshot)
    );
    assert_eq!(renderer.items().len(), count_after_gap);

    let mut foreign_renderer = TimelineRenderer::new(run_id);
    let foreign_event = envelope(
        "epoch-a",
        1,
        RunStreamEvent::Delta {
            run_id: foreign,
            text: "foreign".to_owned(),
        },
    );
    assert_eq!(
        foreign_renderer.apply(&foreign_event),
        Err("timeline_event_run_mismatch".to_owned())
    );
    assert_eq!(foreign_renderer.cursor().sequence, 0);
    assert!(foreign_renderer.items().is_empty());

    let mut terminal_renderer = TimelineRenderer::new(run_id);
    assert_eq!(terminal_renderer.apply(&first), Ok(TimelineApply::Applied));
    let terminal = envelope(
        "epoch-a",
        2,
        RunStreamEvent::Terminal {
            run_id,
            response: response(
                run_id,
                ExecutionStatus::Completed,
                json!({"output": {"text": "done"}}),
            ),
        },
    );
    assert_eq!(
        terminal_renderer.apply(&terminal),
        Ok(TimelineApply::Applied)
    );
    let count_after_terminal = terminal_renderer.items().len();
    let late = envelope(
        "epoch-a",
        3,
        RunStreamEvent::Delta {
            run_id,
            text: "late".to_owned(),
        },
    );
    assert_eq!(
        terminal_renderer.apply(&late),
        Ok(TimelineApply::IgnoredAfterTerminal)
    );
    assert_eq!(terminal_renderer.items().len(), count_after_terminal);
}

#[test]
fn render_is_plain_text_redacted_and_bounded() {
    let run_id = run_id("00000000-0000-0000-0000-000000000013");
    let mut renderer = TimelineRenderer::new(run_id);
    let unsafe_text =
        "\u{1b}[31mred\u{1b}[0m\u{1b}]0;title\u{7} authorization: top-secret token=abc\u{1}";
    let event = RunStreamEnvelope::new(RunStreamEvent::Delta {
        run_id,
        text: unsafe_text.to_owned(),
    });
    assert_eq!(renderer.apply(&event), Ok(TimelineApply::Applied));
    let body = &renderer.items()[0].body;
    assert!(!body.contains('\u{1b}'));
    assert!(!body.contains("top-secret"));
    assert!(!body.contains("abc"));
    assert!(body.contains("[REDACTED]"));
    assert!(!body.contains('\u{1}'));

    let authorization = RunStreamEnvelope::new(RunStreamEvent::Delta {
        run_id,
        text: "Authorization: Bearer ui13-bearer-secret".to_owned(),
    });
    assert_eq!(renderer.apply(&authorization), Ok(TimelineApply::Applied));
    let authorization_body = &renderer.items()[1].body;
    assert!(!authorization_body.contains("ui13-bearer-secret"));
    assert_eq!(authorization_body, "Authorization: [REDACTED]");

    let mut item_renderer = TimelineRenderer::new(run_id);
    let oversized = "界".repeat(MAX_TIMELINE_ITEM_BYTES);
    assert_eq!(
        item_renderer.apply(&RunStreamEnvelope::new(RunStreamEvent::Delta {
            run_id,
            text: oversized,
        })),
        Ok(TimelineApply::Applied)
    );
    assert!(item_renderer.items()[0].truncated);
    assert!(item_renderer.items()[0].body.len() <= MAX_TIMELINE_ITEM_BYTES);
}

#[test]
fn render_exposes_collapse_loading_and_total_item_limits() {
    let run_id = run_id("00000000-0000-0000-0000-000000000013");
    let mut renderer = TimelineRenderer::new(run_id);
    assert_eq!(
        renderer.apply(&RunStreamEnvelope::new(RunStreamEvent::Usage {
            run_id,
            data: json!({"tokens": 3}),
        })),
        Ok(TimelineApply::Applied)
    );
    let id = renderer.items()[0].id.clone();
    renderer.toggle_collapsed(&id).unwrap();
    renderer.set_loading(&id, true).unwrap();
    assert!(renderer.items()[0].collapsed);
    assert!(renderer.items()[0].loading);

    let mut item_limited = TimelineRenderer::new(run_id);
    for _ in 0..(MAX_TIMELINE_ITEMS + 4) {
        let _ = item_limited.apply(&RunStreamEnvelope::new(RunStreamEvent::Delta {
            run_id,
            text: "item".to_owned(),
        }));
    }
    assert!(item_limited.limited());
    assert!(item_limited.items().len() <= MAX_TIMELINE_ITEMS);
    assert_eq!(
        item_limited.items().last().unwrap().kind,
        TimelineItemKind::Limit
    );

    let mut byte_limited = TimelineRenderer::new(run_id);
    for _ in 0..32 {
        let _ = byte_limited.apply(&RunStreamEnvelope::new(RunStreamEvent::Delta {
            run_id,
            text: "b".repeat(MAX_TIMELINE_ITEM_BYTES),
        }));
    }
    assert!(byte_limited.limited());
    assert!(byte_limited.total_bytes() <= MAX_TIMELINE_BYTES);
    assert!(byte_limited
        .items()
        .iter()
        .any(|item| item.kind == TimelineItemKind::Limit));
}
