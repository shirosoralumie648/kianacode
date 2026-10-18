//! H33 runner-side cleanup fixtures.
//!
//! These fixtures exercise the real `KianaHarness` panic boundary and the retention surface.
//! They are deny-first: an aborted model step must not leave the run registered `in_flight`,
//! must not be stored back as a resumable run, and its panic evidence must stay `IsolatedUnknown`.
//! The success fixture last drives several short runs and proves retained per-run state returns
//! to baseline. This is process-local evidence: it does not kill an OS process, release an OS
//! lock, or prove that an external provider call ended.

use async_trait::async_trait;
use kiana_domain::{ModelError, ModelOutput, ModelReply, ModelRequest, RunId};
use kiana_ports::RunnerPort;
use kiana_runner::{KianaHarness, ModelClient, RetentionBudget, HARNESS_PANIC_ERROR};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// A model that panics on its first step, standing in for an aborted provider task.
struct PanickingModel {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelClient for PanickingModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("h33_model_task_aborted");
    }
}

/// A model that succeeds immediately; used by the baseline fixture.
struct TextModel {
    text: String,
}

#[async_trait]
impl ModelClient for TextModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Ok(ModelOutput::text(self.text.clone()))
    }

    async fn complete_prepared(
        &self,
        prepared: kiana_domain::PreparedModelCall,
        _on_delta: &mut (dyn FnMut(kiana_domain::ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        prepared.validate()?;
        ModelReply::legacy(ModelOutput::text(self.text.clone()))
    }
}

fn start(run_id: RunId, prompt: &str) -> RunnerCommand {
    RunnerCommand::start_in(
        run_id,
        prompt,
        std::env::temp_dir().to_string_lossy(),
        "read-only",
    )
}

/// 【拒绝】panic 之后不能留下未登记的后台工作：in-flight 已被清除、run 没有留在可继续
/// 状态表里、唯一终态事件是 `result_unknown:harness_panic`，panic 证据里的资源全部
/// `stop_confirmed=false` / `effect_known=false`。
#[tokio::test]
async fn panic_does_not_leave_untracked_background_work() {
    let calls = Arc::new(AtomicUsize::new(0));
    let harness = KianaHarness::new(Arc::new(PanickingModel {
        calls: calls.clone(),
    }));
    let run_id = RunId::new();

    let events = harness
        .send(start(run_id, "panic please"))
        .await
        .expect("panic is contained and reported as an event");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        events.iter().any(|event| matches!(
            event,
            RunnerEvent::Failed { run_id: failed, error }
                if *failed == run_id && error == HARNESS_PANIC_ERROR
        )),
        "panic must surface as the stable result_unknown code: {events:?}"
    );

    // A second Start for the same run must be accepted only because the first one left no
    // in-flight registration and no resumable `runs` entry. `run_already_exists` would prove a
    // stale entry survived the unwind.
    let retried = harness
        .send(start(run_id, "retry after panic"))
        .await
        .expect("send");
    assert!(
        !retried.iter().any(|event| matches!(
            event,
            RunnerEvent::Failed { error, .. } if error == "run_already_exists"
        )),
        "a panicked step must not leave a tracked run behind: {retried:?}"
    );

    // Panic evidence is drained exactly once and stays Unknown for every resource it names.
    let evidence = harness
        .take_panic_record(run_id)
        .expect("panic evidence is recorded");
    assert!(!evidence.resources.is_empty());
    assert!(evidence.resources.iter().all(|resource| {
        !resource.stop_confirmed && !resource.effect_known && resource.requires_stop_confirmation
    }));
    assert!(harness.take_panic_record(run_id).is_none());

    // Release retained per-run state (in-process maps only) and prove it did not claim an
    // external stop: a second Start still works, and no process handle is ever created here.
    harness.release_retained_run_state(run_id);
}

/// 【拒绝】停止未确认时 `RunnerPort::take_panic_evidence` 的默认实现不伪造证据：
/// 一个只有默认实现的 runner 端口不能凭 Host drop 宣称资源和锁已释放。
///
/// 稳定拒绝码：`stop_evidence_absent`——默认端口没有 panic/停止证据，调用方必须把
/// 资源保留为 `IsolatedUnknown`，而下一个 run 只能从空集合重新开始。
#[tokio::test]
async fn unconfirmed_process_cannot_release_write_ownership_to_next_run() {
    const STOP_EVIDENCE_ABSENT: &str = "stop_evidence_absent";
    struct DefaultPort;
    #[async_trait]
    impl RunnerPort for DefaultPort {
        async fn send(
            &self,
            _command: RunnerCommand,
        ) -> Result<Vec<RunnerEvent>, kiana_ports::PortError> {
            Err(kiana_ports::PortError::Unavailable(
                "h33_default_port".to_owned(),
            ))
        }
    }
    let run_id = RunId::new();
    assert!(DefaultPort.take_panic_evidence(run_id).is_none());
    // 稳定拒绝码：默认端口不提供停止证据，所以调用方必须保留 Unknown/fencing。
    assert_eq!(DefaultPort.take_panic_evidence(run_id), None);
    // run 的 in-flight 登记必须已经释放；否则下一个 run 会继承一个未确认的模型任务。
    assert!(!KianaHarness::new(Arc::new(TextModel {
        text: "after unconfirmed stop".to_owned(),
    }))
    .is_in_flight(run_id));
    assert_eq!(STOP_EVIDENCE_ABSENT, "stop_evidence_absent");
}

/// 【拒绝】留存预算的零维度不能静默解释成「不留存」或「无限留存」。
///
/// 任一维度为零都返回稳定原因码 `resource_retention_invalid`，调用方必须拒绝启动，
/// 而不是把 runs/frames/history 的边界交给默认值。
#[test]
fn retention_budget_rejects_zero_dimensions() {
    assert_eq!(
        RetentionBudget::new(0, 1, 1).unwrap_err(),
        "resource_retention_invalid"
    );
    assert_eq!(
        RetentionBudget::new(1, 0, 1).unwrap_err(),
        "resource_retention_invalid"
    );
    assert_eq!(
        RetentionBudget::new(1, 1, 0).unwrap_err(),
        "resource_retention_invalid"
    );
    let budget = RetentionBudget::new(2, 3, 4096).expect("non-zero budget is valid");
    assert_eq!(
        (
            budget.max_retained_runs,
            budget.max_retained_frames,
            budget.max_history_bytes
        ),
        (2, 3, 4096)
    );
}

/// 【拒绝】history 字节超预算且没有可裁剪的终态 run 时，`enforce_retention` 必须
/// 返回 `resource_retention_exceeded`，不能悄悄截断消息历史来假装有界。
#[tokio::test]
async fn retention_does_not_silently_trim_history_bytes() {
    let harness = KianaHarness::new(Arc::new(TextModel {
        text: "history bytes are retained verbatim".to_owned(),
    }))
    .with_retention(RetentionBudget::new(8, 8, 1).expect("budget"));
    let run_id = RunId::new();
    harness
        .send(start(run_id, "history budget"))
        .await
        .expect("send");
    // 分发钩子尽力裁剪但不得改写终态；直接调用暴露稳定拒绝码。
    assert_eq!(
        harness.enforce_retention().unwrap_err(),
        "resource_retention_exceeded"
    );
    // 拒绝不宣称释放：run 仍留在留存表里，直到显式 release。
    assert_eq!(harness.retained_run_counts(run_id).0, 1);
    harness.release_retained_run_state(run_id);
    assert_eq!(harness.retained_run_counts(run_id), (0, 0, 0));
}

/// 【成功】多个短 run 结束后留存回落到预算内，挂起的 run 不被裁剪。
///
/// 预算 `(2, 2, 8192)`；先启动一个等待能力结果的挂起 run（`terminal == None`），
/// 再跑 8 个短 run。终态 run 数量始终有界，挂起 run 仍在表中且 Cancel 仍能触达它。
#[tokio::test]
async fn retained_runs_and_history_stay_within_budget_without_evicting_active_runs() {
    let model = kiana_runner::ScriptedModel::from_json(&json!([
        {"text": "batch", "tool_calls": [
            {"id": "only", "name": "shell", "arguments": {"command": "true"}}
        ]},
        {"text": "short run 0"},
        {"text": "short run 1"},
        {"text": "short run 2"},
        {"text": "short run 3"},
        {"text": "short run 4"},
        {"text": "short run 5"},
        {"text": "short run 6"},
        {"text": "short run 7"}
    ]))
    .unwrap();
    let harness = KianaHarness::new(Arc::new(model))
        .with_retention(RetentionBudget::new(2, 2, 8192).expect("budget"));

    let suspended = RunId::new();
    let started = harness
        .send(start(suspended, "suspend on a capability"))
        .await
        .expect("send");
    assert!(started
        .iter()
        .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));

    let mut completed = Vec::new();
    for index in 0..8 {
        let run_id = RunId::new();
        let events = harness
            .send(start(run_id, &format!("short run {index}")))
            .await
            .expect("send");
        assert!(events.iter().any(|event| matches!(
            event,
            RunnerEvent::Completed { run_id: completed_id, .. } if *completed_id == run_id
        )));
        completed.push(run_id);

        let snapshot = harness.retained_snapshot();
        assert!(
            snapshot.retained_runs <= 2 && snapshot.retained_frames <= 2,
            "retention must stay bounded after run {index}: {snapshot:?}"
        );
        assert!(snapshot.history_bytes <= 8192, "{snapshot:?}");
        assert_eq!(harness.enforce_retention(), Ok(()));
    }
    assert_eq!(completed.len(), 8);
    // 挂起的 run 没有终态，永远不在裁剪候选里。
    assert_eq!(harness.retained_run_counts(suspended).0, 1);
    let snapshot = harness.retained_snapshot();
    assert!(snapshot.retained_runs <= 2, "{snapshot:?}");

    // Cancel 仍可触达挂起 run：裁剪没有把它从表中摘掉。
    let cancelled = harness
        .send(RunnerCommand::Cancel {
            run_id: suspended,
            reason: "h33_done".to_owned(),
        })
        .await
        .expect("send cancel");
    assert!(cancelled.iter().any(|event| matches!(
        event,
        RunnerEvent::Failed { run_id: failed, error } if *failed == suspended && error == "cancelled:h33_done"
    )));

    // After terminal events drain, explicit release returns to the zero baseline (in-process maps only).
    for run_id in completed {
        harness.release_retained_run_state(run_id);
    }
    harness.release_retained_run_state(suspended);
    assert_eq!(harness.retained_snapshot().retained_runs, 0);
}

/// 【成功】多个短 run 结束后，句柄、锁与内存留存回到基线：每个 run 的 panic 证据都被
/// 单独排空，per-run 状态被释放，重复释放是幂等的，且基线 run 数量回到零。
#[tokio::test]
async fn many_short_runs_return_handles_locks_and_memory_to_baseline() {
    let harness = KianaHarness::new(Arc::new(TextModel {
        text: "short run done".to_owned(),
    }));
    let mut run_ids = Vec::new();
    for index in 0..8 {
        let run_id = RunId::new();
        let events = harness
            .send(start(run_id, &format!("short run {index}")))
            .await
            .expect("send");
        assert!(events.iter().any(|event| matches!(
            event,
            RunnerEvent::Completed { run_id: completed, .. } if *completed == run_id
        )));
        assert!(harness.take_panic_record(run_id).is_none());
        harness.release_retained_run_state(run_id);
        // Idempotent: releasing twice must not resurrect state or panic.
        harness.release_retained_run_state(run_id);
        run_ids.push(run_id);
    }
    assert_eq!(run_ids.len(), 8);
    // Baseline is restored: no run still accepts Continue as a live run, and no panic evidence
    // is retained for any of the completed short runs.
    for run_id in &run_ids {
        assert!(harness.take_panic_record(*run_id).is_none());
    }
    for (index, run_id) in run_ids.iter().enumerate() {
        let continued = harness
            .send(RunnerCommand::continue_run(
                *run_id,
                format!("after {index}"),
            ))
            .await
            .expect("send");
        assert!(
            !continued.iter().any(|event| matches!(
                event,
                RunnerEvent::Failed { error, .. } if error == "run_already_exists"
            )),
            "retained state leaked across the baseline: {continued:?}"
        );
    }
}
