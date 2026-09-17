use kiana_domain::{CapabilityResult, ModelOutput, ModelToolCall, RunId};
use kiana_ports::RunnerPort;
use kiana_runner::{KianaHarness, ModelClient, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::Arc;

fn two_tools() -> ModelOutput {
    ModelOutput {
        text: "two calls".to_owned(),
        tool_calls: vec![
            ModelToolCall {
                id: "first".to_owned(),
                name: "memory.search".to_owned(),
                arguments: json!({"query":"first"}),
            },
            ModelToolCall {
                id: "second".to_owned(),
                name: "memory.search".to_owned(),
                arguments: json!({"query":"second"}),
            },
        ],
        ..ModelOutput::default()
    }
}

#[tokio::test]
async fn invalid_second_call_prevents_all_batch_dispatch() {
    let model = ScriptedModel::new(vec![ModelOutput {
        text: "invalid batch".to_owned(),
        tool_calls: vec![
            ModelToolCall {
                id: "valid-first".to_owned(),
                name: "memory.search".to_owned(),
                arguments: json!({"query":"first"}),
            },
            ModelToolCall {
                id: "invalid-second".to_owned(),
                name: "shell".to_owned(),
                arguments: json!({}),
            },
        ],
        ..ModelOutput::default()
    }]);
    let events = KianaHarness::new(Arc::new(model))
        .send(RunnerCommand::start_in(
            RunId::new(),
            "invalid batch",
            "/h10-invalid",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "invalid_arguments:shell:command")
    }));
}

#[tokio::test]
async fn duplicate_call_id_in_one_message_is_rejected() {
    let model = ScriptedModel::new(vec![ModelOutput {
        text: "duplicate".to_owned(),
        tool_calls: vec![
            ModelToolCall {
                id: "same".to_owned(),
                name: "memory.search".to_owned(),
                arguments: json!({"query":"one"}),
            },
            ModelToolCall {
                id: "same".to_owned(),
                name: "memory.search".to_owned(),
                arguments: json!({"query":"two"}),
            },
        ],
        ..ModelOutput::default()
    }]);
    let events = KianaHarness::new(Arc::new(model))
        .send(RunnerCommand::start_in(
            RunId::new(),
            "duplicate",
            "/h10-duplicate",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error.starts_with("model_tool_identity_invalid"))
    }));
}

#[tokio::test]
async fn invocation_identity_survives_queue_approval_and_restore() {
    let run_id = RunId::new();
    let first_harness = KianaHarness::new(Arc::new(ScriptedModel::new(vec![two_tools()])));
    let started = first_harness
        .send(RunnerCommand::start_in(
            run_id,
            "queue two",
            "/h10-restore",
            "read-only",
        ))
        .await
        .unwrap();
    let first_request = started
        .iter()
        .find_map(|event| match event {
            RunnerEvent::CapabilityRequested { request, .. } => Some(request.request_id),
            _ => None,
        })
        .expect("first invocation request");
    let checkpoint = first_harness.checkpoint(run_id).await.unwrap();
    let queued = checkpoint["pending_tools"].as_array().unwrap();
    assert_eq!(queued.len(), 2);
    let second_request: kiana_domain::RequestId =
        serde_json::from_value(queued[1][0].clone()).unwrap();

    let restored = KianaHarness::new(Arc::new(ScriptedModel::new(Vec::new())));
    restored.restore(run_id, checkpoint).await.unwrap();
    let resumed = restored
        .send(RunnerCommand::CapabilityResult {
            run_id,
            result: CapabilityResult::success(first_request, json!({"hits":[]})),
        })
        .await
        .unwrap();
    let resumed_request = resumed
        .iter()
        .find_map(|event| match event {
            RunnerEvent::CapabilityRequested { request, .. } => Some(request.request_id),
            _ => None,
        })
        .expect("second invocation request after restore");
    assert_eq!(resumed_request, second_request);
}

#[allow(dead_code)]
fn _model_client_type_is_object_safe(_: Arc<dyn ModelClient>) {}
