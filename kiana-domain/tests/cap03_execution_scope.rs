use kiana_domain::{
    capability_action_digest, AuthenticatedPrincipalRef, CapabilityKind, CapabilityRequest,
    ExecutionScope, ProjectIdentity, RequestId, RiskLevel, RunId, ScopeDimension, ScopeLimit,
    ScopeSet, SessionId, TurnId,
};
use serde_json::json;

fn scope(request: &CapabilityRequest, permission_scope: ScopeSet) -> ExecutionScope {
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        kiana_domain::json_digest(&json!("trusted")),
    )
    .unwrap();
    let mut result = ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal,
        project,
        session_id: SessionId::new("session-cap03"),
        run_id: Some(RunId::new()),
        turn_id: Some(TurnId::new()),
        cell_id: None,
        grant_refs: Vec::new(),
        budget_lease_id: None,
        work_packet_id: None,
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope: permission_scope.clone(),
        read_roots: vec!["/repo".to_owned()],
        write_roots: vec!["src".to_owned()],
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes: Vec::new(),
        server_scopes: Vec::new(),
        network_policy: Vec::new(),
        authority_epoch: 1,
        trust_revision: kiana_domain::json_digest(&json!("trusted")),
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: kiana_domain::capability_action_catalog_digest(),
        action_digest: capability_action_digest(request),
        permission_scope_digest: permission_scope.digest(),
        scope_digest: String::new(),
    };
    result.scope_digest = result.digest();
    result
}

#[test]
fn execution_scope_is_server_shaped_and_digest_bound() {
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({"command":"printf ok","sandbox":"read-only"}),
    )
    .with_risk(RiskLevel::ReadOnly);
    let permission = ScopeSet::new(
        ScopeDimension::Restricted(vec!["shell.exec".to_owned()]),
        ScopeDimension::Restricted(vec!["src".to_owned()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::Restricted(10),
        ScopeLimit::Restricted(1),
    )
    .unwrap();
    let execution = scope(&request, permission);
    execution.validate().unwrap();
    execution.validate_for_request(&request).unwrap();
    let mut unknown = serde_json::to_value(&execution).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ExecutionScope>(unknown).is_err());
    let mut tampered = execution.clone();
    tampered.action_digest = kiana_domain::json_digest(&json!("changed"));
    tampered.scope_digest = tampered.digest();
    assert_eq!(
        tampered.validate_for_request(&request).unwrap_err(),
        "execution_scope_action_digest_mismatch"
    );
}

#[test]
fn scope_intersection_never_grows_under_delegation() {
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({"command":"printf ok","sandbox":"read-only"}),
    );
    let parent_permission = ScopeSet::new(
        ScopeDimension::Restricted(vec!["shell.exec".to_owned(), "apply_patch".to_owned()]),
        ScopeDimension::Restricted(vec!["src".to_owned()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::Restricted(10),
        ScopeLimit::Restricted(2),
    )
    .unwrap();
    let child_permission = ScopeSet::new(
        ScopeDimension::Restricted(vec!["shell.exec".to_owned()]),
        ScopeDimension::Restricted(vec!["src/lib".to_owned()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::Restricted(5),
        ScopeLimit::Restricted(1),
    )
    .unwrap();
    let parent = scope(&request, parent_permission);
    let mut child = scope(&request, child_permission);
    child.principal = parent.principal.clone();
    child.project = parent.project.clone();
    child.session_id = parent.session_id.clone();
    child.grant_refs = parent.grant_refs.clone();
    child.scope_digest = child.digest();
    assert!(child.is_subset_of(&parent).unwrap());
}

#[test]
fn empty_effective_scope_never_becomes_workspace_write() {
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({"command":"printf ok","sandbox":"read-only"}),
    );
    let empty = ScopeSet::new(
        ScopeDimension::Restricted(Vec::new()),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::Restricted(0),
        ScopeLimit::Restricted(0),
    )
    .unwrap();
    let mut execution = scope(&request, empty);
    execution.scope_digest = execution.digest();
    assert_eq!(execution.validate().unwrap_err(), "execution_scope_empty");
}
