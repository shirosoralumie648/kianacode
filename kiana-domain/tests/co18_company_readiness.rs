use kiana_domain::*;
use std::collections::{BTreeMap, BTreeSet};

fn approved(id: &str, dependencies: &[&str], inputs: &[&str]) -> WorkPacket {
    let mut packet = WorkPacket::builder_task(id, format!("{id} work"))
        .with_dependencies(dependencies.iter().copied())
        .with_path_allow(["src/"]);
    packet.inputs = inputs.iter().map(|input| (*input).to_owned()).collect();
    packet
        .transition_status(WorkPacketStatus::Approved)
        .expect("approved");
    packet
}

fn graph() -> BTreeMap<String, WorkPacket> {
    let mut root = approved("root", &[], &[]);
    root.status = WorkPacketStatus::Succeeded;
    let child = approved("child", &["root"], &["artifact:input"]);
    BTreeMap::from([(root.id.clone(), root), (child.id.clone(), child)])
}

fn context() -> CompanyReadinessContext {
    let mut context = CompanyReadinessContext {
        project_active: Some(true),
        ..Default::default()
    };
    context.packet_approved.insert("child".to_owned(), true);
    context
        .handoffs
        .insert("child".to_owned(), CompanyHandoffStatus::Acknowledged);
    context
        .assignments
        .insert("child".to_owned(), AssignmentReadiness::Active);
    context
        .required_input_versions
        .insert("artifact:input".to_owned(), 2);
    context
        .input_versions
        .insert("artifact:input".to_owned(), 2);
    context
        .dependency_requirements
        .entry("child".to_owned())
        .or_default()
        .insert("root".to_owned(), DependencyRequirement::RequiresAcceptance);
    context
        .dependency_outcomes
        .insert("root".to_owned(), DependencyOutcome::Accepted);
    context
}

#[test]
fn ready_packet_requires_the_declared_dependency_outcome() {
    let mut context = context();
    context
        .dependency_outcomes
        .insert("root".to_owned(), DependencyOutcome::Succeeded);
    let readiness = company_ready_packets(&graph(), 1_000, &context).expect("readiness");
    assert!(readiness.ready.is_empty());
    let blockers = &readiness.blockers["child"];
    assert!(blockers
        .iter()
        .any(|blocker| blocker.code == "dependency_acceptance_required"));
    assert!(blockers
        .iter()
        .any(|blocker| blocker.dependency_id.as_deref() == Some("root")));
}

#[test]
fn wrong_handoff_assignment_input_version_pause_and_change_are_explainable() {
    let mut context = context();
    context
        .handoffs
        .insert("child".to_owned(), CompanyHandoffStatus::Pending);
    context
        .assignments
        .insert("child".to_owned(), AssignmentReadiness::Expired);
    context
        .input_versions
        .insert("artifact:input".to_owned(), 1);
    context.pending_changes.insert("child".to_owned());
    context.project_active = Some(false);
    let readiness = company_ready_packets(&graph(), 1_000, &context).expect("readiness");
    let codes = readiness.blockers["child"]
        .iter()
        .map(|blocker| blocker.code.as_str())
        .collect::<BTreeSet<_>>();
    assert!(codes.contains("project_paused"));
    assert!(codes.contains("handoff_ack_required"));
    assert!(codes.contains("assignment_expired"));
    assert!(codes.contains("input_version_stale"));
    assert!(codes.contains("change_pending"));
    assert!(!readiness.ready.contains(&"child".to_owned()));
}

#[test]
fn one_snapshot_has_deterministic_ready_order_digest_and_no_claim_side_effect() {
    let packets = graph();
    let before = packets.clone();
    let readiness = company_ready_packets(&packets, 1_000, &context()).expect("readiness");
    assert_eq!(readiness.ready, vec!["child"]);
    assert!(readiness.blocked.is_empty());
    assert_eq!(readiness.digest, readiness.canonical_digest());
    assert_eq!(packets, before);

    let reopened: CompanyReadiness =
        serde_json::from_value(serde_json::to_value(&readiness).expect("serialize"))
            .expect("reopen");
    assert_eq!(reopened, readiness);
    assert!(reopened.validate().is_ok());
}
