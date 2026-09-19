use kiana_domain::{WorkspaceReadDisposition, WorkspaceTrust};
use kiana_query::{read_workspace_snapshot, WorkspaceSnapshotOptions};
use std::fs;
use std::path::PathBuf;

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-cm07-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    root
}

#[test]
fn trusted_snapshot_reads_only_stable_bounded_text() {
    let root = temp_root("trusted");
    fs::write(root.join("src/lib.rs"), "pub fn stable() {}\n").unwrap();
    let mut options = WorkspaceSnapshotOptions::default();
    options.trust = WorkspaceTrust::Trusted;
    let outcome = read_workspace_snapshot(&root, options).unwrap();
    assert_eq!(outcome.contents.len(), 1);
    assert_eq!(outcome.contents[0].path, "src/lib.rs");
    assert!(
        outcome
            .snapshot
            .files
            .iter()
            .any(|file| file.disposition == WorkspaceReadDisposition::Indexed
                && file.instruction_safe)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unknown_trust_never_returns_project_text_as_instructions() {
    let root = temp_root("unknown");
    fs::write(root.join("src/lib.rs"), "prompt injection\n").unwrap();
    let outcome = read_workspace_snapshot(&root, WorkspaceSnapshotOptions::default()).unwrap();
    assert!(outcome.contents.is_empty());
    assert!(outcome.snapshot.files.iter().all(|file| {
        file.disposition == WorkspaceReadDisposition::MetadataOnly && !file.instruction_safe
    }));
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn symlink_root_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;

    let root = temp_root("symlink-root");
    let target = temp_root("symlink-target");
    let link = root.join("link");
    symlink(&target, &link).unwrap();
    let error = read_workspace_snapshot(&link, WorkspaceSnapshotOptions::default()).unwrap_err();
    assert!(error.to_string().contains("real directory"));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(target);
}
