use kiana_domain::{
    json_digest, EventId, MemoryAdmission, MemoryEvidence, MemoryEvidenceQuote,
    MemoryExtractionRequest, MemoryFact, MemoryOrigin, MemoryProposal, MemorySuggestion, RequestId,
    RunId, SessionId, TurnId,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn request_parts() -> (RunId, TurnId, EventId, RequestId, MemoryExtractionRequest) {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let event_id = EventId::new();
    let request_id = RequestId::new();
    let quote = MemoryEvidenceQuote {
        event_id,
        request_id,
        run_id: Some(run_id),
        byte_start: 0,
        byte_end: 14,
        quote: "exact evidence".to_owned(),
        source_digest: digest("source"),
    };
    let request = MemoryExtractionRequest::new(
        run_id,
        turn_id,
        event_id,
        "extractor.v1",
        10,
        12,
        digest("scope"),
        vec![quote],
    )
    .unwrap();
    (run_id, turn_id, event_id, request_id, request)
}

#[test]
fn turn_extraction_is_idempotent_and_evidence_bounded() {
    let (run_id, turn_id, event_id, request_id, first) = request_parts();
    let second = MemoryExtractionRequest::new(
        run_id,
        turn_id,
        event_id,
        "extractor.v1",
        10,
        12,
        digest("scope"),
        vec![MemoryEvidenceQuote {
            event_id,
            request_id,
            run_id: Some(run_id),
            byte_start: 0,
            byte_end: 14,
            quote: "exact evidence".to_owned(),
            source_digest: digest("source"),
        }],
    )
    .unwrap();
    assert_eq!(first.idempotency_key, second.idempotency_key);
    assert_eq!(first.request_digest, second.request_digest);

    let mut evidence = first.evidence.clone();
    for _ in 0..32 {
        let mut item = evidence[0].clone();
        item.event_id = EventId::new();
        item.request_id = RequestId::new();
        evidence.push(item);
    }
    assert_eq!(
        MemoryExtractionRequest::new(
            run_id,
            turn_id,
            event_id,
            "extractor.v1",
            10,
            12,
            digest("scope"),
            evidence,
        )
        .unwrap_err(),
        "memory_extraction_request_header_invalid"
    );
}

#[test]
fn invalid_quote_never_creates_proposal() {
    let (run_id, _turn_id, event_id, request_id, request) = request_parts();
    let proposal = MemoryProposal {
        schema: "kiana.memory-proposal.v1".to_owned(),
        id: "proposal:cm25".to_owned(),
        origin: MemoryOrigin::Model,
        admission_state: MemoryAdmission::Candidate,
        project_root: "/tmp/cm25-project".to_owned(),
        role_id: "builder".to_owned(),
        department_id: "executing".to_owned(),
        session_id: SessionId::new("session-cm25"),
        extractor: "extractor.v1".to_owned(),
        facts: vec![MemoryFact {
            kind: "fact".to_owned(),
            operation: MemorySuggestion::Add,
            collection: "project:code".to_owned(),
            text: "derived fact".to_owned(),
            evidence: vec![MemoryEvidence {
                event_id,
                request_id,
                run_id: Some(run_id),
                quote: "forged evidence".to_owned(),
            }],
            similar_records: Vec::new(),
            target_record_id: None,
        }],
    };
    assert_eq!(
        request.validate_proposal(&proposal).unwrap_err(),
        "memory_extraction_quote_mismatch"
    );
}
