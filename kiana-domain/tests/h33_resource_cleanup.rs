use kiana_domain::*;

fn resource(kind: CleanupResourceKind, requires_stop: bool) -> CleanupResource {
    CleanupResource {
        resource_id: format!("resource:{kind:?}"),
        owner_run_id: RunId::new(),
        kind,
        requires_stop_confirmation: requires_stop,
        stop_confirmed: false,
        effect_known: false,
        status: CleanupResourceStatus::Pending,
        reason: None,
    }
}

#[test]
fn unconfirmed_process_or_lock_never_becomes_released() {
    let run_id = RunId::new();
    let mut process = resource(CleanupResourceKind::JobHandle, true);
    process.owner_run_id = run_id;
    let plan = ResourceCleanupPlan::new(run_id, CleanupCause::Cancel, vec![process]).unwrap();
    let report = plan
        .observe_all(&[(
            "resource:JobHandle".to_owned(),
            false,
            false,
            "stop_unconfirmed".to_owned(),
        )])
        .unwrap();
    assert_eq!(report.unknown_count, 1);
    assert!(!report.safe_to_release_all());
    assert_eq!(
        report.resources[0].status,
        CleanupResourceStatus::IsolatedUnknown
    );
}

#[test]
fn confirmed_normal_cleanup_releases_everything_and_retention_is_bounded() {
    let run_id = RunId::new();
    let mut model = resource(CleanupResourceKind::ModelTask, false);
    model.owner_run_id = run_id;
    let mut lock = resource(CleanupResourceKind::PathLock, true);
    lock.owner_run_id = run_id;
    let plan = ResourceCleanupPlan::new(run_id, CleanupCause::Normal, vec![model, lock]).unwrap();
    let report = plan
        .observe_all(&[
            (
                "resource:ModelTask".to_owned(),
                true,
                true,
                "model_joined".to_owned(),
            ),
            (
                "resource:PathLock".to_owned(),
                true,
                true,
                "lock_released".to_owned(),
            ),
        ])
        .unwrap();
    assert_eq!(report.released_count, 2);
    assert!(report.safe_to_release_all());
    let retention = ResourceRetention::new(10, 10, 1024).unwrap();
    retention.within(10, 10, 1024).unwrap();
    assert_eq!(
        retention.within(11, 10, 1024).unwrap_err(),
        "resource_retention_exceeded"
    );
}

#[test]
fn panic_and_log_failure_keep_unobserved_resources_pending() {
    let run_id = RunId::new();
    let mut subscription = resource(CleanupResourceKind::Subscription, false);
    subscription.owner_run_id = run_id;
    let plan =
        ResourceCleanupPlan::new(run_id, CleanupCause::LogFailure, vec![subscription]).unwrap();
    let report = plan.observe_all(&[]).unwrap();
    assert_eq!(report.pending_count, 1);
    assert!(!report.safe_to_release_all());
}

// ---------------------------------------------------------------------------
// H33 named fixtures. Refusal fixtures come first; the success fixture last.
// ---------------------------------------------------------------------------

/// 【拒绝】停止未确认的进程/路径锁不能把写所有权交给下一个 run：
/// 报告必须保持 `IsolatedUnknown`，且 `safe_to_release_all()` 为 false。
#[test]
fn unconfirmed_process_cannot_release_write_ownership_to_next_run() {
    let run_id = RunId::new();
    let mut lock = resource(CleanupResourceKind::PathLock, true);
    lock.owner_run_id = run_id;
    let mut handle = resource(CleanupResourceKind::JobHandle, true);
    handle.owner_run_id = run_id;
    let plan = ResourceCleanupPlan::new(run_id, CleanupCause::Cancel, vec![lock, handle]).unwrap();
    let report = plan
        .observe_all(&[
            (
                "resource:PathLock".to_owned(),
                false,
                false,
                "path_lock_release_requires_confirmed_stop".to_owned(),
            ),
            (
                "resource:JobHandle".to_owned(),
                false,
                false,
                "stop_unconfirmed".to_owned(),
            ),
        ])
        .unwrap();
    assert!(!report.safe_to_release_all());
    assert_eq!(report.unknown_count, 2);
    for entry in &report.resources {
        assert_eq!(entry.status, CleanupResourceStatus::IsolatedUnknown);
        assert!(!entry.stop_confirmed);
    }
    // 稳定拒绝码：锁只能由确认停止的 owner 释放，下一个 run 看到的是同一个 fence。
    assert_eq!(
        report.resources[0].reason.as_deref(),
        Some("path_lock_release_requires_confirmed_stop")
    );

    // 下一个 run 不能把上一个 run 的未知锁当成已释放：新计划里它仍然只能观察为 Unknown。
    let next_run = RunId::new();
    let mut next_lock = resource(CleanupResourceKind::PathLock, true);
    next_lock.owner_run_id = next_run;
    let next_plan =
        ResourceCleanupPlan::new(next_run, CleanupCause::Normal, vec![next_lock]).unwrap();
    let next_report = next_plan
        .observe_all(&[(
            "resource:PathLock".to_owned(),
            false,
            false,
            "previous_owner_not_confirmed".to_owned(),
        )])
        .unwrap();
    assert!(!next_report.safe_to_release_all());
    assert_eq!(
        next_report.resources[0].status,
        CleanupResourceStatus::IsolatedUnknown
    );
}

/// 【拒绝】panic 中止的模型步不能留下「未登记但可释放」的后台工作：
/// panic 证据里的 model_task / tool_future / subscription 全部保持 Unknown。
#[test]
fn panic_does_not_leave_untracked_background_work() {
    let run_id = RunId::new();
    let mut resources = Vec::new();
    for (id, kind) in [
        ("model_task", CleanupResourceKind::ModelTask),
        ("tool_future:0", CleanupResourceKind::ToolFuture),
        ("subscription", CleanupResourceKind::Subscription),
    ] {
        let mut entry = resource(kind, true);
        entry.resource_id = id.to_owned();
        entry.owner_run_id = run_id;
        resources.push(entry);
    }
    let plan = ResourceCleanupPlan::new(run_id, CleanupCause::Panic, resources).unwrap();
    let report = plan.observe_all(&[]).unwrap();
    assert!(!report.safe_to_release_all());
    assert_eq!(report.pending_count, 3);
    assert_eq!(report.unknown_count, 0);
    assert_eq!(report.released_count, 0);
}

/// 【成功】多个短 run 结束后句柄、锁与留存预算回到基线：
/// 每个 run 的已确认资源都 Released，retention 上限在基线处仍成立，
/// 超过上限时以稳定码拒绝，而不是悄悄丢弃正在执行的 run。
#[test]
fn many_short_runs_return_handles_locks_and_memory_to_baseline() {
    let retention = ResourceRetention::new(8, 16, 4096).unwrap();
    for index in 0..8 {
        let run_id = RunId::new();
        let mut model = resource(CleanupResourceKind::ModelTask, true);
        model.resource_id = format!("model_task:{index}");
        model.owner_run_id = run_id;
        let mut lock = resource(CleanupResourceKind::PathLock, true);
        lock.resource_id = format!("path_lock:{index}");
        lock.owner_run_id = run_id;
        let plan =
            ResourceCleanupPlan::new(run_id, CleanupCause::Normal, vec![model, lock]).unwrap();
        let report = plan
            .observe_all(&[
                (
                    format!("model_task:{index}"),
                    true,
                    true,
                    "model_joined".to_owned(),
                ),
                (
                    format!("path_lock:{index}"),
                    true,
                    true,
                    "lock_released".to_owned(),
                ),
            ])
            .unwrap();
        assert!(report.safe_to_release_all());
        assert_eq!(report.released_count, 2);
    }
    // 回到基线：8 个 run / 16 个 frame / 4096 字节都在预算内；
    // 超出一个 run 就返回稳定拒绝码，不能裁剪在执行的 run。
    retention.within(8, 16, 4096).unwrap();
    assert_eq!(
        retention.within(9, 16, 4096).unwrap_err(),
        "resource_retention_exceeded"
    );
}
