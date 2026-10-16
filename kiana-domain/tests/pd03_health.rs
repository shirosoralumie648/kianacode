use kiana_domain::*;
use serde_json::json;

fn root() -> StorageRoot {
    StorageRoot::new(
        "/tmp/kiana-pd03",
        StorageBackend::LocalFilesystem,
        StorageOwnerScope::new("owner", "instance", None, 1).unwrap(),
    )
    .unwrap()
}

#[test]
fn storage_errors_keep_empty_unavailable_corrupt_conflict_unknown_distinct() {
    let cases = [
        (StorageErrorClass::Empty, StorageRetryDisposition::Never),
        (
            StorageErrorClass::Unavailable,
            StorageRetryDisposition::RetryAfterRead,
        ),
        (
            StorageErrorClass::Corrupt,
            StorageRetryDisposition::Reconcile,
        ),
        (
            StorageErrorClass::Conflict,
            StorageRetryDisposition::RetryAfterRead,
        ),
        (
            StorageErrorClass::Unknown,
            StorageRetryDisposition::Reconcile,
        ),
        (
            StorageErrorClass::ResultUnknown,
            StorageRetryDisposition::Reconcile,
        ),
    ];
    for (class, retry) in cases {
        let error =
            StorageError::new(class, "storage_code", "safe storage message", Some(1)).unwrap();
        assert_eq!(error.retry, retry);
        error.validate().unwrap();
        assert_eq!(
            class.requires_reconciliation(),
            retry == StorageRetryDisposition::Reconcile
        );
    }
}

#[test]
fn storage_health_capabilities_and_integrity_incident_are_strict() {
    let storage_root = root();
    let identity = StoreIdentity::new(&storage_root, 1, 1, 100).unwrap();
    let capabilities = StorageCapabilities::new(true, true, true, true, true, 4096, 32).unwrap();
    let health = StorageHealth::new(
        identity.store_id,
        StorageHealthStatus::Ready,
        capabilities,
        1,
        1,
        100,
        vec!["memory adapter".to_owned()],
    )
    .unwrap();
    health.validate().unwrap();
    let incident = StorageIntegrityIncident::new(
        identity.store_id,
        StorageIntegrityIncidentClass::Corrupt,
        "checksum_mismatch",
        101,
        Some(1),
    )
    .unwrap();
    incident.validate().unwrap();
    let mut encoded = serde_json::to_value(&health).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<StorageHealth>(encoded).is_err());
    let mut bad = health.clone();
    bad.capabilities.fsync = false;
    bad.capabilities.capabilities_digest = bad.capabilities.digest();
    bad.health_digest = bad.digest();
    assert_eq!(
        bad.validate().unwrap_err(),
        "storage_capabilities_durability_conflict"
    );
}

#[test]
fn storage_health_rejects_zero_observed_time() {
    let storage_root = root();
    let identity = StoreIdentity::new(&storage_root, 1, 1, 100).unwrap();
    let capabilities = StorageCapabilities::new(true, true, true, true, true, 4096, 32).unwrap();
    let mut health = StorageHealth::new(
        identity.store_id,
        StorageHealthStatus::Ready,
        capabilities,
        1,
        1,
        100,
        Vec::new(),
    )
    .unwrap();
    health.observed_at_unix_ms = 0;
    health.health_digest = health.digest();

    assert_eq!(health.validate().unwrap_err(), "storage_health_invalid");
}

#[test]
fn storage_integrity_incident_rejects_zero_detected_time() {
    let storage_root = root();
    let identity = StoreIdentity::new(&storage_root, 1, 1, 100).unwrap();
    let mut incident = StorageIntegrityIncident::new(
        identity.store_id,
        StorageIntegrityIncidentClass::Corrupt,
        "checksum_mismatch",
        101,
        Some(1),
    )
    .unwrap();
    incident.detected_at_unix_ms = 0;
    incident.incident_digest = incident.digest();

    assert_eq!(
        incident.validate().unwrap_err(),
        "storage_integrity_incident_invalid"
    );
}
