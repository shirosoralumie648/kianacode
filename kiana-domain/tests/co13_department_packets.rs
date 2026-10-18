//! CO-13 domain fixtures: versioned department work contracts are admitted only against the
//! server-derived approval stage and an admitted CO-05 appointment.
//!
//! Deny paths come first: a Builder write scope or a smuggled grant/lease inside a
//! non-implementation contract, a drifted role/department/basis pair, an unapproved Charter or
//! Plan stage, an inactive or mismatched appointment and a plan binding that does not match the
//! frozen Plan revision. The happy path closes the file: an intake-basis analysis packet and a
//! Plan-basis implementation packet both admit through the same versioned contract, and the
//! admitted evidence carries no runtime authority.
use kiana_domain::*;
use serde_json::json;

fn coverage_digest() -> String {
    json_digest(&json!({"fixture": "co13-department-packet"}))
}

fn result_contract(output_schema: &str) -> ResultContract {
    ResultContract {
        schema: RESULT_CONTRACT_SCHEMA.to_owned(),
        version: 1,
        output_schema: output_schema.to_owned(),
        required_refs: vec!["artifact:proposal".to_owned()],
        max_bytes: 4096,
    }
}

fn analysis_packet() -> DepartmentPacket {
    DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_owned(),
        packet_id: "department-packet-analysis".to_owned(),
        project_id: "project-1".to_owned(),
        version: 1,
        kind: DepartmentPacketKind::IntakeAnalysis,
        from_department: "initiating".to_owned(),
        target_role: "analyst".to_owned(),
        assignment_ref: "assignment:analyst".to_owned(),
        input_basis: PacketInputBasis::Intake,
        input_refs: vec!["artifact:intake".to_owned()],
        result: result_contract("kiana.analysis-result.v1"),
        write_scope: Vec::new(),
        charter_baseline_version: 0,
        plan_ref: None,
        plan_binding: None,
        runtime_grant: None,
        budget_lease: None,
    }
}

fn plan_draft_packet(baseline_version: u64) -> DepartmentPacket {
    DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_owned(),
        packet_id: "department-packet-plan".to_owned(),
        project_id: "project-1".to_owned(),
        version: 1,
        kind: DepartmentPacketKind::PlanDraft,
        from_department: "planning".to_owned(),
        target_role: "pm".to_owned(),
        assignment_ref: "assignment:pm".to_owned(),
        input_basis: PacketInputBasis::Charter,
        input_refs: vec!["artifact:charter".to_owned()],
        result: result_contract("kiana.plan-draft-result.v1"),
        write_scope: vec!["plan/".to_owned()],
        charter_baseline_version: baseline_version,
        plan_ref: None,
        plan_binding: None,
        runtime_grant: None,
        budget_lease: None,
    }
}

fn frozen_plan(
    project_id: &str,
    charter_baseline_version: u64,
    plan_version: u64,
) -> FrozenPlanGraph {
    let mut draft = PlanProposalDraft::new();
    draft
        .push_node(PlanNode {
            node_id: "m1".to_owned(),
            project_id: project_id.to_owned(),
            kind: PlanNodeKind::Milestone,
            version: 1,
            required: true,
            criterion_refs: Vec::new(),
            input_refs: Vec::new(),
        })
        .expect("milestone node");
    draft
        .push_node(PlanNode {
            node_id: "p1".to_owned(),
            project_id: project_id.to_owned(),
            kind: PlanNodeKind::Packet,
            version: 1,
            required: true,
            criterion_refs: Vec::new(),
            input_refs: Vec::new(),
        })
        .expect("packet node");
    draft
        .push_edge(PlanEdge {
            from: "m1".to_owned(),
            to: "p1".to_owned(),
            kind: PlanEdgeKind::ParentChild,
            reference: None,
        })
        .expect("milestone to packet");
    let plan = draft
        .freeze_versioned(
            project_id,
            charter_baseline_version,
            plan_version,
            coverage_digest(),
        )
        .expect("frozen plan proposal");
    FrozenPlanGraph::from_plan(&plan).expect("frozen plan graph")
}

fn implementation_packet(
    project_id: &str,
    plan_ref: Option<&str>,
    binding: Option<DepartmentPlanBinding>,
) -> DepartmentPacket {
    DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_owned(),
        packet_id: "department-packet-implementation".to_owned(),
        project_id: project_id.to_owned(),
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
        charter_baseline_version: 0,
        plan_ref: plan_ref.map(str::to_owned),
        plan_binding: binding,
        runtime_grant: None,
        budget_lease: None,
    }
}

fn plan_binding(graph: &FrozenPlanGraph) -> DepartmentPlanBinding {
    DepartmentPlanBinding {
        plan_version: graph.plan_version,
        plan_digest: graph.plan_digest.clone(),
        charter_baseline_version: graph.charter_baseline_version,
    }
}

fn intake_stage(project_id: &str) -> DepartmentStageFacts {
    let mut stage = DepartmentStageFacts::new(project_id);
    stage.intake_authorized = true;
    stage
}

fn appointment(
    role_id: &str,
    project_id: &str,
    expires_at_unix_ms: u64,
    revoked: bool,
) -> DepartmentRoleAppointment {
    DepartmentRoleAppointment::new(
        format!("assignment:{role_id}"),
        project_id.to_owned(),
        role_id.to_owned(),
        "local-user".to_owned(),
        expires_at_unix_ms,
        revoked,
        1,
    )
    .expect("registered appointment role")
}

fn analyst_appointment(project_id: &str) -> DepartmentRoleAppointment {
    appointment("analyst", project_id, u64::MAX - 1, false)
}

fn builder_appointment(project_id: &str) -> DepartmentRoleAppointment {
    appointment("builder", project_id, u64::MAX - 1, false)
}

#[test]
fn department_packet_cannot_smuggle_builder_write_scope_or_runtime_grants() {
    let mut analysis = analysis_packet();
    analysis.write_scope = vec!["src/lib.rs".to_owned()];
    assert_eq!(
        analysis.validate().unwrap_err(),
        "department_packet_write_scope_denied"
    );

    let mut forged = analysis_packet();
    forged.runtime_grant = Some("grant:forged".to_owned());
    assert_eq!(
        forged.validate().unwrap_err(),
        "department_packet_runtime_authority_forbidden"
    );

    let mut leased = analysis_packet();
    leased.budget_lease = Some("lease:forged".to_owned());
    assert_eq!(
        leased.validate().unwrap_err(),
        "department_packet_runtime_authority_forbidden"
    );

    // The implementation kind is the only one allowed to name workspace paths, and even it must
    // carry its complete plan binding before it can be admitted.
    let implementation = implementation_packet("project-1", None, None);
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
fn department_packet_contract_refuses_role_department_and_input_basis_drift() {
    let mut wrong_role = analysis_packet();
    wrong_role.target_role = "builder".to_owned();
    assert_eq!(
        wrong_role.validate().unwrap_err(),
        "department_packet_role_department_mismatch"
    );

    let mut wrong_department = analysis_packet();
    wrong_department.from_department = "closing".to_owned();
    assert_eq!(
        wrong_department.validate().unwrap_err(),
        "department_packet_role_department_mismatch"
    );

    let mut wrong_output = analysis_packet();
    wrong_output.result.output_schema = "kiana.verification-result.v1".to_owned();
    assert_eq!(
        wrong_output.validate().unwrap_err(),
        "department_packet_output_schema_mismatch"
    );

    let mut wrong_basis = analysis_packet();
    wrong_basis.input_basis = PacketInputBasis::Initiative;
    assert_eq!(
        wrong_basis.validate().unwrap_err(),
        "department_packet_input_basis_invalid"
    );

    let mut premature_plan = analysis_packet();
    premature_plan.plan_ref = Some("plan:p1".to_owned());
    assert_eq!(
        premature_plan.validate().unwrap_err(),
        "department_packet_plan_reference_mismatch"
    );

    let mut missing_scope = implementation_packet(
        "project-1",
        Some("p1"),
        Some(DepartmentPlanBinding {
            plan_version: 1,
            plan_digest: coverage_digest(),
            charter_baseline_version: 1,
        }),
    );
    missing_scope.write_scope.clear();
    assert_eq!(
        missing_scope.validate().unwrap_err(),
        "department_packet_write_scope_required"
    );

    let mut duplicate_scope = implementation_packet(
        "project-1",
        Some("p1"),
        Some(DepartmentPlanBinding {
            plan_version: 1,
            plan_digest: coverage_digest(),
            charter_baseline_version: 1,
        }),
    );
    duplicate_scope.write_scope = vec!["src/lib.rs".to_owned(), "src/lib.rs".to_owned()];
    assert_eq!(
        duplicate_scope.validate().unwrap_err(),
        "department_packet_write_scope_duplicate"
    );

    let mut missing_inputs = analysis_packet();
    missing_inputs.input_refs.clear();
    assert_eq!(
        missing_inputs.validate().unwrap_err(),
        "department_packet_input_refs_required"
    );

    let missing_charter_baseline = plan_draft_packet(0);
    assert_eq!(
        missing_charter_baseline.validate().unwrap_err(),
        "department_packet_charter_baseline_required"
    );

    // A Charter-basis draft has to name the approved Charter version it plans against, and the
    // Intake/Initiative bases may not bind an approved Charter or Plan reference at all.
    let mut premature_charter_basis = analysis_packet();
    premature_charter_basis.charter_baseline_version = 1;
    assert_eq!(
        premature_charter_basis.validate().unwrap_err(),
        "department_packet_plan_reference_mismatch"
    );

    let mut duplicated_baseline = implementation_packet(
        "project-1",
        Some("p1"),
        Some(DepartmentPlanBinding {
            plan_version: 1,
            plan_digest: coverage_digest(),
            charter_baseline_version: 1,
        }),
    );
    duplicated_baseline.charter_baseline_version = 1;
    assert_eq!(
        duplicated_baseline.validate().unwrap_err(),
        "department_packet_plan_reference_mismatch"
    );
}

#[test]
fn department_packet_admission_refuses_unapproved_stage_and_inactive_appointment() {
    let now = 1_000_u64;

    let mut unrecorded = intake_stage("project-1");
    unrecorded.intake_authorized = false;
    assert_eq!(
        analysis_packet()
            .admit(&unrecorded, &analyst_appointment("project-1"), now)
            .unwrap_err(),
        "department_packet_stage_not_admitted"
    );

    let mut other_project = intake_stage("project-1");
    other_project.project_id = "project-2".to_owned();
    assert_eq!(
        analysis_packet()
            .admit(&other_project, &analyst_appointment("project-1"), now)
            .unwrap_err(),
        "department_packet_stage_project_mismatch"
    );

    assert_eq!(
        analysis_packet()
            .admit(
                &intake_stage("project-1"),
                &builder_appointment("project-1"),
                now
            )
            .unwrap_err(),
        "department_packet_appointment_mismatch"
    );

    assert_eq!(
        analysis_packet()
            .admit(
                &intake_stage("project-1"),
                &appointment("analyst", "project-1", now + 1, true),
                now
            )
            .unwrap_err(),
        "department_packet_appointment_inactive"
    );

    assert_eq!(
        analysis_packet()
            .admit(
                &intake_stage("project-1"),
                &appointment("analyst", "project-1", now, false),
                now
            )
            .unwrap_err(),
        "department_packet_appointment_inactive"
    );

    assert_eq!(
        plan_draft_packet(1)
            .admit(
                &intake_stage("project-1"),
                &appointment("pm", "project-1", u64::MAX - 1, false),
                now
            )
            .unwrap_err(),
        "department_packet_charter_not_approved"
    );

    let mut approved_other_baseline = intake_stage("project-1");
    approved_other_baseline.charter_baseline_version = 2;
    assert_eq!(
        plan_draft_packet(1)
            .admit(
                &approved_other_baseline,
                &appointment("pm", "project-1", u64::MAX - 1, false),
                now
            )
            .unwrap_err(),
        "department_packet_charter_baseline_mismatch"
    );

    // An Initiative-basis packet needs an Initiative that is still inside the assessment lane.
    let mut initiative_packet = analysis_packet();
    initiative_packet.packet_id = "department-packet-initiative".to_owned();
    initiative_packet.kind = DepartmentPacketKind::InitiativeAssessment;
    initiative_packet.input_basis = PacketInputBasis::Initiative;
    initiative_packet.result.output_schema = "kiana.assessment-result.v1".to_owned();
    assert_eq!(
        initiative_packet
            .admit(
                &intake_stage("project-1"),
                &analyst_appointment("project-1"),
                now
            )
            .unwrap_err(),
        "department_packet_stage_not_admitted"
    );
}

#[test]
fn department_packet_admission_refuses_drifted_plan_binding() {
    let now = 1_000_u64;
    let project_id = "project-1";

    let mut no_plan = intake_stage(project_id);
    no_plan.charter_baseline_version = 2;
    let binding = DepartmentPlanBinding {
        plan_version: 1,
        plan_digest: coverage_digest(),
        charter_baseline_version: 2,
    };
    assert_eq!(
        implementation_packet(project_id, Some("p1"), Some(binding.clone()))
            .admit(&no_plan, &builder_appointment(project_id), now)
            .unwrap_err(),
        "department_packet_plan_not_approved"
    );

    let frozen = frozen_plan(project_id, 2, 2);

    let mut stage = intake_stage(project_id);
    stage.charter_baseline_version = 2;
    stage.frozen_plan = Some(frozen.clone());

    let mut drifted_version = binding.clone();
    drifted_version.plan_version = 3;
    assert_eq!(
        implementation_packet(project_id, Some("p1"), Some(drifted_version))
            .admit(&stage, &builder_appointment(project_id), now)
            .unwrap_err(),
        "department_packet_plan_version_mismatch"
    );

    let mut drifted_baseline = binding.clone();
    drifted_baseline.charter_baseline_version = 3;
    assert_eq!(
        implementation_packet(project_id, Some("p1"), Some(drifted_baseline))
            .admit(&stage, &builder_appointment(project_id), now)
            .unwrap_err(),
        "department_packet_plan_baseline_mismatch"
    );

    let mut drifted_digest = binding.clone();
    drifted_digest.plan_digest = coverage_digest();
    assert_eq!(
        implementation_packet(project_id, Some("p1"), Some(drifted_digest))
            .admit(&stage, &builder_appointment(project_id), now)
            .unwrap_err(),
        "department_packet_plan_mismatch"
    );

    assert_eq!(
        implementation_packet(project_id, Some("p-unknown"), Some(binding.clone()))
            .admit(&stage, &builder_appointment(project_id), now)
            .unwrap_err(),
        "department_packet_plan_node_unknown"
    );
}

#[test]
fn admitted_department_packet_returns_digest_bound_evidence_without_runtime_authority() {
    let packet = analysis_packet();
    let admission = packet
        .admit(
            &intake_stage("project-1"),
            &analyst_appointment("project-1"),
            1_000,
        )
        .expect("admitted analysis packet");
    admission.validate().expect("admission evidence");
    assert!(!admission.runtime_authority);
    assert_eq!(admission.packet_digest, packet.digest());
    assert_eq!(admission.assignment_version, 1);
    assert_eq!(admission.from_department, "initiating");
    assert_eq!(admission.target_role, "analyst");
    assert_eq!(admission.input_basis, PacketInputBasis::Intake);
    assert_eq!(admission.charter_baseline_version, 0);
    assert_eq!(admission.plan_digest, None);
    assert_eq!(admission.plan_node_ref, None);

    // Re-admitting the same contract yields the same evidence digest: the admission is a pure
    // function of the packet, the stage facts and the appointment.
    let replay = packet
        .admit(
            &intake_stage("project-1"),
            &analyst_appointment("project-1"),
            2_000,
        )
        .expect("idempotent admission");
    assert_eq!(replay.admission_digest, admission.admission_digest);

    // A forged runtime authority flag cannot be validated back into a legal admission.
    let mut forged = admission.clone();
    forged.runtime_authority = true;
    assert_eq!(
        forged.validate().unwrap_err(),
        "department_packet_admission_invalid"
    );

    // The evidence has to stay consistent with its own input basis even when the self-digest is
    // recomputed: an Intake admission may not record an approved Charter baseline, and a
    // Plan-basis admission must record its plan node and revision.
    let mut forged_baseline = admission.clone();
    forged_baseline.charter_baseline_version = 7;
    forged_baseline.admission_digest = forged_baseline.canonical_digest();
    assert_eq!(
        forged_baseline.validate().unwrap_err(),
        "department_packet_admission_invalid"
    );

    let mut forged_plan = admission;
    forged_plan.input_basis = PacketInputBasis::Plan;
    forged_plan.admission_digest = forged_plan.canonical_digest();
    assert_eq!(
        forged_plan.validate().unwrap_err(),
        "department_packet_admission_invalid"
    );
}

#[test]
fn analysis_and_implementation_packets_validate_through_versioned_contracts() {
    // Intake investigation is admitted on the intake authorization alone: no Charter and no Plan
    // exists yet, which is what removes the "a plan is required before a plan can be drafted"
    // cycle. The same contract expresses the Charter draft that follows it.
    let analysis = analysis_packet();
    analysis.validate().expect("analysis packet");
    let analysis_admission = analysis
        .admit(
            &intake_stage("project-1"),
            &analyst_appointment("project-1"),
            1_000,
        )
        .expect("analysis admission");
    assert!(!analysis_admission.runtime_authority);
    assert_eq!(analysis_admission.plan_digest, None);

    let mut charter_draft = analysis_packet();
    charter_draft.packet_id = "department-packet-charter".to_owned();
    charter_draft.kind = DepartmentPacketKind::CharterDraft;
    charter_draft.target_role = "sponsor".to_owned();
    charter_draft.assignment_ref = "assignment:sponsor".to_owned();
    charter_draft.result.output_schema = "kiana.charter-draft-result.v1".to_owned();
    charter_draft.write_scope = vec!["charter/".to_owned()];
    charter_draft.validate().expect("charter draft packet");
    let charter_admission = charter_draft
        .admit(
            &intake_stage("project-1"),
            &appointment("sponsor", "project-1", u64::MAX - 1, false),
            1_000,
        )
        .expect("charter draft admission");
    assert!(!charter_admission.runtime_authority);
    assert_eq!(charter_admission.charter_baseline_version, 0);

    // The Plan draft runs against the approved Charter baseline, still before any frozen Plan.
    let mut charter_stage = intake_stage("project-1");
    charter_stage.charter_baseline_version = 2;
    let plan_draft = plan_draft_packet(2);
    plan_draft.validate().expect("plan draft packet");
    let plan_admission = plan_draft
        .admit(
            &charter_stage,
            &appointment("pm", "project-1", u64::MAX - 1, false),
            1_000,
        )
        .expect("plan draft admission");
    assert!(!plan_admission.runtime_authority);
    assert_eq!(plan_admission.charter_baseline_version, 2);
    assert_eq!(plan_admission.plan_digest, None);

    // Every Plan-basis kind validates through the same versioned contract against the exact
    // frozen Plan revision and its appointed target role.
    let frozen = frozen_plan("project-1", 2, 2);
    let mut stage = intake_stage("project-1");
    stage.charter_baseline_version = 2;
    stage.frozen_plan = Some(frozen.clone());

    let implementation =
        implementation_packet("project-1", Some("p1"), Some(plan_binding(&frozen)));
    implementation.validate().expect("implementation packet");
    let admission = implementation
        .admit(&stage, &builder_appointment("project-1"), 1_000)
        .expect("implementation admission");
    assert!(!admission.runtime_authority);
    assert_eq!(
        admission.plan_digest.as_deref(),
        Some(frozen.plan_digest.as_str())
    );
    assert_eq!(admission.plan_node_ref.as_deref(), Some("p1"));
    assert_eq!(admission.charter_baseline_version, 2);

    for (kind, role, output_schema, write_scope) in [
        (
            DepartmentPacketKind::Verification,
            "qa",
            "kiana.verification-result.v1",
            "gate/report.json",
        ),
        (
            DepartmentPacketKind::DeliveryPreparation,
            "closer",
            "kiana.delivery-preparation-result.v1",
            "receipt/delivery.json",
        ),
        (
            DepartmentPacketKind::KnowledgeCapture,
            "librarian",
            "kiana.knowledge-capture-result.v1",
            "lessons/co13.md",
        ),
    ] {
        let mut packet =
            implementation_packet("project-1", Some("p1"), Some(plan_binding(&frozen)));
        packet.packet_id = format!("department-packet-{}", kind.role_id());
        packet.kind = kind;
        packet.from_department = kind.department_id().to_owned();
        packet.target_role = kind.role_id().to_owned();
        packet.assignment_ref = format!("assignment:{}", kind.role_id());
        packet.result.output_schema = output_schema.to_owned();
        packet.write_scope = vec![write_scope.to_owned()];
        packet.validate().expect("plan-basis department packet");
        let admission = packet
            .admit(
                &stage,
                &appointment(kind.role_id(), "project-1", u64::MAX - 1, false),
                1_000,
            )
            .unwrap_or_else(|reason| panic!("{} admission: {reason}", kind.role_id()));
        assert!(!admission.runtime_authority);
        assert_eq!(admission.plan_node_ref.as_deref(), Some("p1"));
        assert_eq!(
            admission.plan_digest.as_deref(),
            Some(frozen.plan_digest.as_str())
        );
    }

    let mut missing_binding = implementation.clone();
    missing_binding.plan_ref = None;
    assert_eq!(
        missing_binding.validate().unwrap_err(),
        "department_packet_plan_required"
    );

    // Before the Plan is frozen the implementation contract is still refused: the analysis lane
    // above does not widen the executing lane.
    let mut planless_stage = intake_stage("project-1");
    planless_stage.charter_baseline_version = 2;
    assert_eq!(
        implementation
            .clone()
            .admit(&planless_stage, &builder_appointment("project-1"), 1_000)
            .unwrap_err(),
        "department_packet_plan_not_approved"
    );

    let mut unused_binding = implementation;
    unused_binding.plan_binding = None;
    assert_eq!(
        unused_binding.validate().unwrap_err(),
        "department_packet_plan_required"
    );
}
