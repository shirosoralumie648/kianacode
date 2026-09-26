use kiana_domain::*;

fn builder_packet() -> WorkPacket {
    let mut packet = WorkPacket::builder_task("packet-1", "implement the frozen plan");
    packet.project_id = Some(ProjectId::new());
    packet.inputs = vec!["artifact:plan".to_owned()];
    packet.path_allow = vec!["src/".to_owned()];
    packet
}

fn builder_context(packet: &WorkPacket) -> RequestContext {
    let mut context = RequestContext::local("parent-session", "/workspace/project");
    context.assign_role(&RoleSpec::builder());
    context.work_packet_id = Some(packet.id.clone());
    context.path_allow = packet.path_allow.clone();
    context
}

fn department_packet() -> DepartmentPacket {
    DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_owned(),
        packet_id: "department-packet-1".to_owned(),
        project_id: "project-1".to_owned(),
        version: 1,
        kind: DepartmentPacketKind::Implementation,
        from_department: "executing".to_owned(),
        target_role: "builder".to_owned(),
        assignment_ref: "assignment:builder".to_owned(),
        input_basis: PacketInputBasis::Plan,
        input_refs: vec!["artifact:plan".to_owned()],
        result: ResultContract {
            schema: RESULT_CONTRACT_SCHEMA.to_owned(),
            version: 1,
            output_schema: "kiana.implementation-result.v1".to_owned(),
            required_refs: vec!["artifact:result".to_owned()],
            max_bytes: 8_192,
        },
        write_scope: vec!["src/".to_owned()],
        plan_ref: Some("plan:1".to_owned()),
        runtime_grant: None,
        budget_lease: None,
    }
}

#[test]
fn company_run_rejects_forged_scope_and_private_planning_context() {
    let packet = builder_packet();
    let mut context = builder_context(&packet);
    assert!(CompanyTaskScope::company_builder(&context, &packet).is_ok());

    context.path_allow = vec!["other/".to_owned()];
    assert_eq!(
        CompanyTaskScope::company_builder(&context, &packet).unwrap_err(),
        "company_task_path_scope_mismatch"
    );
    let mut private_packet = packet.clone();
    private_packet.inputs = vec!["private:planning-transcript".to_owned()];
    context.path_allow = packet.path_allow.clone();
    assert_eq!(
        CompanyTaskScope::company_builder(&context, &private_packet).unwrap_err(),
        "company_task_private_history_forbidden"
    );
}

#[test]
fn department_tasks_share_scope_contract_with_fresh_isolated_runs() {
    let packet = department_packet();
    let scope = CompanyTaskScope::for_department_packet(
        &packet,
        SessionId::new("planner-session"),
        SessionId::new("builder-session"),
        vec!["artifact:plan".to_owned()],
        vec!["src/".to_owned()],
    )
    .expect("scope");
    let run = scope
        .fresh_run(
            RunId::new(),
            "attempt:1",
            SessionId::new("fresh-builder-session"),
        )
        .expect("fresh run");
    assert_ne!(run.session_id, run.parent_session_id.clone().unwrap());
    assert_eq!(run.packet_id, packet.packet_id);
    assert_eq!(run.input_refs, packet.input_refs);
    assert!(run.validate().is_ok());
    assert_eq!(scope.scope_digest, scope.canonical_digest());

    assert_eq!(
        CompanyTaskScope::for_department_packet(
            &packet,
            SessionId::new("same-session"),
            SessionId::new("same-session"),
            vec!["artifact:plan".to_owned()],
            vec!["src/".to_owned()],
        )
        .unwrap_err(),
        "company_task_session_must_be_fresh"
    );
}

#[test]
fn standalone_mode_cannot_carry_company_binding_and_reopen_preserves_scope() {
    let packet = builder_packet();
    let context = RequestContext::local("standalone-session", "/workspace/project");
    let scope = CompanyTaskScope::standalone(&context, &packet).expect("standalone");
    assert_eq!(scope.mode, TaskExecutionMode::Standalone);
    assert!(scope.project_id.is_none());
    assert!(scope.packet_id.is_none());
    let reopened: CompanyTaskScope =
        serde_json::from_value(serde_json::to_value(&scope).expect("serialize")).expect("reopen");
    assert_eq!(reopened, scope);
    assert!(reopened.validate().is_ok());
}
