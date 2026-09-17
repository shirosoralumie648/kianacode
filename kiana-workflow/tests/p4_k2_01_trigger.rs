use kiana_domain::{
    AutomationAuthority, AutomationCommand, AutomationProof, AutomationState, CapabilityKind,
    CapabilityRequest, RequestContext, RequestId, RiskLevel, TriggerConcurrency, TriggerDefinition,
    TriggerSchedule, WorkflowDefinition, WorkflowNode, WorkflowNodeKind,
};
use kiana_workflow::{definition_key, plan_command};
use serde_json::json;
use std::collections::BTreeMap;

fn authority() -> AutomationAuthority {
    let mut context = RequestContext::local("trigger-session", "/repo");
    context.project_trusted = true;
    context.assign_role(&kiana_domain::RoleSpec::sponsor());
    AutomationAuthority {
        context,
        now_ms: 1_000,
        execution_id: RequestId::new(),
        session_id: kiana_domain::SessionId::new("workflow-execution"),
    }
}

fn definition() -> WorkflowDefinition {
    WorkflowDefinition {
        definition_id: "trigger-workflow".to_owned(),
        version: 1,
        input_keys: Vec::new(),
        output_keys: Vec::new(),
        allowed_roles: vec!["sponsor".to_owned()],
        max_duration_ms: 60_000,
        max_steps: 4,
        nodes: BTreeMap::from([(
            "capability-node".to_owned(),
            WorkflowNode {
                dependencies: Vec::new(),
                kind: WorkflowNodeKind::Capability {
                    request: CapabilityRequest::new(
                        RequestId::new(),
                        CapabilityKind::Process,
                        "process.start",
                        json!({"command":"true"}),
                    )
                    .with_risk(RiskLevel::ReadOnly),
                },
                timeout_ms: 10_000,
                retry_limit: 0,
                compensation: None,
            },
        )]),
        artifacts: BTreeMap::new(),
    }
}

fn trigger() -> TriggerDefinition {
    TriggerDefinition {
        trigger_id: "trigger-one".to_owned(),
        definition_id: "trigger-workflow".to_owned(),
        definition_version: 1,
        inputs: BTreeMap::new(),
        owner_id: "local-user".to_owned(),
        role_id: "sponsor".to_owned(),
        expires_at: 100_000,
        max_firings: 2,
        concurrency: TriggerConcurrency::Reject,
        missed_schedule: kiana_domain::MissedSchedulePolicy::Skip,
        schedule: TriggerSchedule::Manual,
        approval_ref: "event:approval".to_owned(),
    }
}

#[test]
fn trigger_cannot_execute_a_capability_directly() {
    let authority = authority();
    let state = AutomationState::default();
    let proof = AutomationProof::default();
    let (state, effect) = plan_command(
        &state,
        &AutomationCommand::RegisterDefinition {
            definition: definition(),
        },
        &authority,
        &proof,
    )
    .expect("definition registration");
    assert!(effect.is_none());
    assert!(state
        .definitions
        .contains_key(&definition_key("trigger-workflow", 1)));

    let (state, effect) = plan_command(
        &state,
        &AutomationCommand::RegisterTrigger { trigger: trigger() },
        &authority,
        &AutomationProof {
            evidence_refs: vec!["event:approval".to_owned()],
            ..AutomationProof::default()
        },
    )
    .expect("trigger registration");
    assert!(effect.is_none());
    assert!(state.triggers.contains_key("trigger-one"));

    let (state, effect) = plan_command(
        &state,
        &AutomationCommand::Fire {
            trigger_id: "trigger-one".to_owned(),
            firing_key: "manual-1".to_owned(),
            event_ref: None,
        },
        &authority,
        &AutomationProof::default(),
    )
    .expect("manual fire");
    assert!(
        effect.is_none(),
        "Fire must only create a workflow instance"
    );
    assert_eq!(state.triggers["trigger-one"].firings_used, 1);
    let instance_id = "trigger:trigger-one:1";
    assert_eq!(
        state.instances[instance_id].trigger_id.as_deref(),
        Some("trigger-one")
    );

    let second_fire = plan_command(
        &state,
        &AutomationCommand::Fire {
            trigger_id: "trigger-one".to_owned(),
            firing_key: "manual-2".to_owned(),
            event_ref: None,
        },
        &authority,
        &AutomationProof::default(),
    )
    .unwrap_err();
    assert_eq!(second_fire, "trigger_already_running");

    let (advanced, effect) = plan_command(
        &state,
        &AutomationCommand::Advance {
            instance_id: instance_id.to_owned(),
        },
        &authority,
        &AutomationProof::default(),
    )
    .expect("workflow advance");
    assert!(matches!(
        effect,
        Some(kiana_domain::WorkflowEffect::Dispatch {
            kind: WorkflowNodeKind::Capability { .. },
            ..
        })
    ));
    assert!(advanced.instances[instance_id]
        .nodes
        .contains_key("capability-node"));
}
