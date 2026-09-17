use kiana_daemon::{resolve_storage_root, StorageLease};
use kiana_ports::PortError;
use std::fs;
use std::path::PathBuf;

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("kiana-pd01-{label}-{}", std::process::id()))
}

#[test]
fn daemon_storage_resolver_and_lock_reject_scope_conflicts() {
    let project = temp_root("project");
    let home = temp_root("home");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&home).unwrap();
    let previous = std::env::var_os("KIANA_HOME");
    std::env::set_var("KIANA_HOME", &home);
    let root = resolve_storage_root(&project, "owner", "instance", 1).unwrap();
    let lease = StorageLease::acquire(root.clone()).unwrap();
    assert!(lease.root().validate().is_ok());
    assert_eq!(
        StorageLease::acquire(root.clone()).unwrap_err(),
        PortError::Conflict("storage_lock_conflict".to_owned())
    );
    drop(lease);
    let other = resolve_storage_root(&project, "owner", "other-instance", 1).unwrap();
    assert_eq!(
        StorageLease::acquire(other).unwrap_err(),
        PortError::Conflict("store_identity_mismatch".to_owned())
    );
    std::env::remove_var("KIANA_HOME");
    if let Some(value) = previous {
        std::env::set_var("KIANA_HOME", value);
    }
    let _ = fs::remove_dir_all(project);
    let _ = fs::remove_dir_all(home);
}

#[test]
fn daemon_storage_resolver_rejects_project_local_root() {
    let project = temp_root("inside-project");
    fs::create_dir_all(&project).unwrap();
    let previous = std::env::var_os("KIANA_HOME");
    std::env::set_var("KIANA_HOME", project.join(".kiana"));
    assert!(resolve_storage_root(&project, "owner", "instance", 1).is_err());
    std::env::remove_var("KIANA_HOME");
    if let Some(value) = previous {
        std::env::set_var("KIANA_HOME", value);
    }
    let _ = fs::remove_dir_all(project);
}
