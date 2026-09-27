use kiana_domain::{
    SwarmHandoffKind, SwarmHandoffRecord, SwarmHandoffStatus, SwarmPlanId, SWARM_HANDOFF_SCHEMA,
};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn record(kind: SwarmHandoffKind) -> SwarmHandoffRecord {
    let mut record = SwarmHandoffRecord {
        schema: SWARM_HANDOFF_SCHEMA.to_owned(),
        swarm_plan_id: SwarmPlanId::new(),
        sender_id: "parent-cell".to_owned(),
        recipient_id: "child-cell".to_owned(),
        kind,
        packet_ref: "packet:one".to_owned(),
        source_cursor: 3,
        status: if kind == SwarmHandoffKind::Incident {
            SwarmHandoffStatus::NeedsReconciliation
        } else {
            SwarmHandoffStatus::Sent
        },
        summary: "structured swarm handoff".to_owned(),
        evidence_refs: if matches!(
            kind,
            SwarmHandoffKind::Evidence | SwarmHandoffKind::Incident
        ) {
            vec!["artifact:evidence".to_owned()]
        } else {
            Vec::new()
        },
        ack_digest: (kind == SwarmHandoffKind::HandoffAck).then(|| D.to_owned()),
        symposium_round: (kind == SwarmHandoffKind::Symposium).then_some(1),
        symposium_max_rounds: (kind == SwarmHandoffKind::Symposium).then_some(3),
        authority_granted: false,
        record_digest: String::new(),
    };
    record.record_digest = kiana_domain::json_digest(&json!({
        "schema": record.schema,
        "swarm_plan_id": record.swarm_plan_id,
        "sender_id": record.sender_id,
        "recipient_id": record.recipient_id,
        "kind": record.kind,
        "packet_ref": record.packet_ref,
        "source_cursor": record.source_cursor,
        "status": record.status,
        "summary": record.summary,
        "evidence_refs": record.evidence_refs,
        "ack_digest": record.ack_digest,
        "symposium_round": record.symposium_round,
        "symposium_max_rounds": record.symposium_max_rounds,
        "authority_granted": record.authority_granted,
    }));
    record
}

#[test]
fn directed_handoff_ack_evidence_incident_and_bounded_symposium_validate() {
    for kind in [
        SwarmHandoffKind::WorkPacket,
        SwarmHandoffKind::HandoffAck,
        SwarmHandoffKind::StatusReport,
        SwarmHandoffKind::Evidence,
        SwarmHandoffKind::Incident,
        SwarmHandoffKind::Symposium,
    ] {
        record(kind).validate().unwrap();
    }
}

#[test]
fn broadcast_authority_and_symposium_bounds_fail_closed() {
    let mut broadcast = record(SwarmHandoffKind::WorkPacket);
    broadcast.recipient_id = "broadcast".to_owned();
    broadcast.record_digest = kiana_domain::json_digest(&json!({"tampered": true}));
    assert_eq!(broadcast.validate(), Err("swarm_handoff_record_invalid"));

    let mut authority = record(SwarmHandoffKind::StatusReport);
    authority.authority_granted = true;
    authority.record_digest = kiana_domain::json_digest(&json!({"tampered": true}));
    assert_eq!(authority.validate(), Err("swarm_handoff_record_invalid"));

    let mut symposium = record(SwarmHandoffKind::Symposium);
    symposium.symposium_max_rounds = Some(9);
    symposium.record_digest = kiana_domain::json_digest(&json!({"tampered": true}));
    assert_eq!(symposium.validate(), Err("swarm_handoff_record_invalid"));
}
