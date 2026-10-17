//! UI-13 Workbench render fixtures.
//!
//! The renderer is a disposable display projection: it must map every additive stream event to a
//! stable item kind, keep item bodies plain text, and refuse to let arrival order rewrite an
//! earlier fact. These fixtures drive the typed `TimelineRenderer` directly; they do not open a
//! terminal, read EventLog facts, call a provider or prove any external outcome.

use kiana_entrypoints::workbench_render::{
    TimelineApply, TimelineDisplayRole, TimelineItemKind, TimelineRenderer,
    MAX_TIMELINE_ARTIFACTS_PER_RESPONSE, MAX_TIMELINE_BYTES, MAX_TIMELINE_DIFF_BYTES,
    MAX_TIMELINE_ITEMS, MAX_TIMELINE_ITEM_BYTES, MAX_TIMELINE_JSON_BYTES,
};
use kiana_protocol::{
    ExecutionStatus, RequestId, ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent,
    PROTOCOL_SCHEMA,
};
use serde_json::{json, Value};

const SNAPSHOT_FIXTURE: &str = include_str!("fixtures/ui13-render-snapshot.json");
const MAIN_RUN_ID: &str = "00000000-0000-0000-0000-000000000013";

fn fixture() -> Value {
    serde_json::from_str(SNAPSHOT_FIXTURE).expect("valid UI-13 fixture")
}

fn run_id(value: &str) -> RunId {
    RunId::parse_str(value).expect("fixture uses a valid run id")
}

fn main_run_id() -> RunId {
    run_id(MAIN_RUN_ID)
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

fn terminal(run_id: RunId, artifacts: Value) -> RunStreamEnvelope {
    envelope(
        "daemon-ui13",
        1,
        RunStreamEvent::Terminal {
            run_id,
            response: response(
                run_id,
                ExecutionStatus::Completed,
                json!({"output": {"text": "done"}, "artifacts": artifacts}),
            ),
        },
    )
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

fn apply_name(apply: &TimelineApply) -> &'static str {
    match apply {
        TimelineApply::Applied => "Applied",
        TimelineApply::IgnoredDuplicate => "IgnoredDuplicate",
        TimelineApply::IgnoredAfterTerminal => "IgnoredAfterTerminal",
        TimelineApply::GapRequiresSnapshot => "GapRequiresSnapshot",
    }
}

fn scenario<'a>(fixture: &'a Value, name: &str) -> &'a Value {
    fixture["scenarios"]
        .as_array()
        .expect("scenarios")
        .iter()
        .find(|scenario| scenario["name"] == name)
        .unwrap_or_else(|| panic!("missing UI-13 scenario: {name}"))
}

/// A scenario event names only `sequence` and `event`; the envelope identity is fixture-owned.
fn scenario_envelope(epoch: &str, value: &Value) -> RunStreamEnvelope {
    let sequence = value["sequence"].as_u64().expect("scenario sequence");
    RunStreamEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        epoch: epoch.to_owned(),
        sequence,
        ui_cursor: sequence,
        event: serde_json::from_value(value["event"].clone()).expect("scenario event"),
    }
}

fn apply_scenario(scenario: &Value) -> TimelineRenderer {
    let epoch = scenario["epoch"].as_str().expect("scenario epoch");
    let mut renderer = TimelineRenderer::new(main_run_id());
    for value in scenario["events"].as_array().expect("scenario events") {
        assert_eq!(
            renderer.apply(&scenario_envelope(epoch, value)),
            Ok(TimelineApply::Applied),
            "scenario {} must apply its base events",
            scenario["name"]
        );
    }
    renderer
}

fn items(
    renderer: &TimelineRenderer,
    kind: TimelineItemKind,
) -> Vec<&kiana_entrypoints::workbench_render::TimelineItem> {
    renderer
        .items()
        .iter()
        .filter(|item| item.kind == kind)
        .collect()
}

fn assert_bodies(scenario: &Value, renderer: &TimelineRenderer, key: &str, expected: bool) {
    let Some(entries) = scenario[key].as_array() else {
        return;
    };
    for entry in entries {
        let expected_kind = entry["kind"].as_str().expect("expected kind");
        let text = entry["text"].as_str().expect("expected text");
        let matched = renderer
            .items()
            .iter()
            .map(|item| item.kind)
            .find(|candidate| kind_name(*candidate) == expected_kind)
            .is_some_and(|kind| {
                items(renderer, kind)
                    .iter()
                    .any(|item| item.body.contains(text))
            });
        assert_eq!(
            matched,
            expected,
            "scenario {}: kind {} body {} expected to {} {:?}",
            scenario["name"],
            expected_kind,
            if expected { "must" } else { "must not" },
            key,
            text
        );
    }
}

/// Apply a declarative scenario, then assert kinds, cursor, terminal fence, late-event outcomes and
/// body constraints. Returns the renderer after the late events were refused.
fn assert_scenario(scenario: &Value) -> TimelineRenderer {
    let mut renderer = apply_scenario(scenario);
    let actual: Vec<&str> = renderer
        .items()
        .iter()
        .map(|item| kind_name(item.kind))
        .collect();
    let expected: Vec<&str> = scenario["expected_kinds"]
        .as_array()
        .expect("expected kinds")
        .iter()
        .map(|kind| kind.as_str().expect("kind name"))
        .collect();
    assert_eq!(actual, expected, "scenario {}", scenario["name"]);
    assert_eq!(
        renderer.cursor().sequence,
        scenario["expected_cursor"]
            .as_u64()
            .expect("expected cursor"),
        "scenario {} cursor",
        scenario["name"]
    );
    assert_eq!(
        renderer.terminal(),
        scenario["expected_terminal"]
            .as_bool()
            .expect("expected terminal"),
        "scenario {} terminal fence",
        scenario["name"]
    );

    let before_late = renderer.items().len();
    let late = scenario["late_events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let late_applies = scenario["expected_late_applies"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        late.len(),
        late_applies.len(),
        "scenario {} late-event expectations",
        scenario["name"]
    );
    let epoch = scenario["epoch"].as_str().expect("scenario epoch");
    for (value, expected) in late.iter().zip(late_applies) {
        let apply = renderer
            .apply(&scenario_envelope(epoch, value))
            .expect("late scenario event must be classified, not rejected");
        assert_eq!(
            apply_name(&apply),
            expected.as_str().expect("late apply name"),
            "scenario {} late-event outcome",
            scenario["name"]
        );
    }
    if !late.is_empty() {
        assert_eq!(
            renderer.items().len(),
            before_late,
            "scenario {} must not append a fenced or gap-blocked event",
            scenario["name"]
        );
    }

    assert_bodies(scenario, &renderer, "expect_bodies_contain", true);
    assert_bodies(scenario, &renderer, "expect_bodies_exclude", false);
    renderer
}

#[test]
fn render_snapshot_maps_typed_events_and_terminal_artifacts() {
    let fixture = fixture();
    assert_eq!(fixture["schema"], "kiana.ui13-render-snapshot.v1");
    assert_eq!(fixture["step"], "UI-13");
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
    // A record error stays visible in the same run that later completes.
    assert!(
        renderer
            .items()
            .iter()
            .any(|item| item.kind == TimelineItemKind::Error
                && item.body.contains("provider_timeout"))
    );
    // Every artifact referenced by the terminal response is projected exactly once.
    assert_eq!(items(&renderer, TimelineItemKind::Artifact).len(), 2);
}

#[test]
fn deny_first_cursor_handling_rejects_duplicates_gaps_foreign_runs_and_late_events() {
    let primary = main_run_id();
    let foreign = run_id("00000000-0000-0000-0000-000000000099");
    let mut renderer = TimelineRenderer::new(primary);
    let first = envelope(
        "epoch-a",
        1,
        RunStreamEvent::Delta {
            run_id: primary,
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
            run_id: primary,
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
            run_id: primary,
            text: "must wait for snapshot".to_owned(),
        },
    );
    assert_eq!(
        renderer.apply(&after_gap),
        Ok(TimelineApply::GapRequiresSnapshot)
    );
    assert_eq!(renderer.items().len(), count_after_gap);

    let mut foreign_renderer = TimelineRenderer::new(primary);
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

    let mut epoch_renderer = TimelineRenderer::new(primary);
    assert_eq!(epoch_renderer.apply(&first), Ok(TimelineApply::Applied));
    let epoch_changed = envelope(
        "epoch-b",
        2,
        RunStreamEvent::Delta {
            run_id: primary,
            text: "other daemon incarnation".to_owned(),
        },
    );
    assert_eq!(
        epoch_renderer.apply(&epoch_changed),
        Err("stream_epoch_changed".to_owned())
    );
    assert_eq!(epoch_renderer.cursor().sequence, 1);
    assert_eq!(epoch_renderer.items().len(), 1);

    let mut schema_renderer = TimelineRenderer::new(primary);
    let mut drifted = first.clone();
    drifted.schema = "kiana.protocol.v0".to_owned();
    assert_eq!(
        schema_renderer.apply(&drifted),
        Err("timeline_schema_mismatch".to_owned())
    );
    assert!(schema_renderer.items().is_empty());

    let mut terminal_renderer = TimelineRenderer::new(primary);
    assert_eq!(terminal_renderer.apply(&first), Ok(TimelineApply::Applied));
    let terminal = envelope(
        "epoch-a",
        2,
        RunStreamEvent::Terminal {
            run_id: primary,
            response: response(
                primary,
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
            run_id: primary,
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
    let run_id = main_run_id();
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
fn untrusted_markdown_is_rendered_as_inert_plain_text() {
    let run_id = main_run_id();
    for markdown in [
        "**bold** and _italic_",
        "```rust\nfn main() {}\n```",
        "<script>alert(1)</script>",
        "[link](javascript:alert(1))",
        "![image](file:///etc/passwd)",
    ] {
        let mut renderer = TimelineRenderer::new(run_id);
        assert_eq!(
            renderer.apply(&RunStreamEnvelope::new(RunStreamEvent::Delta {
                run_id,
                text: markdown.to_owned(),
            })),
            Ok(TimelineApply::Applied)
        );
        assert_eq!(renderer.items()[0].body, markdown);
    }
}

#[test]
fn render_exposes_collapse_loading_and_total_item_limits() {
    let run_id = main_run_id();
    let mut renderer = TimelineRenderer::new(run_id);
    assert_eq!(
        renderer.apply(&RunStreamEnvelope::new(RunStreamEvent::Usage {
            run_id,
            data: json!({"tokens": 3}),
        })),
        Ok(TimelineApply::Applied)
    );
    let id = renderer.items()[0].id.clone();
    assert!(renderer.items()[0].collapsed);
    renderer.toggle_collapsed(&id).unwrap();
    assert!(!renderer.items()[0].collapsed);
    renderer.toggle_collapsed(&id).unwrap();
    renderer.set_loading(&id, true).unwrap();
    assert!(renderer.items()[0].collapsed);
    assert!(renderer.items()[0].loading);
    assert_eq!(
        renderer.toggle_collapsed("missing-item"),
        Err("timeline_item_not_found".to_owned())
    );

    let mut json_limited = TimelineRenderer::new(run_id);
    assert_eq!(
        json_limited.apply(&RunStreamEnvelope::new(RunStreamEvent::Usage {
            run_id,
            data: json!({"payload": "x".repeat(MAX_TIMELINE_JSON_BYTES * 2)}),
        })),
        Ok(TimelineApply::Applied)
    );
    assert!(json_limited.items()[0].truncated);
    assert!(
        json_limited.items()[0].body.len() <= MAX_TIMELINE_JSON_BYTES,
        "structured JSON rows must respect their own bound"
    );

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

#[test]
fn authorization_redaction_preserves_header_spacing_and_safe_following_lines() {
    let run_id = main_run_id();
    for (input, expected) in [
        (
            "Authorization: Bearer fixture-credential",
            "Authorization: [REDACTED]",
        ),
        (
            "Authorization:\t Basic fixture-credential",
            "Authorization:\t [REDACTED]",
        ),
        (
            "Authorization:Bearer fixture-credential",
            "Authorization:[REDACTED]",
        ),
        (
            "Authorization:  Bearer fixture-credential\npublic next line",
            "Authorization:  [REDACTED]\npublic next line",
        ),
    ] {
        let mut renderer = TimelineRenderer::new(run_id);
        renderer
            .apply(&RunStreamEnvelope::new(RunStreamEvent::Delta {
                run_id,
                text: input.to_owned(),
            }))
            .unwrap();
        assert_eq!(renderer.items()[0].body, expected);
        assert!(!renderer.items()[0].body.contains("fixture-credential"));
    }
}

#[test]
fn arrival_order_and_replays_never_overwrite_the_recorded_error() {
    let fixture = fixture();
    let scenario = scenario(&fixture, "arrival_order_never_overwrites_a_recorded_error");
    let mut renderer = assert_scenario(scenario);

    assert_eq!(renderer.items()[0].kind, TimelineItemKind::Error);
    let error_before = renderer.items()[0].clone();
    assert!(error_before.body.contains("provider_timeout"));

    let epoch = scenario["epoch"].as_str().expect("scenario epoch");
    let replay = scenario_envelope(epoch, &scenario["events"][1]);
    assert_eq!(renderer.apply(&replay), Ok(TimelineApply::IgnoredDuplicate));
    let error_after = renderer.items()[0].clone();
    assert_eq!(error_after.id, error_before.id);
    assert_eq!(error_after.body, error_before.body);
    assert_eq!(items(&renderer, TimelineItemKind::Delta).len(), 1);
}

#[test]
fn tool_results_usage_approvals_and_errors_are_never_assistant_text() {
    for kind in [TimelineItemKind::Delta, TimelineItemKind::Terminal] {
        assert_eq!(kind.display_role(), TimelineDisplayRole::Assistant);
    }
    assert_eq!(
        TimelineItemKind::Artifact.display_role(),
        TimelineDisplayRole::Changed
    );
    for kind in [
        TimelineItemKind::Usage,
        TimelineItemKind::ToolCall,
        TimelineItemKind::Approval,
        TimelineItemKind::Error,
        TimelineItemKind::Unknown,
        TimelineItemKind::Gap,
        TimelineItemKind::Limit,
    ] {
        assert_eq!(kind.display_role(), TimelineDisplayRole::System);
    }

    let run_id = main_run_id();
    let mut renderer = TimelineRenderer::new(run_id);
    for event in [
        RunStreamEvent::Usage {
            run_id,
            data: json!({"input_tokens": 3}),
        },
        RunStreamEvent::ToolCall {
            run_id,
            data: json!({"tool": "shell", "result": "recorded_output"}),
        },
        RunStreamEvent::ApprovalRequested {
            run_id,
            data: json!({"approval_id": "approval-13"}),
        },
        RunStreamEvent::Error {
            run_id,
            data: json!({"code": "provider_timeout"}),
        },
    ] {
        assert_eq!(renderer.apply_event(event), Ok(TimelineApply::Applied));
    }

    assert!(renderer
        .items()
        .iter()
        .all(|item| item.kind.display_role() != TimelineDisplayRole::Assistant));
    let tool = items(&renderer, TimelineItemKind::ToolCall);
    assert_eq!(tool.len(), 1);
    assert!(tool[0].body.contains("recorded_output"));
    assert!(items(&renderer, TimelineItemKind::Delta).is_empty());
    assert!(items(&renderer, TimelineItemKind::Terminal).is_empty());
}

#[test]
fn binary_and_unspecified_artifact_payloads_are_never_inlined() {
    let fixture = fixture();
    assert_scenario(scenario(&fixture, "binary_mime_artifact_is_never_inlined"));

    let run_id = main_run_id();
    let mut renderer = TimelineRenderer::new(run_id);
    assert_eq!(
        renderer.apply(&terminal(
            run_id,
            json!([
                {"id": "blob-13", "data": "UElDTy1VTlNQRUNJRklFRC1EQVRB"},
                {"id": "hex-13", "mime": "application/octet-stream", "encoding": "hex", "content": "5049434f"},
                {"id": "text-13", "mime": "text/plain", "content": "safe text"},
                {"id": "png-13", "mime": "image/png", "content": "iVBORw0KGgo="}
            ]),
        )),
        Ok(TimelineApply::Applied)
    );
    let artifacts = items(&renderer, TimelineItemKind::Artifact);
    assert_eq!(artifacts.len(), 4);
    assert!(artifacts.iter().any(|item| item
        .body
        .contains("unspecified artifact payload not rendered inline")));
    assert!(
        artifacts
            .iter()
            .filter(|item| item.body.contains("binary artifact not rendered inline"))
            .count()
            >= 2
    );
    assert!(artifacts
        .iter()
        .any(|item| item.body == "text-13: safe text"));
    for forbidden in ["UElDTy1VTlNQRUNJRklFRC1EQVRB", "5049434f", "iVBORw0KGgo="] {
        assert!(
            !artifacts.iter().any(|item| item.body.contains(forbidden)),
            "an artifact payload must not be inlined: {forbidden}"
        );
    }
}

#[test]
fn text_diff_artifacts_are_labelled_and_truncated_by_the_declared_policy() {
    let fixture = fixture();
    assert_scenario(scenario(
        &fixture,
        "text_diff_artifact_is_labelled_and_bounded",
    ));

    let run_id = main_run_id();
    let mut renderer = TimelineRenderer::new(run_id);
    let diff = format!(
        "@@ -1 +1 @@\n-{}\n+{}",
        "a".repeat(MAX_TIMELINE_DIFF_BYTES),
        "b".repeat(MAX_TIMELINE_DIFF_BYTES)
    );
    assert_eq!(
        renderer.apply(&terminal(
            run_id,
            json!([{"id": "diff-large", "path": "src/lib.rs", "mime": "text/x-diff", "diff": diff}]),
        )),
        Ok(TimelineApply::Applied)
    );
    let artifact = items(&renderer, TimelineItemKind::Artifact);
    assert_eq!(artifact.len(), 1);
    assert!(artifact[0].truncated);
    assert!(
        artifact[0].body.len() <= MAX_TIMELINE_DIFF_BYTES,
        "a diff body must respect the declared diff cap"
    );
    assert!(artifact[0].body.ends_with("[truncated]"));
    assert!(!artifact[0].body.contains(&"b".repeat(1024)));
}

#[test]
fn terminal_fences_every_late_event_kind() {
    let fixture = fixture();
    let renderer = assert_scenario(scenario(
        &fixture,
        "terminal_fences_late_delta_error_and_approval",
    ));
    assert_eq!(items(&renderer, TimelineItemKind::Delta).len(), 1);
    assert_eq!(items(&renderer, TimelineItemKind::Terminal).len(), 1);
    assert!(items(&renderer, TimelineItemKind::Error).is_empty());
    assert!(items(&renderer, TimelineItemKind::Approval).is_empty());
    assert!(items(&renderer, TimelineItemKind::ToolCall).is_empty());
}

#[test]
fn gap_marker_blocks_both_typed_and_legacy_delivery_until_reset() {
    let fixture = fixture();
    let mut renderer = assert_scenario(scenario(
        &fixture,
        "gap_is_explicit_and_blocks_later_events",
    ));
    assert!(renderer.gap_detected());
    assert!(renderer
        .items()
        .iter()
        .any(|item| item.kind == TimelineItemKind::Gap));

    let run_id = main_run_id();
    assert_eq!(
        renderer.apply_event(RunStreamEvent::Delta {
            run_id,
            text: "legacy delivery must also wait".to_owned(),
        }),
        Ok(TimelineApply::GapRequiresSnapshot)
    );
    assert_eq!(renderer.items().len(), 2);

    renderer.reset(run_id);
    assert!(!renderer.gap_detected());
    assert!(renderer.items().is_empty());
    assert_eq!(
        renderer.apply_event(RunStreamEvent::Delta {
            run_id,
            text: "fresh snapshot".to_owned(),
        }),
        Ok(TimelineApply::Applied)
    );
}

#[test]
fn hostile_stream_keeps_items_and_bytes_bounded() {
    let run_id = main_run_id();
    let mut renderer = TimelineRenderer::new(run_id);
    for index in 0..(MAX_TIMELINE_ITEMS * 8) {
        let event = if index % 7 == 0 {
            RunStreamEvent::Usage {
                run_id,
                data: json!({"payload": "x".repeat(MAX_TIMELINE_JSON_BYTES * 4)}),
            }
        } else {
            RunStreamEvent::Delta {
                run_id,
                text: "z".repeat(4096),
            }
        };
        let _ = renderer.apply_event(event);
    }
    assert!(renderer.limited());
    assert!(renderer.items().len() <= MAX_TIMELINE_ITEMS);
    assert!(renderer.total_bytes() <= MAX_TIMELINE_BYTES);
    assert_eq!(
        items(&renderer, TimelineItemKind::Limit).len(),
        1,
        "the bounded projection emits one explicit limit marker, not one per dropped item"
    );

    // One terminal response cannot multiply into an unbounded item burst either.
    let artifacts: Vec<Value> = (0..(MAX_TIMELINE_ARTIFACTS_PER_RESPONSE + 8))
        .map(|index| json!({"id": format!("artifact-{index}"), "mime": "text/plain"}))
        .collect();
    let mut response_limited = TimelineRenderer::new(run_id);
    assert_eq!(
        response_limited.apply(&terminal(run_id, Value::Array(artifacts))),
        Ok(TimelineApply::Applied)
    );
    assert_eq!(
        items(&response_limited, TimelineItemKind::Artifact).len(),
        MAX_TIMELINE_ARTIFACTS_PER_RESPONSE
    );
}
