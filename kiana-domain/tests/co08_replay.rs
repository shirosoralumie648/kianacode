use kiana_domain::{
    CompanyAuthority, CompanyCommand, CompanyCommandRequest, CompanyEvent, CompanyProof,
    CompanyReplayReducer, MetricDirection, Objective, ObjectiveStatus, RequestId, RuntimeEvent,
    COMPANY_COMMAND_SCHEMA, COMPANY_EVENT_SCHEMA,
};

const ROOT: &str = "/tmp/kiana-co08";
const OWNER: &str = "human-sponsor";
const AGGREGATE: &str = "human-sponsor\n/tmp/kiana-co08";

fn objective() -> Objective {
    Objective {
        objective_id: "objective-1".to_owned(),
        organization_id: "org-1".to_owned(),
        title: "Improve response".to_owned(),
        problem: "slow response".to_owned(),
        metric: "latency".to_owned(),
        baseline: 100.0,
        target: 50.0,
        unit: "ms".to_owned(),
        direction: MetricDirection::AtMost,
        period_start: 1,
        period_end: 2,
        owner_principal_id: OWNER.to_owned(),
        status: ObjectiveStatus::Proposed,
        version: 1,
    }
}

fn event(
    command: CompanyCommand,
    key: &str,
    expected_revision: u64,
    stream_version: u64,
) -> RuntimeEvent {
    let request = CompanyCommandRequest {
        schema: COMPANY_COMMAND_SCHEMA.to_owned(),
        expected_revision,
        idempotency_key: key.to_owned(),
        command: command.clone(),
    };
    let authority = CompanyAuthority {
        actor_id: OWNER.to_owned(),
        role_id: "sponsor".to_owned(),
        session_id: kiana_domain::SessionId::new("co08-session"),
        now_ms: 100 + stream_version,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    };
    let record = CompanyEvent {
        schema: COMPANY_EVENT_SCHEMA.to_owned(),
        project_root: ROOT.to_owned(),
        owner_id: OWNER.to_owned(),
        request,
        authority,
        proof: CompanyProof::default(),
    };
    RuntimeEvent::new(
        RequestId::new(),
        1,
        format!("company.{}", command.event_name()),
        serde_json::to_value(record).unwrap(),
    )
    .unwrap()
    .with_stream_metadata("company", AGGREGATE, stream_version)
    .with_idempotency_key(format!("company:{AGGREGATE}:{key}"))
}

#[test]
fn company_v1_history_rebuilds_identically_after_replay() {
    let events = vec![
        event(
            CompanyCommand::ProposeObjective {
                objective: objective(),
            },
            "objective-propose",
            0,
            1,
        ),
        event(
            CompanyCommand::DecideObjective {
                objective_id: "objective-1".to_owned(),
                approve: true,
            },
            "objective-approve",
            1,
            2,
        ),
    ];
    let mut first = CompanyReplayReducer::new(AGGREGATE, ROOT, OWNER);
    for event in &events {
        first.apply(event).unwrap();
    }
    let mut second = CompanyReplayReducer::new(AGGREGATE, ROOT, OWNER);
    for event in &events {
        second.apply(event).unwrap();
    }
    assert_eq!(first.state(), second.state());
    assert_eq!(first.history().len(), 2);
    assert_eq!(first.state().revision, 2);
}

#[test]
fn replay_rejects_gaps_duplicates_and_unknown_schema_without_state_change() {
    let mut reducer = CompanyReplayReducer::new(AGGREGATE, ROOT, OWNER);
    let gap = event(
        CompanyCommand::ProposeObjective {
            objective: objective(),
        },
        "gap",
        0,
        2,
    );
    assert_eq!(reducer.apply(&gap).unwrap_err(), "company_replay_gap");
    assert_eq!(reducer.state().revision, 0);

    let first = event(
        CompanyCommand::ProposeObjective {
            objective: objective(),
        },
        "duplicate",
        0,
        1,
    );
    reducer.apply(&first).unwrap();
    let mut duplicate = first.clone();
    duplicate.stream_version = Some(2);
    assert_eq!(
        reducer.apply(&duplicate).unwrap_err(),
        "company_replay_duplicate_command"
    );
    assert_eq!(reducer.state().revision, 1);

    let mut unknown = event(
        CompanyCommand::DecideObjective {
            objective_id: "objective-1".to_owned(),
            approve: true,
        },
        "unknown",
        1,
        2,
    );
    unknown.data["schema"] = serde_json::json!("kiana.company-event.v2");
    assert_eq!(
        reducer.apply(&unknown).unwrap_err(),
        "company_event_schema_unsupported"
    );
    assert_eq!(reducer.state().revision, 1);
}

#[test]
fn legacy_v0_event_migrates_only_when_the_shape_is_currently_parseable() {
    let mut legacy = event(
        CompanyCommand::ProposeObjective {
            objective: objective(),
        },
        "legacy",
        0,
        1,
    );
    legacy.data["schema"] = serde_json::json!("kiana.company-event.v0");
    let mut reducer = CompanyReplayReducer::new(AGGREGATE, ROOT, OWNER);
    reducer.apply(&legacy).unwrap();
    assert_eq!(reducer.state().revision, 1);
}
