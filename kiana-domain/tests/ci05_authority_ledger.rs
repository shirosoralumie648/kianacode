use kiana_domain::*;
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!(value))
}

fn event(kind: &str, version: u64, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), version, kind, data)
        .unwrap()
        .with_stream_metadata("authority", "project-authority", version)
}

fn assignments() -> (RoleAssignment, ProjectAssignment, ProjectId, OrganizationId) {
    let principal = AuthenticatedPrincipalRef::local();
    let organization = OrganizationId::new();
    let project = ProjectId::new();
    let role_id = AssignmentId::new();
    let role = RoleAssignment::new(
        role_id,
        principal.clone(),
        organization,
        ROLE_BUILDER,
        DEPARTMENT_EXECUTING,
        vec![project],
        1,
        10_000,
        RoleAssignmentStatus::Active,
        1,
        1,
    )
    .unwrap();
    let project_assignment = ProjectAssignment::new(
        ProjectAssignmentId::new(),
        role_id,
        principal,
        organization,
        project,
        1,
        10_000,
        false,
        1,
        1,
    )
    .unwrap();
    (role, project_assignment, project, organization)
}

#[test]
fn durable_authority_ledger_rebuilds_and_intersects_sharing_scope() {
    let (role, project_assignment, source_project, organization) = assignments();
    let target_project = ProjectId::new();
    let membership =
        Membership::new(MembershipId::new(), PrincipalId::new(), organization, 1, 1).unwrap();
    let boundary = DataBoundary::new(
        DataBoundaryId::new(),
        vec![source_project],
        vec!["internal".to_owned()],
        false,
        1,
    )
    .unwrap();
    let profile = PolicyProfile::new(
        PolicyProfileId::new(),
        vec!["memory.search".to_owned(), "shell.exec".to_owned()],
        boundary.boundary_id,
        1,
    )
    .unwrap();
    let sharing = SharingGrant::new(
        SharingGrantId::new(),
        source_project,
        target_project,
        vec!["artifact:receipt".to_owned()],
        "review",
        vec!["memory.search".to_owned()],
        500,
        1,
    )
    .unwrap();

    let events = vec![
        event(
            "authority.revised",
            1,
            json!({"revision_digest": digest("v1")}),
        ),
        event("membership.bound", 2, json!({"membership": membership})),
        event("role_assignment.bound", 3, json!({"assignment": role})),
        event(
            "project_assignment.bound",
            4,
            json!({"assignment": project_assignment}),
        ),
        event("data_boundary.bound", 5, json!({"boundary": boundary})),
        event("policy_profile.bound", 6, json!({"profile": profile})),
        event("sharing_grant.bound", 7, json!({"grant": sharing})),
    ];
    let ledger = AuthorityLedger::rebuild(&events).unwrap();
    assert_eq!(ledger.authority_epoch, 1);
    assert_eq!(ledger.stream_version, 7);
    assert_eq!(ledger.role_assignments().count(), 1);
    assert_eq!(ledger.project_assignments().count(), 1);
    assert_eq!(
        ledger.sharing_operations(source_project, target_project, 100),
        std::collections::BTreeSet::from(["memory.search".to_owned()])
    );
    assert!(ledger
        .sharing_operations(target_project, source_project, 100)
        .is_empty());
    assert!(!ledger.snapshot_digest().is_empty());

    let mut stale = events.clone();
    stale.push(event(
        "sharing_grant.revoked",
        8,
        json!({"grant_id": sharing.grant_id, "authority_epoch": 2}),
    ));
    let revoked = AuthorityLedger::rebuild(&stale).unwrap();
    assert!(revoked
        .sharing_operations(source_project, target_project, 100)
        .is_empty());
}

#[test]
fn authority_ledger_rejects_unknown_or_noncontiguous_facts() {
    let unknown = event("authority.future", 1, json!({}));
    assert_eq!(
        AuthorityLedger::rebuild(&[unknown])
            .unwrap_err()
            .to_string(),
        "authority_event_kind_unknown"
    );
    let gap = vec![
        event("authority.revised", 1, json!({})),
        event("authority.revised", 3, json!({})),
    ];
    assert_eq!(
        AuthorityLedger::rebuild(&gap).unwrap_err().to_string(),
        "authority_event_version_gap_or_regression"
    );
}
