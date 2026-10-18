//! CO-12 behavioural fixtures: admitting a Plan/WorkGraph revision is one control-plane
//! transaction.
//!
//! These fixtures drive the real `DaemonHost -> ControlPlane -> EventStore` spine instead of
//! grepping source text:
//!
//! - `co12_invalid_plan_graph_commits_no_related_object` proves the approval boundary fails
//!   closed before any related object changes: a Builder subject is denied by the role gate, a
//!   blocking cycle, a missing dependency endpoint, a cross-project node, an empty required
//!   Milestone, a stale self-digest and a drifted Charter baseline each keep their own stable
//!   reason, the aggregate records only rejection facts, and the valid batch draft admitted
//!   afterwards creates no Milestone, Packet or Run and leaves the project revision untouched.
//! - `co12_two_milestone_plan_commits_once_through_the_approval_transaction` proves a plan whose
//!   candidate graph was assembled as one batch (edges before nodes, children before parents) is
//!   validated exactly once at admission, that the admitted digest equals the reviewed digest,
//!   that dependency versions are frozen per node, that replaying the exact command adds no
//!   second fact, and that a different revision of the same project is refused.
//!
//! The fixtures are process-local source evidence: no durable cross-process plan store, no model
//! call, no capability or lease is claimed, and CI is the test authority whose receipt this
//! branch does not wait for.

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
    let root = std::env::temp_dir().join(format!("kiana-co12-{label}-{stamp}"));
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

fn objective() -> Objective {
    Objective {
        objective_id: "objective-co12".to_owned(),
        organization_id: "org-co12".to_owned(),
        title: "Ship a two-milestone plan".to_owned(),
        problem: "The project needs an independently acceptable milestone split".to_owned(),
        metric: "dependency_boundary_rate".to_owned(),
        baseline: 0.0,
        target: 1.0,
        unit: "ratio".to_owned(),
        measurement_method: Some("co12 fixture acceptance ratio".to_owned()),
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
        organization_id: "org-co12".to_owned(),
        objective_refs: vec!["objective-co12".to_owned()],
        sponsor_id: "local-user".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "co12-plan-v1".to_owned(),
        success_criteria: vec!["both milestones are independently acceptable".to_owned()],
        non_goals: vec!["no external publish".to_owned()],
        project_budget_ref: "budget:co12".to_owned(),
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

struct ApprovedFixture {
    project_id: String,
    revision: u64,
    charter_baseline_version: u64,
    project_version: u64,
}

/// Walk the existing Company lanes to one approved project with a frozen Charter baseline. The
/// plan admission below is the only CO-12 command in these fixtures.
async fn approve_project(host: &DaemonHost, root: &Path) -> ApprovedFixture {
    fs::write(
        root.join("charter.json"),
        "charter: two independently acceptable milestones\n",
    )
    .expect("charter");
    fs::write(
        root.join("decision.json"),
        "decision: sponsor approved the bounded plan fixture\n",
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
        "co12-register-charter",
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
        "co12-register-decision",
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
        "co12-propose-objective",
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
        "co12-activate-objective",
        CompanyCommand::DecideObjective {
            objective_id: "objective-co12".to_owned(),
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
        "co12-propose-project",
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
        "co12-start-chartering",
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
        "co12-configure-budget",
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
        "co12-record-direct-intake",
        CompanyCommand::RecordDirectIntake {
            project_id: project_id.clone(),
            reason: "the plan fixture records the skipped Initiative lane explicitly".to_owned(),
        },
    )
    .await;
    let approved = committed(
        host,
        root,
        "sponsor-session",
        RoleSpec::sponsor(),
        &mut revision,
        "co12-approve-project",
        CompanyCommand::ApproveProject {
            project_id: project_id.clone(),
            decision_ref: "artifact:decision".to_owned(),
        },
    )
    .await;
    let state = state_of(&approved);
    ApprovedFixture {
        charter_baseline_version: state["charter_baselines"][&project_id]["version"]
            .as_u64()
            .expect("frozen charter baseline version"),
        project_version: state["projects"][&project_id]["version"]
            .as_u64()
            .expect("project version"),
        revision: state["revision"].as_u64().expect("company revision"),
        project_id,
    }
}

fn plan_node(project_id: &str, id: &str, kind: PlanNodeKind, required: bool) -> PlanNode {
    PlanNode {
        node_id: id.to_owned(),
        project_id: project_id.to_owned(),
        kind,
        version: 1,
        required,
        criterion_refs: Vec::new(),
        input_refs: Vec::new(),
    }
}

fn plan_edge(from: &str, to: &str, kind: PlanEdgeKind, reference: Option<&str>) -> PlanEdge {
    PlanEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
        reference: reference.map(str::to_owned),
    }
}

fn coverage_digest() -> String {
    json_digest(&json!({"fixture": "co12-two-milestone-plan"}))
}

/// Two-milestone candidate assembled as one batch in a deliberately non-topological order:
/// edges and children are pushed before the nodes they point at.
fn two_milestone_draft(project_id: &str) -> PlanProposalDraft {
    let mut draft = PlanProposalDraft::new();
    draft
        .push_edge(plan_edge("m1", "p1", PlanEdgeKind::ParentChild, None))
        .expect("edge before its nodes");
    draft
        .push_edge(plan_edge(
            "m1",
            "m2",
            PlanEdgeKind::RequiresAcceptance,
            Some("acceptance:m1"),
        ))
        .expect("edge before its nodes");
    draft
        .push_edge(plan_edge("m2", "p2", PlanEdgeKind::ParentChild, None))
        .expect("edge");
    draft
        .push_edge(plan_edge("m1", "p2", PlanEdgeKind::Related, None))
        .expect("edge");
    draft
        .push_node(plan_node(project_id, "p1", PlanNodeKind::Packet, true))
        .expect("child before parent");
    draft
        .push_node(plan_node(project_id, "m2", PlanNodeKind::Milestone, true))
        .expect("node");
    draft
        .push_node(plan_node(project_id, "p2", PlanNodeKind::Packet, true))
        .expect("node");
    draft
        .push_node(plan_node(project_id, "m1", PlanNodeKind::Milestone, true))
        .expect("parent");
    draft
}

fn two_milestone_plan(
    project_id: &str,
    charter_baseline_version: u64,
    plan_version: u64,
) -> PlanProposal {
    two_milestone_draft(project_id)
        .freeze_versioned(
            project_id,
            charter_baseline_version,
            plan_version,
            coverage_digest(),
        )
        .expect("two-milestone plan")
}

/// Craft a proposal value directly, exactly like a wire payload that claims to be a plan; the
/// digest is recomputed so every refusal is about the graph contract and never about a stale
/// self-digest.
fn crafted_plan(
    project_id: &str,
    charter_baseline_version: u64,
    plan_version: u64,
    nodes: Vec<PlanNode>,
    edges: Vec<PlanEdge>,
) -> PlanProposal {
    let mut plan = PlanProposal {
        schema: PLAN_PROPOSAL_SCHEMA.to_owned(),
        project_id: project_id.to_owned(),
        charter_baseline_version,
        plan_version,
        nodes: nodes
            .into_iter()
            .map(|node| (node.node_id.clone(), node))
            .collect(),
        edges,
        required_criteria: Vec::new(),
        read_scope: Vec::new(),
        coverage_digest: coverage_digest(),
        digest: String::new(),
    };
    plan.digest = plan.canonical_digest();
    plan
}

#[tokio::test]
async fn co12_invalid_plan_graph_commits_no_related_object() {
    let root = temp_project("deny");
    let host = host();
    let approved = approve_project(&host, &root).await;
    let project_id = approved.project_id.clone();
    let baseline = approved.charter_baseline_version;
    let revision = approved.revision;

    // A Builder subject cannot admit a plan at all: the role gate rejects it before any state read
    // and before any plan material is inspected.
    let builder = refused(
        &host,
        &root,
        "builder-session",
        RoleSpec::builder(),
        revision,
        "co12-builder-freeze",
        CompanyCommand::FreezePlanGraph {
            plan: two_milestone_plan(&project_id, baseline, 1),
        },
    )
    .await;
    assert_eq!(builder.error.as_deref(), Some("company_role_denied"));

    // A blocking cycle in the complete candidate graph is refused with the cycle reason.
    let cycle = crafted_plan(
        &project_id,
        baseline,
        1,
        vec![
            plan_node(&project_id, "m1", PlanNodeKind::Milestone, true),
            plan_node(&project_id, "m2", PlanNodeKind::Milestone, true),
        ],
        vec![
            plan_edge("m1", "m2", PlanEdgeKind::RequiresRunSuccess, Some("run:1")),
            plan_edge("m2", "m1", PlanEdgeKind::RequiresRunSuccess, Some("run:2")),
        ],
    );
    let refused_cycle = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co12-cycle",
        CompanyCommand::FreezePlanGraph { plan: cycle },
    )
    .await;
    assert_eq!(
        refused_cycle.error.as_deref(),
        Some("plan_dependency_cycle")
    );

    // A missing dependency endpoint is reported by the edge that names it.
    let missing = crafted_plan(
        &project_id,
        baseline,
        1,
        vec![
            plan_node(&project_id, "m1", PlanNodeKind::Milestone, true),
            plan_node(&project_id, "p1", PlanNodeKind::Packet, true),
        ],
        vec![
            plan_edge("m1", "p1", PlanEdgeKind::ParentChild, None),
            plan_edge(
                "p1",
                "p-missing",
                PlanEdgeKind::RequiresArtifact,
                Some("artifact:missing"),
            ),
        ],
    );
    let refused_missing = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co12-missing-endpoint",
        CompanyCommand::FreezePlanGraph { plan: missing },
    )
    .await;
    assert_eq!(
        refused_missing.error.as_deref(),
        Some("plan_edge_to_missing")
    );

    // A node from another project cannot enter this plan graph.
    let mut foreign = plan_node(&project_id, "m1", PlanNodeKind::Milestone, true);
    foreign.project_id = "other-project".to_owned();
    let cross_project = crafted_plan(
        &project_id,
        baseline,
        1,
        vec![
            foreign,
            plan_node(&project_id, "p1", PlanNodeKind::Packet, true),
        ],
        vec![plan_edge("m1", "p1", PlanEdgeKind::ParentChild, None)],
    );
    let refused_cross_project = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co12-cross-project",
        CompanyCommand::FreezePlanGraph {
            plan: cross_project,
        },
    )
    .await;
    assert_eq!(
        refused_cross_project.error.as_deref(),
        Some("plan_node_project_or_version_invalid")
    );

    // A required Milestone whose only child output is optional is a planning hole.
    let empty_milestone = crafted_plan(
        &project_id,
        baseline,
        1,
        vec![
            plan_node(&project_id, "m1", PlanNodeKind::Milestone, true),
            plan_node(&project_id, "p1", PlanNodeKind::Packet, true),
            plan_node(&project_id, "p2", PlanNodeKind::Packet, false),
        ],
        vec![
            plan_edge("m1", "p2", PlanEdgeKind::ParentChild, None),
            plan_edge("m1", "p1", PlanEdgeKind::RequiresRunSuccess, Some("run:1")),
        ],
    );
    let refused_empty = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co12-empty-milestone",
        CompanyCommand::FreezePlanGraph {
            plan: empty_milestone,
        },
    )
    .await;
    assert_eq!(
        refused_empty.error.as_deref(),
        Some("plan_required_milestone_empty")
    );

    // A payload that mutates the reviewed graph without recomputing its digest is refused by the
    // self-digest fence, so a wire caller cannot edit an admitted proposal afterwards.
    let mut forged = two_milestone_plan(&project_id, baseline, 2);
    forged.plan_version = 5;
    let refused_forged = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co12-stale-self-digest",
        CompanyCommand::FreezePlanGraph { plan: forged },
    )
    .await;
    assert_eq!(
        refused_forged.error.as_deref(),
        Some("plan_digest_mismatch")
    );

    // The plan has to bind the server-side frozen Charter baseline of this project.
    let drifted_baseline = two_milestone_plan(&project_id, baseline + 1, 2);
    let refused_baseline = refused(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        revision,
        "co12-baseline-drift",
        CompanyCommand::FreezePlanGraph {
            plan: drifted_baseline,
        },
    )
    .await;
    assert_eq!(
        refused_baseline.error.as_deref(),
        Some("plan_graph_baseline_mismatch")
    );

    // Only after every refusal above does the valid batch draft get admitted, and the admitted
    // transaction still creates no related object: no Milestone, Packet or Run, and the project
    // revision stays where the approval left it.
    let plan = two_milestone_plan(&project_id, baseline, 2);
    let mut admission_revision = revision;
    let admitted = committed(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut admission_revision,
        "co12-admit-after-refusals",
        CompanyCommand::FreezePlanGraph { plan: plan.clone() },
    )
    .await;
    let state = state_of(&admitted);
    assert_eq!(
        state["plan_graphs"].as_object().map(|graphs| graphs.len()),
        Some(1)
    );
    assert_eq!(
        state["plan_graphs"][&project_id]["plan_digest"],
        json!(plan.digest)
    );
    assert_eq!(
        state["projects"][&project_id]["version"],
        json!(approved.project_version)
    );
    assert_eq!(
        state["milestones"].as_object().map(|map| map.len()),
        Some(0)
    );
    assert_eq!(state["packets"].as_object().map(|map| map.len()), Some(0));
    assert_eq!(state["runs"].as_object().map(|map| map.len()), Some(0));

    let events = host
        .persisted_events()
        .await
        .expect("event read")
        .expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.PlanGraphFrozen")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.command_rejected")
            .count(),
        7
    );
    assert!(!events.iter().any(|event| matches!(
        event.kind.as_str(),
        "company.MilestoneCreated" | "company.PacketApproved" | "company.RunStartRequested"
    )));
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn co12_two_milestone_plan_commits_once_through_the_approval_transaction() {
    let root = temp_project("admit");
    let host = host();
    let approved = approve_project(&host, &root).await;
    let project_id = approved.project_id.clone();
    let mut revision = approved.revision;

    // The batch draft keeps every boundary visible: Milestone->Packet outputs, the explicit
    // acceptance dependency between the two milestones and one informational related edge.
    let plan = two_milestone_plan(&project_id, approved.charter_baseline_version, 2);
    let order = plan.topological_order().expect("stable topological order");
    assert_eq!(order.len(), 4);
    assert!(order.iter().position(|id| id == "m1") < order.iter().position(|id| id == "m2"));
    assert!(order.iter().position(|id| id == "m2") < order.iter().position(|id| id == "p2"));
    assert_eq!(
        plan.topological_order().expect("deterministic order"),
        order
    );

    let admitted = committed(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        &mut revision,
        "co12-admit-two-milestone-plan",
        CompanyCommand::FreezePlanGraph { plan: plan.clone() },
    )
    .await;
    let state = state_of(&admitted);
    let frozen = &state["plan_graphs"][&project_id];
    assert_eq!(
        frozen["plan_digest"],
        json!(plan.digest),
        "the reviewed digest is the effective graph"
    );
    assert_eq!(frozen["plan_version"], json!(2));
    assert_eq!(
        frozen["charter_baseline_version"],
        json!(approved.charter_baseline_version)
    );
    assert_eq!(frozen["node_versions"]["m1"], json!(1));
    assert_eq!(frozen["node_versions"]["p2"], json!(1));
    assert_eq!(
        frozen["node_versions"]
            .as_object()
            .map(|versions| versions.len()),
        Some(4)
    );
    assert_eq!(
        state["projects"][&project_id]["version"],
        json!(approved.project_version)
    );

    // Replaying the exact reviewed command keeps the same fact and changes no object.
    let replay = send(
        &host,
        &root,
        "pm-session",
        RoleSpec::pm(),
        approved.revision,
        "co12-admit-two-milestone-plan",
        CompanyCommand::FreezePlanGraph { plan: plan.clone() },
    )
    .await;
    assert_eq!(replay.status, ExecutionStatus::Completed, "{replay:?}");
    assert_eq!(replay.output["replayed"], json!(true));
    assert_eq!(
        state_of(&replay)["plan_graphs"][&project_id]["plan_digest"],
        json!(plan.digest)
    );

    // A different revision of the same project is refused and the admitted dependency versions
    // stay frozen.
    let revised = two_milestone_plan(&project_id, approved.charter_baseline_version, 3);
    let refused_revision = refused(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        revision,
        "co12-second-plan-revision",
        CompanyCommand::FreezePlanGraph { plan: revised },
    )
    .await;
    assert_eq!(
        refused_revision.error.as_deref(),
        Some("plan_graph_already_frozen")
    );

    let events = host
        .persisted_events()
        .await
        .expect("event read")
        .expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.PlanGraphFrozen")
            .count(),
        1
    );
    assert!(events.iter().any(|event| {
        event.kind == "company.command_rejected"
            && event.data["reason"] == json!("plan_graph_already_frozen")
    }));
    assert!(!events.iter().any(|event| matches!(
        event.kind.as_str(),
        "company.MilestoneCreated" | "company.PacketApproved" | "company.RunStartRequested"
    )));
    fs::remove_dir_all(root).expect("cleanup");
}
