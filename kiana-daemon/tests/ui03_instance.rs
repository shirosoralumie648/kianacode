use kiana_daemon::{discover_instance, validate_instance_peer, InstanceLease};
use kiana_protocol::{UiTransportKind, PROTOCOL_SCHEMA};
use std::fs;
use std::path::PathBuf;

fn workspace(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-ui03-{label}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn instance_lock_record_discovery_and_peer_checks_are_fail_closed() {
    let root = workspace("lifecycle");
    let lease = InstanceLease::acquire(&root, UiTransportKind::InProcess, "in-process").unwrap();
    lease.record().validate().unwrap();
    assert!(lease.record_path().is_file());
    assert!(lease.lock_path().is_file());

    let discovered = discover_instance(&root).unwrap();
    assert_eq!(discovered.instance_id, lease.record().instance_id);
    validate_instance_peer(&discovered, &root, Some(discovered.authority_epoch)).unwrap();
    assert!(validate_instance_peer(&discovered, "/tmp/other-workspace", None).is_err());

    let duplicate = InstanceLease::acquire(&root, UiTransportKind::InProcess, "in-process");
    assert!(duplicate.is_err());
    drop(lease);
    assert!(discover_instance(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn instance_record_uses_protocol_schema_and_does_not_expose_endpoint_path() {
    let root = workspace("redaction");
    let endpoint = root.join("private.sock");
    let lease = InstanceLease::acquire(
        &root,
        UiTransportKind::UnixSocket,
        &endpoint.to_string_lossy(),
    )
    .unwrap();
    assert_eq!(lease.record().protocol_schema, PROTOCOL_SCHEMA);
    let encoded = serde_json::to_string(lease.record()).unwrap();
    assert!(!encoded.contains(endpoint.to_string_lossy().as_ref()));
    drop(lease);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_workspace_and_record_paths_are_rejected() {
    use std::os::unix::fs::symlink;

    let root = workspace("symlink");
    let outside = workspace("symlink-outside");
    let linked = root.join("linked");
    symlink(&outside, &linked).unwrap();
    assert!(InstanceLease::acquire(&linked, UiTransportKind::InProcess, "in-process").is_err());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
