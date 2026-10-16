use kiana_domain::RunId;
use kiana_runner::{KianaHarness, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn cancel_drains_queued_tool_calls_with_replay_safe_results() {
    let model = ScriptedModel::from_json(&json!([{
        "text":"batch",
        "tool_calls": [
            {"id":"first","name":"shell","arguments":{"command":"true"}},
            {"id":"second","name":"shell","arguments":{"command":"true"}},
            {"id":"third","name":"shell","arguments":{"command":"true"}}
        ]
    }]))
    .unwrap();
    let harness = KianaHarness::new(Arc::new(model));
    let run_id = RunId::new();
    let started = harness
        .send(RunnerCommand::start_in(
            run_id,
            "cancel batch",
            "/repo",
            "read-only",
        ))
        .await
        .unwrap();
    assert_eq!(
        started
            .iter()
            .filter(|event| matches!(event, RunnerEvent::CapabilityRequested { .. }))
            .count(),
        1
    );
    let cancelled = harness
        .send(RunnerCommand::Cancel {
            run_id,
            reason: "user".to_owned(),
        })
        .await
        .unwrap();
    let queued: Vec<_> = cancelled
        .iter()
        .filter_map(|event| match event {
            RunnerEvent::ToolCancelled {
                call_id, result, ..
            } => Some((call_id.as_str(), result)),
            _ => None,
        })
        .collect();
    assert_eq!(
        queued.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        ["second", "third"]
    );
    for (_, result) in queued {
        assert_eq!(result["error"], "cancelled:queued");
        assert_eq!(result["not_executed"], true);
        assert_eq!(result["replay_safe"], true);
    }
    assert!(cancelled.iter().any(
        |event| matches!(event, RunnerEvent::Failed { error, .. } if error == "cancelled:user")
    ));
    assert!(!cancelled.iter().any(|event| matches!(
        event,
        RunnerEvent::CapabilityRequested { .. } | RunnerEvent::Completed { .. }
    )));
}

#[test]
fn synthetic_cancel_results_cannot_become_model_success() {
    let harness = include_str!("../src/harness.rs");
    assert!(harness.contains("if let Some(error) = run.cancellation.error()?"));
    assert!(harness.contains("ToolObservationStatus::CancelledNotStarted"));
    assert!(harness.contains("tool_cancelled_not_started"));
    assert!(!harness.contains(
        "replay_safe:true}),\n                    });\n                    run.messages.push"
    ));
}
