//! CP-23 behavioural fixtures: Company mutations commit through one control-plane transaction.
//!
//! Unlike the source guard (`kiana-core/tests/cp23_company_transaction_guard.rs`), these fixtures
//! drive a real `DaemonHost -> ControlPlane -> EventStore` path with the scripted-model harness and
//! assert observable state instead of grepping source text:
//!
//! - `cp_builder_cannot_self_accept_or_promote_authority` proves a Builder subject is denied every
//!   protected mutation class (project approval, acceptance, budget, closeout, assignment) with
//!   `company_role_denied`, that no related object or Company revision changes, and that the same
//!   command path admits the sponsor subject.
//! - `cp_company_command_conflict_changes_no_related_object` proves a stale-revision conflict and an
//!   idempotency conflict leave the persisted Company aggregate and every related object untouched.
//! - `cp_close_cannot_hide_unresolved_effects` proves `CloseProject` is refused while the acceptance
//!   is missing, the delivery is absent, prepared, or delivered-but-unconfirmed, and only commits a
//!   single `company.ProjectClosed` fact once every effect is resolved.
//!
//! The fixtures do not claim durable cross-process transactions, physical recovery or live effects;
//! CI is the test authority and its receipt is not awaited by the implementation branch.

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

fn temp_project(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-cp23-{label}-{stamp}"));
    fs::create_dir_all(root.join(".git")).expect("project root");
    root
}

fn builder_outputs() -> serde_json::Value {
    json!([
        {
            "text": "implementing the frozen packet",
            "tool_calls": [{
                "id": "cp23-write",
                "name": "apply_patch",
                "arguments": {
                    "patch": "*** Begin Patch\n*** Add File: OUTPUT.txt\n+cp23 company output\n*** End Patch\n"
                }
            }]
        },
        {"text": "OUTPUT.txt created from the approved packet"}
    ])
}

fn host() -> DaemonHost {
    let harness = KianaHarness::new(Arc::new(
        ScriptedModel::from_json(&builder_outputs()).expect("fake model cassette"),
    ));
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
    key: &str,
    command: CompanyCommand,
) -> ResponseEnvelope {
    let request = CompanyCommandRequest {
        schema: COMPANY_COMMAND_SCHEMA.to_owned(),
        expected_revision,
        idempotency_key: key.to_owned(),
        command,
    };
    let envelope = RequestEnvelope::company_command(metadata(root, session, role), request)
        .expect("company command envelope");
    host.handle(envelope).await
}

/// Submit one company command that must commit, and return the committed Company revision.
async fn committed(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    role: RoleSpec,
    revision: &mut u64,
    key: &str,
    command: CompanyCommand,
) -> ResponseEnvelope {
    let response = send(host, root, session, role, *revision, key, command).await;
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

/// Submit one company command that must be refused before any related object changes.
async fn blocked(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    role: RoleSpec,
    expected_revision: u64,
    key: &str,
    command: CompanyCommand,
) -> ResponseEnvelope {
    let response = send(host, root, session, role, expected_revision, key, command).await;
    assert_eq!(
        response.status,
        ExecutionStatus::Blocked,
        "company command {key} must be blocked: {response:?}"
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
        objective_id: "objective-cp23".to_owned(),
        organization_id: "org-cp23".to_owned(),
        title: "Ship a governed coding output".to_owned(),
        problem: "The project needs a reviewable artifact".to_owned(),
        metric: "delivery_rate".to_owned(),
        baseline: 0.0,
        target: 1.0,
        unit: "ratio".to_owned(),
        measurement_method: Some("cp23 fixture acceptance ratio".to_owned()),
        direction: MetricDirection::AtLeast,
        period_start: 1,
        period_end: u64::MAX - 1,
        owner_principal_id: "local-user".to_owned(),
        status: ObjectiveStatus::Proposed,
        version: 1,
    }
}

fn budget_policy(project_id: &str) -> CompanyBudgetPolicy {
    CompanyBudgetPolicy {
        project: ProjectBudget {
            project_id: ProjectId::new(),
            max_runs: 4,
            max_tokens: 100_000,
        },
        runtime: RuntimeBudget {
            max_model_calls: 8,
            max_tokens: 100_000,
            max_wall_time_ms: 60_000,
        },
        quota: Quota {
            scope: project_id.to_owned(),
            model_calls: 8,
            tokens: 100_000,
            concurrency: 1,
        },
    }
}

fn close_command(
    project_id: &str,
    delivery_id: &str,
    receipt_id: &str,
    evidence_refs: &[String],
) -> CompanyCommand {
    CompanyCommand::CloseProject {
        project_id: project_id.to_owned(),
        delivery_id: delivery_id.to_owned(),
        receipt_id: receipt_id.to_owned(),
        evidence_refs: evidence_refs.to_vec(),
    }
}

#[tokio::test]
async fn cp_builder_cannot_self_accept_or_promote_authority() {
    let root = temp_project("self-accept");
    fs::write(root.join("charter.json"), "charter: bounded project\n").expect("charter");
    let host = host();
    let mut revision = 0_u64;

    // Every protected mutation class that must not admit a Builder subject.
    for (key, command) in [
        (
            "builder-approve-project",
            CompanyCommand::ApproveProject {
                project_id: "project-cp23".to_owned(),
                decision_ref: "artifact:charter".to_owned(),
            },
        ),
        (
            "builder-accept-own-work",
            CompanyCommand::DecideAcceptance {
                acceptance_id: "acceptance-cp23".to_owned(),
                review_id: "review-cp23".to_owned(),
                decision: AcceptanceDecision::Accept,
                reasons: Vec::new(),
                waiver_ref: None,
            },
        ),
        (
            "builder-configure-budget",
            CompanyCommand::ConfigureBudget {
                project_id: "project-cp23".to_owned(),
                policy: budget_policy("project-cp23"),
            },
        ),
        (
            "builder-close-project",
            CompanyCommand::CloseProject {
                project_id: "project-cp23".to_owned(),
                delivery_id: "delivery-cp23".to_owned(),
                receipt_id: "closing-cp23".to_owned(),
                evidence_refs: vec!["event:missing".to_owned()],
            },
        ),
        (
            "builder-promote-authority",
            CompanyCommand::Business {
                schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
                action: Box::new(CompanyBusinessAction::Appoint {
                    assignment: BusinessAssignment {
                        assignment_id: "assignment-cp23".to_owned(),
                        organization_id: "org-cp23".to_owned(),
                        owner_principal_id: "local-user".to_owned(),
                        role_id: "sponsor".to_owned(),
                        actor_kind: BusinessActorKind::Agent,
                        project_ids: vec!["project-cp23".to_owned()],
                        expires_at: u64::MAX - 1,
                        version: 1,
                        revoked: false,
                    },
                }),
            },
        ),
    ] {
        let response = blocked(
            &host,
            &root,
            "builder-session",
            RoleSpec::builder(),
            revision,
            key,
            command,
        )
        .await;
        assert_eq!(
            response.error.as_deref(),
            Some("company_role_denied"),
            "builder must not submit {key}"
        );
    }

    // No rejected command changed a related object or advanced the Company revision: the sponsor
    // command below still commits at revision 0 and produces exactly one business fact.
    let proposed = committed(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "sponsor-propose-objective",
        CompanyCommand::ProposeObjective {
            objective: objective(),
        },
    )
    .await;
    assert_eq!(revision, 1);
    assert_eq!(
        proposed.output["state"]["objectives"]["objective-cp23"]["status"],
        "proposed"
    );

    let events = persisted_events(&host).await;
    assert!(
        events
            .iter()
            .filter(|event| event.kind == "company.command_rejected")
            .count()
            >= 5
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.ObjectiveProposed")
            .count(),
        1
    );
    for forbidden in [
        "company.ProjectApproved",
        "company.AcceptanceDecided",
        "company.ProjectBudgetConfigured",
        "company.ProjectClosed",
        "company.business.v2",
    ] {
        assert!(
            !events.iter().any(|event| event.kind == forbidden),
            "denied builder command manufactured a fact: {forbidden}"
        );
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn cp_company_command_conflict_changes_no_related_object() {
    let root = temp_project("conflict");
    fs::write(root.join("charter.json"), "charter: bounded project\n").expect("charter");
    let host = host();
    let mut revision = 0;
    committed(
        &host,
        &root,
        "builder-artifact-session",
        RoleSpec::builder(),
        &mut revision,
        "register-charter",
        CompanyCommand::RegisterArtifact {
            artifact_id: "charter".to_owned(),
            relative_path: "charter.json".to_owned(),
        },
    )
    .await;
    committed(
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
    assert_eq!(revision, 2);

    // A command built on a stale Company revision conflicts instead of overwriting related state.
    let stale = blocked(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        1,
        "stale-activate-objective",
        CompanyCommand::DecideObjective {
            objective_id: "objective-cp23".to_owned(),
            approve: true,
        },
    )
    .await;
    assert_eq!(stale.error.as_deref(), Some("company_revision_conflict"));

    // The same idempotency key with a different payload conflicts instead of replaying.
    let mut changed = objective();
    changed.title = "different objective".to_owned();
    let replay = blocked(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        revision,
        "propose-objective",
        CompanyCommand::ProposeObjective { objective: changed },
    )
    .await;
    assert_eq!(
        replay.error.as_deref(),
        Some("company_idempotency_conflict")
    );

    // The next valid command proves the conflicts changed nothing: one objective, next revision,
    // and the activation only becomes visible now.
    let activated = committed(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "activate-objective",
        CompanyCommand::DecideObjective {
            objective_id: "objective-cp23".to_owned(),
            approve: true,
        },
    )
    .await;
    assert_eq!(revision, 3);
    assert_eq!(
        activated.output["state"]["objectives"]
            .as_object()
            .expect("objectives")
            .len(),
        1
    );
    assert_eq!(
        activated.output["state"]["objectives"]["objective-cp23"]["status"],
        "active"
    );

    let events = persisted_events(&host).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.ObjectiveProposed")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.ObjectiveActivated")
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .filter(|event| event.kind == "company.command_rejected")
            .count()
            >= 2
    );
    assert!(
        !events
            .iter()
            .any(|event| event.kind.starts_with("company.Project")),
        "a conflicting command must not change any related object"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// Run the governed coding project up to the acceptance request, so the closeout fixture can
/// insert deny attempts around the remaining effect steps.
struct ProjectFlow {
    revision: u64,
    project_id: String,
    runtime_evidence_refs: Vec<String>,
}

async fn run_project_to_acceptance_request(host: &DaemonHost, root: &Path) -> ProjectFlow {
    let project_id = ProjectId::new().to_string();
    let milestone_id = "milestone-cp23".to_owned();
    let packet_id = "packet-cp23".to_owned();
    let mut revision = 0;

    for (key, session, role, command) in [
        (
            "register-charter",
            "builder-artifact-session",
            RoleSpec::builder(),
            CompanyCommand::RegisterArtifact {
                artifact_id: "charter".to_owned(),
                relative_path: "charter.json".to_owned(),
            },
        ),
        (
            "register-decision",
            "builder-artifact-session",
            RoleSpec::builder(),
            CompanyCommand::RegisterArtifact {
                artifact_id: "decision".to_owned(),
                relative_path: "decision.json".to_owned(),
            },
        ),
        (
            "propose-objective",
            "sponsor-session",
            RoleSpec::sponsor(),
            CompanyCommand::ProposeObjective {
                objective: objective(),
            },
        ),
        (
            "activate-objective",
            "sponsor-session",
            RoleSpec::sponsor(),
            CompanyCommand::DecideObjective {
                objective_id: "objective-cp23".to_owned(),
                approve: true,
            },
        ),
    ] {
        committed(host, root, session, role, &mut revision, key, command).await;
    }

    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "propose-project",
        CompanyCommand::ProposeProject {
            project: Project {
                project_id: project_id.clone(),
                organization_id: "org-cp23".to_owned(),
                objective_refs: vec!["objective-cp23".to_owned()],
                sponsor_id: "local-user".to_owned(),
                charter_ref: "artifact:charter".to_owned(),
                scope_baseline: "coding-v1".to_owned(),
                success_criteria: vec!["output exists".to_owned()],
                non_goals: vec!["no external publish".to_owned()],
                project_budget_ref: "budget-cp23".to_owned(),
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
    committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "start-chartering",
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
        "configure-budget",
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
        "approve-project",
        CompanyCommand::ApproveProject {
            project_id: project_id.clone(),
            decision_ref: "artifact:decision".to_owned(),
        },
    )
    .await;
    committed(
        host,
        root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "create-milestone",
        CompanyCommand::CreateMilestone {
            milestone: Milestone {
                milestone_id: milestone_id.clone(),
                project_id: project_id.clone(),
                objective_refs: vec!["objective-cp23".to_owned()],
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
    let mut packet = WorkPacket::builder_task(packet_id.clone(), "write OUTPUT.txt")
        .with_path_allow(["OUTPUT.txt"])
        .with_acceptance_tests(["output exists"]);
    packet.project_id = ProjectId::parse_str(&project_id);
    committed(
        host,
        root,
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
    committed(
        host,
        root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "plan-project",
        CompanyCommand::PlanProject {
            project_id: project_id.clone(),
        },
    )
    .await;

    let start = committed(
        host,
        root,
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
        fs::read_to_string(root.join("OUTPUT.txt")).expect("runtime output"),
        "cp23 company output\n"
    );
    let runtime_evidence_refs = start.output["company"]["state"]["runs"][&packet_id]
        ["runtime_receipt"]["event_refs"]
        .as_array()
        .expect("runtime receipt event refs")
        .iter()
        .map(|reference| {
            reference
                .as_str()
                .expect("runtime event reference")
                .to_owned()
        })
        .collect::<Vec<_>>();

    committed(
        host,
        root,
        "builder-session",
        RoleSpec::builder(),
        &mut revision,
        "request-acceptance",
        CompanyCommand::RequestAcceptance {
            acceptance_id: "acceptance-cp23".to_owned(),
            project_id: project_id.clone(),
            packet_id: packet_id.clone(),
            evidence_refs: runtime_evidence_refs.clone(),
        },
    )
    .await;

    ProjectFlow {
        revision,
        project_id,
        runtime_evidence_refs,
    }
}

#[tokio::test]
async fn cp_close_cannot_hide_unresolved_effects() {
    let root = temp_project("closeout");
    fs::write(root.join("charter.json"), "charter: bounded project\n").expect("charter");
    fs::write(root.join("decision.json"), "decision: sponsor approved\n").expect("decision");
    let host = host();
    let mut flow = run_project_to_acceptance_request(&host, &root).await;

    // A project that is not accepted yet cannot be closed, whatever receipt the caller claims.
    let not_accepted = blocked(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        flow.revision,
        "close-before-acceptance",
        close_command(
            &flow.project_id,
            "delivery-cp23",
            "closing-early",
            &flow.runtime_evidence_refs,
        ),
    )
    .await;
    assert_eq!(not_accepted.error.as_deref(), Some("project_not_accepted"));

    committed(
        &host,
        &root,
        "reviewer-session",
        RoleSpec::reviewer(),
        &mut flow.revision,
        "record-review",
        CompanyCommand::RecordReview {
            review_id: "review-cp23".to_owned(),
            acceptance_id: "acceptance-cp23".to_owned(),
            criterion_results: [("output exists".to_owned(), true)].into_iter().collect(),
            evidence_refs: flow.runtime_evidence_refs.clone(),
        },
    )
    .await;
    committed(
        &host,
        &root,
        "reviewer-session",
        RoleSpec::reviewer(),
        &mut flow.revision,
        "accept-review",
        CompanyCommand::DecideAcceptance {
            acceptance_id: "acceptance-cp23".to_owned(),
            review_id: "review-cp23".to_owned(),
            decision: AcceptanceDecision::Accept,
            reasons: Vec::new(),
            waiver_ref: None,
        },
    )
    .await;

    // An invented delivery reference cannot stand in for an unresolved delivery effect.
    let missing_delivery = blocked(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        flow.revision,
        "close-missing-delivery",
        close_command(
            &flow.project_id,
            "delivery-cp23",
            "closing-missing",
            &flow.runtime_evidence_refs,
        ),
    )
    .await;
    assert_eq!(
        missing_delivery.error.as_deref(),
        Some("delivery_not_found")
    );

    committed(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut flow.revision,
        "register-output",
        CompanyCommand::RegisterArtifact {
            artifact_id: "output".to_owned(),
            relative_path: "OUTPUT.txt".to_owned(),
        },
    )
    .await;
    committed(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut flow.revision,
        "prepare-delivery",
        CompanyCommand::PrepareDelivery {
            delivery: Delivery {
                delivery_id: "delivery-cp23".to_owned(),
                project_id: flow.project_id.clone(),
                acceptance_id: "acceptance-cp23".to_owned(),
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

    // A prepared delivery is not a confirmed effect.
    let unconfirmed = blocked(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        flow.revision,
        "close-prepared-delivery",
        close_command(
            &flow.project_id,
            "delivery-cp23",
            "closing-unconfirmed",
            &flow.runtime_evidence_refs,
        ),
    )
    .await;
    assert_eq!(
        unconfirmed.error.as_deref(),
        Some("closing_confirmed_delivery_required")
    );

    committed(
        &host,
        &root,
        "sponsor-delivery-session",
        RoleSpec::sponsor(),
        &mut flow.revision,
        "approve-delivery",
        CompanyCommand::ApproveDelivery {
            delivery_id: "delivery-cp23".to_owned(),
        },
    )
    .await;
    committed(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut flow.revision,
        "deliver-output",
        CompanyCommand::Deliver {
            delivery_id: "delivery-cp23".to_owned(),
            evidence_refs: vec!["artifact:output".to_owned()],
        },
    )
    .await;

    // A delivered but unconfirmed handoff is still an unresolved effect.
    let undelivered = blocked(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        flow.revision,
        "close-delivered-handoff",
        close_command(
            &flow.project_id,
            "delivery-cp23",
            "closing-undelivered",
            &flow.runtime_evidence_refs,
        ),
    )
    .await;
    assert_eq!(
        undelivered.error.as_deref(),
        Some("closing_confirmed_delivery_required")
    );

    let handoff_receipt_ref = flow
        .runtime_evidence_refs
        .first()
        .cloned()
        .expect("completed run evidence");
    committed(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut flow.revision,
        "confirm-delivery",
        CompanyCommand::ConfirmDelivery {
            delivery_id: "delivery-cp23".to_owned(),
            handoff_receipt_ref,
        },
    )
    .await;

    // Only the fully resolved chain commits the closing fact, once.
    let closing = committed(
        &host,
        &root,
        "closer-session",
        RoleSpec::closer(),
        &mut flow.revision,
        "close-project",
        close_command(
            &flow.project_id,
            "delivery-cp23",
            "closing-cp23",
            &flow.runtime_evidence_refs,
        ),
    )
    .await;
    assert_eq!(
        closing.output["state"]["projects"][&flow.project_id]["status"],
        "closed"
    );
    assert!(
        closing.output["state"]["closing_receipts"]["closing-cp23"]["evidence_refs"]
            .as_array()
            .is_some_and(|refs| !refs.is_empty())
    );

    let events = persisted_events(&host).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.ProjectClosed")
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .filter(|event| event.kind == "company.command_rejected")
            .count()
            >= 4
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.AcceptanceDecided")
            .count(),
        1
    );
    fs::remove_dir_all(root).expect("cleanup");
}
