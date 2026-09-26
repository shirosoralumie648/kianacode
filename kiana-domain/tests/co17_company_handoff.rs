use kiana_domain::*;

fn packet() -> HandoffPacketRevision {
    let packet = DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_owned(),
        packet_id: "packet-1".to_owned(),
        project_id: "project-1".to_owned(),
        version: 3,
        kind: DepartmentPacketKind::Implementation,
        from_department: "executing".to_owned(),
        target_role: "builder".to_owned(),
        assignment_ref: "assignment:builder".to_owned(),
        input_basis: PacketInputBasis::Plan,
        input_refs: vec!["artifact:plan".to_owned(), "artifact:scope".to_owned()],
        result: ResultContract {
            schema: RESULT_CONTRACT_SCHEMA.to_owned(),
            version: 1,
            output_schema: "kiana.implementation-result.v1".to_owned(),
            required_refs: vec!["artifact:result".to_owned()],
            max_bytes: 16_384,
        },
        write_scope: vec!["src/".to_owned()],
        plan_ref: Some("plan:1".to_owned()),
        runtime_grant: None,
        budget_lease: None,
    };
    let mut frozen = HandoffPacketRevision::new(packet, String::new(), "budget:project-1");
    frozen.packet_digest = frozen.canonical_packet_digest();
    frozen
}

fn assignment(
    id: &str,
    version: u64,
    principal: &str,
    department: &str,
    role: &str,
    session: &str,
) -> HandoffAssignment {
    HandoffAssignment {
        assignment_id: id.to_owned(),
        assignment_version: version,
        principal_id: principal.to_owned(),
        project_id: "project-1".to_owned(),
        department_id: department.to_owned(),
        role_id: role.to_owned(),
        session_id: SessionId::new(session),
        valid_from: 1,
        expires_at: 10_000,
        revoked: false,
    }
}

fn handoff() -> CompanyHandoff {
    CompanyHandoff::offer(
        "handoff-1",
        packet(),
        assignment(
            "assignment:pm",
            4,
            "pm-principal",
            "planning",
            "pm",
            "session:pm",
        ),
        assignment(
            "assignment:builder",
            7,
            "builder-principal",
            "executing",
            "builder",
            "session:builder",
        ),
        100,
        500,
    )
    .expect("handoff")
}

fn evidence(handoff: &CompanyHandoff) -> HandoffAcceptanceEvidence {
    HandoffAcceptanceEvidence {
        packet_digest: handoff.packet.packet_digest.clone(),
        input_refs: handoff.packet.packet.input_refs.clone(),
        budget_ref: handoff.packet.budget_ref.clone(),
        write_scope: handoff.packet.packet.write_scope.clone(),
    }
}

#[test]
fn handoff_ack_rejects_wrong_recipient_revision_and_expired_assignment() {
    let original = handoff();
    let mut ledger = CompanyHandoffLedger::default();
    ledger.offer(original.clone()).expect("offer");

    let mut wrong_recipient = original.recipient.clone();
    wrong_recipient.session_id = SessionId::new("session:other");
    assert_eq!(
        ledger
            .acknowledge(
                "handoff-1",
                &wrong_recipient,
                true,
                "wrong recipient",
                evidence(&original),
                200,
            )
            .unwrap_err(),
        "handoff_receiver_assignment_invalid"
    );
    assert_eq!(
        ledger.projection("handoff-1").unwrap().status,
        CompanyHandoffStatus::Pending
    );

    let mut wrong_basis = evidence(&original);
    wrong_basis.packet_digest = format!("sha256:{}", "f".repeat(64));
    assert_eq!(
        ledger
            .acknowledge(
                "handoff-1",
                &original.recipient,
                true,
                "stale packet",
                wrong_basis,
                200,
            )
            .unwrap_err(),
        "handoff_acceptance_evidence_mismatch"
    );

    let mut expired_assignment = original.recipient.clone();
    expired_assignment.expires_at = 200;
    assert_eq!(
        ledger
            .acknowledge(
                "handoff-1",
                &expired_assignment,
                true,
                "expired assignment",
                evidence(&original),
                250,
            )
            .unwrap_err(),
        "handoff_receiver_assignment_invalid"
    );
    assert_eq!(
        ledger
            .projection("handoff-1")
            .unwrap()
            .current_owner_assignment_id,
        "assignment:pm"
    );
}

#[test]
fn reject_or_timeout_returns_to_owner_with_escalation_and_resend_is_idempotent() {
    let original = handoff();
    let mut ledger = CompanyHandoffLedger::default();
    ledger.offer(original.clone()).expect("offer");
    ledger
        .acknowledge(
            "handoff-1",
            &original.recipient,
            false,
            "scope is incomplete",
            evidence(&original),
            200,
        )
        .expect("reject");
    let rejected = ledger.projection("handoff-1").unwrap();
    assert_eq!(rejected.status, CompanyHandoffStatus::Rejected);
    assert_eq!(rejected.current_owner_assignment_id, "assignment:pm");
    assert!(rejected.escalation_id.is_some());

    // Replaying the same offer cannot create a second transfer or clear the rejection.
    ledger.offer(original).expect("idempotent resend");
    assert_eq!(
        ledger.projection("handoff-1").unwrap().status,
        CompanyHandoffStatus::Rejected
    );

    let mut timed_out = CompanyHandoffLedger::default();
    let pending = handoff();
    timed_out.offer(pending).expect("offer");
    timed_out.expire("handoff-1", 500).expect("expire");
    let expired = timed_out.projection("handoff-1").unwrap();
    assert_eq!(expired.status, CompanyHandoffStatus::Expired);
    assert_eq!(expired.current_owner_assignment_id, "assignment:pm");
    assert!(expired.escalation_id.is_some());
}

#[test]
fn accepted_ack_changes_accountability_once_without_acquiring_runtime_lease() {
    let original = handoff();
    let mut state = CompanyState::default();
    state
        .offer_accountability_handoff(original.clone())
        .expect("offer");
    state
        .acknowledge_accountability_handoff(
            "handoff-1",
            &original.recipient,
            true,
            "scope and budget verified",
            evidence(&original),
            200,
        )
        .expect("ack");
    let projection = state
        .accountability_handoff_projection("handoff-1")
        .expect("projection");
    assert_eq!(projection.status, CompanyHandoffStatus::Acknowledged);
    assert_eq!(projection.current_owner_assignment_id, "assignment:builder");
    assert_eq!(projection.pending_recipient_assignment_id, None);
    assert_eq!(
        state.accountability_handoffs.handoffs["handoff-1"].runtime_lease_id,
        None
    );
    assert_eq!(
        state
            .acknowledge_accountability_handoff(
                "handoff-1",
                &original.recipient,
                true,
                "duplicate",
                evidence(&original),
                210,
            )
            .unwrap_err(),
        "handoff_not_pending_or_expired"
    );

    let reopened: CompanyState =
        serde_json::from_value(serde_json::to_value(&state).expect("serialize")).expect("reopen");
    assert_eq!(
        reopened.accountability_handoff_projection("handoff-1"),
        state.accountability_handoff_projection("handoff-1")
    );
}
