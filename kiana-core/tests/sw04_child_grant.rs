use kiana_core::derive_swarm_child_grant;
use kiana_domain::{
    AgentTemplate, CapabilityGrant, CapabilityGrantId, CapabilityKind, ProjectId, RoleSpec,
    WorkPacket, CAPABILITY_GRANT_SCHEMA, SWARM_CHILD_TEMPLATE,
};

fn parent(_project: ProjectId) -> CapabilityGrant {
    CapabilityGrant {
        schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
        grant_id: CapabilityGrantId::new(),
        capability: CapabilityKind::Other("coding".to_owned()),
        operation: "builder.packet".to_owned(),
        resources: vec!["workspace".to_owned()],
        paths: vec!["src".to_owned()],
        expires_at_unix_ms: 10_000,
        approval_id: None,
        delegation_allowed: true,
    }
}

fn inputs(project: ProjectId) -> (CapabilityGrant, AgentTemplate, RoleSpec, WorkPacket) {
    let role = RoleSpec::builder();
    let mut template = AgentTemplate::for_role(&role, SWARM_CHILD_TEMPLATE);
    template.template_id = kiana_domain::swarm_child_template_id();
    template.max_depth = 1;
    let mut packet =
        WorkPacket::builder_task("packet-1", "bounded child").with_path_allow(["src/lib.rs"]);
    packet.project_id = Some(project);
    packet.deadline_unix_ms = Some(5_000);
    (parent(project), template, role, packet)
}

#[test]
fn swarm_child_grant_is_partition_specific_and_non_delegable() {
    let principal = kiana_domain::PrincipalId::new();
    let project = ProjectId::new();
    let (parent, template, role, packet) = inputs(project);
    let child = derive_swarm_child_grant(
        &parent,
        &template,
        &role,
        &["src".to_owned()],
        &packet,
        principal,
        project,
        7,
        None,
        1_000,
    )
    .unwrap();
    assert_eq!(child.paths, ["src/lib.rs"]);
    assert!(!child.delegation_allowed);
    assert!(child.expires_at_unix_ms <= parent.expires_at_unix_ms);
    assert!(parent.contains(&child));
}

#[test]
fn swarm_child_grant_rejects_superset_role_project_secret_and_stale_epoch() {
    let principal = kiana_domain::PrincipalId::new();
    let project = ProjectId::new();
    let (parent, template, role, mut packet) = inputs(project);

    packet.path_allow = vec!["secrets".to_owned()];
    assert!(derive_swarm_child_grant(
        &parent,
        &template,
        &role,
        &["src".to_owned()],
        &packet,
        principal,
        project,
        7,
        None,
        1_000,
    )
    .is_err());

    packet.path_allow = vec!["src/lib.rs".to_owned()];
    packet.project_id = Some(ProjectId::new());
    assert_eq!(
        derive_swarm_child_grant(
            &parent,
            &template,
            &role,
            &["src".to_owned()],
            &packet,
            principal,
            project,
            7,
            None,
            1_000,
        )
        .unwrap_err(),
        "swarm_packet_project_mismatch"
    );

    assert_eq!(
        derive_swarm_child_grant(
            &parent,
            &template,
            &role,
            &["src".to_owned()],
            &packet,
            principal,
            project,
            0,
            None,
            1_000,
        )
        .unwrap_err(),
        "swarm_authority_epoch_missing"
    );
}
