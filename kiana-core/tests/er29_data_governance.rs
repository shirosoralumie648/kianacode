//! ER-29 deny fixtures: revoked data must not be returned by a receipt, and a deletion must not
//! leave an untracked projection copy behind.
//!
//! Deny-first: each refusal is asserted before the success path. A receipt that only carries
//! metadata, or that was authorized under an older epoch, must never return payload bytes; a
//! deletion must leave an index tombstone that denies reads on every projection surface.

use kiana_core::{
    project_memory_deletion_index, receipt_data_binding_from_events,
    receipt_redaction_is_not_authorization, seal_governance_events,
};
use kiana_domain::{
    DataPayloadState, DataPolicy, MemoryClassification, MemoryJournalFact, MemoryRecord, Purpose,
    RequestId, RuntimeEvent, MEMORY_FACT_EVENT_KIND, MEMORY_RECORD_SCHEMA_V2,
};
use serde_json::json;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

/// Memory body references are bare 64-hex digests, unlike the prefixed governance digests.
fn bare_digest(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn memory_fact(
    event_id_seed: u64,
    id: &str,
    project_root: &str,
    collection: &str,
    revision: u64,
) -> RuntimeEvent {
    let record = MemoryRecord {
        project_root: project_root.to_owned(),
        schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: id.to_owned(),
        layer: collection
            .split(':')
            .next()
            .unwrap_or(collection)
            .to_owned(),
        collection: collection.to_owned(),
        text: "er29 governed row".to_owned(),
        source: format!("event:{id}"),
        role_id: "builder".to_owned(),
        department_id: "executing".to_owned(),
        session_id: "er29-memory".to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        content_hash: bare_digest('a'),
        origin: kiana_domain::MemoryOrigin::Model,
        admission_state: kiana_domain::MemoryAdmission::Candidate,
        state: kiana_domain::MemoryState::Draft,
        classification: MemoryClassification::Project,
        purpose: Some(Purpose {
            id: "memory.candidate".to_owned(),
            description: "ER-29 fixture".to_owned(),
        }),
        sensitivity: kiana_domain::MemorySensitivity::Internal,
        revision,
        last_mutation_key: Some(format!("er29:{id}")),
        ..MemoryRecord::default()
    };
    let fact = MemoryJournalFact::new(
        "delete",
        format!("er29:{id}"),
        record,
        kiana_domain::MemoryBodyRef::new("memory-stream", bare_digest('a')),
    );
    // Surface the domain validation reason instead of letting the projection collapse it into
    // the generic invalid-fact error.
    fact.validate().unwrap();
    RuntimeEvent::new(
        RequestId::new(),
        1 + event_id_seed,
        MEMORY_FACT_EVENT_KIND,
        serde_json::to_value(fact).unwrap(),
    )
    .unwrap()
}

#[test]
fn receipt_binding_keeps_payload_unknown_without_governance_authorization() {
    let event = RuntimeEvent::new(RequestId::new(), 1, "run.completed", json!({"safe": true}))
        .unwrap()
        .with_redaction_metadata(digest('a'), false, Some(1), vec!["artifact:one".to_owned()]);
    let binding = receipt_data_binding_from_events(
        &digest('b'),
        "/project",
        &DataPolicy::default(),
        &[event],
    )
    .unwrap();
    assert_eq!(binding.payload_state, DataPayloadState::Unknown);
    assert!(receipt_redaction_is_not_authorization(&binding, 1).is_err());
    assert!(binding
        .redact_payload_refs()
        .unwrap()
        .payload_refs
        .is_empty());
}

/// ER-29 named deny fixture: a receipt must not return revoked data.
///
/// The binding is built from a source event whose artifact was later revoked. Even if a caller
/// marks the binding available locally, the receipt's recorded epoch no longer matches the
/// current data epoch, so `authorize_payload` refuses to release the bytes. Redaction removes the
/// refs but never grants a read.
#[test]
fn revoked_data_is_not_returned_by_receipt() {
    let source_epoch = 4;
    let mut policy = DataPolicy::default();
    policy.data_epoch = source_epoch;
    policy.refresh_digest();
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "artifact.committed",
        json!({"safe": true}),
    )
    .unwrap()
    .with_redaction_metadata(
        digest('a'),
        false,
        Some(source_epoch),
        vec!["artifact:revoked".to_owned()],
    );
    let mut binding =
        receipt_data_binding_from_events(&digest('b'), "/project", &policy, &[event]).unwrap();
    assert_eq!(binding.payload_refs.len(), 1);

    // Deny-first: the untouched binding is Unknown and cannot release payload bytes.
    assert_eq!(
        receipt_redaction_is_not_authorization(&binding, source_epoch).unwrap_err(),
        "receipt_payload_authorization_denied"
    );

    // A stale binding marked available under the old epoch is still refused: the revocation
    // advanced the live epoch, and the receipt only carries the epoch it was built at.
    binding.payload_state = DataPayloadState::Available;
    binding.binding_digest = binding.digest();
    assert!(binding.validate().is_ok());
    assert_eq!(
        receipt_redaction_is_not_authorization(&binding, source_epoch + 1).unwrap_err(),
        "receipt_payload_authorization_denied"
    );

    // Redaction is not authorization either: the redacted view has no refs to release.
    let redacted = binding.redact_payload_refs().unwrap();
    assert!(redacted.payload_refs.is_empty());
    assert!(receipt_redaction_is_not_authorization(&redacted, source_epoch).is_err());

    // Only the exact live epoch with refs and an Available state may release the payload.
    assert!(receipt_redaction_is_not_authorization(&binding, source_epoch).is_ok());
}

/// ER-29 named deny fixture: a deletion must not leave an untracked projection copy.
///
/// The deletion index for a project must track the tombstone on every retrieval surface, so a
/// projection copy that was rebuilt after the delete cannot be read back. A foreign-project fact
/// must not enter this index at all.
#[test]
fn deletion_does_not_leave_untracked_projection_copy() {
    let policy = DataPolicy::default();
    let foreign = memory_fact(0, "foreign-secret", "/other/project", "project:code", 3);
    let index = project_memory_deletion_index(&policy, "/this/project", &[foreign], 10).unwrap();
    assert!(
        index.entries.is_empty(),
        "foreign project deletion fact leaked into the index: {:?}",
        index.entries
    );
    // The index remains bound to the requested project, so a read surface for another root
    // still sees a project mismatch instead of inheriting this (empty) index.
    assert!(index.project_root_conflicts("/other/project"));

    // The same fact attributed to this project is tracked: the denial above is not a blanket
    // "ignore every delete".
    let local = memory_fact(0, "local-secret", "/this/project", "project:code", 3);
    let index = project_memory_deletion_index(&policy, "/this/project", &[local], 10).unwrap();
    assert_eq!(index.entries.len(), 1);
    // Every retrieval surface stays pending until its own receipt exists; an untracked
    // projection copy therefore cannot be reported as usable.
    assert_eq!(index.pending_surfaces().len(), 4);
    assert!(
        index
            .check_read("local-secret", 3)
            .unwrap_err()
            .starts_with("memory_deletion_tombstone_revival_denied"),
        "a tracked tombstone must deny reads on every surface"
    );
    // A newer revision of the same row is not covered by the tombstone: deletion is revision
    // bound, so only the exact revision is fenced.
    assert!(index.check_read("local-secret", 4).is_ok());

    // Home-scoped (company/user) collections are shared across project roots by design.
    let company = memory_fact(1, "company-note", "/other/project", "company", 2);
    let index = project_memory_deletion_index(&policy, "/this/project", &[company], 10).unwrap();
    assert_eq!(index.entries.len(), 1);
}

#[test]
fn governance_event_seal_has_no_payload_mutation_path() {
    let event =
        RuntimeEvent::new(RequestId::new(), 1, "run.completed", json!({"safe": true})).unwrap();
    let seal = seal_governance_events(&[event], 2, "sealed-audit").unwrap();
    assert!(seal.sealed);
    assert_eq!(seal.data_epoch, 2);
    assert!(seal.validate().is_ok());
}
