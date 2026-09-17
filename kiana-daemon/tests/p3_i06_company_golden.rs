use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::*;
use kiana_protocol::{
    CompanyCommand, CompanyCommandRequest, ExecutionStatus, PermissionProfile, RequestEnvelope,
    RequestMetadata, ResponseEnvelope, RoleSpec,
};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

fn temp_project() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-p3-i06-{stamp}"));
    fs::create_dir_all(root.join(".git")).expect("project root");
    root
}

fn metadata(root: &Path, session: &str, role: RoleSpec) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(session, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata.permission_profile = PermissionProfile::Balanced;
    metadata.assign_role(&role);
    metadata
}

async fn company_command(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    role: RoleSpec,
    revision: &mut u64,
    key: &str,
    command: CompanyCommand,
) -> ResponseEnvelope {
    let request = CompanyCommandRequest {
        schema: COMPANY_COMMAND_SCHEMA.to_owned(),
        expected_revision: *revision,
        idempotency_key: key.to_owned(),
        command,
    };
    let envelope = RequestEnvelope::company_command(metadata(root, session, role), request)
        .expect("company command envelope");
    let response = host.handle(envelope).await;
    assert_eq!(
        response.status,
        ExecutionStatus::Completed,
        "company command {key}: {response:?}"
    );
    let state = response
        .output
        .get("state")
        .or_else(|| response.output.pointer("/company/state"))
        .expect("company state in response");
    *revision = state["revision"].as_u64().expect("company revision");
    response
}

fn builder_outputs() -> serde_json::Value {
    json!([
        {
            "text": "implementing the frozen packet",
            "tool_calls": [{
                "id": "golden-write",
                "name": "apply_patch",
                "arguments": {
                    "patch": "*** Begin Patch\n*** Add File: OUTPUT.txt\n+company golden output\n*** End Patch\n"
                }
            }]
        },
        {"text": "OUTPUT.txt created from the approved packet"}
    ])
}

fn objective() -> Objective {
    Objective {
        objective_id: "objective-golden".to_owned(),
        organization_id: "org-golden".to_owned(),
        title: "Ship a governed coding output".to_owned(),
        problem: "The project needs a reviewable artifact".to_owned(),
        metric: "delivery_rate".to_owned(),
        baseline: 0.0,
        target: 1.0,
        unit: "ratio".to_owned(),
        direction: MetricDirection::AtLeast,
        period_start: 1,
        period_end: u64::MAX - 1,
        owner_principal_id: "local-user".to_owned(),
        status: ObjectiveStatus::Proposed,
        version: 1,
    }
}

#[tokio::test]
async fn fake_model_coding_project_produces_closing_receipt() {
    let root = temp_project();
    fs::write(
        root.join("charter.json"),
        "charter: bounded coding project\n",
    )
    .expect("charter");
    fs::write(root.join("decision.json"), "decision: sponsor approved\n").expect("decision");
    let harness = KianaHarness::new(Arc::new(
        ScriptedModel::from_json(&builder_outputs()).expect("fake model cassette"),
    ));
    let host = Arc::new(
        DaemonHost::with_harness_and_project_authority(harness, Arc::new(TrustedProject))
            .expect("daemon host"),
    );
    let project_id = "project-golden".to_owned();
    let milestone_id = "milestone-golden".to_owned();
    let packet_id = "packet-golden".to_owned();
    let mut revision = 0;

    company_command(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "register-charter",
        CompanyCommand::RegisterArtifact {
            artifact_id: "charter".to_owned(),
            relative_path: "charter.json".to_owned(),
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "register-decision",
        CompanyCommand::RegisterArtifact {
            artifact_id: "decision".to_owned(),
            relative_path: "decision.json".to_owned(),
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "propose-objective",
        CompanyCommand::ProposeObjective {
            objective: objective(),
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "activate-objective",
        CompanyCommand::DecideObjective {
            objective_id: "objective-golden".to_owned(),
            approve: true,
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "propose-project",
        CompanyCommand::ProposeProject {
            project: Project {
                project_id: project_id.clone(),
                organization_id: "org-golden".to_owned(),
                objective_refs: vec!["objective-golden".to_owned()],
                sponsor_id: "local-user".to_owned(),
                charter_ref: "artifact:charter".to_owned(),
                scope_baseline: "coding-v1".to_owned(),
                success_criteria: vec!["output exists".to_owned()],
                non_goals: vec!["no external publish".to_owned()],
                project_budget_ref: "budget-golden".to_owned(),
                risk_summary: "bounded local fixture".to_owned(),
                decision_ref: None,
                milestone_refs: Vec::new(),
                incident_id: None,
                acceptance_id: None,
                closing_receipt_id: None,
                status: ProjectStatus::Proposed,
                version: 1,
            },
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "start-chartering",
        CompanyCommand::StartChartering {
            project_id: project_id.clone(),
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "approve-project",
        CompanyCommand::ApproveProject {
            project_id: project_id.clone(),
            decision_ref: "artifact:decision".to_owned(),
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "create-milestone",
        CompanyCommand::CreateMilestone {
            milestone: Milestone {
                milestone_id: milestone_id.clone(),
                project_id: project_id.clone(),
                objective_refs: vec!["objective-golden".to_owned()],
                deliverables: vec!["OUTPUT.txt".to_owned()],
                acceptance_criteria: vec!["output exists".to_owned()],
                due_at: u64::MAX - 1,
                dependency_refs: Vec::new(),
                status: MilestoneStatus::Planned,
                version: 1,
            },
        },
    )
    .await;
    let packet = WorkPacket::builder_task(packet_id.clone(), "write OUTPUT.txt")
        .with_path_allow(["OUTPUT.txt"])
        .with_acceptance_tests(["output exists"]);
    company_command(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "approve-packet",
        CompanyCommand::ApprovePacket {
            project_id: project_id.clone(),
            milestone_id: milestone_id.clone(),
            packet,
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "plan-project",
        CompanyCommand::PlanProject {
            project_id: project_id.clone(),
        },
    )
    .await;

    let start = company_command(
        &host,
        &root,
        "builder-session",
        RoleSpec::builder(),
        &mut revision,
        "start-builder",
        CompanyCommand::StartRun {
            project_id: project_id.clone(),
            packet_id: packet_id.clone(),
            sandbox: Some("workspace-write".to_owned()),
        },
    )
    .await;
    assert_eq!(start.output["packet_status"], "succeeded");
    assert_eq!(
        fs::read_to_string(root.join("OUTPUT.txt")).unwrap(),
        "company golden output\n"
    );

    let events = host
        .persisted_events()
        .await
        .expect("event read")
        .expect("events");
    let run_evidence = events
        .iter()
        .find(|event| event.kind == "run.completed")
        .map(|event| format!("event:{}", event.event_id))
        .expect("completed run evidence");
    company_command(
        &host,
        &root,
        "builder-session",
        RoleSpec::builder(),
        &mut revision,
        "request-acceptance",
        CompanyCommand::RequestAcceptance {
            acceptance_id: "acceptance-golden".to_owned(),
            project_id: project_id.clone(),
            packet_id: packet_id.clone(),
            evidence_refs: vec![run_evidence.clone()],
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "reviewer-session",
        RoleSpec::reviewer(),
        &mut revision,
        "record-review",
        CompanyCommand::RecordReview {
            review_id: "review-golden".to_owned(),
            acceptance_id: "acceptance-golden".to_owned(),
            criterion_results: [("output exists".to_owned(), true)].into_iter().collect(),
            evidence_refs: vec![run_evidence.clone()],
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "reviewer-session",
        RoleSpec::reviewer(),
        &mut revision,
        "accept-review",
        CompanyCommand::DecideAcceptance {
            acceptance_id: "acceptance-golden".to_owned(),
            review_id: "review-golden".to_owned(),
            decision: AcceptanceDecision::Accept,
            reasons: Vec::new(),
            waiver_ref: None,
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut revision,
        "register-output",
        CompanyCommand::RegisterArtifact {
            artifact_id: "output".to_owned(),
            relative_path: "OUTPUT.txt".to_owned(),
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut revision,
        "prepare-delivery",
        CompanyCommand::PrepareDelivery {
            delivery: Delivery {
                delivery_id: "delivery-golden".to_owned(),
                project_id: project_id.clone(),
                acceptance_id: "acceptance-golden".to_owned(),
                artifact_refs: vec!["artifact:output".to_owned()],
                recipient_ref: "local-user".to_owned(),
                handoff_receipt_ref: None,
                delivered_at: None,
                incident_id: None,
                status: DeliveryStatus::Prepared,
                version: 1,
            },
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "sponsor-delivery-session",
        RoleSpec::sponsor(),
        &mut revision,
        "approve-delivery",
        CompanyCommand::ApproveDelivery {
            delivery_id: "delivery-golden".to_owned(),
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut revision,
        "deliver-output",
        CompanyCommand::Deliver {
            delivery_id: "delivery-golden".to_owned(),
            evidence_refs: vec!["artifact:output".to_owned()],
        },
    )
    .await;
    company_command(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut revision,
        "confirm-delivery",
        CompanyCommand::ConfirmDelivery {
            delivery_id: "delivery-golden".to_owned(),
            handoff_receipt_ref: run_evidence.clone(),
        },
    )
    .await;
    let closing = company_command(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut revision,
        "close-project",
        CompanyCommand::CloseProject {
            project_id: project_id.clone(),
            delivery_id: "delivery-golden".to_owned(),
            receipt_id: "closing-golden".to_owned(),
            evidence_refs: vec![run_evidence],
        },
    )
    .await;
    assert_eq!(
        closing.output["state"]["projects"][&project_id]["status"],
        "closed"
    );
    assert!(
        closing.output["state"]["closing_receipts"]["closing-golden"]["evidence_refs"]
            .as_array()
            .is_some_and(|refs| !refs.is_empty())
    );

    let _ = fs::remove_dir_all(root);
}
