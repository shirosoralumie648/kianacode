use kiana_skills::{SourceResolveError, SourceResolver, SourceRootKind, SourceTrust};
use std::fs;
use std::path::PathBuf;

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-ext02-{label}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn untrusted_project_source_is_reported_and_not_usable() {
    let root = temp_root("trust");
    let project_skills = root.join(".claude").join("skills");
    fs::create_dir_all(&project_skills).unwrap();

    let resolution = SourceResolver::new(&root, kiana_types::ProjectTrust::Untrusted)
        .resolve()
        .unwrap();
    resolution.summary.validate().unwrap();
    let project = resolution
        .summary
        .roots
        .iter()
        .find(|entry| entry.kind == SourceRootKind::Project)
        .expect("project source decision");
    assert_eq!(project.trust, SourceTrust::Untrusted);
    assert!(!resolution.trusted_paths().contains(&project_skills));
    assert!(!serde_json::to_string(&resolution.summary)
        .unwrap()
        .contains(root.to_string_lossy().as_ref()));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_roots_are_rejected_instead_of_silently_deduped() {
    let root = temp_root("duplicate");
    let result = SourceResolver::new(&root, kiana_types::ProjectTrust::Trusted)
        .with_root(SourceRootKind::Bundled, &root)
        .with_root(SourceRootKind::PluginComponent, &root)
        .resolve();
    assert_eq!(result.unwrap_err(), SourceResolveError::DuplicateRoot);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn resource_resolution_rejects_escape_and_symlink() {
    use std::os::unix::fs::symlink;

    let root = temp_root("resource");
    let outside = root.with_file_name("kiana-ext02-outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    fs::write(root.join("safe.txt"), "safe").unwrap();
    symlink(outside.join("secret.txt"), root.join("linked.txt")).unwrap();

    assert!(matches!(
        SourceResolver::resolve_resource(&root, "safe.txt"),
        Ok(path) if path.ends_with("safe.txt")
    ));
    assert!(matches!(
        SourceResolver::resolve_resource(&root, "../kiana-ext02-outside/secret.txt"),
        Err(SourceResolveError::ResourceInvalid(_))
    ));
    assert!(matches!(
        SourceResolver::resolve_resource(&root, "/etc/passwd"),
        Err(SourceResolveError::ResourceInvalid(_))
    ));
    assert_eq!(
        SourceResolver::resolve_resource(&root, "linked.txt").unwrap_err(),
        SourceResolveError::ResourceSymlink
    );

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
