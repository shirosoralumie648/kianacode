use kiana_domain::*;

fn sponsor_context() -> RequestContext {
    let mut context = RequestContext::local("cp23-session", "/tmp/cp23-company");
    context.actor_id = Some("human-sponsor".to_owned());
    context.assign_role(&RoleSpec::sponsor());
    context
}

fn approval_command() -> CompanyCommand {
    CompanyCommand::ApproveProject {
        project_id: "project-1".to_owned(),
        decision_ref: "decision-1".to_owned(),
    }
}

fn decision_fixture() -> (
    CompanyCommandPolicy,
    RequestContext,
    CompanyCommand,
    HumanDecision,
) {
    let context = sponsor_context();
    let command = approval_command();
    let policy = command.policy();
    let decision = policy
        .decision(&context, &command, 7, 1_000)
        .expect("policy should create a server decision")
        .expect("project approval requires a human decision");
    (policy, context, command, decision)
}

#[test]
fn cp23_accepts_a_server_decision_bound_to_the_current_command_and_scope() {
    let (policy, context, command, decision) = decision_fixture();

    assert_eq!(decision.actor_kind, DecisionActorKind::Human);
    assert_eq!(decision.target_revision, 7);
    assert!(policy
        .validate_decision_for(Some(&decision), &context, &command, 7, 1_001)
        .is_ok());
}

#[test]
fn cp23_rejects_actor_role_and_session_binding_mismatches() {
    let (policy, context, command, decision) = decision_fixture();

    let mut other_actor = context.clone();
    other_actor.actor_id = Some("different-sponsor".to_owned());
    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &other_actor, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );

    let mut other_role = context.clone();
    other_role.assign_role(&RoleSpec::pm());
    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &other_role, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );

    let mut other_session = context.clone();
    other_session.session_id = SessionId::new("different-session");
    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &other_session, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );
}

#[test]
fn cp23_rejects_command_revision_and_scope_digest_mismatches() {
    let (policy, context, command, decision) = decision_fixture();

    let mut changed_command = command.clone();
    if let CompanyCommand::ApproveProject { decision_ref, .. } = &mut changed_command {
        *decision_ref = "different-decision".to_owned();
    }
    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &context, &changed_command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );

    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &context, &command, 8, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );

    let mut changed_scope = decision.clone();
    changed_scope.scope_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        policy
            .validate_decision_for(Some(&changed_scope), &context, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );
}

#[test]
fn cp23_rejects_expired_or_missing_decisions_and_unexpected_decisions() {
    let (policy, context, command, decision) = decision_fixture();

    assert_eq!(
        policy
            .validate_decision_for(
                Some(&decision),
                &context,
                &command,
                7,
                decision.expires_at_unix_ms,
            )
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );
    assert_eq!(
        policy
            .validate_decision_for(None, &context, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_missing"
    );

    let non_human_command = CompanyCommand::RegisterArtifact {
        artifact_id: "artifact-1".to_owned(),
        relative_path: "OUTPUT.txt".to_owned(),
    };
    assert_eq!(
        non_human_command
            .policy()
            .validate_decision_for(Some(&decision), &context, &non_human_command, 7, 1_001,)
            .unwrap_err(),
        "company_unexpected_human_decision"
    );
}
