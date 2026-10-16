//! CM-07 Query workspace scanner 的临时目录集成 fixture。
//!
//! 测试通过 `read_workspace_snapshot()` 真实读取临时目录，检查 Trusted 正文路径、默认 Unknown 的 metadata-only 路径，
//! 以及 Unix symlink root 拒绝；它不证明 ProjectTrust 集成、恶意并发替换防护或生产请求已调用此 scanner。

use kiana_domain::{WorkspaceReadDisposition, WorkspaceTrust};
use kiana_query::{read_workspace_snapshot, WorkspaceSnapshotOptions};
use std::fs;
use std::path::PathBuf;

/// 创建测试专属临时 root；label 让并行用例使用不同目录。
fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-cm07-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    root
}

#[test]
/// Trusted 选项下，稳定 UTF-8 文件应同时出现在正文列表和安全 Indexed snapshot 行。
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
/// 默认 trust 为 Unknown 时，文件正文不得出现在返回内容中，只留下 metadata 行。
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
/// 把 root 本身替换成 symlink 后，scanner 必须在遍历链接目标前拒绝输入。
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
