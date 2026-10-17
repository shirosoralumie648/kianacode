//! CP-24 behavioural fixtures: deterministic scheduling, WorkPacket handoff and child attenuation.
//!
//! These fixtures drive the real domain/ports contracts instead of grepping source text:
//!
//! - `cp_two_schedulers_claim_one_packet_once` runs two schedulers against one
//!   `MemoryWorkflowQueueStore` and proves a ready packet is claimed exactly once; the loser gets
//!   `workflow_queue_claim_contended`, the winner's lease passes
//!   `validate_workflow_queue_transaction`, the loser cannot reuse that lease, and the claimed
//!   item disappears from `ready()`.
//! - `cp_workflow_replay_never_reexecutes_completed_effect` commits
//!   `RegisterDefinition` -> `Start` -> `Advance` through a real `ControlPlane` + `MemoryEventLog`
//!   where the `Advance` dispatches a capability effect; replaying the exact committed command
//!   returns `replayed:true` with the identical state and ledger, and a fresh `ControlPlane` over
//!   the same ledger rebuilds that state through `workflow.snapshot.v1` without new facts.
//! - `cp_child_cannot_gain_parent_missing_scope_or_budget` proves the parent-contained child rule
//!   at the grant derivation and scheduler-claim boundaries: `derive_swarm_child_grant` refuses a
//!   packet path the parent grant lacks, and `WorkflowQueueClaimContract::from_work_packet` plus
//!   the ready projection refuse widened scope, swapped/missing budget leases and later deadlines.
//!
//! These fixtures are process-local source evidence: the queue adapter is in-memory, no durable
//! cross-process scheduler index, timer checkpoint or worker-death recovery is claimed, and CI is
//! the test authority whose receipt this branch does not wait for.

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{
    derive_swarm_child_grant, project_workflow_queue_ready, validate_workflow_queue_transaction,
    ControlPlane, WorkflowQueueReadyView,
};
use kiana_domain::{
    AgentTemplate, AutomationCommand, AutomationCommandRequest, AutomationState, BudgetLeaseId,
    CapabilityGrant, CapabilityGrantId, CapabilityKind, CapabilityRequest, CommandIntent,
    CoreResponse, ExecutionStatus, PermissionProfile, PrincipalId, ProjectId, RequestContext,
    RequestId, RiskLevel, RoleSpec, WorkPacket, WorkPacketStatus, WorkflowDefinition, WorkflowNode,
    WorkflowNodeKind, WorkflowQueueClaimContract, WorkflowQueueClaimRequest,
    WorkflowQueueClaimStatus, WorkflowQueueLeaseStatus, AUTOMATION_COMMAND, AUTOMATION_SCHEMA,
    AUTOMATION_SNAPSHOT, CAPABILITY_GRANT_SCHEMA, SWARM_CHILD_TEMPLATE,
};
use kiana_eventlog::{MemoryEventLog, MemoryWorkflowQueueStore};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort, WorkflowQueueStore};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Create an Approved builder packet with one path grant and one budget lease.
fn approved_packet(id: &str, budget: BudgetLeaseId, path: &str) -> WorkPacket {
    let mut packet = WorkPacket::builder_task(id, format!("{id} work")).with_path_allow([path]);
    packet.budget_lease_id = Some(budget);
    packet
        .transition_status(WorkPacketStatus::Approved)
        .expect("draft packet can be approved");
    packet
}

/// Build the same parent/child graph used by the scheduler handoff fixtures.
fn scheduler_packets(child_path: &str) -> BTreeMap<String, WorkPacket> {
    let budget = BudgetLeaseId::new();
    let parent = approved_packet("parent", budget, "src");
    let mut child = approved_packet("child", budget, child_path);
    child.parent_packet_id = Some(parent.id.clone());
    BTreeMap::from([(parent.id.clone(), parent), (child.id.clone(), child)])
}

/// Derive a claim request for one projected ready item and one scheduler owner.
fn claim_request(
    view: &WorkflowQueueReadyView,
    item_id: &str,
    owner_id: &str,
    fence_token: u64,
) -> WorkflowQueueClaimRequest {
    let claim = view
        .claims
        .iter()
        .find(|claim| claim.item_id == item_id)
        .expect("ready item exists")
        .clone();
    WorkflowQueueClaimRequest {
        claim,
        owner_id: owner_id.to_owned(),
        fence_token,
        authority_epoch: 1,
        observed_at_unix_ms: 1_000,
        lease_ttl_ms: 100,
    }
}

#[tokio::test]
async fn cp_two_schedulers_claim_one_packet_once() {
    // Two scheduler identities race for the same ready packet on the real queue store.
    let packets = scheduler_packets("src/lib.rs");
    let view = project_workflow_queue_ready(&packets, 1_000, 2).expect("ready snapshot");
    let packet = &packets["child"];
    let parent = &packets["parent"];
    let claim = view
        .claims
        .iter()
        .find(|claim| claim.item_id == "child")
        .expect("child is ready")
        .clone();

    let store = MemoryWorkflowQueueStore::new();
    let (left, right) = tokio::join!(
        store.claim(claim_request(&view, "child", "scheduler-a", 1)),
        store.claim(claim_request(&view, "child", "scheduler-b", 2)),
    );
    let (winner, fence_token, lease) = match (left, right) {
        (Ok(lease), Err(error)) => {
            assert!(
                error.to_string().contains("workflow_queue_claim_contended"),
                "losing scheduler must observe contention: {error}"
            );
            ("scheduler-a", 1, lease)
        }
        (Err(error), Ok(lease)) => {
            assert!(
                error.to_string().contains("workflow_queue_claim_contended"),
                "losing scheduler must observe contention: {error}"
            );
            ("scheduler-b", 2, lease)
        }
        (Ok(_), Ok(_)) => panic!("one packet received two leases"),
        (Err(left), Err(right)) => {
            panic!("no scheduler could claim the ready packet: {left} / {right}")
        }
    };
    assert_eq!(lease.owner_id, winner);
    assert_eq!(lease.fence_token, fence_token);
    assert_eq!(lease.status, WorkflowQueueLeaseStatus::Active);

    // The single winner can hand the packet to the existing ControlPlane route.
    validate_workflow_queue_transaction(
        &view,
        &claim,
        packet,
        parent,
        &lease,
        winner,
        fence_token,
        1,
        1_010,
    )
    .expect("the one winner passes the full ready/packet/lease transaction gate");

    // The loser cannot borrow the winner's lease or claim the packet again while it is live.
    let loser = if winner == "scheduler-a" {
        "scheduler-b"
    } else {
        "scheduler-a"
    };
    assert_eq!(
        validate_workflow_queue_transaction(
            &view,
            &claim,
            packet,
            parent,
            &lease,
            loser,
            fence_token,
            1,
            1_010,
        )
        .expect_err("the losing scheduler cannot reuse the winner lease"),
        "workflow_queue_owner_mismatch"
    );
    let repeated = store
        .claim(claim_request(&view, "child", loser, 3))
        .await
        .expect_err("a live lease cannot be claimed twice");
    assert!(
        repeated
            .to_string()
            .contains("workflow_queue_claim_contended"),
        "repeat claim must observe the live lease: {repeated}"
    );
    let ready = store.ready(4).await.expect("ready view after claim");
    assert!(ready.iter().all(|claim| claim.item_id != "child"));
}

/// Approval store that is never expected to stage anything in these fixtures.
struct NoApprovals;

#[async_trait]
impl ApprovalStorePort for NoApprovals {}

/// Runner stub proving the workflow command path never starts a model loop here.
struct NoRunner;

#[async_trait]
impl RunnerPort for NoRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

/// Trusted Balanced PM context accepted by the durable workflow command gate.
fn workflow_context() -> RequestContext {
    let mut context = RequestContext::local("cp24-session", "/cp24-replay-project");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context.assign_role(&RoleSpec::pm());
    context
}

/// Definition whose single node reserves and dispatches a read-only capability effect.
fn replay_definition() -> WorkflowDefinition {
    WorkflowDefinition {
        definition_id: "cp24-replay".to_owned(),
        version: 1,
        input_keys: Vec::new(),
        output_keys: Vec::new(),
        allowed_roles: vec!["pm".to_owned()],
        max_duration_ms: 60_000,
        max_steps: 4,
        nodes: BTreeMap::from([(
            "effect".to_owned(),
            WorkflowNode {
                dependencies: Vec::new(),
                kind: WorkflowNodeKind::Capability {
                    request: CapabilityRequest::new(
                        RequestId::new(),
                        CapabilityKind::Process,
                        "process.start",
                        json!({"command": "true"}),
                    )
                    .with_risk(RiskLevel::ReadOnly),
                },
                timeout_ms: 5_000,
                retry_limit: 0,
                compensation: None,
            },
        )]),
        artifacts: BTreeMap::new(),
    }
}

/// Build one external workflow command request with an explicit idempotency key.
fn command_request(
    expected_revision: u64,
    idempotency_key: &str,
    command: AutomationCommand,
) -> AutomationCommandRequest {
    AutomationCommandRequest {
        schema: AUTOMATION_SCHEMA.to_owned(),
        expected_revision,
        idempotency_key: idempotency_key.to_owned(),
        command,
    }
}

/// Route one workflow command through the single ControlPlane command entry.
async fn send_workflow(
    core: &ControlPlane,
    context: &RequestContext,
    request: AutomationCommandRequest,
) -> CoreResponse {
    core.handle_command(
        context.clone(),
        CommandIntent::new(
            AUTOMATION_COMMAND,
            serde_json::to_value(request).expect("workflow request encodes"),
        ),
    )
    .await
    .expect("workflow command route returns a response")
}

/// Read the folded workflow state out of a completed workflow response.
fn response_state(response: &CoreResponse) -> AutomationState {
    serde_json::from_value(response.output["state"].clone()).expect("workflow state is embedded")
}

#[tokio::test]
async fn cp_workflow_replay_never_reexecutes_completed_effect() {
    let events = Arc::new(MemoryEventLog::new());
    let core = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        Arc::new(NoRunner),
    );
    let context = workflow_context();
    core.synchronize_authority(&context, "cp24-replay-fixture.v1")
        .await
        .expect("install the project authority snapshot required by protected events");

    let registered = send_workflow(
        &core,
        &context,
        command_request(
            0,
            "cp24-register",
            AutomationCommand::RegisterDefinition {
                definition: replay_definition(),
            },
        ),
    )
    .await;
    assert_eq!(
        registered.status,
        ExecutionStatus::Completed,
        "{registered:?}"
    );

    let started = send_workflow(
        &core,
        &context,
        command_request(
            1,
            "cp24-start",
            AutomationCommand::Start {
                instance_id: "cp24-instance".to_owned(),
                definition_id: "cp24-replay".to_owned(),
                version: 1,
                inputs: BTreeMap::new(),
            },
        ),
    )
    .await;
    assert_eq!(started.status, ExecutionStatus::Completed, "{started:?}");

    let advance = command_request(
        2,
        "cp24-advance",
        AutomationCommand::Advance {
            instance_id: "cp24-instance".to_owned(),
        },
    );
    let first = send_workflow(&core, &context, advance.clone()).await;
    assert_eq!(first.status, ExecutionStatus::Completed, "{first:?}");
    let first_state = response_state(&first);
    let node = first_state.instances["cp24-instance"].nodes["effect"].clone();
    assert_eq!(node.attempt, 1, "the first Advance reserves one attempt");
    let dispatch_events = events
        .read_all()
        .await
        .expect("ledger")
        .into_iter()
        .filter(|event| event.request_id == node.execution_id)
        .count();
    assert!(
        dispatch_events > 0,
        "the first Advance must leave committed dispatch evidence"
    );
    let ledger_after_first = events.read_all().await.expect("ledger").len();

    // Replaying the exact committed command consumes the recorded result instead of dispatching
    // the capability a second time: same state, same revision, no new ledger fact.
    let replay = send_workflow(&core, &context, advance).await;
    assert_eq!(replay.status, ExecutionStatus::Completed, "{replay:?}");
    assert_eq!(replay.output["replayed"], json!(true));
    assert_eq!(response_state(&replay), first_state);
    assert_eq!(
        events.read_all().await.expect("ledger").len(),
        ledger_after_first,
        "replay must not append a second dispatch fact"
    );

    // A fresh ControlPlane over the same ledger rebuilds the committed state without dispatch.
    let restarted = ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events.clone(),
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        Arc::new(NoRunner),
    );
    let snapshot = restarted
        .handle_command(
            context.clone(),
            CommandIntent::new(AUTOMATION_SNAPSHOT, json!({})),
        )
        .await
        .expect("snapshot route");
    assert_eq!(snapshot.status, ExecutionStatus::Completed, "{snapshot:?}");
    assert_eq!(response_state(&snapshot), first_state);
    assert_eq!(
        events.read_all().await.expect("ledger").len(),
        ledger_after_first,
        "replaying committed results must not re-execute effects"
    );
}

#[test]
fn cp_child_cannot_gain_parent_missing_scope_or_budget() {
    let now = 1_000_000_u64;
    let role = RoleSpec::builder();
    let mut template = AgentTemplate::for_role(&role, SWARM_CHILD_TEMPLATE);
    template.template_id = kiana_domain::swarm_child_template_id();
    template.max_depth = 1;
    let principal = PrincipalId::new();
    let project = ProjectId::new();
    let parent_grant = CapabilityGrant {
        schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
        grant_id: CapabilityGrantId::new(),
        capability: CapabilityKind::Other("coding".to_owned()),
        operation: "builder.packet".to_owned(),
        resources: vec!["workspace".to_owned()],
        paths: vec!["src".to_owned()],
        expires_at_unix_ms: now + 600_000,
        approval_id: None,
        delegation_allowed: true,
    };

    // Positive control: a packet inside the parent path derives a narrower contained grant.
    let mut contained = WorkPacket::builder_task("child-contained", "child inside parent scope")
        .with_path_allow(["src/lib.rs"]);
    contained.project_id = Some(project);
    contained.deadline_unix_ms = Some(now + 60_000);
    let derived = derive_swarm_child_grant(
        &parent_grant,
        &template,
        &role,
        &["src".to_owned()],
        &contained,
        principal,
        project,
        1,
        None,
        now,
    )
    .expect("contained child packet derives a narrower grant");
    assert!(parent_grant.contains(&derived));
    assert_eq!(derived.paths, vec!["src/lib.rs".to_owned()]);

    // A child cannot gain a path the parent grant never had.
    let mut widened = contained.clone();
    widened.path_allow = vec!["outside".to_owned()];
    assert_eq!(
        derive_swarm_child_grant(
            &parent_grant,
            &template,
            &role,
            &["src".to_owned()],
            &widened,
            principal,
            project,
            1,
            None,
            now,
        )
        .expect_err("a child cannot gain a path the parent grant lacks"),
        "scope_intersection_empty"
    );

    // The scheduler claim contract rejects the same widening before any lease exists.
    let budget = BudgetLeaseId::new();
    let mut parent_packet = approved_packet("parent", budget, "src");
    parent_packet.deadline_unix_ms = Some(now + 120_000);
    let mut child_packet = approved_packet("child", budget, "src/lib.rs");
    child_packet.parent_packet_id = Some(parent_packet.id.clone());
    child_packet.deadline_unix_ms = Some(now + 60_000);
    WorkflowQueueClaimContract::from_work_packet(
        &child_packet,
        &parent_packet,
        true,
        false,
        0,
        2,
        false,
        None,
        now + 300_000,
        now,
        WorkflowQueueClaimStatus::Ready,
    )
    .expect("the contained child remains claimable");

    let mut widened_packet = child_packet.clone();
    widened_packet.path_allow = vec!["outside".to_owned()];
    assert_eq!(
        WorkflowQueueClaimContract::from_work_packet(
            &widened_packet,
            &parent_packet,
            true,
            false,
            0,
            2,
            false,
            None,
            now + 300_000,
            now,
            WorkflowQueueClaimStatus::Ready,
        )
        .expect_err("a child claim cannot widen beyond the parent path scope"),
        "workflow_queue_scope_intersection_invalid"
    );

    let mut swapped_budget = child_packet.clone();
    swapped_budget.budget_lease_id = Some(BudgetLeaseId::new());
    assert_eq!(
        WorkflowQueueClaimContract::from_work_packet(
            &swapped_budget,
            &parent_packet,
            true,
            false,
            0,
            2,
            false,
            None,
            now + 300_000,
            now,
            WorkflowQueueClaimStatus::Ready,
        )
        .expect_err("a child claim cannot replace the parent budget lease"),
        "workflow_queue_budget_subset_invalid"
    );

    let mut missing_budget = child_packet.clone();
    missing_budget.budget_lease_id = None;
    assert_eq!(
        WorkflowQueueClaimContract::from_work_packet(
            &missing_budget,
            &parent_packet,
            true,
            false,
            0,
            2,
            false,
            None,
            now + 300_000,
            now,
            WorkflowQueueClaimStatus::Ready,
        )
        .expect_err("a child claim cannot drop the parent budget lease"),
        "workflow_queue_budget_subset_invalid"
    );

    let mut later_deadline = child_packet.clone();
    later_deadline.deadline_unix_ms = Some(now + 300_000);
    assert_eq!(
        WorkflowQueueClaimContract::from_work_packet(
            &later_deadline,
            &parent_packet,
            true,
            false,
            0,
            2,
            false,
            None,
            now + 600_000,
            now,
            WorkflowQueueClaimStatus::Ready,
        )
        .expect_err("a child claim cannot outlive the parent deadline"),
        "workflow_queue_budget_subset_invalid"
    );

    // The ready projection refuses to publish a claim for the widened child: no dispatch.
    let graph = BTreeMap::from([
        (parent_packet.id.clone(), parent_packet.clone()),
        (widened_packet.id.clone(), widened_packet.clone()),
    ]);
    assert_eq!(
        project_workflow_queue_ready(&graph, now, 2)
            .expect_err("a widened child never becomes dispatchable"),
        "workflow_queue_scope_intersection_invalid"
    );
}
