//! Kiana 的事件存储 adapter（适配器）及若干相关本地存储实现。
//!
//! 【本 crate 在系统中的位置】
//! ```text
//! DaemonHost 装配 Core 与具体存储实现
//!       ↓ 注入 `Arc<dyn EventStorePort>`
//! ControlPlane 通过 EventStorePort 提交 RuntimeEvent / TransitionBatch
//!       ↓
//! 【本 crate】JsonlEventLog 或 MemoryEventLog 实现端口契约
//!       ↓
//! recovery / audit / receipt / query 等读取方按各自用途读取事件事实
//! ```
//!
//! 本 crate **不是全仓唯一的事实写入点**：写入接口定义在 `kiana-ports::EventStorePort`，
//! 事件语义和授权写入由 `kiana-core::ControlPlane` 负责，Daemon 决定装配哪个 adapter。
//! 本 crate 提供端口实现和相关的存储 helper；它既不决定一项业务是否获准，也不替代 Core
//! 对事件内容、权限和状态转换的校验。
//!
//! 【事件事实与投影的关系】
//! run 生命周期事件通过 EventStore 记录，Receipt、UI timeline、索引等读取模型可由这些
//! 事件重新计算或展示；它们不能反过来取代 Core/EventStore 的权威状态。不过，“写入了事件”
//! 只证明 adapter 按自身契约接受并记录了该数据，不自动证明现实世界的副作用正确。
//! 具体功能达到 `local_behavior`、`durable` 或更高证明等级，必须以 `CURRENT_STATUS.md`
//! 和对应的可核验证据为准，不能只从 adapter 类型名或此处注释推出。
//!
//! 【两个 EventStore adapter 的区别】
//!
//! | Adapter | 数据位置与生命周期 | 适用边界 |
//! |---|---|---|
//! | [`JsonlEventLog`] | 按安全约束写入 JSONL 文件；提供本地落盘与重开读取路径 | Daemon 本地装配使用；持久性结论仍需对应运行证据 |
//! | [`MemoryEventLog`] | 事件仅存在于当前进程内存，进程退出即丢失 | 测试、确定性本地流程和显式注入的临时状态 |
//!
//! 两者共享 stream、幂等与事务转换语义，但存储介质和崩溃后的可恢复性不同。
//! JSONL 文件不是密码学上不可篡改的存储；读入时会检查结构/完整性，并按实现处理尾部
//! 截断记录。内存实现不会落盘。不能把“接口行为相同”误写成“持久性相同”。
//!
//! 【其他公开实现】
//! `artifact_store`、`credential_rotation`、`memory_store`、通知去重/outbox、保留策略和
//! workflow queue 提供各自端口或领域对象需要的本地实现；它们不都是 EventStore。`integrity`
//! 扫描 JSONL 文件，`writer_queue` 管理有界写入者准入/关闭/接管状态，不直接替代 event adapter。
//! 写入队列满时必须显式拒绝准入，不能静默丢失已接受的工作。
//!
//! 【初学者阅读顺序】
//!
//! 1. 先看 `kiana-ports::EventStorePort`：它定义 Core 可以请求的 append/read/CAS 语义，
//!    不是某一种文件格式。
//! 2. 再看 `event_store_core`、`journal_core` 和 `stream`：它们解释事件顺序、事务批次、
//!    观察者包装和 stream 投影如何组合；这些模块仍不决定业务授权。
//! 3. 最后看 `jsonl` 与 `memory`：前者把事件写成可重新打开的 JSONL，后者只保存进程内
//!    状态。两者必须遵守同一个端口契约，但故障恢复证据不同。
//! 4. `integrity`、`retention_store`、`notification_outbox`、`workflow_queue` 等是围绕
//!    事实存储的独立本地能力；它们的存在不表示对应的业务流程已经被所有入口接通。
//!
//! 一条事件写入通常经历：`ControlPlane` 先做授权/状态转换和事件载荷校验，调用
//! `EventStorePort`，组合根注入的 adapter 再执行内存更新或文件写入；读取方从事件事实构造
//! receipt、审计、恢复或 UI 投影。adapter 返回成功只说明“本地端口接受了这次操作”，不替
//! Core 证明网络副作用、模型行为或现实世界状态已经发生。

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

// ============================================================================
// 公开导出：外界只能访问这些类型和函数
// ============================================================================

// 【关键适配器】
pub use artifact_store::MemoryArtifactStore; // 产物存储适配器
pub use credential_rotation::MemoryCredentialRotationStore; // 凭证轮换
pub use integrity::{scan_jsonl, IntegrityScanReport, IntegrityScanStatus}; // 完整性检查
pub use jsonl::JsonlEventLog; // 落盘 append-only 实现
pub use ledger_source::{
    read_billing_page, read_ledger_page, BillingSourcePage, BILLING_SOURCE_IS_READ_ONLY,
}; // 账本类只读接口
pub use memory::MemoryEventLog; // 进程内事件存储（测试用）
pub use memory_store::MemoryDataGovernanceStore; // 治理契约存储
pub use notification_dedup::MemoryNotificationDedupStore; // 通知去重
pub use notification_outbox::MemoryNotificationOutboxStore; // 通知发件箱
pub use retention_store::MemoryRetentionStore; // 保留策略
pub use stream::{CommitObservedEventStore, StreamEventStore}; // 观察者装饰器
pub use workflow_queue::MemoryWorkflowQueueStore; // Workflow 队列

// 【写入队列的完整 API】
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

// ============================================================================
// 路径解析：找到默认的事件日志位置
// ============================================================================

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
///
/// 【调用者】Daemon 组合根在没有显式存储路径时调用，随后由 `JsonlEventLog::open` 等构造器
/// 继续检查父目录、文件类型和权限。这个函数只计算路径，不创建目录、不创建文件、不验证
/// JSONL 内容，也不把返回路径注册成全局事实源。
///
/// 【失败情况】`KIANA_HOME` 存在但不是绝对路径时返回 `kiana_home_must_be_absolute`；环境中
/// 没有 `KIANA_HOME` 且没有 `HOME` 时返回 `kiana_home_required`。这些是配置边界错误，不应
/// 被调用方吞掉后随意退回当前工作目录，否则重启后可能读写另一份事件账本。
///
/// 【为什么拼接固定后缀】所有会话事件必须落在 `<home>/sessions/events.jsonl`，让不同入口
/// 共享同一默认位置；它不是项目根，也不是临时 cache。改变后缀会让旧事件看似“消失”，应
/// 通过显式迁移/配置处理，而不是在这里静默尝试多个位置。
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

// ============================================================================
// 测试套件
// ============================================================================
//
// 【测试的组织】
// 以下这些测试覆盖了事件存储的两个实现：
// 1. `MemoryEventLog` — 进程内，主要用于快速反馈和确定性测试
// 2. `JsonlEventLog` — 落盘，主要测试持久化语义和重启恢复
//
// 【关键测试场景】
// - 单一请求内的事件顺序保证
// - 非单调序号的拒绝
// - 重复事件 ID 的检测
// - 幂等性：同一事件重复 append 只保存一次
// - 并发写入时的互斥
// - JSONL 文件的破损恢复
// - 符号链接的拒绝（安全）

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{RequestId, RuntimeEvent};
    use kiana_ports::EventStorePort;
    use serde_json::Value;
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    // =========================================================================
    // 测试辅助函数
    // =========================================================================

    /// 生成临时日志文件的路径。
    ///
    /// 【用途】每个测试创建独立的 JSONL 文件，不互相污染。
    /// 【策略】以当前时间戳 + 纳秒作为文件名，几乎不可能重复。
    fn temp_log() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kiana-eventlog-{stamp}.jsonl"))
    }

    /// 以生产写入器的权限（0o600，只有主人可读可写）写入 JSONL 测试夹具。
    ///
    /// 【为什么需要这个 helper】
    /// `fs::write` 建出的文件权限是 0o644（group 和 other 可读），
    /// 但 `JsonlEventLog::open` 在 unix 上会调用 `validate_unix_storage_file`，
    /// 检查 `mode & 0o077 != 0`（即检查是否 group/other 可读），如果是就拒绝打开。
    ///
    /// 【为什么这个安全检查是对的】
    /// - EventLog 包含所有执行历史，是极度敏感的数据
    /// - World-readable 的事件日志等于数据泄漏
    /// - 生产写入器确实用 0o600 建文件（见 `jsonl.rs`）
    ///
    /// 【所以这个 helper 的作用】
    /// - 让测试夹具符合生产契约
    /// - 不是「放宽安全检查」，而是「造出合法的输入」
    #[cfg(unix)]
    fn write_fixture(path: &Path, contents: impl AsRef<str>) {
        let contents = contents.as_ref();
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
    fn write_fixture(path: &Path, contents: impl AsRef<str>) {
        fs::write(path, contents).unwrap();
    }

    #[tokio::test]
    async fn append_order_is_preserved_per_request() {
        // 【目标】验证同一个 request 内的事件顺序不会乱
        // 【设置】创建两个事件，序号 1 和 2
        // 【验证】读出来的顺序必须和写入顺序一致
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
        // 【目标】如果事件序号不是严格递增，必须拒绝
        // 【场景】先 append 序号 2，再试图 append 序号 1（非单调）
        // 【预期】第二个 append 应该返回 Conflict 错误
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
        // 【目标】同一个事件 ID 不能被写两次，即使用了更大的序号
        // 【关键点】事件 ID 是事件的唯一标识符（由创建时生成），
        //         序号只是这个 request 内的逻辑顺序。
        //         ID 重复意味着这是同一个事件试图重复提交。
        // 【验证】
        //   1. 写第一个事件（id_1, seq=1）成功
        //   2. 修改同一个事件，改变序号为 2，再试图写
        //   3. 应该返回 Conflict("event_id_duplicate")
        //   4. 最终只有 1 个事件被保存
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
        // 【目标】JSONL 落盘的持久性：重新打开同一个文件，旧事件还在
        // 【关键】不能 truncate，只能 append
        // 【场景】
        //   1. 第一次打开：写 request_1 的一个事件
        //   2. 关闭
        //   3. 第二次打开同一文件：再写 request_2 的一个事件
        //   4. 验证：文件里有 2 个事件，顺序正确
        // 【这验证了什么】JSONL 是真的 append-only，不会丢老数据
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
        // 【目标】破损的 JSON 行应该导致打开失败，而不是静默跳过
        // 【场景】JSONL 文件包含一行无法反序列化为 RuntimeEvent 的 JSON
        // 【预期】JsonlEventLog::open 返回错误，错误里包含 "eventlog_corrupt"
        // 【设计原则】fail-closed：宁可拒绝，不要给一个可能错的答案
        let path = temp_log();
        write_fixture(&path, "{\"kind\":\"not-an-event\"}\n");
        let error = JsonlEventLog::open(&path).unwrap_err();
        assert!(error.to_string().contains("eventlog_corrupt"), "{error}");
    }

    #[tokio::test]
    async fn jsonl_repairs_a_torn_final_line_and_preserves_prior_events() {
        // 【目标】文件最后一行被截断了（电源关闭、崩溃等），要能恢复
        // 【场景】
        //   1. 写一个完整的事件到文件（有完整的 \n）
        //   2. 再写半个 JSON（{\"event_id\": 没完整），模拟崩溃中断
        //   3. 重新打开文件
        // 【预期】
        //   - 成功打开（自动删除不完整的最后一行）
        //   - 能读出第一个完整的事件
        //   - 再 append 新事件，文件恢复到可用状态
        // 【这验证了什么】
        //   - 对破损有容错
        //   - 但破损的数据被无情地抛弃（fail-safe，不 fail-open）
        //   - 优先保持一致性而不是最大化数据恢复
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
        // 【目标】`append_expected` 是一个乐观并发控制方法
        // 【用法】expected_version = 流当前的版本号，如果不匹配则拒绝
        // 【场景】空流（还没有事件），预期版本 0，新增版本 1 的事件
        // 【预期】成功
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
        // 【目标】`append_expected` 的乐观锁保证：同一个流的版本只有一个 writer 能推进
        // 【场景】两个并发任务同时尝试给同一个 stream 写 event
        //   - 都预期当前版本是 0（空流）
        //   - 都想写 sequence=1 的 event
        //   - 但 event kind 不同（"first" vs "second"）
        // 【预期】
        //   - 恰好一个成功
        //   - 另一个返回 Conflict("event_stream_version_mismatch")
        //   - 最终流里只有 1 个事件
        // 【这防止了什么】
        //   - Lost update：两个 writer 都认为自己是 seq=1
        //   - 数据竞争导致的覆盖或乱序
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
        // 【目标】幂等性：同一个事件（带幂等性 key）重复 append，只保存一次
        // 【用法】
        //   1. 第一次 append_idempotent(event) → 成功写入
        //   2. 完全相同的事件再 append_idempotent(event) → 不再写入，但返回「已存在」标记
        // 【场景】网络重试或分布式系统的「至少一次」语义
        //   - Worker A 发送「run accepted」
        //   - Worker A 超时，重新发送同一个请求
        //   - EventLog 识别出 idempotency_key 相同，拒绝第二次的真实写入
        // 【验证】
        //   - 第一次返回 `!replayed`
        //   - 第二次返回 `replayed = true`（表示这是一个重播）
        //   - event_id 相同
        //   - 存储里只有 1 条记录
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
