use kiana_domain::{
    CostLedger, EventId, ProjectBudget, ProjectId, Quota, RequestId, RunId, RuntimeBudget,
    UsageRecord,
};

#[test]
fn usage_records_roll_up_without_inventing_cost() {
    let run_id = RunId::new();
    let records = vec![
        UsageRecord {
            event_id: EventId::new(),
            request_id: RequestId::new(),
            run_id,
            step: 1,
            provider_id: Some("fixture-provider".to_owned()),
            model_id: Some("fixture-model".to_owned()),
            input_tokens: Some(12),
            output_tokens: Some(4),
            elapsed_ms: 20,
        },
        UsageRecord {
            event_id: EventId::new(),
            request_id: RequestId::new(),
            run_id,
            step: 2,
            provider_id: Some("fixture-provider".to_owned()),
            model_id: Some("fixture-model".to_owned()),
            input_tokens: Some(8),
            output_tokens: Some(3),
            elapsed_ms: 15,
        },
    ];
    let ledger = CostLedger::from_records(records);
    assert_eq!(ledger.input_tokens, Some(20));
    assert_eq!(ledger.output_tokens, Some(7));
    assert_eq!(ledger.cost_micros, None);
}

#[test]
fn runtime_project_and_quota_limits_remain_separate() {
    let runtime = RuntimeBudget {
        max_model_calls: 2,
        max_tokens: 100,
        max_wall_time_ms: 1_000,
    };
    assert!(runtime.validate().is_ok());
    assert!(runtime.max_wall_time_ms > 0);

    let project = ProjectBudget {
        project_id: ProjectId::new(),
        max_runs: 3,
        max_tokens: 300,
    };
    assert!(project.check_reservation(1, 100, 100).is_ok());
    assert!(project.check_reservation(3, 0, 1).is_err());

    let quota = Quota {
        scope: "project-fixture".to_owned(),
        model_calls: 4,
        tokens: 400,
        concurrency: 2,
    };
    assert!(quota.check_reservation(2, 200, 1).is_ok());
    assert!(quota.check_reservation(5, 1, 1).is_err());
    assert!(quota.check_reservation(1, 401, 1).is_err());
    assert!(quota.check_reservation(1, 1, 3).is_err());
}
