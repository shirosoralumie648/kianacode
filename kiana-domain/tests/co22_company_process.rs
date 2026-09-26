use kiana_domain::*;

fn template() -> CompanyProcessTemplate {
    CompanyProcessTemplate::default_v1()
}

fn state() -> CompanyProcessState {
    CompanyProcessState::new(
        "process-1",
        "project-1",
        "assignment:pm",
        Some("workflow-instance-1".to_owned()),
        &template(),
    )
    .expect("state")
}

fn refs(name: &str) -> Vec<String> {
    vec![format!("artifact:{name}")]
}

#[test]
fn company_process_cannot_execute_tools_or_complete_on_model_text() {
    let template = template();
    let initial = state();
    assert_eq!(
        plan_company_process(
            &initial,
            &template,
            &CompanyProcessEvent::PlanApproved {
                evidence_refs: refs("forged"),
            }
        )
        .unwrap_err(),
        "company_process_gate_not_open"
    );
    let forged = serde_json::json!({
        "type": "intake_accepted",
        "evidence_refs": ["artifact:intake"],
        "command": "shell.exec"
    });
    assert!(serde_json::from_value::<CompanyProcessEvent>(forged).is_err());
    let mut drifted = template.clone();
    drifted.template_hash = format!("sha256:{}", "f".repeat(64));
    assert_eq!(
        plan_company_process(
            &initial,
            &drifted,
            &CompanyProcessEvent::IntakeAccepted {
                evidence_refs: refs("intake"),
            }
        )
        .unwrap_err(),
        "company_process_template_drift"
    );
}

#[test]
fn same_business_events_produce_the_same_process_intents() {
    let template = template();
    let initial = state();
    let event = CompanyProcessEvent::IntakeAccepted {
        evidence_refs: refs("intake"),
    };
    let (left, left_intent) = plan_company_process(&initial, &template, &event).expect("left");
    let (right, right_intent) = plan_company_process(&initial, &template, &event).expect("right");
    assert_eq!(left, right);
    assert_eq!(left_intent, right_intent);
    let intent = left_intent.expect("intent");
    assert_eq!(intent.role_id, "sponsor");
    assert!(!intent.human_task);
    assert_eq!(
        intent.workflow_instance_id.as_deref(),
        Some("workflow-instance-1")
    );

    let (charter, _) = plan_company_process(
        &left,
        &template,
        &CompanyProcessEvent::CharterApproved {
            evidence_refs: refs("charter"),
        },
    )
    .expect("charter");
    let (plan, _) = plan_company_process(
        &charter,
        &template,
        &CompanyProcessEvent::PlanApproved {
            evidence_refs: refs("plan"),
        },
    )
    .expect("plan");
    let (handoff, _) = plan_company_process(
        &plan,
        &template,
        &CompanyProcessEvent::HandoffAcknowledged {
            evidence_refs: refs("handoff"),
        },
    )
    .expect("handoff");
    let (build, _) = plan_company_process(
        &handoff,
        &template,
        &CompanyProcessEvent::RunCompleted {
            evidence_refs: refs("run"),
        },
    )
    .expect("run");
    let (verify, verify_intent) = plan_company_process(
        &build,
        &template,
        &CompanyProcessEvent::ReviewAccepted {
            evidence_refs: refs("review"),
        },
    )
    .expect("review");
    let verify_intent = verify_intent.expect("human intent");
    assert_eq!(verify_intent.to_node, CompanyProcessNode::HumanApproval);
    assert!(verify_intent.human_task);
    assert_eq!(verify_intent.role_id, "sponsor");

    let (approval, approval_intent) = plan_company_process(
        &verify,
        &template,
        &CompanyProcessEvent::SponsorDecision {
            approved: true,
            evidence_refs: refs("sponsor-decision"),
        },
    )
    .expect("approval");
    assert_eq!(approval.node, CompanyProcessNode::Deliver);
    assert_eq!(approval_intent.expect("approval intent").role_id, "closer");
}

#[test]
fn process_replay_is_noop_and_state_reopens_with_workflow_association() {
    let template = template();
    let initial = state();
    let event = CompanyProcessEvent::IntakeAccepted {
        evidence_refs: refs("intake"),
    };
    let (next, _) = plan_company_process(&initial, &template, &event).expect("advance");
    let (replayed, intent) = plan_company_process(&next, &template, &event).expect("replay");
    assert_eq!(replayed, next);
    assert!(intent.is_none());
    let reopened: CompanyProcessState =
        serde_json::from_value(serde_json::to_value(&next).expect("serialize")).expect("reopen");
    assert_eq!(reopened, next);
    assert!(reopened.validate(&template).is_ok());
}
