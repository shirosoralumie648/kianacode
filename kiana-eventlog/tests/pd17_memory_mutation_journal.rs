//! PD-17 event-store contract for the Memory mutation journal.
//!
//! A `memory.fact` row is appended to one idempotent stream keyed by
//! `memory.fact:<stream id>:<mutation key>`.  A retried append must replay the committed fact
//! instead of creating a second row; a changed payload under the same key, a stale expected
//! version and a duplicate mutation key must all fail closed rather than become a second fact.
//! The JSONL reopen fixture additionally proves the same replay and rebuild behavior survives a
//! close/reopen of the durable adapter.

use kiana_domain::{
    json_digest, project_memory_facts, AuthenticatedPrincipalRef, EventId, EvidenceStatus,
    MemoryAdmission, MemoryBodyRef, MemoryCollection, MemoryEvidence, MemoryJournalFact,
    MemoryMutation, MemoryMutationAuthority, MemoryMutationJournalStage, MemoryMutationOperation,
    MemoryMutationTarget, MemoryOrigin, MemoryRecord, MemoryScope, MemorySensitivity, MemoryState,
    ProjectIdentity, Purpose, RequestId, RuntimeEvent, SourceKind, SourceRef,
    MEMORY_FACT_EVENT_KIND, MEMORY_RECORD_SCHEMA_V2, MEMORY_STREAM,
};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const STREAM: &str = "sha256:pd17-eventlog";

fn content_hash(text: &str) -> String {
    json_digest(&json!({"text": text}))
        .trim_start_matches("sha256:")
        .to_owned()
}

fn scope() -> MemoryScope {
    MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        ProjectIdentity::new(
            "/tmp/pd17-eventlog",
            "/tmp/pd17-eventlog",
            Some(1),
            Some(2),
            "trust",
        )
        .unwrap(),
        "pd17-eventlog-session",
        vec![MemoryCollection::parse("project:code").unwrap()],
        Purpose {
            id: "memory.mutation".to_owned(),
            description: "PD-17 eventlog fixture".to_owned(),
        },
        true,
    )
    .unwrap()
}

fn record(id: &str, text: &str, revision: u64, key: &str) -> MemoryRecord {
    MemoryRecord {
        project_root: "/tmp/pd17-eventlog".to_owned(),
        schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: id.to_owned(),
        layer: "project".to_owned(),
        collection: "project:code".to_owned(),
        text: text.to_owned(),
        source: "event:pd17-eventlog".to_owned(),
        role_id: "builder".to_owned(),
        department_id: "executing".to_owned(),
        session_id: "pd17-eventlog-session".to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        content_hash: content_hash(text),
        origin: MemoryOrigin::Model,
        admission_state: MemoryAdmission::Candidate,
        state: MemoryState::Draft,
        classification: kiana_domain::MemoryClassification::Project,
        purpose: Some(Purpose {
            id: "memory.candidate".to_owned(),
            description: "PD-17 eventlog fixture".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        revision,
        last_mutation_key: Some(key.to_owned()),
        ..MemoryRecord::default()
    }
}

fn approved(mut record: MemoryRecord, reviewer: &str, key: &str) -> MemoryRecord {
    record.revision = 2;
    record.admission_state = MemoryAdmission::Qualified;
    record.state = MemoryState::Active;
    record.evidence = vec![MemoryEvidence {
        event_id: EventId::new(),
        request_id: RequestId::new(),
        run_id: None,
        quote: record.text.clone(),
    }];
    record.purpose = Some(Purpose {
        id: "memory.review".to_owned(),
        description: "PD-17 eventlog approval".to_owned(),
    });
    record.reviewed_by = Some(reviewer.to_owned());
    record.reviewed_at_ms = Some(2);
    record.last_mutation_key = Some(key.to_owned());
    record
}

fn mutation(
    record: &MemoryRecord,
    operation: MemoryMutationOperation,
    authority: MemoryMutationAuthority,
    expected_revision: u64,
    key: &str,
) -> MemoryMutation {
    MemoryMutation::new(
        format!("mutation:{key}"),
        operation,
        "local-user",
        scope(),
        vec![MemoryMutationTarget::new(
            &record.id,
            &record.collection,
            expected_revision,
        )],
        vec![SourceRef::new(
            "event:pd17-eventlog",
            SourceKind::Event,
            "event://pd17-eventlog",
            "cursor:1",
            json_digest(&json!({"stream": STREAM})),
            Some(1),
            EvidenceStatus::Attributed,
        )
        .unwrap()],
        1,
        1,
        key,
        &json!({"key": key}),
    )
    .unwrap()
    .with_authority(authority)
    .unwrap()
}

fn fact(
    operation: &str,
    record: &MemoryRecord,
    mutation: MemoryMutation,
    stage: MemoryMutationJournalStage,
) -> MemoryJournalFact {
    MemoryJournalFact::new(
        operation,
        mutation.idempotency_key.clone(),
        record.clone(),
        MemoryBodyRef::new(STREAM, record.content_hash.clone()),
    )
    .with_mutation(mutation, stage)
    .unwrap()
}

fn event(request: RequestId, version: u64, key: &str, fact: MemoryJournalFact) -> RuntimeEvent {
    RuntimeEvent::new(
        request,
        1,
        MEMORY_FACT_EVENT_KIND,
        serde_json::to_value(fact).unwrap(),
    )
    .unwrap()
    .with_stream_metadata(MEMORY_STREAM, STREAM, version)
    .with_idempotency_key(key)
}

/// Candidate fact plus the approval fact that promotes it, as the daemon commits them.
fn journal_events(request: RequestId) -> (RuntimeEvent, RuntimeEvent) {
    let candidate = record("memory-1", "candidate body", 1, "pd17:eventlog:candidate");
    let candidate_event = event(
        request,
        1,
        "memory.fact:pd17-eventlog:candidate",
        fact(
            "write",
            &candidate,
            mutation(
                &candidate,
                MemoryMutationOperation::Add,
                MemoryMutationAuthority::Agent,
                0,
                "pd17:eventlog:candidate",
            ),
            MemoryMutationJournalStage::Candidate,
        ),
    );
    let approved = approved(candidate, "operator", "pd17:eventlog:approve");
    let approve_event = event(
        request,
        2,
        "memory.fact:pd17-eventlog:approve",
        fact(
            "review",
            &approved,
            mutation(
                &approved,
                MemoryMutationOperation::Approve,
                MemoryMutationAuthority::Human,
                1,
                "pd17:eventlog:approve",
            ),
            MemoryMutationJournalStage::Approve,
        ),
    );
    (candidate_event, approve_event)
}

#[tokio::test]
async fn replayed_memory_fact_is_not_a_second_row_and_changed_or_stale_retries_conflict() {
    let store = MemoryEventLog::new();
    let request = RequestId::new();
    let (candidate_event, approve_event) = journal_events(request);

    let committed = store
        .append_idempotent_expected(candidate_event.clone(), Some(0))
        .await
        .unwrap();
    assert!(!committed.replayed);
    assert_eq!(committed.event.stream_version, Some(1));

    // A retried append replays the committed fact even after the stream version advanced: the
    // original event identity and version come back and no second row is created.
    let replay = store
        .append_idempotent_expected(candidate_event.clone(), Some(99))
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.event.event_id, candidate_event.event_id);
    assert_eq!(replay.event.stream_version, Some(1));
    assert_eq!(
        store
            .read_stream(MEMORY_STREAM, STREAM)
            .await
            .unwrap()
            .len(),
        1
    );

    let committed_approve = store
        .append_idempotent_expected(approve_event.clone(), Some(1))
        .await
        .unwrap();
    assert!(!committed_approve.replayed);
    assert_eq!(committed_approve.event.stream_version, Some(2));

    // The same fact key with a changed payload is a conflict, not a new revision.
    let mut changed = approve_event.clone();
    changed.data["record"]["text"] = json!("changed body");
    let conflict = store
        .append_idempotent_expected(changed, None)
        .await
        .unwrap_err();
    assert_eq!(
        conflict,
        PortError::Conflict("event_idempotency_key_payload_mismatch".to_owned())
    );

    // A stale expected stream version cannot append a new fact either.
    let other = record("memory-2", "second body", 1, "pd17:eventlog:stale");
    let stale_event = event(
        request,
        3,
        "memory.fact:pd17-eventlog:stale",
        fact(
            "write",
            &other,
            mutation(
                &other,
                MemoryMutationOperation::Add,
                MemoryMutationAuthority::Agent,
                0,
                "pd17:eventlog:stale",
            ),
            MemoryMutationJournalStage::Candidate,
        ),
    );
    let stale = store
        .append_idempotent_expected(stale_event, Some(1))
        .await
        .unwrap_err();
    assert_eq!(
        stale,
        PortError::Conflict("event_stream_version_mismatch".to_owned())
    );
    assert_eq!(
        store
            .read_stream(MEMORY_STREAM, STREAM)
            .await
            .unwrap()
            .len(),
        2
    );

    let projection =
        project_memory_facts(&store.read_stream(MEMORY_STREAM, STREAM).await.unwrap()).unwrap();
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.records.len(), 1);
    assert_eq!(
        projection.records["memory-1"]["text"],
        json!("candidate body")
    );
    assert_eq!(projection.records["memory-1"]["origin"], json!("model"));
    assert_eq!(
        projection.records["memory-1"]["admission_state"],
        json!("qualified")
    );
    assert_eq!(projection.records["memory-1"]["state"], json!("active"));
    assert_eq!(projection.records["memory-1"]["revision"], json!(2));
    assert_eq!(projection.mutation_digests.len(), 2);
}

#[tokio::test]
async fn duplicate_mutation_keys_are_rejected_by_the_rebuild_not_silently_folded() {
    let store = MemoryEventLog::new();
    let request = RequestId::new();
    let first = record("memory-1", "first body", 1, "pd17:eventlog:duplicate");
    let second = record("memory-2", "second body", 1, "pd17:eventlog:duplicate");

    // Distinct fact keys mean the store cannot fold these two facts; the duplicate mutation key
    // must therefore be caught by the journal rebuild instead of becoming a second record.
    for (version, event_key, record) in [
        (1_u64, "memory.fact:pd17-eventlog:duplicate-1", &first),
        (2_u64, "memory.fact:pd17-eventlog:duplicate-2", &second),
    ] {
        let candidate_event = event(
            request,
            version,
            event_key,
            fact(
                "write",
                record,
                mutation(
                    record,
                    MemoryMutationOperation::Add,
                    MemoryMutationAuthority::Agent,
                    0,
                    "pd17:eventlog:duplicate",
                ),
                MemoryMutationJournalStage::Candidate,
            ),
        );
        store
            .append_idempotent_expected(candidate_event, Some(version - 1))
            .await
            .unwrap();
    }

    let stream = store.read_stream(MEMORY_STREAM, STREAM).await.unwrap();
    assert_eq!(stream.len(), 2);
    assert_eq!(
        project_memory_facts(&stream).unwrap_err(),
        "memory_mutation_journal_duplicate"
    );
}

fn temp_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd17-memory-journal-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
}

#[cfg(unix)]
#[tokio::test]
async fn jsonl_reopen_keeps_one_replayed_fact_and_rebuilds_the_record() {
    let path = temp_path();
    let request = RequestId::new();
    let (candidate_event, approve_event) = journal_events(request);
    {
        let store = JsonlEventLog::open(&path).unwrap();
        if !store.supports_atomic_transitions() {
            return;
        }
        assert!(
            !store
                .append_idempotent_expected(candidate_event.clone(), Some(0))
                .await
                .unwrap()
                .replayed
        );
        assert!(
            store
                .append_idempotent_expected(candidate_event, Some(0))
                .await
                .unwrap()
                .replayed
        );
        assert!(
            !store
                .append_idempotent_expected(approve_event, Some(1))
                .await
                .unwrap()
                .replayed
        );
        store.flush().await.unwrap();
    }

    let reopened = JsonlEventLog::open(&path).unwrap();
    let stream = reopened.read_stream(MEMORY_STREAM, STREAM).await.unwrap();
    assert_eq!(stream.len(), 2);
    let projection = project_memory_facts(&stream).unwrap();
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.records.len(), 1);
    assert_eq!(projection.records["memory-1"]["origin"], json!("model"));
    assert_eq!(
        projection.records["memory-1"]["admission_state"],
        json!("qualified")
    );
    assert_eq!(projection.records["memory-1"]["state"], json!("active"));
    assert_eq!(projection.records["memory-1"]["revision"], json!(2));
    assert_eq!(
        projection.records["memory-1"]["reviewed_by"],
        json!("operator")
    );
    assert_eq!(projection.mutation_digests.len(), 2);
    drop(reopened);
    cleanup(&path);
}
