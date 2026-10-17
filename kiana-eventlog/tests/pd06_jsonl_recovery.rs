use kiana_domain::{
    canonical_journal_bytes, AggregateVersion, JournalFrame, JournalFramePayload, JournalHeader,
    RequestId, RuntimeEvent, TransitionBatch, MAX_JOURNAL_LOG_BYTES,
};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::EventStorePort;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 以生产写入器使用的私有权限（0o600）写入 JSONL 夹具。
///
/// `fs::write` 建出的文件是 0o644（group/other 可读），而 `JsonlEventLog::open`
/// 在 unix 上会以 `mode & 0o077 != 0` 判定 `eventlog_permissions_too_broad` 并拒绝打开。
/// 那是正确的安全检查——world-readable 的事件日志本身就是泄漏面，生产写入器也确实
/// 用 `openat(..., 0o600)` 建文件。这里修的是夹具：让它们按生产契约造出合法输入。
fn write_fixture(path: &Path, contents: impl AsRef<[u8]>) {
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
        file.write_all(contents).unwrap();
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents).unwrap();
    }
}

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd06-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("jsonl.lock"));
}

#[tokio::test]
async fn jsonl_frame_checksum_and_torn_tail_recovery_are_bounded() {
    let torn = temp_path("tail");
    let event =
        RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({"safe": true})).unwrap();
    let encoded = serde_json::to_string(&event).unwrap();
    write_fixture(&torn, format!("{encoded}\n{{\"event_id\":"));
    let store = JsonlEventLog::open(&torn).unwrap();
    assert_eq!(store.read_all().await.unwrap(), vec![event]);
    assert_eq!(fs::read_to_string(&torn).unwrap(), format!("{encoded}\n"));
    drop(store);
    cleanup(&torn);

    let tampered = temp_path("checksum");
    let frame = JournalFrame::new(JournalFramePayload::Event {
        event: RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap(),
    })
    .unwrap();
    let header = serde_json::to_string(&JournalHeader::default()).unwrap();
    let mut body = serde_json::to_value(frame).unwrap();
    body["body_sha256"] = json!("0".repeat(64));
    write_fixture(
        &tampered,
        format!("{}\n{}\n", header, serde_json::to_string(&body).unwrap()),
    );
    assert!(JsonlEventLog::open(&tampered)
        .unwrap_err()
        .to_string()
        .contains("eventlog_corrupt"));
    cleanup(&tampered);
}

#[test]
fn jsonl_v2_rejects_duplicate_wire_keys_before_header_or_frame_decode() {
    let duplicate = temp_path("duplicate-key");
    write_fixture(
        &duplicate,
        br#"{"schema":"kiana.journal-header.v2","schema":"kiana.journal-header.v2","required":true,"writer_version":2}
"#,
    );
    let error = JsonlEventLog::open(&duplicate).unwrap_err();
    assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    cleanup(&duplicate);

    let nested = temp_path("nested-duplicate-key");
    let frame = JournalFrame::new(JournalFramePayload::Event {
        event: RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap(),
    })
    .unwrap();
    let encoded = serde_json::to_string(&frame).unwrap().replacen(
        r#""body":{"type":"event","event":"#,
        r#""body":{"type":"event","type":"event","event":"#,
        1,
    );
    assert!(encoded.contains(r#""type":"event","type":"event""#));
    write_fixture(
        &nested,
        format!(
            "{}\n{encoded}\n",
            serde_json::to_string(&JournalHeader::default()).unwrap()
        ),
    );
    let error = JsonlEventLog::open(&nested).unwrap_err();
    assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    cleanup(&nested);
}

#[test]
fn jsonl_v2_source_contract_keeps_checksum_sync_lock_and_bounded_tail() {
    let source = include_str!("../src/jsonl.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let journal_core = include_str!("../src/journal_core.rs");
    for marker in [
        "JournalHeader",
        "JournalFrame",
        ".accept_frame(frame)",
        "sync_all",
        "sync_directory",
        "libc::flock",
        "O_APPEND",
        "O_NOFOLLOW",
        "eventlog_repair_failed",
        "MAX_JOURNAL_LOG_BYTES",
        "uncertain_write",
        "parse_json_value_reject_duplicates",
        "eventlog_parent_changed",
        "eventlog_symlink_rejected",
        "eventlog_hardlink_rejected",
        "eventlog_disk_limit",
        "eventlog_worker_failed",
        "eventlog_worker_queue_full",
        "eventlog_closed",
        "eventlog_file_removed",
        "eventlog_committed_prefix_truncated",
    ] {
        assert!(
            source.contains(marker),
            "JSONL durability marker missing: {marker}"
        );
    }
    for marker in [
        "body_sha256",
        "encoded.len() as u64 != self.body_len",
        "journal_sha256(&encoded) != self.body_sha256",
        "journal_frame_integrity_failed",
    ] {
        assert!(
            journal.contains(marker),
            "journal frame integrity marker missing: {marker}"
        );
    }
    let (_, accept_frame) = journal_core
        .split_once("pub fn accept_frame")
        .expect("journal core must accept decoded frames");
    let validation = accept_frame
        .find("frame.validate().map_err(invalid)?;")
        .expect("journal core must validate frame integrity");
    let body = accept_frame
        .find("match frame.body")
        .expect("journal core must apply validated frame contents");
    assert!(validation < body, "frame integrity must precede body apply");
}

/// 拒绝路径：文件中间（而非可修复尾部）出现损坏记录时，读取必须整体失败，
/// 不得裁剪文件，也不得把损坏行之后的记录当作已提交事实暴露。
#[test]
fn jsonl_v2_rejects_a_corrupt_middle_record_without_truncating_or_exposing() {
    let middle = temp_path("middle");
    let first = JournalFrame::new(JournalFramePayload::Event {
        event: RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({"n": 1})).unwrap(),
    })
    .unwrap();
    let second = JournalFrame::new(JournalFramePayload::Event {
        event: RuntimeEvent::new(RequestId::new(), 2, "run.completed", json!({"n": 2})).unwrap(),
    })
    .unwrap();
    let first_encoded = String::from_utf8(canonical_journal_bytes(&first).unwrap()).unwrap();
    let second_encoded = String::from_utf8(canonical_journal_bytes(&second).unwrap()).unwrap();
    let contents = format!(
        "{}\n{}\n{{\"schema\":\"kiana.transition-frame.v1\" not-json}}\n{}\n",
        serde_json::to_string(&JournalHeader::default()).unwrap(),
        first_encoded,
        second_encoded,
    );
    write_fixture(&middle, contents.clone());
    let error = JsonlEventLog::open(&middle).unwrap_err();
    assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    // 中间损坏是 committed prefix 的一部分：只能报错，不能复用“可修复尾部”的裁剪逻辑。
    assert_eq!(fs::read_to_string(&middle).unwrap(), contents);
    cleanup(&middle);
}

/// 拒绝路径：打开后父目录被整体替换（rename + 重建同名目录）时，新目录不得收到任何新事实。
/// 这是 `verify_parent` 依固定目录描述符判定身份的结果，不是对路径字符串的信任。
#[cfg(unix)]
#[tokio::test]
async fn jsonl_v2_refuses_directory_replacement_after_open() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-pd06-dir-{stamp}"));
    let parent = root.join("sessions");
    let moved = root.join("sessions-original");
    let path = parent.join("events.jsonl");
    fs::create_dir_all(&parent).unwrap();

    let first = RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({"n": 1})).unwrap();
    let store = JsonlEventLog::open(&path).unwrap();
    store.append(first.clone()).await.unwrap();

    fs::rename(&parent, &moved).unwrap();
    fs::create_dir(&parent).unwrap();
    let second = RuntimeEvent::new(RequestId::new(), 2, "run.completed", json!({"n": 2})).unwrap();
    let error = store.append(second).await.unwrap_err();
    assert!(
        error.to_string().contains("eventlog_parent_changed"),
        "{error}"
    );

    // 已提交前缀仍在被换走的原目录里；替换后的同名目录没有收到第二条事实。
    assert_eq!(
        fs::read_to_string(moved.join("events.jsonl")).unwrap(),
        format!("{}\n", serde_json::to_string(&first).unwrap())
    );
    assert!(!path.exists(), "replacement parent must not receive new facts");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

/// 拒绝路径：进程锁文件被换成符号链接或硬链接时，下一次操作必须 fail-closed，
/// 不能在一个身份可疑的锁上继续追加事实。
#[cfg(unix)]
#[tokio::test]
async fn jsonl_v2_refuses_a_replaced_or_hardlinked_writer_lock() {
    use std::os::unix::fs::symlink;

    let path = temp_path("lock");
    let store = JsonlEventLog::open(&path).unwrap();
    store
        .append(RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap())
        .await
        .unwrap();
    let lock_path = path.with_extension("jsonl.lock");
    assert!(lock_path.exists());

    fs::remove_file(&lock_path).unwrap();
    symlink(&path, &lock_path).unwrap();
    let error = store
        .append(RuntimeEvent::new(RequestId::new(), 2, "run.started", json!({})).unwrap())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("eventlog_symlink_rejected"),
        "{error}"
    );

    fs::remove_file(&lock_path).unwrap();
    let other = temp_path("lock-target");
    write_fixture(&other, b"lock-target\n");
    fs::hard_link(&other, &lock_path).unwrap();
    let error = store
        .append(RuntimeEvent::new(RequestId::new(), 3, "run.started", json!({})).unwrap())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("eventlog_hardlink_rejected"),
        "{error}"
    );

    drop(store);
    cleanup(&path);
    cleanup(&other);
}

/// 拒绝路径：日志超过容量上限（磁盘满的等价上界）时，打开必须拒绝，
/// 不能把不可写/不可恢复的存储当作空库或正常库暴露给上层。
#[test]
fn jsonl_v2_refuses_an_over_capacity_log_before_exposing_any_fact() {
    let path = temp_path("capacity");
    {
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(&path).unwrap();
        // 稀疏文件：只扩展逻辑长度，不实际占用 512 MiB 磁盘块。
        file.set_len(MAX_JOURNAL_LOG_BYTES + 1).unwrap();
    }
    let error = JsonlEventLog::open(&path).unwrap_err();
    assert!(error.to_string().contains("eventlog_disk_limit"), "{error}");
    cleanup(&path);
}

/// 成功路径：Flush/Shutdown 返回可观察的 ack（健康回执携带 durable 游标与 closed 标志），
/// 关闭后新写入被拒绝；吞吐受有界 worker 限制，但拒绝是显式错误而不是静默丢数据。
#[tokio::test]
async fn jsonl_v2_flush_and_shutdown_acks_are_observable_and_close_rejects_writes() {
    let path = temp_path("ack");
    let store = JsonlEventLog::open(&path).unwrap();
    store
        .append(RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap())
        .await
        .unwrap();

    let flushed = store.flush().await.unwrap();
    assert!(flushed.durable);
    assert!(!flushed.closed);
    assert_eq!(flushed.last_durable_cursor, 1);
    assert!(flushed.validate().is_ok());

    let closed = store.close().await.unwrap();
    assert!(closed.closed);
    assert!(closed.validate().is_ok());

    let error = store
        .append(RuntimeEvent::new(RequestId::new(), 2, "run.started", json!({})).unwrap())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("eventlog_closed"), "{error}");
    cleanup(&path);
}

/// 成功路径：模拟进程在写完完整 v2 提交帧之后、写后续记录时被 kill -9，
/// 重开必须保留完整 prefix，只修复不完整的尾部字节，且重复重开结果一致。
#[tokio::test]
async fn jsonl_v2_reopen_keeps_the_complete_prefix_and_repairs_only_the_torn_tail() {
    let path = temp_path("v2-prefix");
    let request_id = RequestId::new();
    let first = RuntimeEvent::new(request_id, 1, "run.started", json!({"sequence": 1}))
        .unwrap()
        .with_stream_metadata("run", "run-v2-prefix", 1)
        .with_idempotency_key("pd06:v2-prefix:1");
    let batch = TransitionBatch {
        command_id: request_id,
        command_digest: "b".repeat(64),
        expected_versions: vec![AggregateVersion::new("run", "run-v2-prefix", 0)],
        events: vec![first.clone()],
    };

    let store = JsonlEventLog::open(&path).unwrap();
    assert!(store.commit_transition(batch).await.unwrap().receipt().is_some());
    drop(store);

    // 在完整提交帧之后追加一段不完整的帧前缀，等价于写入过程中被强杀留下的尾部。
    let committed = fs::read(&path).unwrap();
    let mut torn = committed.clone();
    torn.extend_from_slice(b"{\"schema\":\"kiana.transition-frame.v1\",\"body\":");
    write_fixture(&path, &torn);

    let reopened = JsonlEventLog::open(&path).unwrap();
    assert_eq!(reopened.read_all().await.unwrap(), vec![first.clone()]);
    // 完整 prefix 逐字节保留；只有不完整的尾部被裁掉。
    assert_eq!(fs::read(&path).unwrap(), committed);
    drop(reopened);

    let again = JsonlEventLog::open(&path).unwrap();
    assert_eq!(again.read_all().await.unwrap(), vec![first]);
    drop(again);
    cleanup(&path);
}
