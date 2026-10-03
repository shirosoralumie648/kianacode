//! Kiana 的 **append-only 事件存储适配器**。
//!
//! # 这个 crate 是什么
//!
//! 它是全仓库**唯一**能写事实的地方。控制面判定了一件事之后，那件事要变成
//! 「永久可查、不可篡改」的记录，就只能经过这里。
//!
//! 所以整个仓库里到处都能看到同一句话的不同说法——「显示不等于事实」
//! 「翻译不等于记账」「Receipt 不是日志」。它们都在描述同一个边界：
//! **EventLog 是事实的边界，其它都是投影。**
//!
//! # 两类适配器，注意它们的地位完全不同
//!
//! | | 是什么 | 能不能作为证据 |
//! |---|---|---|
//! | [`JsonlEventLog`] | 落盘的、真的 append-only | 这是「持久」的含义所在 |
//! | [`MemoryEventLog` | 进程内，退出即消失 | 只能用于测试与临时投影 |
//!
//! ⚠ 内存实现**不是**「轻量的持久实现」。它没有落盘、没有 fsync、
//!   进程结束就没了。用它跑出来的一切，**只能**算 local_behavior 及以下，
//!   永远不能被写成 durable —— 这个区分不是注释里的客套，是结论能否成立的前提。
//!
//! # 这个 crate 里的其它模块
//!
//! 它们大多不是「事件存储」本身，而是围绕它的契约：产物存储、凭据轮换、
//! 审计契约、治理契约、完整性扫描、通知去重与 outbox、保留策略、workflow 队列、
//! 以及 PD-27 那个有界写入队列。
//!
//! 有界写入队列（`writer_queue`）值得单独一提：队列满了要**可观察地拒绝**，
//! 而不是静默丢事件——静默丢失会让人以为「没发生的事真的没发生」。
//! Append-only event storage adapters for Kiana.

mod artifact_store;
mod audit_contract;
mod credential_rotation;
mod event_store_core;
mod governance_contract;
mod integrity;
mod journal_core;
mod jsonl;
mod ledger_source;
mod memory;
mod memory_store;
mod notification_dedup;
mod notification_outbox;
mod retention_store;
mod stream;
mod workflow_queue;
mod writer_queue;

pub use artifact_store::MemoryArtifactStore;
pub use credential_rotation::MemoryCredentialRotationStore;
pub use integrity::{scan_jsonl, IntegrityScanReport, IntegrityScanStatus};
pub use jsonl::JsonlEventLog;
pub use ledger_source::{
    read_billing_page, read_ledger_page, BillingSourcePage, BILLING_SOURCE_IS_READ_ONLY,
};
pub use memory::MemoryEventLog;
pub use memory_store::MemoryDataGovernanceStore;
pub use notification_dedup::MemoryNotificationDedupStore;
pub use notification_outbox::MemoryNotificationOutboxStore;
pub use retention_store::MemoryRetentionStore;
pub use stream::{CommitObservedEventStore, StreamEventStore};
pub use workflow_queue::MemoryWorkflowQueueStore;
pub use writer_queue::{
    WriterAdmission, WriterAdmissionReport, WriterAdmissionStatus, WriterQueuePolicy,
    WriterQueueState, WriterRegistry, WriterShutdown, WriterShutdownMode, WriterTakeover,
    WriterTakeoverReport, MAX_OUTSTANDING_LEASES, MAX_QUEUE_WRITERS, MAX_WRITER_LABEL,
    WRITER_ADMISSION_REPORT_SCHEMA, WRITER_ADMISSION_SCHEMA, WRITER_QUEUE_POLICY_SCHEMA,
    WRITER_QUEUE_VERSION, WRITER_REGISTRY_SCHEMA, WRITER_SHUTDOWN_SCHEMA,
    WRITER_TAKEOVER_REPORT_SCHEMA, WRITER_TAKEOVER_SCHEMA,
};

use kiana_ports::PortError;
use std::path::{Path, PathBuf};

/// 决定会话事件默认落在哪个文件。
///
/// ```text
/// KIANA_HOME 设了 → 用它
/// 否则            → $HOME/.kiana
/// 最终路径         → <上面那个目录>/sessions/events.jsonl
/// ```
///
/// 【⚠ `KIANA_HOME` 必须是绝对路径，否则直接拒绝】
/// 这一条是整个函数里最容易被「顺手放宽」的地方。
/// 一个相对的 `KIANA_HOME` 会按**进程当时的工作目录**解析，
/// 于是同一份配置在不同目录下会指向不同的文件——
/// 数据散落在「启动它时恰好在哪」，而事后没人说得清该去哪找。
///
/// 和 `dispatch.rs` 里 `project_root_identity` 是同一条纪律：
/// **拿不准的时候，宁可拒绝，也不要给一个看起来能用的答案。**
pub fn default_sessions_log_path() -> Result<PathBuf, PortError> {
    let home = if let Ok(value) = std::env::var("KIANA_HOME") {
        let home = PathBuf::from(value.trim());
        if !home.is_absolute() {
            return Err(PortError::Failed("kiana_home_must_be_absolute".to_owned()));
        }
        home
    } else {
        let home = std::env::var("HOME")
            .map_err(|_| PortError::Failed("kiana_home_required".to_owned()))?;
        PathBuf::from(home).join(".kiana")
    };
    Ok(home.join("sessions").join("events.jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{RequestId, RuntimeEvent};
    use kiana_ports::EventStorePort;
    use serde_json::Value;
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_log() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kiana-eventlog-{stamp}.jsonl"))
    }

    /// 以生产写入器使用的私有权限（0o600）写入 JSONL 夹具。
    ///
    /// 【为什么要专门写这个 helper】
    /// `fs::write` 建出来的文件是 0o644（group/other 可读），而 `JsonlEventLog::open`
    /// 在 unix 上会调用 `validate_unix_storage_file`，以
    /// `mode & 0o077 != 0` 判定 `eventlog_permissions_too_broad` 并拒绝打开。
    /// 那是**正确**的安全检查——一个 world-readable 的事件日志本身就是泄漏面，
    /// 生产写入器也确实用 0o600 建文件（`jsonl.rs` 的 `openat(..., 0o600)`）。
    /// 所以这里修的是夹具：让它们按生产契约造出合法的输入，而不是放宽那条检查。
    fn write_fixture(path: &Path, contents: impl AsRef<str>) {
        let contents = contents.as_ref();
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            file.write_all(contents.as_bytes()).unwrap();
        }
        #[cfg(not(unix))]
        {
            fs::write(path, contents).unwrap();
        }
    }

    #[tokio::test]
    async fn append_order_is_preserved_per_request() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        store
            .append(RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap())
            .await
            .unwrap();
        store
            .append(RuntimeEvent::new(request_id, 2, "completed", Value::Null).unwrap())
            .await
            .unwrap();
        let events = store.read_request(&request_id).await.unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            ["accepted", "completed"]
        );
    }

    #[tokio::test]
    async fn duplicate_or_non_monotonic_sequence_is_rejected() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        store
            .append(RuntimeEvent::new(request_id, 2, "running", Value::Null).unwrap())
            .await
            .unwrap();
        let error = store
            .append(RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Conflict("event_sequence_not_monotonic".to_owned())
        );
    }

    #[tokio::test]
    async fn duplicate_event_id_is_rejected_before_append() {
        // 相同事件 ID 即使使用更大的序号也不能再次写入事实源。
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        let first = RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap();
        store.append(first.clone()).await.unwrap();

        let mut duplicate = first;
        duplicate.sequence = 2;
        let error = store.append(duplicate).await.unwrap_err();
        assert_eq!(error, PortError::Conflict("event_id_duplicate".to_owned()));
        assert_eq!(store.read_request(&request_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn jsonl_survives_reopen_and_does_not_truncate_the_first_run() {
        let path = temp_log();
        let first_request = RequestId::new();
        let second_request = RequestId::new();
        {
            let store = JsonlEventLog::open(&path).unwrap();
            store
                .append(RuntimeEvent::new(first_request, 1, "run.completed", Value::Null).unwrap())
                .await
                .unwrap();
        }
        {
            let store = JsonlEventLog::open(&path).unwrap();
            store
                .append(RuntimeEvent::new(second_request, 1, "run.completed", Value::Null).unwrap())
                .await
                .unwrap();
            let all = store.read_all().await.unwrap();
            assert_eq!(all.len(), 2);
            assert_eq!(all[0].request_id, first_request);
            assert_eq!(all[1].request_id, second_request);
        }
        let reopened = JsonlEventLog::open(&path).unwrap();
        let all = reopened.read_all().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].request_id, first_request);
    }

    #[test]
    fn jsonl_corrupt_line_fails_closed() {
        let path = temp_log();
        write_fixture(&path, "{\"kind\":\"not-an-event\"}\n");
        let error = JsonlEventLog::open(&path).unwrap_err();
        assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    }

    #[tokio::test]
    async fn jsonl_repairs_a_torn_final_line_and_preserves_prior_events() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap();
        let encoded = serde_json::to_string(&first).unwrap();
        write_fixture(&path, format!("{encoded}\n{{\"event_id\":"));

        let store = JsonlEventLog::open(&path).unwrap();
        assert_eq!(store.read_all().await.unwrap(), vec![first.clone()]);
        let repaired = fs::read_to_string(&path).unwrap();
        assert_eq!(repaired, format!("{encoded}\n"));

        let second = RuntimeEvent::new(request_id, 2, "run.completed", Value::Null).unwrap();
        store.append(second.clone()).await.unwrap();
        assert_eq!(
            store.read_request(&request_id).await.unwrap(),
            vec![first, second]
        );
    }

    #[tokio::test]
    async fn jsonl_repairs_a_valid_unterminated_final_line_before_append() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap();
        let first_encoded = serde_json::to_string(&first).unwrap();
        write_fixture(&path, &first_encoded);

        let second = RuntimeEvent::new(request_id, 2, "run.completed", Value::Null).unwrap();
        let second_encoded = serde_json::to_string(&second).unwrap();
        {
            let store = JsonlEventLog::open(&path).unwrap();
            assert_eq!(store.read_all().await.unwrap(), vec![first.clone()]);
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                format!("{first_encoded}\n")
            );
            store.append(second.clone()).await.unwrap();
        }

        let reopened = JsonlEventLog::open(&path).unwrap();
        assert_eq!(reopened.read_all().await.unwrap(), vec![first, second]);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{first_encoded}\n{second_encoded}\n")
        );
    }

    #[test]
    fn jsonl_malformed_final_complete_line_fails_closed() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap();
        let encoded = serde_json::to_string(&first).unwrap();
        write_fixture(&path, format!("{encoded}\n{{\"kind\":\"not-an-event\"}}"));
        let error = JsonlEventLog::open(&path).unwrap_err();
        assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    }

    #[test]
    fn jsonl_malformed_first_line_fails_closed() {
        let path = temp_log();
        write_fixture(&path, "{\"event_id\":");
        let error = JsonlEventLog::open(&path).unwrap_err();
        assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    }

    #[tokio::test]
    async fn append_expected_zero_on_empty_stream_succeeds() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        store
            .append_expected(
                RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap(),
                Some(0),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn append_expected_requires_matching_stream_version() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        store
            .append(RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap())
            .await
            .unwrap();

        let error = store
            .append_expected(
                RuntimeEvent::new(request_id, 2, "completed", Value::Null).unwrap(),
                Some(0),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Conflict("event_stream_version_mismatch".to_owned())
        );

        store
            .append_expected(
                RuntimeEvent::new(request_id, 2, "completed", Value::Null).unwrap(),
                Some(1),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn concurrent_expected_appends_allow_only_one_writer() {
        let store = Arc::new(MemoryEventLog::new());
        let request_id = RequestId::new();
        let first = store.append_expected(
            RuntimeEvent::new(request_id, 1, "first", Value::Null).unwrap(),
            Some(0),
        );
        let second = store.append_expected(
            RuntimeEvent::new(request_id, 1, "second", Value::Null).unwrap(),
            Some(0),
        );
        let (first, second) = tokio::join!(first, second);
        let results = [first, second];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    matches!(
                        result,
                        Err(PortError::Conflict(reason))
                            if reason == "event_stream_version_mismatch"
                    )
                })
                .count(),
            1
        );
        assert_eq!(store.read_request(&request_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn idempotent_retry_replays_without_appending() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_stream_metadata("request", request_id.to_string(), 1)
            .with_idempotency_key("request-accepted-1");
        let first = store.append_idempotent(event.clone()).await.unwrap();
        let retry = store.append_idempotent(event).await.unwrap();

        assert!(!first.replayed);
        assert!(retry.replayed);
        assert_eq!(first.event.event_id, retry.event.event_id);
        assert_eq!(store.read_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn idempotent_expected_retry_replays_before_cas_check() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_idempotency_key("expected-retry");
        let first = store
            .append_idempotent_expected(event.clone(), Some(0))
            .await
            .unwrap();
        let retry = store
            .append_idempotent_expected(event, Some(0))
            .await
            .unwrap();
        assert!(!first.replayed);
        assert!(retry.replayed);
        assert_eq!(store.read_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn idempotency_key_rejects_a_different_payload() {
        let store = MemoryEventLog::new();
        let request_id = RequestId::new();
        let first = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_idempotency_key("same-key");
        store.append_idempotent(first).await.unwrap();

        let second = RuntimeEvent::new(request_id, 1, "run.completed", Value::Null)
            .unwrap()
            .with_idempotency_key("same-key");
        let error = store.append_idempotent(second).await.unwrap_err();
        assert_eq!(
            error,
            PortError::Conflict("event_idempotency_key_payload_mismatch".to_owned())
        );
    }

    #[tokio::test]
    async fn jsonl_idempotent_retry_reloads_and_replays() {
        let path = temp_log();
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_idempotency_key("disk-key");
        let first = JsonlEventLog::open(&path).unwrap();
        let first_result = first
            .append_idempotent_expected(event.clone(), Some(0))
            .await
            .unwrap();
        let second = JsonlEventLog::open(&path).unwrap();
        let retry = second
            .append_idempotent_expected(event, Some(0))
            .await
            .unwrap();

        assert!(!first_result.replayed);
        assert!(retry.replayed);
        assert_eq!(retry.event.event_id, first_result.event.event_id);
        assert_eq!(second.read_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn concurrent_jsonl_idempotent_appends_create_one_record() {
        let path = temp_log();
        let request_id = RequestId::new();
        let event = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null)
            .unwrap()
            .with_idempotency_key("concurrent-key");
        let first = std::sync::Arc::new(JsonlEventLog::open(&path).unwrap());
        let second = std::sync::Arc::new(JsonlEventLog::open(&path).unwrap());
        let (left, right) = tokio::join!(
            first.append_idempotent(event.clone()),
            second.append_idempotent(event),
        );
        let results = [left.unwrap(), right.unwrap()];

        assert_eq!(results.iter().filter(|result| !result.replayed).count(), 1);
        assert_eq!(results.iter().filter(|result| result.replayed).count(), 1);
        assert_eq!(
            JsonlEventLog::open(&path)
                .unwrap()
                .read_all()
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn open_jsonl_instance_reads_events_appended_by_another_instance() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = JsonlEventLog::open(&path).unwrap();
        let second = JsonlEventLog::open(&path).unwrap();
        first
            .append(RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap())
            .await
            .unwrap();

        assert_eq!(second.read_request(&request_id).await.unwrap().len(), 1);
        assert_eq!(second.read_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn concurrent_jsonl_read_and_append_do_not_deadlock() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = std::sync::Arc::new(JsonlEventLog::open(&path).unwrap());
        let second = std::sync::Arc::new(JsonlEventLog::open(&path).unwrap());
        first
            .append(RuntimeEvent::new(request_id, 1, "run.started", Value::Null).unwrap())
            .await
            .unwrap();

        let read = second.read_all();
        let append =
            first.append(RuntimeEvent::new(request_id, 2, "run.completed", Value::Null).unwrap());
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let (read, append) = tokio::join!(read, append);
            (read.unwrap(), append.unwrap())
        })
        .await
        .expect("eventlog append/read lock-order deadlock");
        assert!(matches!(result.0.len(), 1 | 2));
        assert_eq!(second.read_all().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn jsonl_expected_append_reloads_under_process_lock() {
        let path = temp_log();
        let request_id = RequestId::new();
        let first = JsonlEventLog::open(&path).unwrap();
        let second = JsonlEventLog::open(&path).unwrap();
        first
            .append_expected(
                RuntimeEvent::new(request_id, 1, "first", Value::Null).unwrap(),
                Some(0),
            )
            .await
            .unwrap();
        second
            .append_expected(
                RuntimeEvent::new(request_id, 2, "second", Value::Null).unwrap(),
                Some(1),
            )
            .await
            .unwrap();
        assert_eq!(second.read_request(&request_id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn aggregate_stream_cas_spans_request_ids() {
        let store = MemoryEventLog::new();
        let first_request = RequestId::new();
        let second_request = RequestId::new();
        let first = RuntimeEvent::new(first_request, 1, "run.started", Value::Null)
            .unwrap()
            .with_stream_metadata("run", "run-1", 1)
            .with_idempotency_key("run-1:1");
        store.append_expected(first, Some(0)).await.unwrap();

        let second = RuntimeEvent::new(second_request, 1, "run.completed", Value::Null)
            .unwrap()
            .with_stream_metadata("run", "run-1", 2)
            .with_idempotency_key("run-1:2");
        store.append_expected(second, Some(1)).await.unwrap();

        let stale = RuntimeEvent::new(RequestId::new(), 1, "run.failed", Value::Null)
            .unwrap()
            .with_stream_metadata("run", "run-1", 2)
            .with_idempotency_key("run-1:stale");
        assert_eq!(
            store.append_expected(stale, Some(1)).await.unwrap_err(),
            PortError::Conflict("event_stream_version_mismatch".to_owned())
        );
        assert_eq!(store.read_all().await.unwrap().len(), 2);
        assert_eq!(store.read_stream("run", "run-1").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn append_expected_conflict_does_not_write_jsonl() {
        let path = temp_log();
        let request_id = RequestId::new();
        let store = JsonlEventLog::open(&path).unwrap();
        store
            .append(RuntimeEvent::new(request_id, 1, "accepted", Value::Null).unwrap())
            .await
            .unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let error = store
            .append_expected(
                RuntimeEvent::new(request_id, 2, "completed", Value::Null).unwrap(),
                Some(0),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            PortError::Conflict("event_stream_version_mismatch".to_owned())
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn jsonl_rejects_existing_symlink_without_mutating_its_target() {
        use std::os::unix::fs::symlink;

        let root = temp_log();
        let outside = temp_log();
        let request_id = RequestId::new();
        let existing = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap();
        fs::write(
            &outside,
            format!("{}\n", serde_json::to_string(&existing).unwrap()),
        )
        .unwrap();
        symlink(&outside, &root).unwrap();

        let error = JsonlEventLog::open(&root).unwrap_err();
        assert_eq!(
            error,
            PortError::Failed("eventlog_symlink_rejected".to_owned())
        );
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            format!("{}\n", serde_json::to_string(&existing).unwrap())
        );
        let _ = fs::remove_file(&root);
        let _ = fs::remove_file(root.with_extension("jsonl.lock"));
        let _ = fs::remove_file(outside);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn jsonl_append_rejects_path_replaced_by_symlink() {
        use std::os::unix::fs::symlink;

        let root = temp_log();
        let outside = temp_log();
        let request_id = RequestId::new();
        let existing = RuntimeEvent::new(request_id, 1, "run.accepted", Value::Null).unwrap();
        let next = RuntimeEvent::new(request_id, 2, "run.completed", Value::Null).unwrap();
        let store = JsonlEventLog::open(&root).unwrap();
        store.append(existing.clone()).await.unwrap();
        write_fixture(&outside, "outside\n");
        fs::remove_file(&root).unwrap();
        symlink(&outside, &root).unwrap();

        let error = store.append(next).await.unwrap_err();
        assert_eq!(
            error,
            PortError::Failed("eventlog_symlink_rejected".to_owned())
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside\n");
        let _ = fs::remove_file(&root);
        let _ = fs::remove_file(root.with_extension("jsonl.lock"));
        let _ = fs::remove_file(outside);
    }
}
