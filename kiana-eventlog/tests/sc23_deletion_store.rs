use kiana_domain::{
    DeleteRequest, DeletionManifest, DeletionPropagation, DeletionTombstone, RequestId,
};
use kiana_eventlog::MemoryRetentionStore;
use kiana_ports::RetentionStorePort;
use std::collections::BTreeSet;

fn request() -> DeleteRequest {
    DeleteRequest::new(
        RequestId::new(),
        "/repo",
        BTreeSet::from(["src/input.txt".to_owned()]),
        "delete",
        "data_subject_request",
        "principal:operator",
        7,
        3,
        5,
    )
    .unwrap()
}

#[tokio::test]
async fn typed_tombstone_and_manifest_are_recorded_only_after_boundary_checks() {
    let store = MemoryRetentionStore::new();
    let store_id = kiana_domain::StoreIdentityId::new();
    let request = request();
    let tombstone = DeletionTombstone::new(
        &request,
        "src/input.txt",
        format!("sha256:{}", "a".repeat(64)),
        4,
    )
    .unwrap();
    let manifest = DeletionManifest::new(
        &request,
        4,
        std::slice::from_ref(&tombstone),
        vec![DeletionPropagation::unknown(
            "external",
            "adapter_receipt_required",
        )],
    )
    .unwrap();
    RetentionStorePort::append_deletion_tombstone(&store, store_id, tombstone)
        .await
        .unwrap();
    RetentionStorePort::append_deletion_manifest(&store, store_id, manifest)
        .await
        .unwrap();
}

#[tokio::test]
async fn manifest_cannot_be_recorded_before_its_tombstone() {
    let store = MemoryRetentionStore::new();
    let store_id = kiana_domain::StoreIdentityId::new();
    let request = request();
    let tombstone = DeletionTombstone::new(
        &request,
        "src/input.txt",
        format!("sha256:{}", "a".repeat(64)),
        4,
    )
    .unwrap();
    let manifest = DeletionManifest::new(
        &request,
        4,
        std::slice::from_ref(&tombstone),
        vec![DeletionPropagation::unknown(
            "external",
            "adapter_receipt_required",
        )],
    )
    .unwrap();
    assert_eq!(
        RetentionStorePort::append_deletion_manifest(&store, store_id, manifest)
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:deletion_tombstone_missing"
    );
}
