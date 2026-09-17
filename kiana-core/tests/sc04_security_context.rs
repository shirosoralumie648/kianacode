use kiana_core::{SecurityContext, SECURITY_CONTEXT_SCHEMA};
use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, ProjectIdentity, RequestContext, RoleSpec,
    SecurityReasonCode,
};
use serde_json::json;

fn fixture_context() -> (
    RequestContext,
    AuthenticatedPrincipalRef,
    ProjectIdentity,
    RoleSpec,
) {
    let request = RequestContext::local("session-sc04", "/repo");
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        json_digest(&json!({"trusted":true})),
    )
    .unwrap();
    (request, principal, project, RoleSpec::builder())
}

#[test]
fn security_context_is_server_owned_versioned_and_round_trips() {
    let (mut request, principal, project, role) = fixture_context();
    request.project_trusted = true;
    let context = SecurityContext::from_server(
        &request,
        principal,
        project,
        &role,
        true,
        json_digest(&json!({"policy":"sc04"})),
        3,
        2,
    )
    .unwrap();
    assert_eq!(context.schema, SECURITY_CONTEXT_SCHEMA);
    assert_eq!(context.authority_epoch, 3);
    assert_eq!(context.data_epoch, 2);
    assert!(context.validate_server().is_ok());
    let encoded = context.to_json().unwrap();
    assert_eq!(SecurityContext::from_json(&encoded).unwrap(), context);

    let applied = context.apply_to_request(request).unwrap();
    assert_eq!(applied.actor_id.as_deref(), Some("local-user"));
    assert!(applied.project_trusted);
    assert_eq!(applied.role_id, role.role_id);
}

#[test]
fn caller_identity_project_role_and_trust_assertions_fail_closed() {
    let (request, principal, project, role) = fixture_context();
    let policy = json_digest(&json!({"policy":"sc04"}));

    let mut actor = request.clone();
    actor.actor_id = Some("caller-controlled".to_owned());
    assert_eq!(
        SecurityContext::from_server(
            &actor,
            principal.clone(),
            project.clone(),
            &role,
            true,
            policy.clone(),
            1,
            1,
        )
        .unwrap_err(),
        SecurityReasonCode::AuthCallerUntrusted.as_str()
    );

    let mut anonymous = request.clone();
    anonymous.actor_id = None;
    assert_eq!(
        SecurityContext::from_server(
            &anonymous,
            principal.clone(),
            project.clone(),
            &role,
            true,
            policy.clone(),
            1,
            1,
        )
        .unwrap_err(),
        SecurityReasonCode::AuthPrincipalMissing.as_str()
    );

    let mut foreign_project = request.clone();
    foreign_project.project_root = "/other".to_owned();
    assert_eq!(
        SecurityContext::from_server(
            &foreign_project,
            principal.clone(),
            project.clone(),
            &role,
            true,
            policy.clone(),
            1,
            1,
        )
        .unwrap_err(),
        SecurityReasonCode::AuthProjectMismatch.as_str()
    );

    let mut foreign_role = request.clone();
    foreign_role.role_id = "reviewer".to_owned();
    assert_eq!(
        SecurityContext::from_server(
            &foreign_role,
            principal.clone(),
            project.clone(),
            &role,
            true,
            policy.clone(),
            1,
            1,
        )
        .unwrap_err(),
        SecurityReasonCode::AuthRoleMismatch.as_str()
    );

    let mut forged_trust = request;
    forged_trust.project_trusted = true;
    let untrusted = SecurityContext::from_server(
        &forged_trust,
        principal,
        project,
        &role,
        false,
        policy,
        1,
        1,
    );
    assert_eq!(
        untrusted.unwrap_err(),
        SecurityReasonCode::AuthCallerUntrusted.as_str()
    );
}

#[test]
fn untrusted_server_snapshot_cannot_be_used_for_effects() {
    let (request, principal, project, role) = fixture_context();
    let context = SecurityContext::from_server(
        &request,
        principal,
        project,
        &role,
        false,
        json_digest(&json!({"policy":"sc04"})),
        1,
        1,
    )
    .unwrap();
    assert_eq!(
        context.require_trusted_for_effect().unwrap_err(),
        "AUTH_PROJECT_UNTRUSTED"
    );
}
