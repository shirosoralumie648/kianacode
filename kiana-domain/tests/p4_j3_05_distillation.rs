use kiana_domain::{
    DistillationEvidence, DistillationVerdict, EventId, MemoryAdmission, MemoryDistillationJob,
    RequestId, RoleSpec, RunId, SessionId, MEMORY_DISTILLATION_SCHEMA,
};
use serde_json::json;

fn job() -> MemoryDistillationJob {
    let source_event_id = EventId::new();
    MemoryDistillationJob {
        schema: "kiana.memory-distillation-job.v1".to_owned(),
        job_id: source_event_id.to_string(),
        actor_id: "local-user".to_owned(),
        project_root: "/tmp/p4-j3-05".to_owned(),
        role_id: RoleSpec::pm().role_id,
        department_id: "planning".to_owned(),
        source_session_id: SessionId::new("planner-session"),
        source_run_id: Some(RunId::new()),
        source_event_id,
        source_request_id: RequestId::new(),
        kind: "lesson".to_owned(),
        evidence: vec![DistillationEvidence {
            event_id: source_event_id,
            request_id: RequestId::new(),
            run_id: Some(RunId::new()),
            kind: "run.completed".to_owned(),
            text: "the packet completed with bounded evidence".to_owned(),
        }],
        similar_records: vec![json!({
            "id": "old-lesson",
            "collection": "department:planning",
            "text": "keep evidence near the decision"
        })],
    }
}

#[test]
fn run_distillation_lands_as_lesson_candidate() {
    let job = job();
    let source = job.evidence[0].event_id;
    let output = json!({
        "schema": MEMORY_DISTILLATION_SCHEMA,
        "verdict": "retain",
        "reason": "bounded evidence is reusable",
        "lessons": [{
            "kind": "lesson",
            "text": "keep bounded evidence near the decision",
            "evidence": [{
                "event_id": source,
                "quote": "bounded evidence"
            }]
        }]
    });
    let (verdict, reason, proposal) = job
        .validate_output(&output.to_string())
        .expect("valid distillation output");
    assert_eq!(verdict, DistillationVerdict::Retain);
    assert_eq!(reason, "bounded evidence is reusable");
    let proposal = proposal.expect("lesson candidate");
    assert_eq!(proposal.admission_state, MemoryAdmission::Candidate);
    assert_eq!(proposal.facts[0].kind, "lesson");
    assert_eq!(proposal.facts[0].collection, "department:planning");
    assert_eq!(proposal.facts[0].evidence[0].event_id, source);
    assert_eq!(proposal.facts[0].evidence[0].quote, "bounded evidence");

    let mut bad_quote = output;
    bad_quote["lessons"][0]["evidence"][0]["quote"] = json!("not in source");
    assert_eq!(
        job.validate_output(&bad_quote.to_string()).unwrap_err(),
        "memory_distillation_quote_mismatch"
    );

    let discard = json!({
        "schema": MEMORY_DISTILLATION_SCHEMA,
        "verdict": "discard",
        "reason": "not reusable",
        "lessons": []
    });
    let (verdict, _, proposal) = job.validate_output(&discard.to_string()).unwrap();
    assert_eq!(verdict, DistillationVerdict::Discard);
    assert!(proposal.is_none());
}
