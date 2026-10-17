use kiana_daemon::{
    resolve_storage_root, resolve_surface_storage_root, StorageLease, StorageSurface,
    LOCAL_STORAGE_AUTHORITY_EPOCH, LOCAL_STORAGE_INSTANCE_ID,
};
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

#[test]
fn daemon_storage_resolver_rejects_existing_non_directory_root() {
    let project = temp_root("file-root-project");
    let storage_file = temp_root("file-root");
    fs::create_dir_all(&project).unwrap();
    fs::write(&storage_file, b"storage root is not a directory").unwrap();
    let previous = std::env::var_os("KIANA_HOME");
    std::env::set_var("KIANA_HOME", &storage_file);

    assert_eq!(
        resolve_storage_root(&project, "owner", "instance", 1).unwrap_err(),
        PortError::Failed("storage_root_not_directory".to_owned())
    );

    std::env::remove_var("KIANA_HOME");
    if let Some(value) = previous {
        std::env::set_var("KIANA_HOME", value);
    }
    let _ = fs::remove_dir_all(project);
    let _ = fs::remove_file(storage_file);
}

#[test]
fn daemon_storage_resolver_rejects_filesystem_root() {
    let project = temp_root("filesystem-root-project");
    fs::create_dir_all(&project).unwrap();
    let previous = std::env::var_os("KIANA_HOME");
    std::env::set_var("KIANA_HOME", "/");

    assert_eq!(
        resolve_storage_root(&project, "owner", "instance", 1).unwrap_err(),
        PortError::Failed("storage_root_filesystem_root_denied".to_owned())
    );

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
    StorageLease::acquire(root.clone())
        .unwrap()
        .release()
        .unwrap();
    fs::write(path.join("locks/storage.lock"), b"{}").unwrap();

    assert_eq!(
        StorageLease::acquire(root).unwrap_err(),
        PortError::Failed("storage_lock_corrupt".to_owned())
    );
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn daemon_storage_lock_conflict_rejects_foreign_scope_with_invalid_digest_as_corrupt() {
    let (path, root) = fixture_root("lock-foreign-invalid-digest");
    let lease = StorageLease::acquire(root.clone()).unwrap();
    let mut record = lease.record().clone();
    lease.release().unwrap();

    record.owner_id = "foreign-owner".to_owned();
    record.store_id = kiana_domain::StoreIdentityId::new();
    record.lock_digest = "invalid-digest".to_owned();
    fs::write(
        path.join("locks/storage.lock"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();

    assert_eq!(
        StorageLease::acquire(root).unwrap_err(),
        PortError::Failed("storage_lock_corrupt".to_owned())
    );
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn daemon_storage_lock_conflict_rejects_foreign_scope_with_zero_time_as_corrupt() {
    let (path, root) = fixture_root("lock-foreign-zero-time");
    let lease = StorageLease::acquire(root.clone()).unwrap();
    let mut record = lease.record().clone();
    lease.release().unwrap();

    record.owner_id = "foreign-owner".to_owned();
    record.acquired_at_unix_ms = 0;
    record.lock_digest = record.digest();
    fs::write(
        path.join("locks/storage.lock"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();

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

#[test]
fn daemon_storage_surface_roots_are_identical_across_surfaces() {
    let project = temp_root("surface-project");
    let home = temp_root("surface-home");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&home).unwrap();
    let previous = std::env::var_os("KIANA_HOME");
    std::env::set_var("KIANA_HOME", &home);

    let mut roots = Vec::new();
    let mut bindings = Vec::new();
    for surface in StorageSurface::ALL {
        let (root, binding) = resolve_surface_storage_root(surface, &project, "owner").unwrap();
        binding.validate().unwrap();
        assert_eq!(binding.root_id, root.root_id);
        assert_eq!(binding.surface, surface);
        roots.push(root);
        bindings.push(binding);
    }

    assert_eq!(bindings.len(), 3);
    assert_eq!(bindings[0].surface.as_str(), "cli");
    assert_eq!(bindings[1].surface.as_str(), "web");
    assert_eq!(bindings[2].surface.as_str(), "workbench");
    for root in &roots {
        assert_eq!(root.root_id, roots[0].root_id);
        assert_eq!(root.canonical_path, roots[0].canonical_path);
        assert_eq!(root.root_digest, roots[0].root_digest);
    }
    assert_eq!(roots[0].owner_scope.instance_id, LOCAL_STORAGE_INSTANCE_ID);
    assert_eq!(
        roots[0].owner_scope.authority_epoch,
        LOCAL_STORAGE_AUTHORITY_EPOCH
    );
    for binding in &bindings {
        assert_eq!(binding.root_id, roots[0].root_id);
        assert_eq!(binding.canonical_path, roots[0].canonical_path);
        assert_eq!(binding.owner_scope_digest, roots[0].owner_scope.scope_digest);
        assert_eq!(binding.instance_id, LOCAL_STORAGE_INSTANCE_ID);
        assert_eq!(binding.authority_epoch, LOCAL_STORAGE_AUTHORITY_EPOCH);
    }

    std::env::remove_var("KIANA_HOME");
    if let Some(value) = previous {
        std::env::set_var("KIANA_HOME", value);
    }
    let _ = fs::remove_dir_all(project);
    let _ = fs::remove_dir_all(home);
}

#[test]
fn daemon_storage_surface_binding_rejects_scope_and_digest_tampering() {
    let project = temp_root("binding-project");
    let home = temp_root("binding-home");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&home).unwrap();
    let previous = std::env::var_os("KIANA_HOME");
    std::env::set_var("KIANA_HOME", &home);

    let (_root, binding) = resolve_surface_storage_root(StorageSurface::Web, &project, "owner")
        .expect("surface binding");

    let mut drifted = binding.clone();
    drifted.instance_id = "foreign-instance".to_owned();
    drifted.binding_digest = drifted.digest();
    assert_eq!(drifted.validate().unwrap_err(), "storage_surface_binding_scope_invalid");

    let mut forged = binding;
    forged.canonical_path = "/tmp/kiana-pd01-elsewhere".to_owned();
    assert_eq!(forged.validate().unwrap_err(), "storage_surface_binding_digest_mismatch");

    std::env::remove_var("KIANA_HOME");
    if let Some(value) = previous {
        std::env::set_var("KIANA_HOME", value);
    }
    let _ = fs::remove_dir_all(project);
    let _ = fs::remove_dir_all(home);
}

#[test]
fn daemon_storage_identity_survives_a_new_process() {
    const CHILD_ENV: &str = "KIANA_PD01_RESTART_CHILD";
    const PROJECT_ENV: &str = "KIANA_PD01_RESTART_PROJECT";
    const OUT_ENV: &str = "KIANA_PD01_RESTART_OUT";

    if std::env::var_os(CHILD_ENV).is_some() {
        let project = PathBuf::from(std::env::var_os(PROJECT_ENV).unwrap());
        let out = PathBuf::from(std::env::var_os(OUT_ENV).unwrap());
        let (root, binding) = resolve_surface_storage_root(StorageSurface::Cli, &project, "owner")
            .expect("child surface root");
        let lease = StorageLease::acquire(root).unwrap();
        assert_eq!(binding.root_id, lease.root().root_id);
        fs::write(&out, lease.identity().store_id.to_string()).unwrap();
        lease.release().unwrap();
        return;
    }

    let project = temp_root("restart-project");
    let home = temp_root("restart-home");
    let out = temp_root("restart-identity.txt");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&home).unwrap();
    let _ = fs::remove_file(&out);

    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("daemon_storage_identity_survives_a_new_process")
        .env(CHILD_ENV, "1")
        .env(PROJECT_ENV, &project)
        .env(OUT_ENV, &out)
        .env("KIANA_HOME", &home)
        .status()
        .unwrap();
    assert!(status.success(), "restart child process failed");

    let created_store_id = fs::read_to_string(&out).unwrap();
    assert!(!created_store_id.trim().is_empty());

    let previous = std::env::var_os("KIANA_HOME");
    std::env::set_var("KIANA_HOME", &home);
    let (root, binding) = resolve_surface_storage_root(StorageSurface::Workbench, &project, "owner")
        .expect("surface root after restart");
    let lease = StorageLease::acquire(root).unwrap();
    assert_eq!(binding.root_id, lease.root().root_id);
    assert_eq!(lease.identity().store_id.to_string(), created_store_id.trim());
    lease.release().unwrap();

    std::env::remove_var("KIANA_HOME");
    if let Some(value) = previous {
        std::env::set_var("KIANA_HOME", value);
    }
    let _ = fs::remove_file(out);
    let _ = fs::remove_dir_all(project);
    let _ = fs::remove_dir_all(home);
}
