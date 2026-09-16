use kiana_domain::{
    CellId, CompanyAuthority, CompanyCommand, CompanyPacket, CompanyProof, CompanyState, Project,
    ProjectStatus, RequestId, SessionId, WorkPacket, WorkPacketStatus,
};

fn project() -> Project {
    Project {
        project_id: "project-1".to_owned(),
        organization_id: "org-1".to_owned(),
        objective_refs: vec!["objective-1".to_owned()],
        sponsor_id: "sponsor-1".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope-v1".to_owned(),
        success_criteria: vec!["criterion".to_owned()],
        non_goals: vec!["none".to_owned()],
        project_budget_ref: "budget-1".to_owned(),
        risk_summary: "none".to_owned(),
        decision_ref: Some("decision-1".to_owned()),
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: None,
        closing_receipt_id: None,
        status: ProjectStatus::Active,
        version: 1,
    }
}

fn expired_packet() -> WorkPacket {
    let mut packet = WorkPacket::builder_task("packet-1", "reclaim this packet");
    packet.status = WorkPacketStatus::Approved;
    packet.claim = Some(kiana_domain::PacketClaim {
        owner: CellId::new(),
        owner_session_id: SessionId::new("stale-worker"),
        lease_expires_at: 100,
        heartbeat_at: 50,
    });
    packet
}

#[test]
fn expired_lease_is_reclaimed_without_double_dispatch() {
    let mut state = CompanyState::default();
    state.projects.insert("project-1".to_owned(), project());
    state.packets.insert(
        "packet-1".to_owned(),
        CompanyPacket {
            project_id: "project-1".to_owned(),
            milestone_id: "milestone-1".to_owned(),
            packet: expired_packet(),
            version: 1,
        },
    );
    let authority = CompanyAuthority {
        actor_id: "builder".to_owned(),
        role_id: "builder".to_owned(),
        session_id: SessionId::new("reclaimer"),
        now_ms: 200,
        execution_request_id: RequestId::new(),
        execution_cell_id: Some(CellId::new()),
    };
    let command = CompanyCommand::ReclaimPacketClaim {
        packet_id: "packet-1".to_owned(),
    };
    let reclaimed = state
        .transition(&command, &authority, &CompanyProof::default())
        .unwrap();
    assert!(reclaimed.packets["packet-1"].packet.claim.is_none());

    // Replaying the same reclaim cannot manufacture another dispatch or a second claim removal.
    assert_eq!(
        reclaimed
            .transition(&command, &authority, &CompanyProof::default())
            .unwrap_err()
            .to_string(),
        "company_packet_not_claimed"
    );
    let ready = kiana_domain::ready_packets(&reclaimed.project_packets("project-1"), 200).unwrap();
    assert_eq!(ready.ready, vec!["packet-1"]);
}

#[test]
fn heartbeat_renewal_requires_the_current_owner_and_live_lease() {
    let mut claim = kiana_domain::PacketClaim {
        owner: CellId::new(),
        owner_session_id: SessionId::new("worker"),
        lease_expires_at: 1_000,
        heartbeat_at: 100,
    };
    let original = claim.clone();
    assert_eq!(
        claim.renew(CellId::new(), 200, None).unwrap_err(),
        "packet_claim_owner_mismatch"
    );
    assert_eq!(claim, original);
    claim.renew(original.owner, 200, None).unwrap();
    assert!(claim.lease_expires_at > claim.heartbeat_at);
    claim.lease_expires_at = 200;
    let snapshot = claim.clone();
    assert_eq!(
        claim.renew(original.owner, 201, None).unwrap_err(),
        "packet_claim_expired"
    );
    assert_eq!(claim, snapshot);
}
