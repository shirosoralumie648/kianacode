use kiana_domain::{
    quoted_memory_proposal, DepartmentSpec, MemoryAdmission, RequestId, RoleSpec, RuntimeEvent,
    SessionId, Symposium,
};
use serde_json::json;

#[test]
fn department_resolutions_enter_the_department_memory_layer() {
    let departments = DepartmentSpec::catalog();
    assert_eq!(departments.len(), 5);

    for department in departments {
        assert!(
            department.can_convene,
            "{} cannot convene",
            department.department_id
        );
        let meeting = Symposium::department(
            &department.department_id,
            format!("meeting-{}", department.department_id),
            "choose a bounded next action",
            Symposium::DEFAULT_MAX_ROUNDS,
        )
        .expect("department meeting");
        meeting.validate().expect("meeting contract");
        let chair = RoleSpec::lookup(&meeting.chair).expect("department chair");
        assert_eq!(chair.department_id, department.department_id);

        let source = RuntimeEvent::new(
            RequestId::new(),
            1,
            "symposium.closed",
            json!({
                "department_id": department.department_id,
                "decision": {"summary": "retain the bounded decision with evidence"}
            }),
        )
        .expect("decision source");
        let session = SessionId::new(meeting.speaker_session_id(&meeting.chair));
        let proposal = quoted_memory_proposal(
            &source,
            (
                "/tmp/p4-e03",
                &chair.role_id,
                &department.department_id,
                &session,
            ),
            None,
            "decision",
            "retain the bounded decision with evidence",
            std::slice::from_ref(&source),
        )
        .expect("proposal derivation")
        .expect("non-empty decision candidate");

        assert_eq!(proposal.admission_state, MemoryAdmission::Candidate);
        assert_eq!(proposal.facts.len(), 1);
        let fact = &proposal.facts[0];
        assert_eq!(fact.kind, "decision");
        assert_eq!(
            fact.collection,
            format!("department:{}", department.department_id)
        );
        assert_eq!(fact.evidence[0].event_id, source.event_id);
        assert_eq!(
            fact.evidence[0].quote,
            "retain the bounded decision with evidence"
        );
        assert!(!proposal.facts[0].text.is_empty());
    }
}
