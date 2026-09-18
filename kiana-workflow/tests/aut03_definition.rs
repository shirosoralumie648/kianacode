use kiana_domain::{
    AutomationAuthority, AutomationCommand, AutomationProof, AutomationState, PermissionProfile,
    RequestContext, RequestId, RoleSpec, WorkflowDefinition, WorkflowNode, WorkflowNodeKind,
};
use kiana_workflow::{plan_command, validate_definition};
use serde_json::json;
use std::collections::BTreeMap;

fn definition() -> WorkflowDefinition {
    WorkflowDefinition {
        definition_id: "aut03-definition".to_owned(),
        version: 1,
        input_keys: Vec::new(),
        output_keys: vec!["result".to_owned()],
        allowed_roles: vec!["pm".to_owned()],
        max_duration_ms: 10_000,
        max_steps: 8,
        nodes: BTreeMap::from([(
            "start".to_owned(),
            WorkflowNode {
                dependencies: Vec::new(),
                kind: WorkflowNodeKind::Literal {
                    values: BTreeMap::from([("result".to_owned(), json!("ok"))]),
                },
                timeout_ms: 1_000,
                retry_limit: 0,
                compensation: None,
            },
        )]),
        artifacts: BTreeMap::new(),
    }
}

fn authority() -> AutomationAuthority {
    let mut context = RequestContext::local("aut03-session", "/aut03-project");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context.assign_role(&RoleSpec::pm());
    AutomationAuthority {
        context,
        now_ms: 100,
        execution_id: RequestId::new(),
        session_id: kiana_domain::SessionId::new("aut03-runtime"),
    }
}

#[test]
fn definition_digest_is_stable_and_tamper_bound() {
    let definition = definition();
    let digest = definition.digest();
    assert!(digest.starts_with("sha256:") && digest.len() == 71);
    definition.validate_digest(&digest).unwrap();
    let mut changed = definition.clone();
    changed.max_steps += 1;
    assert_eq!(
        changed.validate_digest(&digest).unwrap_err(),
        "workflow_definition_digest_mismatch"
    );
    assert_eq!(definition.digest(), definition.clone().digest());
}

#[test]
fn definition_validation_rejects_cycle_missing_artifact_and_bad_project() {
    let mut cycle = definition();
    cycle.nodes.insert(
        "cycle".to_owned(),
        WorkflowNode {
            dependencies: vec!["start".to_owned(), "cycle".to_owned()],
            kind: WorkflowNodeKind::Literal {
                values: BTreeMap::new(),
            },
            timeout_ms: 1_000,
            retry_limit: 0,
            compensation: None,
        },
    );
    assert_eq!(
        validate_definition(&cycle).unwrap_err(),
        "workflow_dependency_graph_invalid"
    );

    let mut missing_artifact = definition();
    missing_artifact.artifacts.insert(
        "artifact".to_owned(),
        kiana_domain::WorkflowArtifact {
            generated_by: "start".to_owned(),
            requires: vec!["missing".to_owned()],
            output_key: "out".to_owned(),
        },
    );
    assert_eq!(
        validate_definition(&missing_artifact).unwrap_err(),
        "workflow_artifact_dependency_missing"
    );

    let mut bad_project = definition();
    bad_project.nodes.insert(
        "agent".to_owned(),
        WorkflowNode {
            dependencies: vec!["start".to_owned()],
            kind: WorkflowNodeKind::AgentTask {
                project_id: "bad\nproject".to_owned(),
                packet_id: "packet".to_owned(),
                sandbox: None,
            },
            timeout_ms: 1_000,
            retry_limit: 0,
            compensation: None,
        },
    );
    assert_eq!(
        validate_definition(&bad_project).unwrap_err(),
        "workflow_packet_required"
    );
}

#[test]
fn register_definition_version_is_immutable_and_unknown_fields_fail() {
    let authority = authority();
    let proof = AutomationProof::default();
    let original = definition();
    let (state, effect) = plan_command(
        &AutomationState::default(),
        &AutomationCommand::RegisterDefinition {
            definition: original.clone(),
        },
        &authority,
        &proof,
    )
    .unwrap();
    assert!(effect.is_none());
    let mut changed = original;
    changed.output_keys = vec!["changed".to_owned()];
    assert_eq!(
        plan_command(
            &state,
            &AutomationCommand::RegisterDefinition {
                definition: changed
            },
            &authority,
            &proof,
        )
        .unwrap_err(),
        "workflow_definition_version_immutable"
    );

    let mut encoded = serde_json::to_value(definition()).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<WorkflowDefinition>(encoded).is_err());
}
