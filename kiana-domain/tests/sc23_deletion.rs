use kiana_domain::{
    DeleteRequest, DeletionManifest, DeletionPropagation, DeletionTombstone, RequestId,
};
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

#[test]
fn delete_request_and_tombstone_advance_data_epoch_without_payload() {
    let request = request();
    let tombstone = DeletionTombstone::new(
        &request,
        "src/input.txt",
        format!("sha256:{}", "a".repeat(64)),
        4,
    )
    .unwrap();
    let encoded = serde_json::to_string(&tombstone).unwrap();
    assert_eq!(tombstone.data_epoch, 4);
    assert!(!encoded.contains("payload"));
    tombstone.validate().unwrap();
}

#[test]
fn propagation_manifest_preserves_unknown_targets_as_non_success() {
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
        &[tombstone],
        vec![DeletionPropagation::unknown(
            "external",
            "adapter_receipt_required",
        )],
    )
    .unwrap();
    assert!(manifest.has_unknown_propagation());
    manifest.validate().unwrap();
}
