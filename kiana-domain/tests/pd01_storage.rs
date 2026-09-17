use kiana_domain::*;
use serde_json::json;

fn owner(instance: &str) -> StorageOwnerScope {
    StorageOwnerScope::new("daemon-owner", instance, None, 7).unwrap()
}

#[test]
fn storage_root_namespaces_and_identity_are_stable_and_strict() {
    let scope = owner("instance-a");
    let root = StorageRoot::new("/tmp/kiana-pd01", StorageBackend::LocalFilesystem, scope).unwrap();
    root.validate().unwrap();
    assert!(root
        .namespace_path(StorageNamespace::Facts)
        .unwrap()
        .ends_with("/facts"));
    let same = StorageRoot::new(
        "/tmp/kiana-pd01",
        StorageBackend::LocalFilesystem,
        owner("instance-a"),
    )
    .unwrap();
    assert_eq!(root.root_id, same.root_id);
    assert_eq!(root.root_digest, same.root_digest);
    let identity = StoreIdentity::new(&root, 1, 1, 100).unwrap();
    identity.validate(&root).unwrap();
    let lock = StorageLockRecord::new(&identity, &root.owner_scope, 100).unwrap();
    lock.validate(&identity, &root.owner_scope).unwrap();

    let mut unknown = serde_json::to_value(&root).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<StorageRoot>(unknown).is_err());
}

#[test]
fn storage_root_rejects_relative_network_and_owner_mismatch() {
    assert_eq!(
        StorageRoot::new(
            "relative/.kiana",
            StorageBackend::LocalFilesystem,
            owner("a")
        )
        .unwrap_err(),
        "storage_root_path_invalid"
    );
    let local = StorageRoot::new(
        "/tmp/kiana-pd01-network",
        StorageBackend::LocalFilesystem,
        owner("a"),
    )
    .unwrap();
    let mut network = local.clone();
    network.backend = StorageBackend::NetworkFilesystem;
    network.root_digest = network.digest();
    assert_eq!(
        network.validate().unwrap_err(),
        "storage_network_filesystem_unsupported"
    );
    let identity = StoreIdentity::new(&local, 1, 1, 1).unwrap();
    assert_eq!(
        identity
            .validate(
                &StorageRoot::new(
                    "/tmp/kiana-pd01-network",
                    StorageBackend::LocalFilesystem,
                    owner("b")
                )
                .unwrap()
            )
            .unwrap_err(),
        "store_identity_mismatch"
    );
}
