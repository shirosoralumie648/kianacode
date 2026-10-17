//! CAP-17 source guard for run-wide cancellation, execution-set stop and Unknown fencing.

use kiana_domain::{
    ExecutionId, ExecutionStopKind, ExecutionStopState, ProcessGroupState, RunExecutionStopSet,
    RunId, StopMethod, StopReport, StopSetResolution,
};

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-17 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cancel_persists_intent_then_stops_all_execution_classes() {
    let domain = include_str!("../../kiana-domain/src/cancellation.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let approvals = include_str!("../src/approvals.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let shell = include_str!("../../kiana-daemon/src/harness_capabilities.rs");

    require(
        domain,
        &[
            "RunCancellationFact",
            "RunCancellationState::Stopping",
            "RunCancellationState::Cancelled",
            "RunCancellationState::ResultUnknown",
            "stop_requested",
            "stop_confirmed",
        ],
        "durable cancellation",
    );
    require(
        lifecycle,
        &[
            "record_cancel_requested",
            "run.cancelling",
            "signal_cancel",
            "await_capability_stop",
            "run.cancelled",
            "run.result_unknown",
            "record_terminal_event",
        ],
        "control-plane order",
    );
    require(
        dispatch,
        &["commit_confirmed", "result.delivery_claimed"],
        "queued/late dispatch fence",
    );
    // `not_executed` 这个事实由 lifecycle.rs 记录（dispatch.rs 里没有），
    // 守卫的 dispatch 组原来断的是它，所以这里单独对 lifecycle 断。
    assert!(
        lifecycle.contains("not_executed"),
        "CAP-17 not_executed fact missing from lifecycle"
    );
    require(
        approvals,
        &["cancel_pending_tools", "not_executed"],
        "approval fence",
    );
    require(
        runner,
        &[
            "cancellation.request",
            "pending_tools.drain(..)",
            "ToolCancelled",
        ],
        "runner queued fence",
    );
    require(
        supervisor,
        &[
            "ProcessSupervisor",
            "StopReport",
            "confirmed",
            "wait_leader",
        ],
        "process stop evidence",
    );
    require(
        shell,
        &[
            "ProcessSupervisor::stop",
            "stop_report",
            "shell_result_unknown:cancel_stop_unconfirmed",
        ],
        "shell stop routing",
    );
    let persisted = lifecycle.find("record_cancel_requested(").unwrap();
    let signal = lifecycle.find("self.signal_cancel(run_id)").unwrap();
    assert!(
        persisted < signal,
        "cancel signal must follow durable intent"
    );
    for forbidden in [
        "cancelled_without_stop_confirmation_is_success",
        "auto_retry_after_cancel",
    ] {
        assert!(
            !lifecycle.contains(forbidden),
            "CAP-17 bypass marker present: {forbidden}"
        );
    }
}

#[test]
fn cancel_waits_for_all_inflight_executions_and_hooks() {
    let capability = ExecutionStopKind::Capability;
    let mut set = RunExecutionStopSet::new(RunId::new());
    let first = ExecutionId::new();
    let second = ExecutionId::new();
    let hook = ExecutionId::new();
    set.register(first, capability).unwrap();
    set.register(second, capability).unwrap();
    set.register(hook, ExecutionStopKind::Hook).unwrap();

    let mut targets = set.begin_stop();
    targets.sort();
    let mut expected = vec![first, second, hook];
    expected.sort();
    assert_eq!(
        targets, expected,
        "cancel must broadcast to every inflight execution and hook"
    );
    assert_eq!(set.resolution(), StopSetResolution::Pending);

    set.confirm_stop_without_report(first).unwrap();
    assert_eq!(
        set.resolution(),
        StopSetResolution::Pending,
        "cancel cannot confirm while a sibling execution/hook is still stopping"
    );
    set.confirm_stop_without_report(hook).unwrap();
    assert_eq!(set.resolution(), StopSetResolution::Pending);
    assert_eq!(set.unconfirmed(), vec![second]);

    set.confirm_stop_without_report(second).unwrap();
    assert_eq!(set.resolution(), StopSetResolution::Confirmed);
    assert!(set.all_resolved());
}

#[test]
fn dropping_blocking_write_future_does_not_confirm_stop() {
    let capability = ExecutionStopKind::Capability;
    let mut set = RunExecutionStopSet::new(RunId::new());
    let execution = ExecutionId::new();
    set.register(execution, capability).unwrap();
    set.begin_stop();
    set.note_blocking_commit_dropped(execution).unwrap();

    let entry = set.entry(&execution).expect("registered execution");
    assert!(entry.blocking_commit_dropped);
    assert_eq!(entry.state, ExecutionStopState::Stopping);
    assert_ne!(
        set.resolution(),
        StopSetResolution::Confirmed,
        "dropping the blocking write future must never stand in for a confirmed stop"
    );
    assert_eq!(set.unconfirmed(), vec![execution]);

    // Only explicit stop evidence resolves the entry.
    set.confirm_stop_without_report(execution).unwrap();
    assert_eq!(set.resolution(), StopSetResolution::Confirmed);
}

#[test]
fn cancel_after_dispatch_cannot_release_writable_scope_early() {
    let capability = ExecutionStopKind::Capability;
    let mut set = RunExecutionStopSet::new(RunId::new());
    let dispatching = ExecutionId::new();
    let completed = ExecutionId::new();
    set.register(dispatching, capability).unwrap();
    set.register(completed, capability).unwrap();

    // A capability that finished before cancel no longer owes a stop and may release scope.
    set.mark_completed(completed).unwrap();
    assert!(set.release_writable_scope(completed).is_ok());

    // A dispatched execution still in flight must keep its writable scope fenced.
    assert_eq!(
        set.release_writable_scope(dispatching),
        Err("writable_scope_release_requires_confirmed_stop".to_owned())
    );
    set.begin_stop();
    assert_eq!(
        set.release_writable_scope(dispatching),
        Err("writable_scope_release_requires_confirmed_stop".to_owned())
    );
    set.confirm_stop_without_report(dispatching).unwrap();
    assert!(set.release_writable_scope(dispatching).is_ok());
}

#[test]
fn late_completion_and_stop_report_after_cancel_are_fenced() {
    let capability = ExecutionStopKind::Capability;
    let mut set = RunExecutionStopSet::new(RunId::new());
    let execution = ExecutionId::new();
    set.register(execution, capability).unwrap();
    set.begin_stop();
    assert_eq!(
        set.mark_completed(execution),
        Err("late_completion_after_cancel_requires_reconciliation".to_owned()),
        "a result arriving after cancel is a diagnostic fact, not a completion"
    );

    let report = StopReport::new(
        execution.to_string(),
        None,
        Some(4242),
        StopMethod::Term,
        true,
        false,
        true,
        ProcessGroupState::Empty,
        true,
        5,
    )
    .unwrap();
    set.confirm_stop(report.clone()).unwrap();
    assert_eq!(set.entry(&execution).unwrap().stop_report, Some(report));

    // A late stop report for an execution that already completed before cancel is rejected.
    let mut completed = RunExecutionStopSet::new(RunId::new());
    let other = ExecutionId::new();
    completed.register(other, capability).unwrap();
    completed.mark_completed(other).unwrap();
    let late = StopReport::new(
        other.to_string(),
        None,
        Some(77),
        StopMethod::Kill,
        false,
        true,
        true,
        ProcessGroupState::Empty,
        true,
        3,
    )
    .unwrap();
    assert_eq!(
        completed.confirm_stop(late),
        Err("late_stop_report_after_completion".to_owned())
    );
}

#[test]
fn cancel_resolves_each_queued_approval_execution_and_commit_stage() {
    // Queued: nothing has started, so there is no in-flight stop debt.
    let queued = RunExecutionStopSet::new(RunId::new());
    assert!(queued.is_empty());
    assert_eq!(queued.resolution(), StopSetResolution::Confirmed);

    // Approval pending: cancellation is handled by invalidating the pending invocation
    // (`not_executed`); the stop set is still empty and must not block the cancel.
    let approval = RunExecutionStopSet::new(RunId::new());
    assert_eq!(approval.resolution(), StopSetResolution::Confirmed);

    // Executing then commit/blocking stage: the set stays Pending until stop is confirmed.
    let capability = ExecutionStopKind::Capability;
    let mut running = RunExecutionStopSet::new(RunId::new());
    let execution = ExecutionId::new();
    running.register(execution, capability).unwrap();
    running.begin_stop();
    assert_eq!(running.resolution(), StopSetResolution::Pending);
    running.note_blocking_commit_dropped(execution).unwrap();
    assert_eq!(running.resolution(), StopSetResolution::Pending);
    running.confirm_stop_without_report(execution).unwrap();
    assert_eq!(running.resolution(), StopSetResolution::Confirmed);

    // Cancelling an empty set converges immediately; a registration that arrives afterwards must
    // be tracked as Stopping, never reset back to Inflight by a turn-boundary reset.
    let mut restarted = RunExecutionStopSet::new(RunId::new());
    restarted.begin_stop();
    assert_eq!(restarted.resolution(), StopSetResolution::Confirmed);
    let late = ExecutionId::new();
    restarted.register(late, capability).unwrap();
    let entry = restarted.entry(&late).expect("registered");
    assert_eq!(entry.state, ExecutionStopState::Stopping);
    assert_eq!(restarted.resolution(), StopSetResolution::Pending);
}

#[test]
fn execution_set_stop_tracking_is_wired_into_the_control_plane() {
    let tracker = include_str!("../src/stop_tracker.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let sessions = include_str!("../src/sessions.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");

    require(
        tracker,
        &[
            "RunStopChannel",
            "RunExecutionStopSet",
            "register_execution_stop",
            "resolve_execution_stop",
            "begin_stop",
            "stop_requested",
            "StopSetResolution::Confirmed",
            "mark_stop_unconfirmed",
        ],
        "core stop tracker",
    );
    require(
        dispatch,
        &[
            "register_execution_stop",
            "ExecutionStopKind::Capability",
            "resolve_execution_stop",
        ],
        "dispatch registration",
    );
    require(
        sessions,
        &[
            "capability_stops",
            "begin_stop()",
            "StopSetResolution::Pending",
        ],
        "execution-set await",
    );

    // The per-run stop signal must be a set, not a single boolean that a later
    // execution could overwrite.
    assert!(
        !sessions.contains("watch::Sender<Option<bool>>"),
        "CAP-17 must not collapse the execution set back to a single stop boolean"
    );

    // Cancel still persists intent before signaling.
    let persisted = lifecycle.find("record_cancel_requested(").unwrap();
    let signal = lifecycle.find("self.signal_cancel(run_id)").unwrap();
    assert!(
        persisted < signal,
        "cancel signal must follow the durable intent"
    );
}

#[test]
fn cancel_race_fixtures_remain_part_of_the_remote_gate() {
    let race = include_str!("../../kiana-daemon/tests/p0_j1_04_cancel_race.rs");
    let host = include_str!("../../kiana-daemon/tests/daemon_host.rs");
    require(
        race,
        &[
            "cancelling_mid_stream_never_completes_or_emits_a_late_delta",
            "run.cancelled",
            "run.result_unknown",
        ],
        "cancel race fixture",
    );
    require(
        host,
        &[
            "cancelling_mid_stream_never_completes_or_emits_a_late_delta",
            "cancel_stops_in_flight_shell_before_it_writes",
            "shell_timeout_ms_is_brokered_as_a_timed_out_tool_result",
        ],
        "daemon integration fixtures",
    );
}
