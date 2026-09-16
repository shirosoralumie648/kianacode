use kiana_domain::{
    CompanyCommand, CompanyCommandPolicy, DecisionActorKind, DecisionOption, DecisionPurpose,
    HumanTask, HumanTaskStatus, RequestContext, RoleSpec,
};

fn human_context(role: &RoleSpec) -> RequestContext {
    let mut context = RequestContext::local("co05", "/tmp/kiana-co05");
    context.actor_id = Some("human-sponsor".to_owned());
    context.assign_role(role);
    context
}

#[test]
fn policy_separates_human_decision_from_agent_and_records_purpose() {
    let command = CompanyCommand::DecideAcceptance {
        acceptance_id: "acceptance-1".to_owned(),
        review_id: "review-1".to_owned(),
        decision: kiana_domain::AcceptanceDecision::Reject,
        reasons: vec!["criterion incomplete".to_owned()],
        waiver_ref: None,
    };
    let policy = command.policy();
    assert_eq!(policy.purpose, DecisionPurpose::AcceptanceDecision);
    policy.validate().unwrap();

    let context = human_context(&RoleSpec::sponsor());
    policy.authorize_context(&context).unwrap();
    let decision = policy
        .decision(&context, &command, 7, 100)
        .unwrap()
        .unwrap();
    decision.validate().unwrap();
    assert_eq!(decision.option, DecisionOption::Reject);
    assert_eq!(decision.actor_kind, DecisionActorKind::Human);
    assert_eq!(decision.target_revision, 7);
    assert!(decision.active_at(100));
    assert!(!decision.active_at(decision.expires_at_unix_ms));

    let mut agent = context;
    agent.cell_id = Some(kiana_domain::CellId::new());
    assert_eq!(
        policy.authorize_context(&agent).unwrap_err(),
        "company_human_decision_required"
    );
}

#[test]
fn human_task_binds_options_target_digest_and_scope() {
    let command = CompanyCommand::ApproveProject {
        project_id: "project-1".to_owned(),
        decision_ref: "artifact:decision".to_owned(),
    };
    let policy = CompanyCommandPolicy::for_command(&command);
    let context = human_context(&RoleSpec::sponsor());
    let decision = policy
        .decision(&context, &command, 3, 100)
        .unwrap()
        .unwrap();
    let task = HumanTask {
        schema: kiana_domain::HUMAN_TASK_SCHEMA.to_owned(),
        task_id: "task-1".to_owned(),
        purpose: policy.purpose,
        target_kind: "company_command".to_owned(),
        target_id: command.event_name().to_owned(),
        target_revision: 3,
        target_digest: decision.target_digest.clone(),
        scope_digest: decision.scope_digest.clone(),
        created_by: "human-sponsor".to_owned(),
        assigned_to: Some("human-sponsor".to_owned()),
        allowed_options: policy.purpose.allowed_options().to_vec(),
        expires_at_unix_ms: decision.expires_at_unix_ms,
        authority_epoch: 1,
        status: HumanTaskStatus::Decided,
        decision: Some(decision),
    };
    task.validate().unwrap();
    let mut forged = serde_json::to_value(task).unwrap();
    forged["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<HumanTask>(forged).is_err());
}
