use kiana_domain::{
    json_digest, DistillationEvidence, DistillationVerdict, EventId, MemoryAdmission,
    MemoryDistillationJob, MemoryDistillationSource, MemoryDistillationSourceStatus, RequestId,
    RoleSpec, RunId, SessionId, MEMORY_DISTILLATION_SCHEMA,
};
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

fn source(status: MemoryDistillationSourceStatus, event_kind: &str) -> MemoryDistillationSource {
    MemoryDistillationSource::run_terminal(
        status,
        event_kind,
        RunId::new(),
        EventId::new(),
        RequestId::new(),
        "planning",
        digest(event_kind),
    )
    .unwrap()
}

fn job(source: &MemoryDistillationSource) -> MemoryDistillationJob {
    MemoryDistillationJob {
        schema: "kiana.memory-distillation-job.v1".to_owned(),
        job_id: source.source_event_id.to_string(),
        actor_id: "local-user".to_owned(),
        project_root: "/tmp/cm26-project".to_owned(),
        role_id: RoleSpec::pm().role_id,
        department_id: source.department_id.clone(),
        source_session_id: SessionId::new("planner-session"),
        source_run_id: source.source_run_id,
        source_event_id: source.source_event_id,
        source_request_id: source.source_request_id,
        kind: "lesson".to_owned(),
        evidence: vec![DistillationEvidence {
            event_id: source.source_event_id,
            request_id: source.source_request_id,
            run_id: source.source_run_id,
            kind: source.event_kind.clone(),
            text: "bounded terminal evidence".to_owned(),
        }],
        similar_records: Vec::new(),
    }
}

fn retain_output(event_id: EventId) -> String {
    json!({
        "schema": MEMORY_DISTILLATION_SCHEMA,
        "verdict": "retain",
        "reason": "the bounded lesson is reusable",
        "lessons": [{
            "kind": "lesson",
            "text": "keep terminal evidence bounded",
            "evidence": [{"event_id": event_id, "quote": "terminal evidence"}]
        }]
    })
    .to_string()
}

#[test]
fn run_and_decision_sources_share_one_candidate_contract() {
    let confirmed = source(MemoryDistillationSourceStatus::Confirmed, "run.completed");
    let job = job(&confirmed);
    let (verdict, _, proposal) = job
        .validate_output_with_source(&retain_output(confirmed.source_event_id), &confirmed)
        .expect("confirmed source can produce a candidate");
    assert_eq!(verdict, DistillationVerdict::Retain);
    let proposal = proposal.expect("retain output creates a proposal");
    assert_eq!(proposal.admission_state, MemoryAdmission::Candidate);
    assert_eq!(proposal.facts[0].kind, "lesson");
    assert!(confirmed.validate_proposal_source(&proposal).is_ok());

    let decision = MemoryDistillationSource::published_decision(
        EventId::new(),
        RequestId::new(),
        "planning",
        "dec-cm26",
        digest("published-decision"),
    )
    .unwrap();
    assert!(decision.can_qualify_candidate());
}

#[test]
fn unknown_source_cannot_become_verified_lesson() {
    let unknown = source(
        MemoryDistillationSourceStatus::Unknown,
        "run.result_unknown",
    );
    let job = job(&unknown);
    assert!(!unknown.can_qualify_candidate());
    assert_eq!(
        job.validate_output_with_source(&retain_output(unknown.source_event_id), &unknown)
            .unwrap_err(),
        "memory_distillation_source_unknown"
    );
}
