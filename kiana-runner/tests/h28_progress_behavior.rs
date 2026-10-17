//! H28 runner behavior: bounded progress observation on real harness turns.
//!
//! 这些用例驱动真实 `KianaHarness` 模型循环，证明进度 reducer 已接入：
//! 重复失败/交替失败、空 turn 和停滞 poll 都有界停止；推进中的长 poll 不会被误杀。
//! 工具仍只产生 `CapabilityRequested`，结果由测试模拟 Broker 回灌。

use kiana_domain::{CapabilityResult, ModelOutput, ModelToolCall, RequestId, RunId};
use kiana_runner::{KianaHarness, ModelClient, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::{Arc, Mutex};

/// 记录每次模型请求的脚本模型；输出顺序与录制一致。
struct CapturingModel {
    outputs: Mutex<std::collections::VecDeque<ModelOutput>>,
    requests: Mutex<Vec<kiana_domain::ModelRequest>>,
}

impl CapturingModel {
    fn new(outputs: Vec<ModelOutput>) -> Arc<Self> {
        Arc::new(Self {
            outputs: Mutex::new(std::collections::VecDeque::from(outputs)),
            requests: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait::async_trait]
impl ModelClient for CapturingModel {
    async fn complete(&self, request: kiana_domain::ModelRequest) -> Result<ModelOutput, String> {
        self.requests.lock().unwrap().push(request);
        self.outputs
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| "h28_model_exhausted".to_owned())
    }
}

fn shell_call(id: &str, command: &str) -> ModelOutput {
    ModelOutput {
        text: "run command".to_owned(),
        tool_calls: vec![ModelToolCall {
            id: id.to_owned(),
            name: "shell".to_owned(),
            arguments: json!({"command": command}),
        }],
        ..ModelOutput::default()
    }
}

/// 取最近一次发出的能力请求；驱动循环每次回灌当前挂起的调用。
fn latest_request(events: &[RunnerEvent]) -> RequestId {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            RunnerEvent::CapabilityRequested { request, .. } => Some(request.request_id),
            _ => None,
        })
        .expect("a capability request is pending")
}

fn stopped(events: &[RunnerEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            RunnerEvent::ClarificationRequested { .. }
                | RunnerEvent::Completed { .. }
                | RunnerEvent::Failed { .. }
        )
    })
}

fn has_feedback(request: &kiana_domain::ModelRequest) -> bool {
    request
        .messages
        .iter()
        .any(|message| message.text.starts_with("harness-progress-feedback:"))
}

#[tokio::test]
async fn alternating_failed_calls_stop_with_bounded_clarification() {
    let model = CapturingModel::new(vec![
        shell_call("h28-a1", "false --a"),
        shell_call("h28-b1", "false --b"),
        shell_call("h28-a2", "false --a"),
        shell_call("h28-b2", "false --b"),
        ModelOutput::text("must not be reached"),
    ]);
    let harness = KianaHarness::new(model.clone());
    let run_id = RunId::new();
    let mut events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "alternating failures",
            "/h28-alternating",
            "read-only",
        ))
        .await
        .unwrap();
    for failure in [
        "execution_failed:case-a",
        "execution_failed:case-b",
        "execution_failed:case-a",
        "execution_failed:case-b",
    ] {
        let request = latest_request(&events);
        events = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::failure(request, failure),
            })
            .await
            .unwrap();
        if stopped(&events) {
            break;
        }
    }

    let clarification = events
        .iter()
        .find_map(|event| match event {
            RunnerEvent::ClarificationRequested { request, .. } => Some(request),
            _ => None,
        })
        .expect("alternating failures must stop with a bounded clarification");
    assert_eq!(clarification.run_id, run_id);
    assert!(clarification.required);
    assert!(!clarification.question.trim().is_empty());
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Failed { .. })));
    // 第 1 步 + 三次继续的模型步：第 4 次观察命中 A→B→A 阶梯后不再继续调用模型。
    assert_eq!(model.requests.lock().unwrap().len(), 4);

    let requests = model.requests.lock().unwrap();
    assert!(
        requests.iter().any(has_feedback),
        "the bounded feedback must reach the next model request"
    );
}

#[tokio::test]
async fn identical_failed_calls_hit_no_progress_limit() {
    let model = CapturingModel::new(vec![
        shell_call("h28-f1", "false --1"),
        shell_call("h28-f2", "false --2"),
        shell_call("h28-f3", "false --3"),
    ]);
    let harness = KianaHarness::new(model.clone());
    let run_id = RunId::new();
    let mut events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "identical failures",
            "/h28-identical",
            "read-only",
        ))
        .await
        .unwrap();
    for _ in 0..3 {
        let request = latest_request(&events);
        events = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::failure(request, "execution_failed:stuck"),
            })
            .await
            .unwrap();
        if stopped(&events) {
            break;
        }
    }
    assert!(events.iter().any(|event| matches!(
        event,
        RunnerEvent::Failed { error, .. } if error == "no_progress:repeated_failure_limit"
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(model.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn empty_turn_cannot_complete_silently_without_bounded_feedback() {
    let model = CapturingModel::new(vec![
        ModelOutput::text(""),
        ModelOutput::text(""),
        ModelOutput::text("third"),
    ]);
    let harness = KianaHarness::new(model.clone());
    let run_id = RunId::new();
    let events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "empty turn",
            "/h28-empty",
            "read-only",
        ))
        .await
        .unwrap();

    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::ClarificationRequested { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(model.requests.lock().unwrap().len(), 2);
    let requests = model.requests.lock().unwrap();
    assert!(
        requests.iter().any(has_feedback),
        "first empty turn must receive one bounded feedback"
    );
}

#[tokio::test]
async fn progressing_job_poll_is_bounded_without_false_loop_failure() {
    let model = CapturingModel::new(vec![
        shell_call("h28-p1", "poll --cursor 1"),
        shell_call("h28-p2", "poll --cursor 2"),
        shell_call("h28-p3", "poll --cursor 3"),
        shell_call("h28-p4", "poll --cursor 4"),
        shell_call("h28-p5", "poll --cursor 5"),
        ModelOutput::text("poll complete"),
    ]);
    let harness = KianaHarness::new(model.clone());
    let run_id = RunId::new();
    let mut events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "progressing poll",
            "/h28-poll",
            "read-only",
        ))
        .await
        .unwrap();
    for cursor in 1..=5u64 {
        let request = latest_request(&events);
        events = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request, json!({"cursor": cursor, "stdout": ""})),
            })
            .await
            .unwrap();
    }
    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Failed { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::ClarificationRequested { .. })));
    assert_eq!(model.requests.lock().unwrap().len(), 6);
}

#[tokio::test]
async fn stalled_job_poll_stops_instead_of_polling_forever() {
    let model = CapturingModel::new(vec![
        shell_call("h28-s1", "poll --1"),
        shell_call("h28-s2", "poll --2"),
        shell_call("h28-s3", "poll --3"),
        shell_call("h28-s4", "poll --4"),
    ]);
    let harness = KianaHarness::new(model.clone());
    let run_id = RunId::new();
    let mut events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "stalled poll",
            "/h28-stalled",
            "read-only",
        ))
        .await
        .unwrap();
    for _ in 0..3 {
        let request = latest_request(&events);
        events = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request, json!({"cursor": 7})),
            })
            .await
            .unwrap();
        if stopped(&events) {
            break;
        }
    }
    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::ClarificationRequested { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(model.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn new_model_config_does_not_disable_repeated_tool_call_protection() {
    // 只改步数的新配置仍保留默认重复调用阈值 3；连续第 3 次相同调用被既有保护拒绝，
    // 证明“新模型配置”不会默认解除重复调用保护。
    let model = CapturingModel::new(vec![
        shell_call("h28-d1", "true"),
        shell_call("h28-d2", "true"),
        shell_call("h28-d3", "true"),
    ]);
    let harness = KianaHarness::with_config(
        model.clone(),
        kiana_runner::RuntimeConfig {
            max_steps_per_turn: 99,
            ..kiana_runner::RuntimeConfig::default()
        },
    );
    let run_id = RunId::new();
    let mut events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "configured repeat",
            "/h28-configured",
            "read-only",
        ))
        .await
        .unwrap();
    for _ in 0..2 {
        let request = latest_request(&events);
        events = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request, json!({"ok": true})),
            })
            .await
            .unwrap();
    }
    assert!(events.iter().any(|event| matches!(
        event,
        RunnerEvent::Failed { error, .. } if error == "repeated_tool_call:shell"
    )));
    assert_eq!(model.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn scripted_model_stays_compatible_with_harness_contract() {
    // 保留一个最简单的契约用例：纯文本 turn 仍然正常完成，不被进度门误伤。
    let model = ScriptedModel::new(vec![ModelOutput::text("plain answer")]);
    let harness = KianaHarness::new(Arc::new(model));
    let run_id = RunId::new();
    let events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "plain",
            "/h28-plain",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
}
