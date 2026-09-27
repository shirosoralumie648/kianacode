use kiana_domain::{
    BackupChunk, BackupChunkKind, BackupIntegrity, BackupManifest, DeploymentPhase,
    DeploymentProfile, DeploymentRevision, InstanceId, StorageRootId, BACKUP_MANIFEST_SCHEMA,
    BACKUP_VERSION, DEPLOYMENT_REVISION_SCHEMA,
};
use uuid::Uuid;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn instance() -> InstanceId {
    InstanceId::from_uuid(Uuid::from_u128(4))
}

fn root() -> StorageRootId {
    StorageRootId::from_uuid(Uuid::from_u128(1))
}

fn chunk(id: &str, kind: BackupChunkKind, reference: &str, byte: char) -> BackupChunk {
    BackupChunk::new(
        id,
        kind,
        reference,
        4_096,
        digest(byte),
        BackupIntegrity::ContentHashVerified,
    )
    .expect("chunk")
}

fn chunks() -> Vec<BackupChunk> {
    vec![
        chunk(
            "a-artifact",
            BackupChunkKind::Artifact,
            "artifact/0001",
            'a',
        ),
        chunk(
            "b-eventlog",
            BackupChunkKind::EventLog,
            "eventlog/0001",
            'b',
        ),
        chunk(
            "c-migration",
            BackupChunkKind::MigrationRegistry,
            "migration/registry",
            'c',
        ),
    ]
}

fn manifest() -> BackupManifest {
    BackupManifest::new(
        "backup-1",
        instance(),
        root(),
        "revision-1",
        1_234,
        5,
        2,
        3,
        chunks(),
        digest('d'),
    )
    .expect("manifest")
}

#[test]
fn full_manifest_is_verifiable_and_redacted_export_drops_references() {
    let value = manifest();
    value.validate().expect("manifest validates");
    value.restorable().expect("restorable");

    let exported = value.export_redacted().expect("export");
    assert_eq!(exported["schema"], "kiana.backup-set-ref.v1");
    assert_eq!(exported["chunk_count"], 3);
    assert_eq!(exported["manifest_digest"], value.manifest_digest);
    // A redacted export carries counts and digests, never a chunk reference.
    let rendered = exported.to_string();
    assert!(!rendered.contains("eventlog/0001"));
    assert!(!rendered.contains("artifact/0001"));
}

#[test]
fn manifest_without_source_cursor_or_epochs_is_rejected() {
    for (cursor, generation, data_epoch, authority_epoch) in [
        (0, 5, 2, 3),
        (1_234, 0, 2, 3),
        (1_234, 5, 0, 3),
        (1_234, 5, 2, 0),
    ] {
        let mut value = manifest();
        value.source_cursor = cursor;
        value.projection_generation = generation;
        value.data_epoch = data_epoch;
        value.authority_epoch = authority_epoch;
        value.manifest_digest = value.digest();
        assert_eq!(
            value.validate().unwrap_err(),
            "backup_manifest_header_invalid",
            "cursor={cursor} generation={generation} data={data_epoch} authority={authority_epoch}"
        );
    }
}

#[test]
fn tampered_chunk_digest_or_manifest_digest_fails_closed() {
    let mut chunk_tamper = manifest();
    chunk_tamper.chunks[0].byte_length = 8_192;
    chunk_tamper.manifest_digest = chunk_tamper.digest();
    assert_eq!(
        chunk_tamper.validate().unwrap_err(),
        "backup_chunk_digest_mismatch"
    );

    let mut manifest_tamper = manifest();
    manifest_tamper.config_digest = digest('e');
    // The manifest digest still describes the old config, so the binding is broken.
    assert_eq!(
        manifest_tamper.validate().unwrap_err(),
        "backup_manifest_digest_mismatch"
    );

    let mut cursor_tamper = manifest();
    cursor_tamper.source_cursor = 9_999;
    cursor_tamper.manifest_digest = cursor_tamper.digest();
    // A recomputed digest is internally consistent; only validate() ordering reveals the truth,
    // so a tampered cursor that is re-signed still has to fail the header check separately.
    cursor_tamper
        .validate()
        .expect("recomputed digest is self-consistent");
    assert_ne!(cursor_tamper.manifest_digest, manifest().manifest_digest);
}

#[test]
fn absolute_path_traversal_and_secret_references_are_rejected() {
    for reference in [
        "/etc/passwd",
        "../../etc/shadow",
        "C:\\Windows\\system32",
        "file:///root/.ssh/id_rsa",
        "artifact//double",
        "artifact/./here",
        "artifact/with space",
    ] {
        let error = BackupChunk::new(
            "a",
            BackupChunkKind::Artifact,
            reference,
            1,
            digest('a'),
            BackupIntegrity::ContentHashVerified,
        )
        .expect_err("absolute path must be rejected");
        assert_eq!(error, "backup_chunk_reference_invalid", "ref={reference}");
    }

    let secret = BackupChunk::new(
        "a",
        BackupChunkKind::Artifact,
        "artifact/sk-abcdefghijklmnopqrstuvwxyz0123456789",
        1,
        digest('a'),
        BackupIntegrity::ContentHashVerified,
    )
    .expect_err("secret must be rejected");
    assert!(
        secret.ends_with("_not_redacted") || secret.ends_with("_secret_detected"),
        "unexpected error for a secret-bearing reference: {secret}"
    );
}

#[test]
fn torn_tail_and_unknown_integrity_make_the_whole_backup_unusable() {
    for integrity in [BackupIntegrity::TornTail, BackupIntegrity::Unknown] {
        let mut value = manifest();
        value.chunks[1].integrity = integrity;
        value.chunks[1].chunk_digest_binding = value.chunks[1].digest();
        value.manifest_digest = value.digest();
        value.validate().expect("structurally valid");

        let error = value.restorable().expect_err("unusable");
        assert_eq!(
            error,
            format!("backup_manifest_integrity_{}", integrity.as_str())
        );
    }
}

#[test]
fn manifest_must_cover_eventlog_and_migration_registry() {
    let mut no_eventlog = manifest();
    no_eventlog
        .chunks
        .retain(|item| item.kind != BackupChunkKind::EventLog);
    no_eventlog.manifest_digest = no_eventlog.digest();
    assert_eq!(
        no_eventlog.validate_chunks().unwrap_err(),
        "backup_manifest_missing_event_log"
    );

    let mut no_registry = manifest();
    no_registry
        .chunks
        .retain(|item| item.kind != BackupChunkKind::MigrationRegistry);
    no_registry.manifest_digest = no_registry.digest();
    assert_eq!(
        no_registry.validate_chunks().unwrap_err(),
        "backup_manifest_missing_migration_registry"
    );
}

#[test]
fn duplicate_and_out_of_order_chunks_are_rejected() {
    let mut duplicate = manifest();
    duplicate.chunks[2] = duplicate.chunks[1].clone();
    duplicate.manifest_digest = duplicate.digest();
    assert_eq!(
        duplicate.validate_chunks().unwrap_err(),
        "backup_manifest_chunk_duplicate"
    );

    let mut unordered = manifest();
    unordered.chunks.swap(0, 2);
    unordered.manifest_digest = unordered.digest();
    assert_eq!(
        unordered.validate_chunks().unwrap_err(),
        "backup_manifest_chunk_order_invalid"
    );
}

#[test]
fn manifest_is_bound_to_the_revision_it_was_taken_under() {
    let value = manifest();
    let revision = DeploymentRevision::new(
        "revision-1",
        "release-1",
        DeploymentProfile::ManagedLocal,
        instance(),
        root(),
        "build-1",
        2,
        3,
        DeploymentPhase::Serving,
        "healthy",
        1_000,
        None,
        None,
    )
    .expect("revision");
    value
        .validate_against_revision(&revision)
        .expect("same revision binds");

    let foreign = DeploymentRevision::new(
        "revision-2",
        "release-1",
        DeploymentProfile::ManagedLocal,
        instance(),
        root(),
        "build-1",
        2,
        99,
        DeploymentPhase::Serving,
        "healthy",
        1_000,
        None,
        None,
    )
    .expect("revision");
    assert_eq!(
        value.validate_against_revision(&foreign).unwrap_err(),
        "backup_manifest_revision_binding_invalid"
    );

    assert_eq!(revision.schema, DEPLOYMENT_REVISION_SCHEMA);
    assert_eq!(value.schema, BACKUP_MANIFEST_SCHEMA);
    assert_eq!(value.version, BACKUP_VERSION);
}
