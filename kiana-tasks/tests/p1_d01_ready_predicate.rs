use kiana_domain::{CellId, PacketClaim, WorkPacket, WorkPacketStatus};
use std::collections::BTreeMap;

#[test]
fn single_ready_predicate_agrees_across_three_callers() {
    let now = 10_000;
    let mut root = WorkPacket::builder_task("root", "root work");
    root.transition_status(WorkPacketStatus::Approved).unwrap();
    let mut child = WorkPacket::builder_task("child", "dependent work").with_dependencies(["root"]);
    child.transition_status(WorkPacketStatus::Approved).unwrap();
    let mut active_claim = WorkPacket::builder_task("active", "claimed work");
    active_claim
        .transition_status(WorkPacketStatus::Approved)
        .unwrap();
    active_claim.claim = Some(PacketClaim {
        owner: CellId::new(),
        owner_session_id: kiana_domain::SessionId::new("session-1"),
        lease_expires_at: now + 100,
        heartbeat_at: now,
    });
    let mut expired_claim = WorkPacket::builder_task("expired", "expired work");
    expired_claim
        .transition_status(WorkPacketStatus::Approved)
        .unwrap();
    expired_claim.claim = Some(PacketClaim {
        owner: CellId::new(),
        owner_session_id: kiana_domain::SessionId::new("session-2"),
        lease_expires_at: now - 1,
        heartbeat_at: now - 2,
    });

    let graph = BTreeMap::from([
        (root.id.clone(), root),
        (child.id.clone(), child),
        (active_claim.id.clone(), active_claim),
        (expired_claim.id.clone(), expired_claim),
    ]);
    let canonical = kiana_domain::ready_packets(&graph, now).unwrap();
    let legacy_adapter = kiana_tasks::ready_packets(&graph, now).unwrap();
    assert_eq!(canonical, legacy_adapter);
    assert_eq!(canonical.ready, vec!["expired", "root"]);
    assert_eq!(canonical.expired_claims, vec!["expired"]);
    assert_eq!(canonical.blocked["child"], "dependency_incomplete:root");
    assert_eq!(canonical.blocked["active"], "packet_claimed");
}
