use kiana_domain::{
    DataClass, DataPayloadState, EventId, LegalHoldReceipt, RetentionDecision,
    RetentionDisposition, RetentionScan, StoreIdentityId,
};
use kiana_eventlog::MemoryRetentionStore;
use kiana_ports::RetentionStorePort;

fn scan() -> RetentionScan {
    RetentionScan::new(
        "/repo",
        7,
        3,
        5,
        4,
        vec![EventId::new()],
        vec![RetentionDecision {
            object_ref: "src/input.txt".to_owned(),
            class: DataClass::Confidential,
            purpose_id: "audit".to_owned(),
            source_digest: format!("sha256:{}", "a".repeat(64)),
            payload: DataPayloadState::Available,
            disposition: RetentionDisposition::Retain,
            retain_until_ms: 100,
            hold_id: None,
        }],
        Vec::<LegalHoldReceipt>::new(),
    )
    .unwrap()
}

#[tokio::test]
async fn retention_plan_preserves_scan_and_source_projection_boundary() {
    let store = MemoryRetentionStore::new();
    let store_id = StoreIdentityId::new();
    store.append_scan(store_id, scan()).await.unwrap();
    let plan = store.plan_retention(store_id, 5).await.unwrap();
    assert_eq!(plan["source_cursor"], 5);
    assert_eq!(plan["projection_cursor"], 4);
    assert_eq!(plan["scan"]["schema"], "kiana.retention-scan.v1");
}

#[tokio::test]
async fn tombstone_and_purge_remain_explicitly_deferred_to_delete_step() {
    let store = MemoryRetentionStore::new();
    let store_id = StoreIdentityId::new();
    assert_eq!(
        store
            .append_tombstone(store_id, "src/input.txt", "expired")
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:retention_tombstone_deferred_to_sc23"
    );
}
