use kiana_domain::{RequestId, RuntimeEvent};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::EventStorePort;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-pd28-eventlog-{label}-{}-{stamp}.jsonl",
        std::process::id()
    ))
}

fn cleanup(path: &PathBuf) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("jsonl.lock"));
}

#[tokio::test]
async fn secret_sentinel_is_rejected_before_eventlog_fact_creation() {
    let path = temp_path("secret");
    let store = JsonlEventLog::open(&path).expect("open fixture journal");
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "run.accepted",
        json!({"token": "raw-token"}),
    )
    .expect("fixture event");
    assert!(store.append(event).await.is_err());
    assert!(store.read_all().await.expect("read empty journal").is_empty());
    assert!(!path.exists(), "denied append must not create journal facts");
    cleanup(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn journal_symlink_is_rejected_without_following_target() {
    use std::os::unix::fs::symlink;

    let path = temp_path("symlink");
    let target = temp_path("symlink-target");
    fs::write(&target, b"outside").expect("target");
    fs::set_permissions(&target, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .expect("target permissions");
    symlink(&target, &path).expect("journal symlink");
    let error = JsonlEventLog::open(&path).expect_err("symlink must be denied");
    assert!(error.to_string().contains("symlink"));
    assert_eq!(fs::read(&target).expect("target read"), b"outside");
    cleanup(&path);
    cleanup(&target);
}

#[cfg(unix)]
#[tokio::test]
async fn journal_hardlink_is_rejected_without_mutating_sibling() {
    let path = temp_path("hardlink");
    let sibling = temp_path("hardlink-sibling");
    fs::write(&sibling, b"outside").expect("sibling");
    fs::set_permissions(&sibling, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .expect("sibling permissions");
    fs::hard_link(&sibling, &path).expect("journal hardlink");
    let error = JsonlEventLog::open(&path).expect_err("hardlink must be denied");
    assert!(error.to_string().contains("hardlink"));
    assert_eq!(fs::read(&sibling).expect("sibling read"), b"outside");
    cleanup(&path);
    cleanup(&sibling);
}

#[cfg(unix)]
#[tokio::test]
async fn broad_permissions_are_rejected_for_journal_and_lock_files() {
    let journal = temp_path("permissions-journal");
    fs::write(&journal, b"").expect("journal");
    fs::set_permissions(&journal, std::os::unix::fs::PermissionsExt::from_mode(0o640))
        .expect("journal permissions");
    let error = JsonlEventLog::open(&journal).expect_err("broad journal mode must be denied");
    assert!(error.to_string().contains("permissions"));
    cleanup(&journal);

    let lock = temp_path("permissions-lock");
    let lock_path = lock.with_extension("jsonl.lock");
    fs::write(&lock_path, b"").expect("lock");
    fs::set_permissions(&lock_path, std::os::unix::fs::PermissionsExt::from_mode(0o640))
        .expect("lock permissions");
    let error = JsonlEventLog::open(&lock).expect_err("broad lock mode must be denied");
    assert!(error.to_string().contains("permissions"));
    cleanup(&lock);
}
