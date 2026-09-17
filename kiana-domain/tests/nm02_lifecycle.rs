use kiana_domain::*;
use serde_json::json;

#[test]
fn handoff_lifecycle_is_directed_and_ack_or_reject_is_terminal() {
    let handoff = CommunicationMessage::new(
        "handoff-1",
        CommunicationMessageKind::Handoff,
        "planner",
        Some("builder".to_owned()),
        "packet handoff",
        "Execute the frozen packet.",
    )
    .unwrap();
    let sent = CommunicationLifecycleEvent::new(
        &handoff,
        None,
        CommunicationLifecycleStatus::Sent,
        "planner",
        "",
        Vec::new(),
        1,
    )
    .unwrap();
    assert!(!sent.authority_granted);
    let acknowledged = CommunicationLifecycleEvent::new(
        &handoff,
        Some(CommunicationLifecycleStatus::Sent),
        CommunicationLifecycleStatus::Acknowledged,
        "builder",
        "accepted frozen packet",
        Vec::new(),
        2,
    )
    .unwrap();
    acknowledged.validate(&handoff).unwrap();
    assert_eq!(
        CommunicationLifecycleEvent::new(
            &handoff,
            Some(CommunicationLifecycleStatus::Acknowledged),
            CommunicationLifecycleStatus::Rejected,
            "builder",
            "late reject",
            Vec::new(),
            3,
        )
        .unwrap_err(),
        "communication_lifecycle_transition_invalid"
    );
    assert_eq!(
        CommunicationMessage::new(
            "handoff-2",
            CommunicationMessageKind::Handoff,
            "planner",
            None,
            "handoff",
            "missing recipient",
        )
        .unwrap_err(),
        "communication_handoff_ack_required"
    );
}

#[test]
fn message_kinds_never_grant_authority_and_incident_escalation_is_evidenced() {
    let command = CommunicationMessage::new(
        "command-1",
        CommunicationMessageKind::Command,
        "planner",
        Some("builder".to_owned()),
        "review",
        "request a review",
    )
    .unwrap()
    .with_action_ref("company.review")
    .unwrap();
    assert!(!command.grants_authority());
    let decision = CommunicationMessage::new(
        "decision-1",
        CommunicationMessageKind::Decision,
        "reviewer",
        Some("closer".to_owned()),
        "decision",
        "accept the evidence",
    )
    .unwrap()
    .with_action_ref("company.accept")
    .unwrap();
    assert!(!decision.grants_authority());
    let evidence = CommunicationMessage::new(
        "evidence-1",
        CommunicationMessageKind::Evidence,
        "builder",
        Some("reviewer".to_owned()),
        "evidence",
        "artifact is ready",
    )
    .unwrap();
    assert!(!evidence.grants_authority());

    let incident = CommunicationMessage::new(
        "incident-1",
        CommunicationMessageKind::Incident,
        "builder",
        Some("pm".to_owned()),
        "incident",
        "worker stopped unexpectedly",
    )
    .unwrap();
    let escalation = CommunicationLifecycleEvent::new(
        &incident,
        Some(CommunicationLifecycleStatus::Sent),
        CommunicationLifecycleStatus::Escalated,
        "builder",
        "needs operator review",
        vec!["evidence:run-1".to_owned()],
        2,
    )
    .unwrap();
    assert!(!escalation.authority_granted);
    assert_eq!(escalation.evidence_refs, vec!["evidence:run-1"]);

    let mut tampered = serde_json::to_value(&escalation).unwrap();
    tampered["unexpected"] = json!(true);
    assert!(serde_json::from_value::<CommunicationLifecycleEvent>(tampered).is_err());
}
