use kiana_domain::{
    AutomationAuthority, AutomationCommand, AutomationProof, AutomationState, CoreResponse,
    ExecutionStatus, PermissionProfile, RequestContext, RequestId, RoleSpec, RuntimeReceiptRef,
    SessionId, WorkflowDefinition, WorkflowEffect, WorkflowIncident, WorkflowInstanceStatus,
    WorkflowNode, WorkflowNodeKind, WorkflowNodeStatus,
};
use kiana_workflow::plan_command;
use serde_json::json;
use std::collections::BTreeMap;

fn authority() -> AutomationAuthority {
    let mut context = RequestContext::local("er28-session", "/er28-project");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context.assign_role(&RoleSpec::lookup("pm").expect("pm role fixture"));
    AutomationAuthority {
        context,
        now_ms: 100,
        execution_id: RequestId::new(),
        session_id: SessionId::new("er28-runtime"),
    }
}

fn definition() -> WorkflowDefinition {
    WorkflowDefinition {
        definition_id: "er28-fixture".to_owned(),
        version: 1,
        input_keys: Vec::new(),
        output_keys: Vec::new(),
        allowed_roles: vec!["pm".to_owned()],
        max_duration_ms: 10_000,
        max_steps: 4,
        nodes: BTreeMap::from([(
            "run".to_owned(),
            WorkflowNode {
                dependencies: Vec::new(),
                kind: WorkflowNodeKind::AgentTask {
                    project_id: "project-1".to_owned(),
                    packet_id: "packet-1".to_owned(),
                    sandbox: None,
                },
                timeout_ms: 1_000,
                retry_limit: 1,
                compensation: None,
            },
        )]),
        artifacts: BTreeMap::new(),
    }
}

/// Fold the committed reservation facts (definition, start, advance) and return the
/// planner's optional dispatch intent. The intent is a request for Core, never an
/// executed effect.
fn committed_reservation(a: &AutomationAuthority) -> (AutomationState, Option<WorkflowEffect>) {
    let mut state = AutomationState::default();
    let (next, _) = plan_command(
        &state,
        &AutomationCommand::RegisterDefinition {
            definition: definition(),
        },
        a,
        &AutomationProof::default(),
    )
    .unwrap();
    state = next;
    let (next, _) = plan_command(
        &state,
        &AutomationCommand::Start {
            instance_id: "instance-1".to_owned(),
            definition_id: "er28-fixture".to_owned(),
            version: 1,
            inputs: BTreeMap::new(),
        },
        a,
        &AutomationProof::default(),
    )
    .unwrap();
    state = next;
    plan_command(
        &state,
        &AutomationCommand::Advance {
            instance_id: "instance-1".to_owned(),
        },
        a,
        &AutomationProof::default(),
    )
    .unwrap()
}

fn reserved_state(a: &AutomationAuthority) -> AutomationState {
    let (next, effect) = committed_reservation(a);
    assert!(effect.is_some());
    next
}

#[test]
fn unknown_runtime_requires_bound_incident_and_receipt() {
    let a = authority();
    let state = reserved_state(&a);
    let node = &state.instances["instance-1"].nodes["run"];
    assert_eq!(node.status, WorkflowNodeStatus::Reserved);
    let refs = vec!["event:runtime".to_owned()];
    let receipt = RuntimeReceiptRef::new(
        node.execution_id,
        ExecutionStatus::ResultUnknown,
        refs.clone(),
    )
    .unwrap();
    let incident = WorkflowIncident::new(
        "incident-er28",
        "instance-1",
        "run",
        node.execution_id,
        "workflow_runtime_result_unknown",
        refs.clone(),
    )
    .unwrap();
    let proof = AutomationProof {
        response: Some(CoreResponse {
            request_id: node.execution_id,
            status: ExecutionStatus::ResultUnknown,
            output: json!(null),
            error: Some("unknown".to_owned()),
        }),
        evidence_refs: refs,
        runtime_receipt: Some(receipt),
        incident: Some(incident),
        ..AutomationProof::default()
    };
    let (next, effect) = plan_command(
        &state,
        &AutomationCommand::RecordObservation {
            instance_id: "instance-1".to_owned(),
            node_id: "run".to_owned(),
        },
        &a,
        &proof,
    )
    .unwrap();
    assert!(effect.is_none());
    assert_eq!(
        next.instances["instance-1"].nodes["run"].status,
        WorkflowNodeStatus::ResultUnknown
    );
    assert_eq!(next.incidents.len(), 1);
    assert!(next.instances["instance-1"].nodes["run"]
        .incident_id
        .is_some());

    let mut missing = proof;
    missing.incident = None;
    assert_eq!(
        plan_command(
            &state,
            &AutomationCommand::RecordObservation {
                instance_id: "instance-1".to_owned(),
                node_id: "run".to_owned(),
            },
            &a,
            &missing,
        )
        .unwrap_err(),
        "workflow_incident_required"
    );
}

#[test]
fn workflow_replay_does_not_execute_effect() {
    let a = authority();
    let (state, effect) = committed_reservation(&a);
    let dispatch = effect.expect("advance returns a dispatch intent, not an executed effect");
    let WorkflowEffect::Dispatch {
        instance_id,
        node_id,
        execution_id,
        ..
    } = &dispatch
    else {
        panic!("agent task advance must return a dispatch intent");
    };
    assert_eq!(instance_id, "instance-1");
    assert_eq!(node_id, "run");
    assert_eq!(*execution_id, a.execution_id);

    // The planner only reserved the node: it never fabricates a runtime receipt,
    // incident, output or terminal state before Core observes the effect.
    let instance = &state.instances["instance-1"];
    assert!(!instance.status.terminal());
    let node = &instance.nodes["run"];
    assert_eq!(node.status, WorkflowNodeStatus::Reserved);
    assert!(node.runtime_receipt.is_none());
    assert!(node.incident_id.is_none());
    assert!(!node.output_recorded);
    assert!(state.incidents.is_empty());

    // Replaying the same committed facts is deterministic and returns the same intent.
    let (replayed, replayed_effect) = committed_reservation(&a);
    assert_eq!(state, replayed);
    assert_eq!(replayed_effect, Some(dispatch));

    // A repeated Advance against the committed reservation cannot dispatch a second effect.
    let duplicate = plan_command(
        &state,
        &AutomationCommand::Advance {
            instance_id: "instance-1".to_owned(),
        },
        &a,
        &AutomationProof::default(),
    );
    assert_eq!(duplicate.unwrap_err(), "workflow_execution_in_flight");

    // The pure planner has no execution surface at all.
    let source = include_str!("../src/durable.rs");
    for marker in [
        "KianaHarness",
        "CapabilityBroker",
        "tokio::spawn",
        "std::process::Command",
    ] {
        assert!(
            !source.contains(marker),
            "workflow planner must not execute effects: {marker}"
        );
    }
    assert!(source.contains("WorkflowEffect::Dispatch"));
}
