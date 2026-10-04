use kiana_daemon::{resolve_storage_root, StorageLease};
#[cfg(unix)]
use kiana_domain::{StorageBackend, StorageOwnerScope, StorageRoot};
use kiana_ports::PortError;
use std::fs;
use std::path::PathBuf;

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("kiana-pd01-{label}-{}", std::process::id()))
}

#[cfg(unix)]
fn fixture_root(label: &str) -> (PathBuf, StorageRoot) {
    let path = temp_root(label);
    fs::create_dir_all(&path).unwrap();
    let root = StorageRoot::new(
        path.to_string_lossy(),
        StorageBackend::LocalFilesystem,
        StorageOwnerScope::new("owner", "instance", None, 1).unwrap(),
    )
    .unwrap();
    (path, root)
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
    let identity = lease.identity().clone();
    assert!(lease.root().validate().is_ok());
    assert_eq!(
        StorageLease::acquire(root.clone()).unwrap_err(),
        PortError::Conflict("storage_lock_conflict".to_owned())
    );
    drop(lease);
    let reopened = StorageLease::acquire(root.clone()).unwrap();
    assert_eq!(reopened.identity(), &identity);
    reopened.release().unwrap();
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

#[cfg(unix)]
#[test]
fn daemon_storage_acquisition_rejects_a_replaced_root_and_namespace_symlinks() {
    use std::os::unix::fs::symlink;

    let outside = temp_root("namespace-outside");
    fs::create_dir_all(&outside).unwrap();
    let (path, root) = fixture_root("namespace-symlinks");
    for namespace in root.namespaces.keys() {
        let namespace_path = path.join(namespace.as_str());
        let _ = fs::remove_dir_all(&namespace_path);
        symlink(&outside, &namespace_path).unwrap();
        assert!(matches!(
            StorageLease::acquire(root.clone()),
            Err(PortError::Failed(reason)) if reason.starts_with("storage_directory_open_failed")
        ));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert!(!path.join("meta/store-identity.json").exists());
        fs::remove_file(namespace_path).unwrap();
    }
    fs::remove_dir_all(&path).unwrap();
    symlink(&outside, &path).unwrap();
    assert!(matches!(
        StorageLease::acquire(root),
        Err(PortError::Failed(reason)) if reason.starts_with("storage_directory_open_failed")
    ));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_file(path).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn daemon_storage_identity_rejects_fifo_directory_hardlink_and_oversized_bytes() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    let (path, root) = fixture_root("identity-types");
    fs::create_dir_all(path.join("meta")).unwrap();
    let identity_path = path.join("meta/store-identity.json");
    let name = CString::new(identity_path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(
        StorageLease::acquire(root.clone()).unwrap_err(),
        PortError::Failed("storage_file_type_denied".to_owned())
    );
    fs::remove_file(&identity_path).unwrap();
    fs::create_dir(&identity_path).unwrap();
    assert_eq!(
        StorageLease::acquire(root.clone()).unwrap_err(),
        PortError::Failed("storage_file_type_denied".to_owned())
    );
    fs::remove_dir(&identity_path).unwrap();
    let linked = path.join("linked-identity.json");
    fs::write(&linked, b"{}").unwrap();
    fs::hard_link(&linked, &identity_path).unwrap();
    assert_eq!(
        StorageLease::acquire(root.clone()).unwrap_err(),
        PortError::Failed("storage_file_type_denied".to_owned())
    );
    fs::remove_file(&identity_path).unwrap();
    symlink(&linked, &identity_path).unwrap();
    assert!(matches!(
        StorageLease::acquire(root.clone()),
        Err(PortError::Failed(reason)) if reason.starts_with("storage_file_open_failed")
    ));
    fs::remove_file(&identity_path).unwrap();
    fs::write(&identity_path, vec![b' '; 16 * 1024 + 1]).unwrap();
    assert_eq!(
        StorageLease::acquire(root).unwrap_err(),
        PortError::Failed("storage_size_exceeded".to_owned())
    );
    assert!(!path.join("locks/storage.lock").exists());
    fs::remove_dir_all(path).unwrap();
}

#[cfg(unix)]
#[test]
fn daemon_storage_lock_conflict_rejects_a_fifo_without_reading_it() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let (path, root) = fixture_root("lock-fifo");
    fs::create_dir_all(path.join("locks")).unwrap();
    let lock_path = path.join("locks/storage.lock");
    let name = CString::new(lock_path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(
        StorageLease::acquire(root).unwrap_err(),
        PortError::Failed("storage_file_type_denied".to_owned())
    );
    assert!(fs::symlink_metadata(lock_path).is_ok());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn daemon_storage_lock_conflict_rejects_a_malformed_record_distinctly() {
    let (path, root) = fixture_root("lock-malformed");
    StorageLease::acquire(root.clone()).unwrap().release().unwrap();
    fs::write(path.join("locks/storage.lock"), b"{}").unwrap();

    assert_eq!(
        StorageLease::acquire(root).unwrap_err(),
        PortError::Failed("storage_lock_corrupt".to_owned())
    );
    fs::remove_dir_all(path).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn daemon_storage_lock_initialization_failure_releases_owned_file() {
    const CHILD_ENV: &str = "KIANA_PD01_LOCK_INIT_FAILURE_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let (path, root) = fixture_root("lock-init-failure");
        StorageLease::acquire(root.clone())
            .unwrap()
            .release()
            .unwrap();

        assert_ne!(
            unsafe { libc::signal(libc::SIGXFSZ, libc::SIG_IGN) },
            libc::SIG_ERR
        );
        let mut original_limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, &mut original_limit) },
            0
        );
        let mut failure_limit = original_limit;
        failure_limit.rlim_cur = 0;
        assert_eq!(
            unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &failure_limit) },
            0
        );

        assert!(matches!(
            StorageLease::acquire(root.clone()),
            Err(PortError::Failed(reason)) if reason.starts_with("storage_lock_sync:")
        ));
        let lock_path = path.join("locks/storage.lock");
        assert_eq!(
            fs::symlink_metadata(&lock_path).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );

        assert_eq!(
            unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &original_limit) },
            0
        );
        StorageLease::acquire(root).unwrap().release().unwrap();
        fs::remove_dir_all(path).unwrap();
        return;
    }

    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("daemon_storage_lock_initialization_failure_releases_owned_file")
        .env(CHILD_ENV, "1")
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(unix)]
#[test]
fn daemon_storage_release_keeps_a_replacement_lock() {
    let (path, root) = fixture_root("lock-replaced");
    let lease = StorageLease::acquire(root).unwrap();
    let lock_path = lease.lock_path().to_path_buf();
    fs::rename(&lock_path, path.join("locks/old.lock")).unwrap();
    fs::write(&lock_path, b"replacement").unwrap();
    assert_eq!(
        lease.release().unwrap_err(),
        PortError::Failed("storage_file_replaced".to_owned())
    );
    assert_eq!(fs::read(lock_path).unwrap(), b"replacement");
    fs::remove_dir_all(path).unwrap();
}

#[cfg(unix)]
#[test]
fn daemon_storage_release_uses_the_opened_lock_directory() {
    use std::os::unix::fs::symlink;

    let (path, root) = fixture_root("lock-directory-replaced");
    let lease = StorageLease::acquire(root).unwrap();
    let outside = temp_root("lock-directory-outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("storage.lock"), b"outside").unwrap();
    fs::rename(path.join("locks"), path.join("old-locks")).unwrap();
    symlink(&outside, path.join("locks")).unwrap();
    lease.release().unwrap();
    assert_eq!(fs::read(outside.join("storage.lock")).unwrap(), b"outside");
    assert!(!path.join("old-locks/storage.lock").exists());
    fs::remove_dir_all(path).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
