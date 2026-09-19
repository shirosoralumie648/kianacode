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
