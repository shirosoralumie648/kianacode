use kiana_domain::*;

fn child(
    project_id: &str,
    packet_id: &str,
    department_id: &str,
    run_id: Option<&str>,
    stop_state: ChildStopState,
) -> ProjectControlChild {
    ProjectControlChild {
        project_id: project_id.to_owned(),
        packet_id: packet_id.to_owned(),
        department_id: department_id.to_owned(),
        run_id: run_id.map(str::to_owned),
        dispatch_intent_id: run_id.map(|run| format!("dispatch:{run}")),
        stop_state,
        authority_epoch: 7,
    }
}

fn plan(
    control_id: &str,
    project_id: &str,
    action: ProjectControlAction,
    from_status: ProjectStatus,
    to_status: ProjectStatus,
    children: Vec<ProjectControlChild>,
) -> ProjectControlPlan {
    let mut value = ProjectControlPlan {
        schema: PROJECT_CONTROL_SCHEMA.to_owned(),
        control_id: control_id.to_owned(),
        project_id: project_id.to_owned(),
        action,
        from_status,
        to_status,
        baseline_version: 3,
        authority_epoch: 7,
        approval_ref: (action == ProjectControlAction::Resume)
            .then(|| "approval:resume-1".to_owned()),
        reason: "operator control".to_owned(),
        dispatch_blocked: !matches!(action, ProjectControlAction::Resume),
        reconcile_required: to_status == ProjectStatus::ResultUnknown,
        children,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn project_pause_and_cancel_propagate_without_affecting_unrelated_projects() {
    let mut ledger = ProjectControlLedger::default();
    let pause = plan(
        "control-a-pause",
        "project-a",
        ProjectControlAction::Pause,
        ProjectStatus::Active,
        ProjectStatus::Paused,
        vec![child(
            "project-a",
            "packet-a",
            "engineering",
            Some("run-a"),
            ChildStopState::Stopped,
        )],
    );
    ledger.record(pause.clone()).expect("pause");
    let unrelated = plan(
        "control-b-pause",
        "project-b",
        ProjectControlAction::Pause,
        ProjectStatus::Planned,
        ProjectStatus::Paused,
        vec![child(
            "project-b",
            "packet-b",
            "design",
            None,
            ChildStopState::NotStarted,
        )],
    );
    ledger.record(unrelated).expect("unrelated pause");
    assert_eq!(
        ledger.latest("project-a").map(|p| p.control_id.as_str()),
        Some("control-a-pause")
    );
    assert_eq!(
        ledger.latest("project-b").map(|p| p.project_id.as_str()),
        Some("project-b")
    );

    let request = plan(
        "control-a-cancel-request",
        "project-a",
        ProjectControlAction::CancelRequest,
        ProjectStatus::Paused,
        ProjectStatus::CancelRequested,
        vec![child(
            "project-a",
            "packet-a",
            "engineering",
            Some("run-a"),
            ChildStopState::StopRequested,
        )],
    );
    ledger.record(request).expect("cancel request");
    assert_eq!(
        ledger
            .record(plan(
                "control-a-cancel-confirm-bad",
                "project-a",
                ProjectControlAction::CancelConfirm,
                ProjectStatus::CancelRequested,
                ProjectStatus::Cancelled,
                vec![child(
                    "project-a",
                    "packet-a",
                    "engineering",
                    Some("run-a"),
                    ChildStopState::StopRequested,
                )],
            ))
            .unwrap_err(),
        "project_cancel_with_unconfirmed_child"
    );
    ledger
        .record(plan(
            "control-a-cancel-confirm",
            "project-a",
            ProjectControlAction::CancelConfirm,
            ProjectStatus::CancelRequested,
            ProjectStatus::Cancelled,
            vec![child(
                "project-a",
                "packet-a",
                "engineering",
                Some("run-a"),
                ChildStopState::Stopped,
            )],
        ))
        .expect("cancel confirmed");
}

#[test]
fn unknown_child_never_reports_cancelled_and_resume_requires_fresh_fences() {
    let mut unknown = plan(
        "control-unknown",
        "project-a",
        ProjectControlAction::CancelConfirm,
        ProjectStatus::CancelRequested,
        ProjectStatus::ResultUnknown,
        vec![child(
            "project-a",
            "packet-a",
            "engineering",
            Some("run-a"),
            ChildStopState::Unknown,
        )],
    );
    unknown.reconcile_required = true;
    unknown.digest = unknown.canonical_digest();
    unknown.validate().expect("unknown reconciliation");

    let mut bad_resume = plan(
        "control-resume-bad",
        "project-a",
        ProjectControlAction::Resume,
        ProjectStatus::Paused,
        ProjectStatus::Active,
        vec![child(
            "project-a",
            "packet-a",
            "engineering",
            Some("run-a"),
            ChildStopState::Unknown,
        )],
    );
    bad_resume.digest = bad_resume.canonical_digest();
    assert_eq!(
        bad_resume.validate().unwrap_err(),
        "project_control_resume_fence_invalid"
    );
}

#[test]
fn controls_are_idempotent_but_different_payloads_and_cross_project_children_are_rejected() {
    let mut ledger = ProjectControlLedger::default();
    let value = plan(
        "control-1",
        "project-a",
        ProjectControlAction::Pause,
        ProjectStatus::Active,
        ProjectStatus::Paused,
        vec![child(
            "project-a",
            "packet-a",
            "engineering",
            None,
            ChildStopState::NotStarted,
        )],
    );
    ledger.record(value.clone()).expect("first");
    ledger.record(value.clone()).expect("replay");
    let mut drift = value.clone();
    drift.reason = "different".to_owned();
    drift.digest = drift.canonical_digest();
    assert_eq!(
        ledger.record(drift).unwrap_err(),
        "project_control_duplicate_digest_mismatch"
    );

    let mut cross = value;
    cross.control_id = "control-cross".to_owned();
    cross.children[0].project_id = "project-b".to_owned();
    cross.digest = cross.canonical_digest();
    assert_eq!(
        ledger.record(cross).unwrap_err(),
        "project_control_unrelated_project"
    );
}
