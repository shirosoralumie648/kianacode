use kiana_domain::{
    schema_contract, InstanceId, OperationId, OperationLeaseCas, OperationLeaseState,
    StorageRootId, OPERATION_LEASE_CAS_SCHEMA, OPERATION_LEASE_SCHEMA,
};
use uuid::Uuid;

fn root() -> StorageRootId {
    StorageRootId::from_uuid(Uuid::from_u128(1))
}

fn operation(value: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(value))
}

fn instance(value: u128) -> InstanceId {
    InstanceId::from_uuid(Uuid::from_u128(value))
}

fn fence(value: u128) -> kiana_domain::FenceTokenId {
    kiana_domain::FenceTokenId::from_uuid(Uuid::from_u128(value))
}

#[test]
fn operation_lease_schemas_are_registered_strictly() {
    for schema in [OPERATION_LEASE_SCHEMA, OPERATION_LEASE_CAS_SCHEMA] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
}

#[test]
fn single_writer_cas_denies_competition_stale_owner_fence_and_epoch_rollback() {
    let mut cas = OperationLeaseCas::new(root(), 4, 7).unwrap();
    let lease = cas
        .acquire(0, operation(10), instance(20), fence(30), 4, 7, 100, 100)
        .unwrap();
    assert_eq!(lease.state, OperationLeaseState::Active);
    assert_eq!(cas.revision(), 1);

    assert_eq!(
        cas.acquire(1, operation(11), instance(21), fence(31), 4, 7, 150, 100)
            .unwrap_err(),
        "operation_lease_already_active"
    );
    assert_eq!(
        cas.acquire(0, operation(11), instance(21), fence(31), 4, 7, 150, 100)
            .unwrap_err(),
        "operation_lease_cas_conflict"
    );
    assert_eq!(
        cas.heartbeat(1, operation(10), instance(21), fence(30), 4, 7, 150, 100)
            .unwrap_err(),
        "operation_lease_owner_mismatch"
    );
    assert_eq!(
        cas.heartbeat(1, operation(10), instance(20), fence(31), 4, 7, 150, 100)
            .unwrap_err(),
        "operation_lease_fence_mismatch"
    );
    assert_eq!(
        cas.heartbeat(1, operation(10), instance(20), fence(30), 3, 7, 150, 100)
            .unwrap_err(),
        "operation_lease_authority_epoch_rollback"
    );
    assert_eq!(
        cas.heartbeat(1, operation(10), instance(20), fence(30), 4, 6, 150, 100)
            .unwrap_err(),
        "operation_lease_data_epoch_rollback"
    );
}

#[test]
fn expired_lease_requires_explicit_reclaim_and_new_fence_before_reacquire() {
    let mut cas = OperationLeaseCas::new(root(), 4, 7).unwrap();
    let first = cas
        .acquire(0, operation(10), instance(20), fence(30), 4, 7, 100, 100)
        .unwrap();
    let renewed = cas
        .heartbeat(1, operation(10), instance(20), fence(30), 4, 7, 150, 100)
        .unwrap();
    assert_eq!(renewed.heartbeat_seq, 2);
    assert_eq!(
        cas.acquire(2, operation(11), instance(21), fence(31), 4, 7, 250, 100)
            .unwrap_err(),
        "operation_lease_expired_reclaim_required"
    );
    assert_eq!(
        cas.expire(2, 249).unwrap_err(),
        "operation_lease_expiry_not_due"
    );
    let expired = cas.expire(2, 250).unwrap();
    assert_eq!(expired.state, OperationLeaseState::Expired);
    assert_eq!(cas.revision(), 3);

    let reacquired = cas
        .acquire(3, operation(11), instance(21), fence(31), 5, 8, 300, 100)
        .unwrap();
    assert_eq!(reacquired.state, OperationLeaseState::Active);
    assert_eq!(reacquired.cas_revision, 4);
    assert_eq!(reacquired.authority_epoch, 5);
    assert_eq!(reacquired.data_epoch, 8);
    assert_ne!(reacquired.fence_token, first.fence_token);

    let encoded = serde_json::to_string(&cas).unwrap();
    assert_eq!(
        serde_json::from_str::<OperationLeaseCas>(&encoded).unwrap(),
        cas
    );
}

#[test]
fn release_is_fenced_and_cannot_be_reused() {
    let mut cas = OperationLeaseCas::new(root(), 4, 7).unwrap();
    cas.acquire(0, operation(10), instance(20), fence(30), 4, 7, 100, 100)
        .unwrap();
    assert_eq!(
        cas.release(1, operation(10), instance(20), fence(31), 150)
            .unwrap_err(),
        "operation_lease_fence_mismatch"
    );
    let released = cas
        .release(1, operation(10), instance(20), fence(30), 150)
        .unwrap();
    assert_eq!(released.state, OperationLeaseState::Released);
    assert!(cas.active().is_none());
    assert_eq!(
        cas.release(2, operation(10), instance(20), fence(30), 151)
            .unwrap_err(),
        "operation_lease_missing"
    );
    assert_eq!(
        cas.acquire(2, operation(11), instance(21), fence(30), 4, 7, 160, 100)
            .unwrap_err(),
        "operation_lease_fence_token_reused"
    );
}
