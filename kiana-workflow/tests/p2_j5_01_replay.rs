use kiana_domain::{
    AutomationAuthority, AutomationCommand, AutomationProof, AutomationState, PermissionProfile,
    RequestContext, RequestId, RoleSpec, WorkflowDefinition, WorkflowInstanceStatus, WorkflowNode,
    WorkflowNodeKind,
};
use kiana_workflow::plan_command;
use serde_json::json;
use std::collections::BTreeMap;

fn fixture_definition() -> WorkflowDefinition {
    WorkflowDefinition {
        definition_id: "p2-j5-fixture".to_owned(),
        version: 1,
        input_keys: Vec::new(),
        output_keys: vec!["result".to_owned()],
        allowed_roles: vec!["pm".to_owned()],
        max_duration_ms: 1_000,
        max_steps: 4,
        nodes: BTreeMap::from([(
            "literal".to_owned(),
            WorkflowNode {
                dependencies: Vec::new(),
                kind: WorkflowNodeKind::Literal {
                    values: BTreeMap::from([("result".to_owned(), json!("fixture-ok"))]),
                },
                timeout_ms: 500,
                retry_limit: 1,
                compensation: None,
            },
        )]),
        artifacts: BTreeMap::new(),
    }
}

fn fixture_authority() -> AutomationAuthority {
    let mut context = RequestContext::local("p2-j5-session", "/p2-j5-project");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context.assign_role(&RoleSpec::lookup("pm").expect("pm role fixture"));
    AutomationAuthority {
        context,
        now_ms: 100,
        execution_id: RequestId::new(),
        session_id: kiana_domain::SessionId::new("p2-j5-runtime"),
    }
}

fn planned_state(
    definition: WorkflowDefinition,
    authority: &AutomationAuthority,
) -> AutomationState {
    let mut state = AutomationState::default();
    let proof = AutomationProof::default();
    let register = AutomationCommand::RegisterDefinition { definition };
    let (next, effect) = plan_command(&state, &register, authority, &proof).expect("register");
    assert!(effect.is_none());
    state = next;
    let start = AutomationCommand::Start {
        instance_id: "fixture-instance".to_owned(),
        definition_id: "p2-j5-fixture".to_owned(),
        version: 1,
        inputs: BTreeMap::new(),
    };
    let (next, effect) = plan_command(&state, &start, authority, &proof).expect("start");
    assert!(effect.is_none());
    state = next;
    let advance = AutomationCommand::Advance {
        instance_id: "fixture-instance".to_owned(),
    };
    let (state, effect) = plan_command(&state, &advance, authority, &proof).expect("advance");
    assert!(effect.is_none());
    state
}

#[test]
fn workflow_definition_replays_after_restart() {
    let authority = fixture_authority();
    let first = planned_state(fixture_definition(), &authority);
    let second = planned_state(fixture_definition(), &authority);
    assert_eq!(first, second);
    assert_eq!(first.revision, 3);
    let instance = first.instances.get("fixture-instance").expect("instance");
    assert_eq!(instance.definition_version, 1);
    assert_eq!(instance.status, WorkflowInstanceStatus::Succeeded);
    assert_eq!(instance.outputs["result"], json!("fixture-ok"));
}
