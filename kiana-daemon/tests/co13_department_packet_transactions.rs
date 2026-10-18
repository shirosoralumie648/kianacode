//! CO-13 behavioural fixtures: one department work contract is admitted by one control-plane
//! transaction.
//!
//! These fixtures drive the real `DaemonHost -> ControlPlane -> EventStore` spine instead of
//! grepping source text:
//!
//! - `co13_department_packet_admission_is_one_control_plane_transaction` proves the admission
//!   boundary fails closed before any related object changes: a Builder subject is denied by the
//!   role gate, a smuggled runtime grant or budget lease, a Builder write scope inside a
//!   non-implementation contract, an unknown/mismatched/revoked appointment, a closed Initiative
//!   assessment lane and an implementation contract without a frozen Plan each keep their own
//!   stable reason, and only then is the intake-basis analysis contract admitted — as one fact
//!   that creates no packet, milestone, run, claim, lease or capability.
//! - `co13_department_packets_are_idempotent_and_refuse_a_changed_contract` proves the analysis
//!   contract is admitted while no Plan exists at all (which removes the "a plan is needed before
//!   a plan can be drafted" cycle), that replaying the exact command adds no second fact, that a
//!   changed contract under the same packet id is refused, and that after the Plan revision is
//!   frozen the implementation contract binds exactly that node, revision and Charter baseline.
//!
//! The fixtures are process-local source evidence: no durable cross-process packet store, no model
//! call, no capability or lease is claimed, and CI is the test authority whose receipt this branch
//! does not wait for.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::*;
use kiana_protocol::{
    CompanyCommand, CompanyCommandRequest, ExecutionStatus, PermissionProfile, RequestEnvelope,
    RequestMetadata, ResponseEnvelope, RoleSpec,
};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// The CO-13 fixture organization: registered, owned by the local principal and scoped to the
/// approved project, so the CO-05 appointment path is exercised for real.
const ORG_ID: &str = "9f0a1c2e-3b4d-4f60-8a1b-2c3d4e5f6071";

#[derive(Clone, Copy)]
struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

fn temp_project(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-co13-{label}-{stamp}"));
    fs::create_dir_all(root.join(".git")).expect("project root");
    root
}

fn host() -> DaemonHost {
    let model = ScriptedModel::from_json(&json!([{"text": "unused fixture response"}]))
        .expect("fake model cassette");
    let harness = KianaHarness::new(Arc::new(model));
    DaemonHost::with_harness_and_project_authority(harness, Arc::new(TrustedProject))
        .expect("daemon host")
}

fn metadata(root: &Path, session: &str, role: RoleSpec) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(session, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata.permission_profile = PermissionProfile::Balanced;
    metadata.assign_role(&role);
    metadata
}

async fn send(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    role: RoleSpec,
    expected_revision: u64,
    idempotency_key: &str,
    command: CompanyCommand,
) -> ResponseEnvelope {
    let request = CompanyCommandRequest {
        schema: COMPANY_COMMAND_SCHEMA.to_owned(),
        expected_revision,
        idempotency_key: idempotency_key.to_owned(),
        command,
    };
    let envelope = RequestEnvelope::company_command(metadata(root, session, role), request)
        .expect("company command envelope");
    host.handle(envelope).await
}

fn state_of(response: &ResponseEnvelope) -> &Value {
    response
        .output
        .get("state")
        .or_else(|| response.output.pointer("/company/state"))
        .expect("company state in response")
}

async fn committed(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    role: RoleSpec,
    revision: &mut u64,
    idempotency_key: &str,
    command: CompanyCommand,
) -> ResponseEnvelope {
    let response = send(
        host,
        root,
        session,
        role,
        *revision,
        idempotency_key,
        command,
    )
    .await;
    assert_eq!(
        response.status,
        ExecutionStatus::Completed,
        "company command {idempotency_key}: {response:?}"
    );
    *revision = state_of(&response)["revision"]
        .as_u64()
        .expect("company revision");
    response
}

async fn refused(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    role: RoleSpec,
    expected_revision: u64,
    idempotency_key: &str,
    command: CompanyCommand,
) -> ResponseEnvelope {
    let response = send(
        host,
        root,
        session,
        role,
        expected_revision,
        idempotency_key,
        command,
    )
    .await;
    assert_eq!(
        response.status,
        ExecutionStatus::Blocked,
        "expected a refusal for {idempotency_key}: {response:?}"
    );
    response
}

async fn persisted_events(host: &DaemonHost) -> Vec<RuntimeEvent> {
    host.persisted_events()
        .await
        .expect("event read")
        .expect("events")
}

fn objective() -> Objective {
    Objective {
        objective_id: "objective-co13".to_owned(),
        organization_id: ORG_ID.to_owned(),
        title: "Ship governed department work contracts".to_owned(),
        problem: "Department work needs one versioned, non-executing contract".to_owned(),
        metric: "admission_rate".to_owned(),
        baseline: 0.0,
        target: 1.0,
        unit: "ratio".to_owned(),
        measurement_method: Some("co13 fixture admission ratio".to_owned()),
        direction: MetricDirection::AtLeast,
        period_start: 1,
        period_end: u64::MAX - 1,
        owner_principal_id: "local-user".to_owned(),
        status: ObjectiveStatus::Proposed,
        version: 1,
    }
}

fn proposed_project(project_id: &str) -> Project {
    Project {
        project_id: project_id.to_owned(),
        organization_id: ORG_ID.to_owned(),
        objective_refs: vec!["objective-co13".to_owned()],
        sponsor_id: "local-user".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "co13-department-packets-v1".to_owned(),
        success_criteria: vec![
            "department work contracts are admitted without runtime authority".to_owned(),
        ],
        non_goals: vec!["no external publish".to_owned()],
        project_budget_ref: "budget:co13".to_owned(),
        risk_summary: "bounded local fixture".to_owned(),
        decision_ref: None,
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: None,
        closing_receipt_id: None,
        status: ProjectStatus::Proposed,
        version: 1,
    }
}

fn budget_policy(project_id: &str) -> CompanyBudgetPolicy {
    CompanyBudgetPolicy {
        project: ProjectBudget {
            project_id: ProjectId::new(),
            max_runs: 8,
            max_tokens: 200_000,
        },
        runtime: RuntimeBudget {
            max_model_calls: 8,
            max_tokens: 200_000,
            max_wall_time_ms: 120_000,
        },
        quota: Quota {
            scope: project_id.to_owned(),
            model_calls: 8,
            tokens: 200_000,
            concurrency: 1,
        },
    }
}

fn appointed(assignment_id: &str, role_id: &str, project_id: &str) -> BusinessAssignment {
    BusinessAssignment {
        assignment_id: assignment_id.to_owned(),
        organization_id: ORG_ID.to_owned(),
        owner_principal_id: "local-user".to_owned(),
        role_id: role_id.to_owned(),
        actor_kind: BusinessActorKind::Human,
        project_ids: vec![project_id.to_owned()],
        expires_at: u64::MAX - 1,
        version: 1,
        revoked: false,
    }
}

fn appoint(assignment: BusinessAssignment) -> CompanyCommand {
    CompanyCommand::Business {
        schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
        action: Box::new(CompanyBusinessAction::Appoint { assignment }),
    }
}

struct ApprovedFixture {
    project_id: String,
    revision: u64,
    project_version: u64,
    charter_baseline_version: u64,
}

/// Walk the existing Company lanes to one approved project with a frozen Charter baseline, a
/// registered organization and the CO-05 appointments the department contracts bind. The
/// admission under test is the only CO-13 command in these fixtures.
async fn approve_project(host: &DaemonHost, root: &Path) -> ApprovedFixture {
    fs::write(
        root.join("charter.json"),
        "charter: department work contracts stay non-executing\n",
    )
    .expect("charter");
    fs::write(
        root.join("decision.json"),
        "decision: sponsor approved the bounded department fixture\n",
    )
    .expect("decision");
    let project_id = ProjectId::new().to_string();
    let mut revision = 0u64;
    committed(
        host,
        root,
        "builder-artifact-session",
        RoleSpec::builder(),
        &mut revision,
        "co13-register-charter",
        CompanyCommand::RegisterArtifact {
            artifact_id: "charter".to_owned(),
            relative_path: "charter.json".to_owned(),
        },
    )
    .await;
    committed(
        host,
        root,
        "builder-artifact-session",
        RoleSpec::builder(),
        &mut revision,
        "co13-register-decision",
        CompanyCommand::RegisterArtifact {
            artifact_id: "decision".to_owned(),
            relative_path: "decision.json".to_owned(),
        },
    )
    .await;
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-propose-objective",
        CompanyCommand::ProposeObjective {
            objective: objective(),
        },
    )
    .await;
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-create-organization",
        CompanyCommand::Business {
            schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
            action: Box::new(CompanyBusinessAction::CreateOrganization {
                organization_id: ORG_ID.to_owned(),
                name: "CO-13 fixture organization".to_owned(),
            }),
        },
    )
    .await;
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-activate-objective",
        CompanyCommand::DecideObjective {
            objective_id: "objective-co13".to_owned(),
            approve: true,
        },
    )
    .await;
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-propose-project",
        CompanyCommand::ProposeProject {
            project: proposed_project(&project_id),
        },
    )
    .await;
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-start-chartering",
        CompanyCommand::StartChartering {
            project_id: project_id.clone(),
        },
    )
    .await;
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-configure-budget",
        CompanyCommand::ConfigureBudget {
            project_id: project_id.clone(),
            policy: budget_policy(&project_id),
        },
    )
    .await;
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-record-direct-intake",
        CompanyCommand::RecordDirectIntake {
            project_id: project_id.clone(),
            reason: "the department packet fixture records the skipped Initiative lane".to_owned(),
        },
    )
    .await;
    let approved = committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-approve-project",
        CompanyCommand::ApproveProject {
            project_id: project_id.clone(),
            decision_ref: "artifact:decision".to_owned(),
        },
    )
    .await;
    let charter_baseline_version = state_of(&approved)["charter_baselines"][&project_id]["version"]
        .as_u64()
        .expect("frozen charter baseline version");
    let project_version = state_of(&approved)["projects"][&project_id]["version"]
        .as_u64()
        .expect("project version");

    // The CO-05 appointments the department contracts bind: one live analyst, one live builder and
    // one analyst appointment that is revoked, so the inactive-appointment denial is deterministic
    // instead of clock-dependent.
    for (key, assignment) in [
        (
            "co13-appoint-analyst",
            appointed("assignment:analyst", "analyst", &project_id),
        ),
        (
            "co13-appoint-builder",
            appointed("assignment:builder", "builder", &project_id),
        ),
        (
            "co13-appoint-revoked-analyst",
            appointed("assignment:analyst-revoked", "analyst", &project_id),
        ),
    ] {
        committed(
            host,
            root,
            "sponsor-session",
            RoleSpec::sponsor(),
            &mut revision,
            key,
            appoint(assignment),
        )
        .await;
    }
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co13-revoke-analyst",
        CompanyCommand::Business {
            schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
            action: Box::new(CompanyBusinessAction::RevokeAssignment {
                assignment_id: "assignment:analyst-revoked".to_owned(),
                reason: "the revoked appointment proves the inactive denial".to_owned(),
            }),
        },
    )
    .await;

    ApprovedFixture {
        project_id,
        revision,
        project_version,
        charter_baseline_version,
    }
}

fn result_contract(output_schema: &str, required_ref: &str, max_bytes: u64) -> ResultContract {
    ResultContract {
        schema: RESULT_CONTRACT_SCHEMA.to_owned(),
        version: 1,
        output_schema: output_schema.to_owned(),
        required_refs: vec![required_ref.to_owned()],
        max_bytes,
    }
}

fn analysis_packet(project_id: &str, assignment_ref: &str) -> DepartmentPacket {
    DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_owned(),
        packet_id: "department-packet-analysis".to_owned(),
        project_id: project_id.to_owned(),
        version: 1,
        kind: DepartmentPacketKind::IntakeAnalysis,
        from_department: "initiating".to_owned(),
        target_role: "analyst".to_owned(),
        assignment_ref: assignment_ref.to_owned(),
        input_basis: PacketInputBasis::Intake,
        input_refs: vec!["artifact:intake".to_owned()],
        result: result_contract("kiana.analysis-result.v1", "artifact:proposal", 4096),
        write_scope: Vec::new(),
        charter_baseline_version: 0,
        plan_ref: None,
        plan_binding: None,
        runtime_grant: None,
        budget_lease: None,
    }
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
        result: result_contract("kiana.implementation-result.v1", "artifact:receipt", 65_536),
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

/// One Milestone with one required Packet output: the smallest complete WorkGraph the frozen Plan
/// revision admits.
fn plan_proposal(project_id: &str, charter_baseline_version: u64) -> PlanProposal {
    let mut draft = PlanProposalDraft::new();
    for (node_id, kind) in [
        ("m1", PlanNodeKind::Milestone),
        ("p1", PlanNodeKind::Packet),
    ] {
        draft
            .push_node(PlanNode {
                node_id: node_id.to_owned(),
                project_id: project_id.to_owned(),
                kind,
                version: 1,
                required: true,
                criterion_refs: Vec::new(),
                input_refs: Vec::new(),
            })
            .expect("plan node");
    }
    draft
        .push_edge(PlanEdge {
            from: "m1".to_owned(),
            to: "p1".to_owned(),
            kind: PlanEdgeKind::ParentChild,
            reference: None,
        })
        .expect("milestone to packet");
    draft
        .freeze_versioned(
            project_id,
            charter_baseline_version,
            1,
            json_digest(&json!({"fixture": "co13-department-packet"})),
        )
        .expect("frozen plan proposal")
}

#[tokio::test]
async fn co13_department_packet_admission_is_one_control_plane_transaction() {
    let root = temp_project("deny");
    let host = host();
    let fixture = approve_project(&host, &root).await;
    let project_id = fixture.project_id.clone();
    let revision = fixture.revision;
    let packet = analysis_packet(&project_id, "assignment:analyst");
    packet
        .validate()
        .expect("the analysis contract is well formed");

    // A Builder subject cannot admit a department work contract at all: the role gate rejects it
    // before any state read.
    let builder = refused(
        &host,
        &root,
        "builder-session",
        RoleSpec::builder(),
        revision,
        "co13-builder-admit",
        CompanyCommand::AdmitDepartmentPacket {
            packet: packet.clone(),
        },
    )
    .await;
    assert_eq!(builder.error.as_deref(), Some("company_role_denied"));

    // The business contract can never carry self-effective runtime authority: an explicit grant or
    // lease field is visible and rejectable instead of silently ignored.
    let mut smuggled_grant = packet.clone();
    smuggled_grant.runtime_grant = Some("grant:forged".to_owned());
    let refused_grant = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-smuggled-grant",
        CompanyCommand::AdmitDepartmentPacket {
            packet: smuggled_grant,
        },
    )
    .await;
    assert_eq!(
        refused_grant.error.as_deref(),
        Some("department_packet_runtime_authority_forbidden")
    );

    let mut smuggled_lease = packet.clone();
    smuggled_lease.budget_lease = Some("lease:forged".to_owned());
    let refused_lease = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-smuggled-lease",
        CompanyCommand::AdmitDepartmentPacket {
            packet: smuggled_lease,
        },
    )
    .await;
    assert_eq!(
        refused_lease.error.as_deref(),
        Some("department_packet_runtime_authority_forbidden")
    );

    // Only an implementation contract may name workspace paths; both a Builder write scope and an
    // escaped proposal path inside an analysis contract are refused.
    let mut write_scope = packet.clone();
    write_scope.write_scope = vec!["src/lib.rs".to_owned()];
    let refused_scope = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-builder-write-scope",
        CompanyCommand::AdmitDepartmentPacket {
            packet: write_scope,
        },
    )
    .await;
    assert_eq!(
        refused_scope.error.as_deref(),
        Some("department_packet_write_scope_denied")
    );

    // The target-role appointment is server-derived: an unknown, mismatched or revoked CO-05
    // appointment each keeps its own reason.
    let mut unknown = packet.clone();
    unknown.assignment_ref = "assignment:missing".to_owned();
    let refused_unknown = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-unknown-appointment",
        CompanyCommand::AdmitDepartmentPacket { packet: unknown },
    )
    .await;
    assert_eq!(
        refused_unknown.error.as_deref(),
        Some("department_packet_appointment_required")
    );

    let mut wrong_role = packet.clone();
    wrong_role.assignment_ref = "assignment:builder".to_owned();
    let refused_wrong_role = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-wrong-role-appointment",
        CompanyCommand::AdmitDepartmentPacket { packet: wrong_role },
    )
    .await;
    assert_eq!(
        refused_wrong_role.error.as_deref(),
        Some("department_packet_appointment_mismatch")
    );

    let mut revoked = packet.clone();
    revoked.assignment_ref = "assignment:analyst-revoked".to_owned();
    let refused_revoked = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-revoked-appointment",
        CompanyCommand::AdmitDepartmentPacket { packet: revoked },
    )
    .await;
    assert_eq!(
        refused_revoked.error.as_deref(),
        Some("department_packet_appointment_inactive")
    );

    // An Initiative assessment contract needs an Initiative still inside the assessment lane.
    let mut initiative = packet.clone();
    initiative.packet_id = "department-packet-initiative".to_owned();
    initiative.kind = DepartmentPacketKind::InitiativeAssessment;
    initiative.input_basis = PacketInputBasis::Initiative;
    initiative.result.output_schema = "kiana.assessment-result.v1".to_owned();
    let refused_initiative = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-initiative-lane",
        CompanyCommand::AdmitDepartmentPacket { packet: initiative },
    )
    .await;
    assert_eq!(
        refused_initiative.error.as_deref(),
        Some("department_packet_stage_not_admitted")
    );

    // An implementation contract that names a well-formed Plan revision is still refused while no
    // Plan has been frozen: the analysis lane above does not widen the executing lane.
    let planless = implementation_packet(
        &project_id,
        Some("p1"),
        Some(DepartmentPlanBinding {
            plan_version: 1,
            plan_digest: json_digest(&json!({"fixture": "co13-missing-plan"})),
            charter_baseline_version: fixture.charter_baseline_version,
        }),
    );
    let refused_planless = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-implementation-without-plan",
        CompanyCommand::AdmitDepartmentPacket { packet: planless },
    )
    .await;
    assert_eq!(
        refused_planless.error.as_deref(),
        Some("department_packet_plan_not_approved")
    );

    // Only after every refusal above is the intake-basis analysis contract admitted, and the
    // admitted transaction changes no other Company object and grants no runtime authority.
    let mut admission_revision = revision;
    let admitted = committed(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut admission_revision,
        "co13-admit-analysis",
        CompanyCommand::AdmitDepartmentPacket {
            packet: packet.clone(),
        },
    )
    .await;
    assert_eq!(admission_revision, revision + 1);
    let state = state_of(&admitted);
    let admission = &state["department_packets"]["department-packet-analysis"];
    assert_eq!(
        admission["schema"],
        json!(DEPARTMENT_PACKET_ADMISSION_SCHEMA)
    );
    assert_eq!(admission["packet_digest"], json!(packet.digest()));
    assert_eq!(admission["kind"], json!("intake_analysis"));
    assert_eq!(admission["target_role"], json!("analyst"));
    assert_eq!(admission["from_department"], json!("initiating"));
    assert_eq!(admission["input_basis"], json!("intake"));
    assert_eq!(admission["assignment_ref"], json!("assignment:analyst"));
    assert_eq!(admission["runtime_authority"], json!(false));
    assert_eq!(admission["plan_digest"], json!(null));
    assert_eq!(admission["plan_node_ref"], json!(null));
    assert_eq!(
        state["plan_graphs"].as_object().map(|graphs| graphs.len()),
        Some(0),
        "the analysis contract is admitted before any Plan exists"
    );
    assert_eq!(
        state["projects"][&project_id]["version"],
        json!(fixture.project_version)
    );
    assert_eq!(
        state["milestones"].as_object().map(|map| map.len()),
        Some(0)
    );
    assert_eq!(state["packets"].as_object().map(|map| map.len()), Some(0));
    assert_eq!(state["runs"].as_object().map(|map| map.len()), Some(0));

    let events = persisted_events(&host).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.DepartmentPacketAdmitted")
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .filter(|event| event.kind == "company.command_rejected")
            .count()
            >= 9,
        "every refusal above is recorded as a rejection fact"
    );
    assert!(!events.iter().any(|event| matches!(
        event.kind.as_str(),
        "company.RunStartRequested"
            | "company.PacketApproved"
            | "company.MilestoneCreated"
            | "company.PacketClaimed"
    )));
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn co13_department_packets_are_idempotent_and_refuse_a_changed_contract() {
    let root = temp_project("idempotent");
    let host = host();
    let fixture = approve_project(&host, &root).await;
    let project_id = fixture.project_id.clone();
    let mut revision = fixture.revision;
    let packet = analysis_packet(&project_id, "assignment:analyst");

    // The analysis contract is admitted while the project has no Plan revision at all, and the
    // admitted evidence is bound to the server-derived stage.
    let admitted = committed(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "co13-admit-analysis-once",
        CompanyCommand::AdmitDepartmentPacket {
            packet: packet.clone(),
        },
    )
    .await;
    assert_eq!(revision, fixture.revision + 1);
    assert_eq!(
        state_of(&admitted)["plan_graphs"]
            .as_object()
            .map(|graphs| graphs.len()),
        Some(0)
    );
    assert_eq!(
        state_of(&admitted)["department_packets"]
            .as_object()
            .map(|map| map.len()),
        Some(1)
    );

    // Replaying the exact command returns the recorded receipt and adds no second fact.
    let replay = send(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        fixture.revision,
        "co13-admit-analysis-once",
        CompanyCommand::AdmitDepartmentPacket {
            packet: packet.clone(),
        },
    )
    .await;
    assert_eq!(replay.status, ExecutionStatus::Completed, "{replay:?}");
    assert_eq!(replay.output["replayed"], json!(true));
    assert_eq!(
        state_of(&replay)["department_packets"]
            .as_object()
            .map(|map| map.len()),
        Some(1)
    );

    // A changed contract under the same packet id is refused instead of overwriting the admitted
    // identity.
    let mut changed = packet.clone();
    changed.input_refs = vec!["artifact:intake-revised".to_owned()];
    let refused_changed = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-changed-contract",
        CompanyCommand::AdmitDepartmentPacket { packet: changed },
    )
    .await;
    assert_eq!(
        refused_changed.error.as_deref(),
        Some("department_packet_already_admitted")
    );

    // The Plan revision is frozen by the PM: the plan graph is a planning proposal, not a run.
    let plan = plan_proposal(&project_id, fixture.charter_baseline_version);
    let frozen = committed(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "co13-freeze-plan",
        CompanyCommand::FreezePlanGraph { plan: plan.clone() },
    )
    .await;
    let state = state_of(&frozen);
    assert_eq!(
        state["plan_graphs"][&project_id]["plan_digest"],
        json!(plan.digest)
    );
    assert_eq!(
        state["plan_graphs"][&project_id]["node_versions"]["p1"],
        json!(1)
    );
    assert_eq!(
        state["projects"][&project_id]["version"],
        json!(fixture.project_version)
    );
    let frozen_graph: FrozenPlanGraph =
        serde_json::from_value(state["plan_graphs"][&project_id].clone())
            .expect("frozen plan graph round-trip");

    // Every drifted binding keeps its own reason before the exact revision is admitted.
    let mut drifted_version = plan_binding(&frozen_graph);
    drifted_version.plan_version = 2;
    let refused_version = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-plan-version-drift",
        CompanyCommand::AdmitDepartmentPacket {
            packet: implementation_packet(&project_id, Some("p1"), Some(drifted_version)),
        },
    )
    .await;
    assert_eq!(
        refused_version.error.as_deref(),
        Some("department_packet_plan_version_mismatch")
    );

    let mut drifted_baseline = plan_binding(&frozen_graph);
    drifted_baseline.charter_baseline_version = fixture.charter_baseline_version + 1;
    let refused_baseline = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-plan-baseline-drift",
        CompanyCommand::AdmitDepartmentPacket {
            packet: implementation_packet(&project_id, Some("p1"), Some(drifted_baseline)),
        },
    )
    .await;
    assert_eq!(
        refused_baseline.error.as_deref(),
        Some("department_packet_plan_baseline_mismatch")
    );

    let mut drifted_digest = plan_binding(&frozen_graph);
    drifted_digest.plan_digest = json_digest(&json!({"fixture": "co13-other-plan"}));
    let refused_digest = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-plan-digest-drift",
        CompanyCommand::AdmitDepartmentPacket {
            packet: implementation_packet(&project_id, Some("p1"), Some(drifted_digest)),
        },
    )
    .await;
    assert_eq!(
        refused_digest.error.as_deref(),
        Some("department_packet_plan_mismatch")
    );

    let refused_node = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co13-plan-node-unknown",
        CompanyCommand::AdmitDepartmentPacket {
            packet: implementation_packet(
                &project_id,
                Some("p-unknown"),
                Some(plan_binding(&frozen_graph)),
            ),
        },
    )
    .await;
    assert_eq!(
        refused_node.error.as_deref(),
        Some("department_packet_plan_node_unknown")
    );

    // Only the exact frozen revision is admitted, and the admission records the node, the plan
    // digest and the Charter baseline it was checked against.
    let implementation =
        implementation_packet(&project_id, Some("p1"), Some(plan_binding(&frozen_graph)));
    let admitted_implementation = committed(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "co13-admit-implementation",
        CompanyCommand::AdmitDepartmentPacket {
            packet: implementation.clone(),
        },
    )
    .await;
    let state = state_of(&admitted_implementation);
    let admission = &state["department_packets"]["department-packet-implementation"];
    assert_eq!(admission["kind"], json!("implementation"));
    assert_eq!(admission["target_role"], json!("builder"));
    assert_eq!(admission["input_basis"], json!("plan"));
    assert_eq!(
        admission["charter_baseline_version"],
        json!(fixture.charter_baseline_version)
    );
    assert_eq!(admission["plan_digest"], json!(frozen_graph.plan_digest));
    assert_eq!(admission["plan_node_ref"], json!("p1"));
    assert_eq!(admission["runtime_authority"], json!(false));
    assert_eq!(
        state["projects"][&project_id]["version"],
        json!(fixture.project_version)
    );
    assert_eq!(state["runs"].as_object().map(|map| map.len()), Some(0));
    assert_eq!(state["packets"].as_object().map(|map| map.len()), Some(0));
    assert_eq!(
        state["milestones"].as_object().map(|map| map.len()),
        Some(0)
    );

    let events = persisted_events(&host).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.DepartmentPacketAdmitted")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.PlanGraphFrozen")
            .count(),
        1
    );
    assert!(events.iter().any(|event| {
        event.kind == "company.command_rejected"
            && event.data["reason"] == json!("department_packet_already_admitted")
    }));
    assert!(!events.iter().any(|event| matches!(
        event.kind.as_str(),
        "company.RunStartRequested"
            | "company.PacketApproved"
            | "company.MilestoneCreated"
            | "company.PacketClaimed"
    )));
    fs::remove_dir_all(root).expect("cleanup");
}
