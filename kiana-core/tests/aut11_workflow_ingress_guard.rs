//! AUT-11 source guard for event -> occurrence -> Fire and no payload-to-capability shortcut.

#[test]
fn ingress_requires_existing_workflow_evidence_route() {
    let domain = include_str!("../../kiana-domain/src/workflow_event_ingress.rs");
    let daemon = include_str!("../../kiana-daemon/src/workflow_ingress.rs");
    let core = include_str!("../src/automation.rs");
    for marker in [
        "WorkflowEventIngress",
        "WorkflowEventSourcePolicy",
        "WorkflowEventOccurrence",
        "payload_filter",
        "occurrence_key",
        "to_fire_command",
    ] {
        assert!(
            domain.contains(marker),
            "AUT-11 domain marker missing: {marker}"
        );
    }
    for marker in [
        "source_not_allowlisted",
        "signature_invalid",
        "dedupe_payload_conflict",
        "hmac::verify",
    ] {
        assert!(
            daemon.contains(marker),
            "AUT-11 daemon marker missing: {marker}"
        );
    }
    assert!(core.contains("workflow_event_reference_required"));
    assert!(core.contains("workflow_evidence_not_found"));
    assert!(!domain.contains("CapabilityRequest"));
    assert!(!daemon.contains("CapabilityBrokerPort::execute"));
}
