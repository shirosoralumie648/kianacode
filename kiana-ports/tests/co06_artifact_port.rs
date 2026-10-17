use kiana_domain::{
    ArtifactProvenance, ArtifactPublicationIntent, ArtifactPublicationState, ArtifactVersion,
    EventId, ProjectId,
};
use kiana_ports::{
    ArtifactContentPort, ArtifactStorePort, InMemoryArtifactContentStore, PortError,
};

/// A store that implements only the required artifact boundary and inherits the fail-closed
/// publication-journal defaults. This pins the compatibility contract for existing adapters.
struct JournallessStore;

impl ArtifactStorePort for JournallessStore {}

#[tokio::test]
async fn artifact_read_port_requires_persisted_hash_matching_blob() {
    let scope = kiana_domain::json_digest(&serde_json::json!({"project": ProjectId::new()}));
    let version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-artifact.v1",
        b"immutable",
        scope,
        ArtifactProvenance {
            producer_kind: "fixture".to_owned(),
            producer_id: "co06".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "fixture".to_owned(),
        },
        100,
    )
    .unwrap();
    let reference = version.as_ref();
    let store = InMemoryArtifactContentStore::new();
    assert!(matches!(
        store.read_artifact(&reference).await,
        Err(PortError::Unavailable(reason)) if reason == "artifact_blob_missing"
    ));
    store.put(&version, b"immutable".to_vec()).await.unwrap();
    assert_eq!(store.read_artifact(&reference).await.unwrap(), b"immutable");

    let mut forged = reference.clone();
    forged.content_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    assert!(matches!(
        store.read_artifact(&forged).await,
        Err(PortError::Conflict(reason)) if reason == "artifact_content_hash_mismatch"
    ));

    let mut foreign_scope = reference;
    foreign_scope.scope_digest = kiana_domain::json_digest(&serde_json::json!({"foreign": true}));
    assert!(matches!(
        store.read_artifact(&foreign_scope).await,
        Err(PortError::Conflict(reason)) if reason == "artifact_scope_mismatch"
    ));
}

#[tokio::test]
async fn artifact_read_port_rejects_nil_identity_before_lookup() {
    let scope = kiana_domain::json_digest(&serde_json::json!({"project": ProjectId::new()}));
    let version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-artifact.v1",
        b"immutable",
        scope,
        ArtifactProvenance {
            producer_kind: "fixture".to_owned(),
            producer_id: "co06".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "fixture".to_owned(),
        },
        100,
    )
    .unwrap();
    let mut reference = version.as_ref();
    reference.artifact_id =
        kiana_domain::ArtifactId::parse_str("00000000-0000-0000-0000-000000000000")
            .expect("nil UUID parses as a typed id");
    let store = InMemoryArtifactContentStore::new();
    assert!(matches!(
        store.read_artifact(&reference).await,
        Err(PortError::Failed(reason))
            if reason == "artifact_reference_invalid:artifact_ref_identity_invalid"
    ));
}

#[tokio::test]
async fn artifact_store_publication_journal_defaults_fail_closed() {
    let scope = kiana_domain::json_digest(&serde_json::json!({"project": ProjectId::new()}));
    let version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-artifact.v1",
        b"immutable",
        scope,
        ArtifactProvenance {
            producer_kind: "fixture".to_owned(),
            producer_id: "co06".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "fixture".to_owned(),
        },
        100,
    )
    .unwrap();
    let intent = ArtifactPublicationIntent::new(
        "command-1",
        "idempotency-1",
        "aggregate-1",
        version.as_ref(),
        100,
    )
    .unwrap();
    let store = JournallessStore;
    assert!(matches!(
        store.record_publication_intent(&intent).await,
        Err(PortError::Unavailable(reason)) if reason == "artifact_publication_journal_unsupported"
    ));
    assert!(matches!(
        store
            .resolve_publication_intent(
                "command-1",
                ArtifactPublicationState::Committed,
                Some(EventId::new()),
                200,
            )
            .await,
        Err(PortError::Unavailable(reason)) if reason == "artifact_publication_journal_unsupported"
    ));
    assert!(matches!(
        store.pending_publication_intents().await,
        Err(PortError::Unavailable(reason)) if reason == "artifact_publication_journal_unsupported"
    ));
    // Retention reads the same journal, and byte reclamation is never a default: an adapter that
    // has not opted in must deny both instead of pretending the plan is empty-but-backable.
    assert!(matches!(
        store.publication_intents().await,
        Err(PortError::Unavailable(reason)) if reason == "artifact_publication_journal_unsupported"
    ));
    assert!(matches!(
        store.remove_orphan_blob(&version.as_ref()).await,
        Err(PortError::Unavailable(reason)) if reason == "artifact_orphan_removal_unsupported"
    ));
}
