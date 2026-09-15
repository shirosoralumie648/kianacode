use kiana_domain::{AuthenticatedPrincipalRef, ProjectIdentity, SessionAssignment};
use serde_json::json;

#[test]
fn principal_and_project_identity_are_server_shaped_and_deterministic() {
    let principal = AuthenticatedPrincipalRef::local();
    principal.validate().unwrap();
    assert_eq!(principal.principal_id, "local-user");
    assert!(!principal.principal_digest.is_empty());

    let first =
        ProjectIdentity::new("/repo", "/repo", Some(1), Some(2), json_digest("trusted")).unwrap();
    let second = ProjectIdentity::new(
        "/alias/../repo",
        "/repo",
        Some(1),
        Some(2),
        json_digest("trusted"),
    )
    .unwrap();
    assert_eq!(first.project_id, second.project_id);
    assert_eq!(first.identity_digest, second.identity_digest);
}

#[test]
fn assignment_binds_principal_project_role_and_department() {
    let principal = AuthenticatedPrincipalRef::local();
    let project =
        ProjectIdentity::new("/repo", "/repo", None, None, json_digest("trusted")).unwrap();
    let assignment = SessionAssignment::new(
        principal,
        project,
        "session-1",
        "builder",
        "executing",
        1,
        1,
    )
    .unwrap();
    assignment.validate().unwrap();
    let mut encoded = serde_json::to_value(&assignment).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SessionAssignment>(encoded).is_err());
    let mut tampered = assignment.clone();
    tampered.role_id = "reviewer".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "session_assignment_digest_mismatch"
    );
}

fn json_digest(value: &str) -> String {
    kiana_domain::json_digest(&json!(value))
}
