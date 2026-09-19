use kiana_domain::{
    AutomationAuthority, AutomationCommand, AutomationProof, AutomationState, CapabilityKind,
    CapabilityRequest, PermissionProfile, RequestContext, RequestId, RiskLevel, RoleSpec,
    WorkflowDefinition, WorkflowIntentKind, WorkflowNode, WorkflowNodeKind,
};
use kiana_workflow::{plan_command, plan_command_intent};
use serde_json::json;
use std::collections::BTreeMap;

fn authority() -> AutomationAuthority {
    let mut context = RequestContext::local("aut06-session", "/aut06-project");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context.assign_role(&RoleSpec::pm());
    AutomationAuthority {
        context,
        now_ms: 100,
        execution_id: RequestId::new(),
        session_id: kiana_domain::SessionId::new("aut06-runtime"),
    }
}

fn definition(kind: WorkflowNodeKind) -> WorkflowDefinition {
    WorkflowDefinition {
        definition_id: "aut06-definition".to_owned(),
        version: 1,
        input_keys: Vec::new(),
        output_keys: Vec::new(),
        allowed_roles: vec!["pm".to_owned()],
        max_duration_ms: 10_000,
        max_steps: 4,
        nodes: BTreeMap::from([(
            "node".to_owned(),
            WorkflowNode {
                dependencies: Vec::new(),
                kind,
                timeout_ms: 1_000,
                retry_limit: 0,
                compensation: None,
            },
        )]),
        artifacts: BTreeMap::new(),
    }
}

#[test]
fn planner_intent_distinguishes_terminal_reserve_and_dispatch_without_io() {
    let authority = authority();
    let proof = AutomationProof::default();
    let literal = definition(WorkflowNodeKind::Literal {
        values: BTreeMap::new(),
    });
    let (state, _) = plan_command(
        &AutomationState::default(),
        &AutomationCommand::RegisterDefinition {
            definition: literal,
        },
        &authority,
        &proof,
    )
    .unwrap();
    let (state, _) = plan_command(
        &state,
        &AutomationCommand::Start {
            instance_id: "literal".to_owned(),
            definition_id: "aut06-definition".to_owned(),
            version: 1,
            inputs: BTreeMap::new(),
        },
        &authority,
        &proof,
    )
    .unwrap();
    let (_next, intent, effect) = plan_command_intent(
        &state,
        &AutomationCommand::Advance {
            instance_id: "literal".to_owned(),
        },
        &authority,
        &proof,
    )
    .unwrap();
    assert_eq!(intent.kind, WorkflowIntentKind::Terminal);
    assert!(effect.is_none());
    intent.validate().unwrap();

    let dispatch = definition(WorkflowNodeKind::Capability {
        request: CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Process,
            "process.start",
            json!({"command":"true"}),
        )
        .with_risk(RiskLevel::ReadOnly),
    });
    let mut state = AutomationState::default();
    state = plan_command(
        &state,
        &AutomationCommand::RegisterDefinition {
            definition: dispatch,
        },
        &authority,
        &proof,
    )
    .unwrap()
    .0;
    state = plan_command(
        &state,
        &AutomationCommand::Start {
            instance_id: "dispatch".to_owned(),
            definition_id: "aut06-definition".to_owned(),
            version: 1,
            inputs: BTreeMap::new(),
        },
        &authority,
        &proof,
    )
    .unwrap()
    .0;
    let (_next, intent, effect) = plan_command_intent(
        &state,
        &AutomationCommand::Advance {
            instance_id: "dispatch".to_owned(),
        },
        &authority,
        &proof,
    )
    .unwrap();
    assert_eq!(intent.kind, WorkflowIntentKind::Dispatch);
    assert!(effect.is_some());
}
