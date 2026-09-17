use kiana_domain::{
    capability_action_digest, AuthenticatedPrincipalRef, CapabilityKind, CapabilityRequest,
    ExecutionScope, InvocationId, InvocationResumeBinding, ProjectIdentity, RequestContext,
    RequestId, RiskLevel, RunId, ScopeDimension, ScopeLimit, ScopeSet, SessionId, StepId, TurnId,
};
use serde_json::json;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn fixture() -> (
    RunId,
    RequestId,
    TurnId,
    StepId,
    CapabilityRequest,
    RequestContext,
) {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let turn_id = TurnId::new();
    let step_id = StepId::new();
    let mut request = CapabilityRequest::new(
        request_id,
        CapabilityKind::Process,
        "shell.exec",
        json!({
            "command": "printf ok",
            "sandbox": "read-only",
            "call_id": "h14-call",
            "run_id": run_id,
            "turn_id": turn_id,
            "step_id": step_id,
            "pending_batch_digest": digest('b'),
        }),
    )
    .with_risk(RiskLevel::ReadOnly);
    let permission_scope = ScopeSet::new(
        ScopeDimension::Restricted(vec!["shell.exec".to_owned()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::Restricted(1),
        ScopeLimit::Restricted(1),
    )
    .unwrap();
    let project = ProjectIdentity::new("/repo", "/repo", None, None, digest('p')).unwrap();
    let mut scope = ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal: AuthenticatedPrincipalRef::local(),
        project,
        session_id: SessionId::new("h14-session"),
        run_id: Some(run_id),
        turn_id: Some(turn_id),
        cell_id: None,
        grant_refs: Vec::new(),
        budget_lease_id: None,
        work_packet_id: None,
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope: permission_scope.clone(),
        read_roots: vec!["/repo".to_owned()],
        write_roots: Vec::new(),
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes: Vec::new(),
        server_scopes: Vec::new(),
        network_policy: Vec::new(),
        authority_epoch: 7,
        trust_revision: digest('t'),
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: kiana_domain::capability_action_catalog_digest(),
        action_digest: capability_action_digest(&request),
        permission_scope_digest: permission_scope.digest(),
        scope_digest: String::new(),
    };
    scope.scope_digest = scope.digest();
    request.execution_scope = Some(scope);
    let mut context = RequestContext::local("h14-session", "/repo");
    context.request_id = request_id;
    context.project_trusted = true;
    (run_id, request_id, turn_id, step_id, request, context)
}

#[test]
fn binding_round_trips_and_keeps_server_owned_identity() {
    let (run_id, event_request_id, turn_id, step_id, request, context) = fixture();
    let binding = InvocationResumeBinding::from_request(
        run_id,
        event_request_id,
        &request,
        &context,
        "read-only",
        digest('b'),
    )
    .unwrap();
    assert_eq!(
        binding.invocation_id,
        InvocationId::from_uuid(request.request_id.as_uuid())
    );
    assert_eq!(binding.turn_id, Some(turn_id));
    assert_eq!(binding.step_id, Some(step_id));
    binding
        .validate_against(run_id, event_request_id, &request, &context, "read-only")
        .unwrap();
    let encoded = serde_json::to_value(&binding).unwrap();
    let decoded: InvocationResumeBinding = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, binding);
}

#[test]
fn changed_parameters_or_owner_cannot_resume_the_binding() {
    let (run_id, event_request_id, _turn_id, _step_id, request, context) = fixture();
    let binding = InvocationResumeBinding::from_request(
        run_id,
        event_request_id,
        &request,
        &context,
        "read-only",
        digest('b'),
    )
    .unwrap();
    let mut changed = request.clone();
    changed.arguments["command"] = json!("printf changed");
    assert_eq!(
        binding
            .validate_against(run_id, event_request_id, &changed, &context, "read-only")
            .unwrap_err(),
        "invocation_resume_binding_changed"
    );
    let mut foreign = context.clone();
    foreign.actor_id = Some("different-owner".to_owned());
    assert_eq!(
        binding
            .validate_against(run_id, event_request_id, &request, &foreign, "read-only")
            .unwrap_err(),
        "invocation_resume_binding_changed"
    );
}
