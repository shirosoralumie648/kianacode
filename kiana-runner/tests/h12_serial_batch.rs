use kiana_domain::{CapabilityResult, ModelOutput, ModelToolCall, RequestId, RunId};
use kiana_runner::{KianaHarness, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::Arc;

fn batch_output(count: usize) -> ModelOutput {
    ModelOutput {
        text: "batch".to_owned(),
        tool_calls: (0..count)
            .map(|index| ModelToolCall {
                id: format!("h12-{index}"),
                name: "memory.search".to_owned(),
                arguments: json!({"query": format!("query-{index}")}),
            })
            .collect(),
        ..ModelOutput::default()
    }
}

fn requests(events: &[RunnerEvent]) -> Vec<RequestId> {
    events
        .iter()
        .filter_map(|event| match event {
            RunnerEvent::CapabilityRequested { request, .. } => Some(request.request_id),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn wrong_result_does_not_consume_pending_call() {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::new(vec![
        batch_output(1),
        ModelOutput::text("after correction"),
    ])));
    let run_id = RunId::new();
    let started = harness
        .send(RunnerCommand::start_in(
            run_id,
            "wrong result",
            "/h12-wrong",
            "read-only",
        ))
        .await
        .unwrap();
    let expected = requests(&started)[0];
    let wrong = harness
        .send(RunnerCommand::CapabilityResult {
            run_id,
            result: CapabilityResult::success(RequestId::new(), json!({"wrong":true})),
        })
        .await
        .unwrap();
    assert!(wrong.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "capability_result_mismatch")
    }));
    let corrected = harness
        .send(RunnerCommand::CapabilityResult {
            run_id,
            result: CapabilityResult::success(expected, json!({"ok":true})),
        })
        .await
        .unwrap();
    assert!(corrected
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
}

#[tokio::test]
async fn cancelled_batch_never_dispatches_remaining_calls() {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::new(vec![batch_output(3)])));
    let run_id = RunId::new();
    let started = harness
        .send(RunnerCommand::start_in(
            run_id,
            "cancel batch",
            "/h12-cancel",
            "read-only",
        ))
        .await
        .unwrap();
    assert_eq!(requests(&started).len(), 1);
    let cancelled = harness
        .send(RunnerCommand::Cancel {
            run_id,
            reason: "user".to_owned(),
        })
        .await
        .unwrap();
    let synthetic = cancelled
        .iter()
        .filter(|event| matches!(event, RunnerEvent::ToolCancelled { .. }))
        .count();
    assert_eq!(synthetic, 2);
    assert!(!cancelled
        .iter()
        .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
}

#[tokio::test]
async fn three_serial_calls_produce_three_ordered_results_before_model() {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::new(vec![
        batch_output(3),
        ModelOutput::text("all three observed"),
    ])));
    let run_id = RunId::new();
    let mut events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "serial batch",
            "/h12-serial",
            "read-only",
        ))
        .await
        .unwrap();
    let mut order = requests(&events);
    for index in 0..3 {
        let request_id = *order.last().expect("serial request");
        events = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request_id, json!({"index":index})),
            })
            .await
            .unwrap();
        order.extend(requests(&events));
    }
    assert_eq!(order.len(), 3);
    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
}
