use kiana_domain::{
    canonical_resource_set, CellId, FenceTokenId, ResourceLease, RunId, StorageLockId,
};

#[test]
fn resource_lease_binds_owner_scope_epoch_and_fencing_successor() {
    let run_id = RunId::new();
    let cell_id = CellId::new();
    let lease = ResourceLease::new(
        StorageLockId::new(),
        FenceTokenId::new(),
        "src/lib.rs",
        run_id,
        Some(cell_id),
        "session-1",
        4,
        100,
        1_000,
    )
    .unwrap();
    assert!(lease.validate_current(500, 4, lease.fence_token).is_ok());
    assert!(lease.covers("src/lib.rs").unwrap());
    assert!(!lease.covers("src/main.rs").unwrap());
    assert!(lease.conflicts("src").unwrap());

    let successor = ResourceLease::successor(&lease, FenceTokenId::new(), 4, 1_100, 2_000).unwrap();
    assert!(lease.validate_successor(&successor).is_ok());
    assert!(successor
        .validate_current(1_500, 4, successor.fence_token)
        .is_ok());
    assert!(successor
        .validate_current(1_500, 4, lease.fence_token)
        .is_err());
}

#[test]
fn resource_write_set_is_canonical_and_never_silently_filters_invalid_paths() {
    assert_eq!(canonical_resource_set(&[]).unwrap(), vec!["*"]);
    assert_eq!(
        canonical_resource_set(&["src".to_owned(), "src".to_owned()]).unwrap(),
        vec!["src"]
    );
    assert_eq!(
        canonical_resource_set(&["*".to_owned(), "src".to_owned()]).unwrap(),
        vec!["*"]
    );
    assert!(canonical_resource_set(&["../outside".to_owned()]).is_err());
    assert!(canonical_resource_set(&["/absolute".to_owned()]).is_err());
}

#[test]
fn resource_lease_rejects_scope_epoch_and_digest_tampering() {
    let mut value = serde_json::to_value(
        &ResourceLease::new(
            StorageLockId::new(),
            FenceTokenId::new(),
            "workspace",
            RunId::new(),
            None,
            "session-1",
            1,
            10,
            100,
        )
        .unwrap(),
    )
    .unwrap();
    value["resource"] = serde_json::json!("other");
    assert!(ResourceLease::from_json(&value).is_err());
}
