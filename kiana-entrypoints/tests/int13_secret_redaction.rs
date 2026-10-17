//! INT-13 Workbench/UI fixtures: the timeline projection is plain text and every event kind is
//! redacted before it becomes a display row. Credential shapes must not reach the rendered body,
//! while the terminal state itself is still reported truthfully (hiding a secret never rewrites
//! the run status).
//!
//! These fixtures drive the in-process `TimelineRenderer`; they do not open a browser, do not read
//! EventLog facts and do not prove any external provider outcome.

use kiana_entrypoints::workbench_render::{TimelineItemKind, TimelineRenderer};
use kiana_protocol::{
    scan_secret_sentinels, ExecutionStatus, RequestId, ResponseEnvelope, RunId, RunStreamEvent,
    SecretScanChannel, PROTOCOL_SCHEMA,
};
use serde_json::json;

const SENTINELS: [&str; 6] = [
    "ui-int13-access",
    "ui-int13-bearer",
    "ui-int13-password",
    "UI_SIGNATURE",
    "sk-live-ui-int13",
    "ui-int13-provider",
];

fn run_id() -> RunId {
    RunId::parse_str("00000000-0000-0000-0000-000000000013").expect("fixture run id")
}

fn response(
    run_id: RunId,
    status: ExecutionStatus,
    output: serde_json::Value,
    error: Option<&str>,
) -> ResponseEnvelope {
    ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status,
        output,
        error: error.map(str::to_owned),
    }
}

fn assert_scrubbed(renderer: &TimelineRenderer) {
    for item in renderer.items() {
        for sentinel in SENTINELS {
            assert!(
                !item.body.contains(sentinel),
                "{:?} leaked {sentinel}: {}",
                item.kind,
                item.body
            );
        }
        assert!(
            scan_secret_sentinels(SecretScanChannel::Transcript, &item.body).is_ok(),
            "{:?} published a residual shape: {}",
            item.kind,
            item.body
        );
    }
}

#[test]
fn every_timeline_event_kind_is_redacted_before_it_becomes_a_display_row() {
    let run_id = run_id();
    let mut renderer = TimelineRenderer::new(run_id);
    let events = [
        RunStreamEvent::Delta {
            run_id,
            text: "access_token=ui-int13-access".to_owned(),
        },
        RunStreamEvent::Delta {
            run_id,
            text: "Authorization: Bearer ui-int13-bearer".to_owned(),
        },
        RunStreamEvent::Delta {
            run_id,
            text: "fetch https://user:ui-int13-password@example.test/v1".to_owned(),
        },
        RunStreamEvent::Delta {
            run_id,
            text: "eyJpbnQxMyI6InNlbnNpdGl2ZSJ9.eyJzZWNyZXQiOiJpbnQxMyJ9.UI_SIGNATURE".to_owned(),
        },
        RunStreamEvent::Usage {
            run_id,
            data: json!({"note": "key artifact/sk-live-ui-int13"}),
        },
        RunStreamEvent::ToolCall {
            run_id,
            data: json!({
                "tool": "shell",
                "arguments": {"command": "curl -H 'Authorization: Bearer ui-int13-bearer' https://example.test"},
            }),
        },
        RunStreamEvent::ApprovalRequested {
            run_id,
            data: json!({"reason": "access_token=ui-int13-access"}),
        },
        RunStreamEvent::Error {
            run_id,
            data: json!({"error": "provider 500 Authorization: Bearer ui-int13-provider"}),
        },
    ];
    for event in events {
        renderer.apply_event(event).expect("timeline event");
    }

    assert_eq!(renderer.items().len(), 8, "{:?}", renderer.items());
    assert_scrubbed(&renderer);
    assert!(renderer
        .items()
        .iter()
        .any(|item| item.body.contains("[REDACTED]")));
    // The projection still shows the recorded error instead of silently dropping it.
    assert!(renderer
        .items()
        .iter()
        .any(|item| item.kind == TimelineItemKind::Error));
}

#[test]
fn terminal_response_errors_and_artifacts_are_scrubbed_without_rewriting_status() {
    let run_id = run_id();
    let mut renderer = TimelineRenderer::new(run_id);
    renderer
        .apply_event(RunStreamEvent::Delta {
            run_id,
            text: "work in progress".to_owned(),
        })
        .expect("delta");
    renderer
        .apply_event(RunStreamEvent::Terminal {
            run_id,
            response: response(
                run_id,
                ExecutionStatus::Failed,
                json!({
                    "output": {"text": "done access_token=ui-int13-access"},
                    "files_changed": [
                        "src/safe.rs",
                        "artifact/sk-live-ui-int13"
                    ],
                }),
                Some("provider 500 Authorization: Bearer ui-int13-provider"),
            ),
        })
        .expect("terminal");

    assert!(renderer.terminal());
    assert_scrubbed(&renderer);
    let terminal = renderer
        .items()
        .iter()
        .find(|item| item.kind == TimelineItemKind::Terminal)
        .expect("terminal row");
    // Status is a fact, not a secret: it survives the scrub.
    assert!(terminal.body.contains(ExecutionStatus::Failed.as_str()));
    assert!(terminal.body.contains("[REDACTED]"));
    let artifacts: Vec<_> = renderer
        .items()
        .iter()
        .filter(|item| item.kind == TimelineItemKind::Artifact)
        .collect();
    assert_eq!(artifacts.len(), 2, "{:?}", renderer.items());
    assert!(artifacts
        .iter()
        .any(|item| item.body.contains("src/safe.rs")));
}
