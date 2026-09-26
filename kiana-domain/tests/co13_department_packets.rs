use kiana_domain::*;

fn result() -> ResultContract {
    ResultContract {
        schema: RESULT_CONTRACT_SCHEMA.to_owned(),
        version: 1,
        output_schema: "kiana.analysis-result.v1".to_owned(),
        required_refs: vec!["artifact:proposal".to_owned()],
        max_bytes: 4096,
    }
}

fn packet(kind: DepartmentPacketKind, basis: PacketInputBasis) -> DepartmentPacket {
    DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_owned(),
        packet_id: "department-packet-1".to_owned(),
        project_id: "project-1".to_owned(),
        version: 1,
        kind,
        from_department: "initiating".to_owned(),
        target_role: "analyst".to_owned(),
        assignment_ref: "assignment:analyst".to_owned(),
        input_basis: basis,
        input_refs: vec!["artifact:intake".to_owned()],
        result: result(),
        write_scope: Vec::new(),
        plan_ref: None,
        runtime_grant: None,
        budget_lease: None,
    }
}

#[test]
fn department_packet_cannot_smuggle_builder_write_scope_or_runtime_grants() {
    let mut analysis = packet(
        DepartmentPacketKind::IntakeAnalysis,
        PacketInputBasis::Intake,
    );
    analysis.write_scope = vec!["src/lib.rs".to_owned()];
    assert_eq!(
        analysis.validate().unwrap_err(),
        "department_packet_write_scope_denied"
    );

    let mut forged = packet(
        DepartmentPacketKind::IntakeAnalysis,
        PacketInputBasis::Intake,
    );
    forged.runtime_grant = Some("grant:forged".to_owned());
    assert_eq!(
        forged.validate().unwrap_err(),
        "department_packet_runtime_authority_forbidden"
    );

    let mut implementation = packet(DepartmentPacketKind::Implementation, PacketInputBasis::Plan);
    implementation.from_department = "executing".to_owned();
    implementation.target_role = "builder".to_owned();
    implementation.write_scope = vec!["src/lib.rs".to_owned()];
    assert_eq!(
        implementation.validate().unwrap_err(),
        "department_packet_plan_required"
    );

    let mut legacy = WorkPacket::builder_task("legacy", "keep Builder v1");
    legacy.assignee_role = "analyst".to_owned();
    assert_eq!(
        legacy.validate().unwrap_err(),
        "packet_role_must_be_builder"
    );
}

#[test]
fn analysis_and_implementation_packets_validate_through_versioned_contracts() {
    let analysis = packet(
        DepartmentPacketKind::IntakeAnalysis,
        PacketInputBasis::Intake,
    );
    analysis.validate().expect("analysis packet");

    let mut implementation = DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_owned(),
        packet_id: "implementation-1".to_owned(),
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
            required_refs: vec!["artifact:receipt".to_owned()],
            max_bytes: 65_536,
        },
        write_scope: vec!["src/lib.rs".to_owned()],
        plan_ref: Some("plan:v1".to_owned()),
        runtime_grant: None,
        budget_lease: None,
    };
    implementation.validate().expect("implementation packet");

    implementation.plan_ref = None;
    assert_eq!(
        implementation.validate().unwrap_err(),
        "department_packet_plan_required"
    );
}
