//! JSONL（每行一条 JSON 记录）事件存储 adapter（适配器），实现 `EventStorePort`。
//!
//! 【本模块在系统中的位置】
//! ```text
//! DaemonHost 创建 JsonlEventLog 并注入 ControlPlane
//!     ↓
//! ControlPlane 经 EventStorePort 提交事件或原子状态转换
//!     ↓
//! 【本模块】把事件/事务帧编码后写入 JSONL 文件，并维护可查询的内存索引
//!     ↓
//! 重新打开时从文件重放记录，供 Core 的恢复、收据和查询流程读取
//! ```
//!
//! `kiana-eventlog/src/memory.rs` 提供相同端口的进程内实现；它适合测试或显式注入，进程
//! 退出后数据消失。当前本地 Daemon 通常装配本模块。两者的接口相似不代表持久性相同。
//!
//! 【写入与读取的数据流】
//! `RuntimeEvent` / `TransitionBatch`
//! → 校验顺序、幂等键或转换前置条件
//! → 编码成单行 JSON（v2 事务帧带校验和）
//! → 写文件、flush、同步文件；Unix 上再同步目录并复核文件身份
//! → 更新 `DiskCache` 中的 JournalState、偏移量与游标
//! 读取时方向相反：从缓存增量读取新字节 → 严格解析 JSON → 校验帧形状和校验和
//! → 更新 `JournalState` → 由 `read_all`、`read_stream`、`read_command` 等端口方法返回。
//!
//! 【原子性、恢复与证明边界】
//! 单个 `TransitionBatch` 是否受支持由 `supports_atomic_transitions()` 报告；当前实现仅在
//! Unix 平台报告支持。事务帧可用于重放和校验，但写入失败时可能无法确定数据是否已落盘，
//! 因此提交会返回 `Unknown`，调用方必须核对命令收据，不能把未知结果当作可盲目重试的失败。
//! 启动重放时，符合条件的末尾不完整记录会被截断；已提交前缀损坏、帧校验失败或不支持的
//! 记录形状会报错。**这不是“事件永久可查”或“文件不可篡改”的保证**：文件系统、平台能力、
//! 崩溃时序和仓库证据共同决定实际证明等级，须查 `CURRENT_STATUS.md` 与对应回执。
//!
//! 【与相邻文件的关系】
//! `lib.rs` 暴露默认日志路径及 adapter；`journal_core.rs` 负责内存中的事件/事务索引和状态
//! 转换校验；`event_store_core.rs` 提供共用的追加计划及幂等键规则；`integrity.rs` 通过本模块
//! 读取文件并生成扫描报告。上游主要是 Daemon 装配的 ControlPlane，下游是读事件的 Core、
//! 收据和恢复逻辑；本模块只负责存储 adapter，不决定业务授权是否通过。
use crate::event_store_core::{validate_idempotency_key, AppendPlan};
use crate::journal_core::{append_result, capabilities, JournalState, TransitionPlan};
use async_trait::async_trait;
use kiana_domain::*;
use kiana_ports::{EventAppendResult, EventStorePort, PortError};
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::Semaphore;

// 每个 JsonlEventLog 实例最多允许 16 个阻塞存储任务同时占用工作线程准入名额。
// 超出时 with_store_state 会立即返回队列已满，而不是让 Tokio 的 spawn_blocking 队列
// 无限积压；这会把背压（backpressure，即下游处理跟不上时向调用方施加限制）显式传回。
const MAX_STORAGE_WORKERS: usize = 16;
fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}
fn io_error(code: &str, e: std::io::Error) -> PortError {
    failed(format!("{code}:{e}"))
}
fn unknown(command_id: RequestId, reason: impl ToString) -> CommitOutcome {
    CommitOutcome::Unknown {
        command_id,
        reason: reason.to_string(),
    }
}

#[derive(Debug)]
struct JournalFiles {
    path: PathBuf,
    parent: PathBuf,
    #[cfg(unix)]
    directory: File,
}
impl JournalFiles {
    fn open(path: PathBuf) -> Result<Self, PortError> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        create_parent_directories(&parent)?;
        #[cfg(unix)]
        let directory = open_directory(&parent)?;
        Ok(Self {
            path,
            parent,
            #[cfg(unix)]
            directory,
        })
    }
    /// 检查当前父目录路径是否仍指向打开 `JournalFiles` 时固定的目录对象。
    ///
    /// Unix 上，`open_directory` 持有目录文件描述符；本方法对路径执行 `symlink_metadata`，
    /// 再与该文件描述符的 device/inode 快照比较。若路径已经被换成符号链接或另一个目录，
    /// 后续文件操作会拒绝继续。`open_name` 又通过固定目录描述符相对打开文件名，避免重新
    /// 从完整路径解析目录。
    ///
    /// ⚠ device/inode 是本地文件系统身份线索，不是不可伪造的永久标识；文件系统异常、inode
    /// 复用或检查之后立即发生的替换仍需由前后复核和打开的文件描述符共同处理。此检查降低
    /// 路径被替换后误操作的风险，不等于密码学证明，也不能单独证明所有竞态均已消失。
    /// 非 Unix 构建没有此处的目录文件描述符比较，相关能力通过 storage capability 报告。
    fn verify_parent(&self) -> Result<(), PortError> {
        #[cfg(unix)]
        {
            let current = fs::symlink_metadata(&self.parent)
                .map_err(|e| io_error("eventlog_parent_changed", e))?;
            let pinned = self
                .directory
                .metadata()
                .map_err(|e| io_error("eventlog_parent_metadata_failed", e))?;
            if !current.is_dir() || current.dev() != pinned.dev() || current.ino() != pinned.ino() {
                return Err(failed("eventlog_parent_changed"));
            }
        }
        Ok(())
    }
    fn open_name(&self, path: &Path, create: bool) -> Result<Option<File>, PortError> {
        self.verify_parent()?;
        #[cfg(unix)]
        {
            let name = path
                .file_name()
                .ok_or_else(|| failed("eventlog_path_required"))?;
            let name = std::ffi::CString::new(name.as_bytes())
                .map_err(|_| failed("eventlog_path_invalid"))?;
            let flags = libc::O_RDWR
                | libc::O_APPEND
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | if create { libc::O_CREAT } else { 0 };
            let fd =
                unsafe { libc::openat(self.directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
            if fd < 0 {
                let e = std::io::Error::last_os_error();
                if !create && e.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                if e.raw_os_error() == Some(libc::ELOOP) {
                    return Err(failed("eventlog_symlink_rejected"));
                }
                return Err(io_error("eventlog_open_failed", e));
            }
            let file = unsafe { File::from_raw_fd(fd) };
            let metadata = file
                .metadata()
                .map_err(|e| io_error("eventlog_metadata_failed", e))?;
            validate_unix_storage_file(&metadata)?;
            Ok(Some(file))
        }
        #[cfg(not(unix))]
        {
            // 非 Unix 的标准 OpenOptions 没有跨平台的“禁止跟随符号链接”标志，因此在打开前
            // 后各检查一次目录项，并通过 capability 明示仍存在的检查/使用竞态。两次检查
            // 能缩小风险窗口，但不能等同 Unix openat(O_NOFOLLOW) 的原子路径约束。
            reject_non_unix_symlink(path)?;
            let mut options = OpenOptions::new();
            options.read(true).write(true).append(true).create(create);
            match options.open(path) {
                Ok(file) => {
                    reject_non_unix_symlink(path)?;
                    let metadata = file
                        .metadata()
                        .map_err(|e| io_error("eventlog_metadata_failed", e))?;
                    if !metadata.is_file() {
                        return Err(failed("eventlog_not_regular_file"));
                    }
                    Ok(Some(file))
                }
                Err(e) if !create && e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(io_error("eventlog_open_failed", e)),
            }
        }
    }
    fn lock(&self) -> Result<ProcessEventLogLock, PortError> {
        let path = self.path.with_extension("jsonl.lock");
        let file = self
            .open_name(&path, true)?
            .ok_or_else(|| failed("eventlog_lock_missing"))?;
        #[cfg(unix)]
        {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(io_error(
                    "eventlog_lock_acquire_failed",
                    std::io::Error::last_os_error(),
                ));
            }
        }
        self.verify_named_file(&path, &file)?;
        Ok(ProcessEventLogLock(file))
    }
    fn verify_named_file(&self, path: &Path, file: &File) -> Result<(), PortError> {
        self.verify_parent()?;
        #[cfg(unix)]
        {
            let current = self
                .open_name(path, false)?
                .ok_or_else(|| failed("eventlog_file_removed"))?;
            let current = current
                .metadata()
                .map_err(|e| io_error("eventlog_metadata_failed", e))?;
            let pinned = file
                .metadata()
                .map_err(|e| io_error("eventlog_metadata_failed", e))?;
            if current.dev() != pinned.dev() || current.ino() != pinned.ino() {
                return Err(failed("eventlog_file_replaced"));
            }
        }
        #[cfg(not(unix))]
        {
            reject_non_unix_symlink(path)?;
            if !file
                .metadata()
                .map_err(|e| io_error("eventlog_metadata_failed", e))?
                .is_file()
            {
                return Err(failed("eventlog_not_regular_file"));
            }
        }
        Ok(())
    }
    /// 在支持的平台上同步**目录本身**，而不只是文件。
    ///
    /// 【为什么这是必须的】
    /// 在大多数文件系统上，一个**新建文件**的持久性不取决于文件内容有没有落盘，
    /// 而取决于**目录项**有没有落盘。文件写完并 fsync 之后，目录项仍可能只在内存里——
    /// 这时断电，文件就「不存在」，而事件已经被判定为写入成功。
    ///
    /// 也就是说：只 fsync 文件，得到的是「文件内容安全，但目录项可能丢」。
    /// 那正好是最坏的一种丢失——**看起来写成功了，其实没有**。
    ///
    /// 【Unix 分支在 fsync 前后各调一次 verify_parent】
    /// 因为 fsync 目录这个动作作用于已打开的目录描述符；前后复核可发现目录名在操作期间被换走。
    ///
    /// ⚠ 非 Unix 分支没有目录文件描述符，因此这里不会执行目录 `sync_all`，路径复核也无法
    ///   提供 Unix 分支的相同保证。这个差异应由调用方检查平台 capability 并结合实际证据
    ///   评估；不能仅凭此函数或平台名称宣布某项系统级 proof level 成立或不成立。
    fn sync_directory(&self) -> Result<(), PortError> {
        self.verify_parent()?;
        #[cfg(unix)]
        {
            self.directory
                .sync_all()
                .map_err(|e| io_error("eventlog_directory_sync_failed", e))?;
        }
        self.verify_parent()
    }
}
struct ProcessEventLogLock(File);
impl Drop for ProcessEventLogLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

/// 当前进程对 JSONL 文件的已验证视图。
///
/// 文件是跨进程共享的事实来源；这个缓存只是减少重复解析的索引。每次存储操作仍会在文件
/// 锁内调用 `load_delta` 检查外部新增内容，因此不能把缓存当成独立权威状态。
#[derive(Debug, Default)]
struct DiskCache {
    /// 已从文件中验证并折叠出的事件、命令收据和流索引。
    journal: JournalState,
    /// 已确认解析的文件字节位置；下一次增量重放从此处继续。
    offset: u64,
    /// 已消费的 JSONL 行数，用于生成损坏记录的定位信息。
    line: u64,
    /// 当前读取到的日志写入格式版本；0 表示仍是兼容旧格式。
    writer_version: u32,
    /// 上次确认的文件身份快照；Unix 使用 device/inode，其他平台使用占位值。
    identity: Option<(u64, u64)>,
    /// 上次观察到的修改时间，仅用于触发内容复核，不能单独作为文件未变的证明。
    modified: Option<SystemTime>,
    /// 最近写入或同步的结果不确定；下一次重放需重新建立同步屏障。
    uncertain_write: bool,
    /// 本次重放是否把不完整/无换行的日志尾部回退到上一个已提交边界。只用于向调用方报告
    /// “打开时修复过尾部”这一事实，不改变提交语义，也不能被当作业务成功。
    repaired_tail: bool,
    /// 被尾部修复影响的字节数（截断长度或补写的换行）。仅用于恢复报告。
    repaired_bytes: u64,
}

/// 一次成功打开对文件尾部所做的恢复动作。
///
/// 这是只读诊断数据：它说明加载器为了暴露已提交前缀而回退了不完整的尾记录，或补写了缺失的
/// 换行。它不是把未确认写入变成事实的凭据，也不能替代更高层的未提交/`result_unknown` 判定。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JournalRecovery {
    /// 打开时是否修复过日志尾部（回退不完整尾记录或补写缺失换行）。
    pub repaired_tail: bool,
    /// 上述修复影响的字节数；未修复时为 0。
    pub repaired_bytes: u64,
}
#[derive(Debug)]
struct Inner {
    files: JournalFiles,
    cache: Mutex<DiskCache>,
    lifecycle: AtomicU8,
}
/// 将 EventStorePort 操作映射到本地 JSONL 文件的持久化 adapter。
///
/// 通常由 `DaemonHost` 在组装阶段创建，再以 `Arc<dyn EventStorePort>` 注入 Core。`inner`
/// 共享文件、缓存和关闭状态；`workers` 限制同步文件 I/O 的并发准入。业务代码应依赖端口，
/// 而不直接操作这里的路径或缓存。
#[derive(Debug)]
pub struct JsonlEventLog {
    path: PathBuf,
    inner: Arc<Inner>,
    workers: Arc<Semaphore>,
}
impl JsonlEventLog {
    /// 按 `KIANA_HOME` 或默认用户目录定位会话事件日志并打开它。
    ///
    /// 【调用者与数据流】`DaemonHost` 本地装配路径调用此方法；路径由 `lib.rs` 的
    /// `default_sessions_log_path()` 计算。打开时创建所需父目录、获取进程间锁，并将已有记录
    /// 重放到新的 `DiskCache`，之后 `EventStorePort` 的读写都通过该缓存和同一日志文件完成。
    ///
    /// 【失败情况】配置路径缺失/非法、目录或文件权限不符合约束、锁无法取得、文件内容损坏
    /// 或 I/O 失败时返回 `PortError`；调用者无法获得一个部分初始化的 adapter。
    pub fn open_default() -> Result<Self, PortError> {
        Self::open(crate::default_sessions_log_path()?)
    }
    /// 打开指定日志文件并同步重放已有内容。
    ///
    /// 【调用者】同步启动代码或 `open_async` 的阻塞工作线程。
    /// 【输入】绝对路径按原样使用；相对路径以调用进程当前工作目录为基准转成绝对路径。
    /// 【输出】返回带有已重放缓存的 adapter；后续读写通过 `EventStorePort` 完成。
    /// 【副作用】可能创建父目录和锁文件，并读取、修复符合规则的末尾不完整记录。
    /// 【失败情况】路径解析、父目录安全校验、文件锁定、日志重放或文件 I/O 失败时返回错误。
    ///
    /// ⚠ 该函数会同步等待文件系统操作。异步运行时应使用 `open_async`，避免在异步执行器
    /// 的工作线程上直接做启动 I/O；同步版本保留给现有同步装配调用方。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PortError> {
        Ok(Self::open_with_recovery(path)?.0)
    }
    /// 打开指定日志文件，并额外返回这次打开对文件尾部做过的恢复动作。
    ///
    /// 【与 `open` 的关系】`open` 只保留 adapter；本变体同时把加载器回退不完整尾记录（或补写
    /// 缺失换行）的事实暴露给调用方，供启动完整性扫描生成恢复报告。它不引入第二条写入或修复
    /// 路径：尾部处理仍由 `load_delta` 完成，返回的 `JournalRecovery` 只是该结果的只读回执。
    pub fn open_with_recovery(
        path: impl AsRef<Path>,
    ) -> Result<(Self, JournalRecovery), PortError> {
        let path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| io_error("eventlog_current_directory_failed", e))?
                .join(path)
        };
        let files = JournalFiles::open(path.clone())?;
        let _lock = files.lock()?;
        let mut cache = DiskCache::default();
        if let Some(mut file) = files.open_name(&path, false)? {
            load_delta(&files, &mut file, &mut cache)?;
        }
        let recovery = JournalRecovery {
            repaired_tail: cache.repaired_tail,
            repaired_bytes: cache.repaired_bytes,
        };
        let store = Self {
            path,
            inner: Arc::new(Inner {
                files,
                cache: Mutex::new(cache),
                lifecycle: AtomicU8::new(0),
            }),
            workers: Arc::new(Semaphore::new(MAX_STORAGE_WORKERS)),
        };
        Ok((store, recovery))
    }
    /// 将同步打开/重放工作移到 Tokio 的阻塞线程池中执行。
    ///
    /// 入口等异步调用方通过 `.await` 等待打开结果，但文件系统 I/O 不直接占用异步 worker。
    /// 线程任务本身失败时返回 `eventlog_open_worker_failed`；底层路径、锁或重放错误原样
    /// 作为 `PortError` 传回调用者。
    pub async fn open_async(path: impl AsRef<Path>) -> Result<Self, PortError> {
        let path = path.as_ref().to_path_buf();
        tokio::task::spawn_blocking(move || Self::open(path))
            .await
            .map_err(|_| failed("eventlog_open_worker_failed"))?
    }
    /// 把 `open_with_recovery` 移到 Tokio 阻塞线程池执行，供异步完整性扫描使用。
    pub async fn open_with_recovery_async(
        path: impl AsRef<Path>,
    ) -> Result<(Self, JournalRecovery), PortError> {
        let path = path.as_ref().to_path_buf();
        tokio::task::spawn_blocking(move || Self::open_with_recovery(path))
            .await
            .map_err(|_| failed("eventlog_open_worker_failed"))?
    }
    /// 使用默认会话日志路径，并通过 `open_async` 在阻塞线程中完成文件打开和重放。
    pub async fn open_default_async() -> Result<Self, PortError> {
        Self::open_async(crate::default_sessions_log_path()?).await
    }
    /// 返回 adapter 当前使用的日志文件路径；不读取或刷新文件内容。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 报告当前平台对日志文件和锁文件提供的安全能力。
    ///
    /// Unix 可使用目录文件描述符、`openat`/`O_NOFOLLOW` 和 device/inode 比对；非 Unix 实现
    /// 保留显式符号链接检查，但无法证明无竞态的 no-follow 打开，也没有可移植的硬链接身份
    /// 校验。上层必须根据返回能力决定哪些保证可依赖，不能把不同平台当成同一实现。
    pub fn storage_security_capabilities(&self) -> StorageSecurityCapabilities {
        StorageSecurityCapabilities::current()
    }
    async fn with_store<T: Send + 'static>(
        &self,
        f: impl FnOnce(&JournalFiles, &mut DiskCache) -> Result<T, PortError> + Send + 'static,
    ) -> Result<T, PortError> {
        self.with_store_state(f, false).await
    }
    async fn with_store_state<T: Send + 'static>(
        &self,
        f: impl FnOnce(&JournalFiles, &mut DiskCache) -> Result<T, PortError> + Send + 'static,
        allow_closing: bool,
    ) -> Result<T, PortError> {
        // 先在提交任务前检查一次生命周期，在线程池任务开始后再检查一次：任务可能在排队期间
        // 遇到 close。Semaphore permit 在闭包结束时自动释放，保证成功和错误路径都归还名额。
        // 文件锁串行化跨实例/跨进程操作，Mutex 再保护本实例缓存；这里按固定顺序获取两者，
        // 后续闭包不应改变顺序，以免并发路径互相等待。
        // try_acquire_owned 不排队，因此超过 16 个并发 I/O 时会以 Unavailable 显式拒绝。
        let lifecycle = self.inner.lifecycle.load(Ordering::Acquire);
        if lifecycle != 0 && !(allow_closing && lifecycle == 1) {
            return Err(failed("eventlog_closed"));
        }
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| PortError::Unavailable("eventlog_worker_queue_full".into()))?;
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let lifecycle = inner.lifecycle.load(Ordering::Acquire);
            if lifecycle != 0 && !(allow_closing && lifecycle == 1) {
                return Err(failed("eventlog_closed"));
            }
            let _lock = inner.files.lock()?;
            let mut cache = inner
                .cache
                .lock()
                .map_err(|_| failed("eventlog_cache_poisoned"))?;
            if let Some(mut file) = inner.files.open_name(&inner.files.path, false)? {
                // 另一个进程可能已经追加事件；执行本次读写闭包前先把缓存追到文件末尾。
                load_delta(&inner.files, &mut file, &mut cache)?;
            } else if cache.identity.is_some() {
                return Err(failed("eventlog_file_removed"));
            }
            f(&inner.files, &mut cache)
        })
        .await
        .map_err(|_| PortError::Unavailable("eventlog_worker_failed".into()))?
    }

    async fn flush_health(&self) -> Result<EventStoreHealth, PortError> {
        // flush_store 会对已打开的日志执行文件同步、目录同步（若平台支持）和文件身份复核，
        // 然后把缓存中的事件数与写入格式汇总为健康视图。I/O 失败时不返回“看起来健康”的值。
        self.with_store(|files, cache| {
            flush_store(files, cache)?;
            Ok(EventStoreHealth::new(
                cache.writer_version,
                true,
                true,
                cache.journal.events.len() as EventCursor,
                false,
            ))
        })
        .await
    }
}
#[async_trait]
impl EventStorePort for JsonlEventLog {
    fn supports_atomic_transitions(&self) -> bool {
        // 当前只有 Unix 路径提供本实现要求的原子事务帧能力；非 Unix 调用方必须先检查此值，
        // commit_transition 会明确返回 unsupported，而不能悄悄退化成多条独立 append。
        cfg!(unix)
    }
    fn capabilities(&self) -> EventStoreCapabilities {
        let mut result = capabilities(cfg!(unix));
        result.atomic_transitions = cfg!(unix);
        result
    }
    async fn flush(&self) -> Result<EventStoreHealth, PortError> {
        self.flush_health().await
    }
    // 读取当前已重放状态的健康快照；进入 with_store 时会先重放外部进程新增的日志尾部。
    async fn health(&self) -> Result<EventStoreHealth, PortError> {
        self.with_store(|_, cache| {
            Ok(EventStoreHealth::new(
                cache.writer_version,
                true,
                true,
                cache.journal.events.len() as EventCursor,
                false,
            ))
        })
        .await
    }
    // 返回当前已重放日志在提交边界上的只读游标快照。
    //
    // 与 `health()` 相同，进入 with_store 时会先追平磁盘尾部；这里不额外强制一次 fsync，
    // 因此 `last_durable_cursor` 沿用本 adapter 既有的写入屏障声明，而不是本次读取新产生的
    // 持久化证据。查询方必须把这两个字段当成 adapter 声明，不能据此升级证明等级。
    async fn cursor_snapshot(&self) -> Result<EventLogCursorSnapshot, PortError> {
        self.with_store(|_, cache| {
            cache
                .journal
                .cursor_snapshot(cache.journal.committed_cursor())
        })
        .await
    }
    // 先执行 flush，再返回已确认的逻辑事件游标。
    // 游标表示日志中已重放的逻辑事件数量，不是文件字节偏移。同步失败会作为 PortError
    // 返回，而不会给调用者一个无法确认是否落盘的游标。
    async fn last_durable_cursor(&self) -> Result<EventCursor, PortError> {
        Ok(self.flush_health().await?.last_durable_cursor)
    }
    async fn close(&self) -> Result<EventStoreHealth, PortError> {
        // 生命周期状态：0=开放、1=正在关闭、2=已关闭。CAS 只允许一个调用者执行关闭；
        // with_store_state(..., true) 让这次最终 flush 在状态 1 下通过，其它新操作会被拒绝。
        self.inner
            .lifecycle
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|state| {
                if state == 1 {
                    failed("eventlog_close_in_progress")
                } else {
                    failed("eventlog_closed")
                }
            })?;
        let result = self
            .with_store_state(
                |files, cache| {
                    flush_store(files, cache)?;
                    Ok(EventStoreHealth::new(
                        cache.writer_version,
                        true,
                        true,
                        cache.journal.events.len() as EventCursor,
                        true,
                    ))
                },
                true,
            )
            .await;
        self.inner.lifecycle.store(2, Ordering::Release);
        result
    }
    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        // 此路径提交一个带前置条件的事务帧，是 Core 原子状态转换使用的端口操作。
        // 它先在内存 JournalState 中计划冲突/重放/追加，再将帧同步写盘，成功后才应用到缓存。
        // 如果写入或 worker 返回阶段出错，磁盘上可能已经有完整帧，因此返回 Unknown，
        // 把核对责任交给调用方按 command_id 查询；不能直接当作“肯定没发生”再次执行。
        if !self.supports_atomic_transitions() {
            return Err(PortError::Unavailable(
                "event_store_atomic_transitions_unsupported".into(),
            ));
        }
        batch.validate_identity().map_err(failed)?;
        let command_id = batch.command_id;
        let result = self
            .with_store(move |files, cache| {
                match cache.journal.plan_transition(batch, EventId::new())? {
                    TransitionPlan::Conflict(changed) => Ok(CommitOutcome::Conflict { changed }),
                    TransitionPlan::Replay(original) => match confirm_sync(files, cache) {
                        Ok(()) => Ok(CommitOutcome::Replayed { original }),
                        Err(e) => Ok(unknown(command_id, e)),
                    },
                    TransitionPlan::Append { batch, receipt } => {
                        let frame = JournalFrame::new(JournalFramePayload::Transition {
                            batch: batch.clone(),
                            receipt: receipt.clone(),
                        })
                        .map_err(failed)?;
                        let encoded = encode_line(&frame)?;
                        check_disk_capacity(cache, encoded.len())?;
                        if let Err(e) = write_frame(files, cache, &encoded, true) {
                            cache.uncertain_write = true;
                            return Ok(unknown(command_id, e));
                        }
                        cache.journal.apply_transition(batch, receipt.clone());
                        Ok(CommitOutcome::Committed { receipt })
                    }
                }
            })
            .await;
        match result {
            Err(PortError::Unavailable(reason)) if reason == "eventlog_worker_failed" => {
                Ok(unknown(command_id, reason))
            }
            other => other,
        }
    }
    async fn read_command(&self, id: &RequestId) -> Result<Option<CommandReceipt>, PortError> {
        // Receipt 是判断某个 command 是否已提交的重要恢复依据。找到收据时额外同步并复核
        // 日志文件；未找到时返回 None，表示当前已验证日志中没有该 command 的收据。
        let id = *id;
        self.with_store(move |files, cache| {
            let receipt = cache.journal.commands.get(&id).cloned();
            if receipt.is_some() {
                confirm_sync(files, cache)?;
            }
            Ok(receipt)
        })
        .await
    }
    async fn read_from(&self, cursor: u64, limit: usize) -> Result<JournalPage, PortError> {
        // 返回从逻辑游标开始的一页事件，实际的边界、下一游标和 has_more 由 JournalState
        // 统一计算。这里先通过 with_store 追平外部写入；本函数不把游标解释成文件字节位置。
        self.with_store(move |_, cache| cache.journal.page(cursor, limit))
            .await
    }
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        // 兼容入口：没有显式预期流版本时复用 append_expected 的顺序校验和持久化路径。
        self.append_expected(event, None).await
    }
    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<(), PortError> {
        // 兼容逐事件追加端口，并可选检查调用方给出的目标流预期版本 expected。它不会替代优先用于
        // authority transition 的 commit_transition。若 I/O 失败，事件可能已部分或完整写入，
        // 所以返回带 result_unknown 的 Unavailable；后续需从日志重放确认，不能假定未追加。
        self.with_store(move |files, cache| {
            let AppendPlan::Append(event) = cache.journal.plan_legacy(event, expected, false)?
            else {
                unreachable!("non-idempotent append");
            };
            let (encoded, upgrade) = encode_legacy_record(cache, &event)?;
            check_disk_capacity(cache, encoded.len())?;
            if let Err(e) = write_frame(files, cache, &encoded, upgrade) {
                cache.uncertain_write = true;
                return Err(PortError::Unavailable(format!(
                    "eventlog_append_result_unknown:{e}"
                )));
            }
            cache.journal.apply_legacy(event);
            Ok(())
        })
        .await
    }
    async fn append_idempotent(&self, event: RuntimeEvent) -> Result<EventAppendResult, PortError> {
        self.append_idempotent_expected(event, None).await
    }
    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        // 幂等追加要求事件含合法幂等键。相同请求重放时返回原事件和 replay 标志，避免重复
        // 记账；新事件则走相同落盘路径。写入结果不确定时仍必须先核对日志，再决定是否重试。
        validate_idempotency_key(&event)?;
        self.with_store(move |files, cache| {
            match cache.journal.plan_legacy(event, expected, true)? {
                AppendPlan::Replay(event) => {
                    confirm_sync(files, cache)?;
                    Ok(append_result(event, true))
                }
                AppendPlan::Append(event) => {
                    let (encoded, upgrade) = encode_legacy_record(cache, &event)?;
                    check_disk_capacity(cache, encoded.len())?;
                    if let Err(e) = write_frame(files, cache, &encoded, upgrade) {
                        cache.uncertain_write = true;
                        return Err(PortError::Unavailable(format!(
                            "eventlog_append_result_unknown:{e}"
                        )));
                    }
                    cache.journal.apply_legacy(event.clone());
                    Ok(append_result(event, false))
                }
            }
        })
        .await
    }
    async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        // 按 RequestId 返回已重放事件中的请求子集，供上层恢复某次请求的时间线。
        let id = *id;
        self.with_store(move |_, cache| Ok(cache.journal.request(&id)))
            .await
    }
    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        // 返回当前日志的全部逻辑事件快照。此 API 会复制事件集合，适合审计/完整性扫描等
        // 明确需要全量数据的调用方，不应被高频分页 UI 用来替代 read_from。
        self.with_store(|_, cache| Ok(cache.journal.events.clone()))
            .await
    }
    async fn read_stream(&self, kind: &str, id: &str) -> Result<Vec<RuntimeEvent>, PortError> {
        // 以领域流类型和流 ID 查询一个聚合子流；具体匹配和顺序由 JournalState 实现，避免
        // 不同 EventStore adapter 各自发明事件关联规则。
        let (kind, id) = (kind.to_owned(), id.to_owned());
        self.with_store(move |_, cache| Ok(cache.journal.stream(&kind, &id)))
            .await
    }
}
/// 用领域层定义的规范编码生成单行 JSON，并执行帧大小上限检查。
///
/// 输出包含结尾换行，供 JSONL 逐行解析。编码失败或超过上限时在写盘前返回错误。
fn encode_line(value: &impl serde_compat::JournalEncode) -> Result<Vec<u8>, PortError> {
    let mut bytes = value.journal_bytes().map_err(failed)?;
    if bytes.len() > MAX_JOURNAL_FRAME_BYTES {
        return Err(failed("eventlog_frame_size_limit"));
    }
    bytes.push(b'\n');
    Ok(bytes)
}
// 帧/Header 的 JSON 编码由领域 DTO 的 canonical_journal_bytes 统一负责，避免在 adapter 中
// 另造一份字段顺序或校验和规则。下面这个小 trait 只把两类受支持的记录接到 encode_line。
mod serde_compat {
    pub(super) trait JournalEncode {
        fn journal_bytes(&self) -> Result<Vec<u8>, String>;
    }
    impl JournalEncode for kiana_domain::JournalFrame {
        fn journal_bytes(&self) -> Result<Vec<u8>, String> {
            kiana_domain::canonical_journal_bytes(self)
        }
    }
    impl JournalEncode for kiana_domain::JournalHeader {
        fn journal_bytes(&self) -> Result<Vec<u8>, String> {
            kiana_domain::canonical_journal_bytes(self)
        }
    }
}
/// 在追加前检查日志总字节上限，并为尚未升级的日志预留 v2 header 空间。
///
/// `256` 是 header 的固定容量预留，不是事件数或行数。若修改 header 字段或 schema，维护者
/// 必须确认其实际编码仍能放入预留值；否则升级时可能越过这里的容量保护或被过度拒绝。
fn check_disk_capacity(cache: &DiskCache, next: usize) -> Result<(), PortError> {
    let header = if cache.writer_version < JOURNAL_WRITER_VERSION {
        256
    } else {
        0
    };
    if cache
        .offset
        .saturating_add(next as u64)
        .saturating_add(header)
        > MAX_JOURNAL_LOG_BYTES
    {
        return Err(failed("eventlog_disk_limit"));
    }
    Ok(())
}
fn encode_legacy_record(
    cache: &DiskCache,
    event: &RuntimeEvent,
) -> Result<(Vec<u8>, bool), PortError> {
    if cache.writer_version == 0 {
        // 旧格式日志继续逐行写 RuntimeEvent，直到首次原子命令需要升级格式；那次写入会先
        // 安装 v2 header，再以带校验和的 JournalFrame 写新事件。这样老日志可继续读取，
        // 又不会把 header 之前的数据误当成 v2 帧。
        let mut bytes =
            serde_json::to_vec(event).map_err(|e| failed(format!("eventlog_encode_failed:{e}")))?;
        if bytes.len() > MAX_JOURNAL_EVENT_BYTES {
            return Err(failed("eventlog_event_size_limit"));
        }
        bytes.push(b'\n');
        return Ok((bytes, false));
    }
    let frame = JournalFrame::new(JournalFramePayload::Event {
        event: event.clone(),
    })
    .map_err(failed)?;
    Ok((encode_line(&frame)?, true))
}
fn write_frame(
    files: &JournalFiles,
    cache: &mut DiskCache,
    bytes: &[u8],
    upgrade: bool,
) -> Result<(), PortError> {
    // 调用者持有进程间锁和本地缓存锁。写入顺序是：校验文件快照 →（必要时）先安装并同步
    // v2 header → 写记录并同步文件 → 同步目录 → 复核命名路径仍指向同一文件 → 最后推进缓存。
    // header 与后续事件不是一个文件系统事务；若中间失败，调用者需要保留 uncertain 状态并
    // 由下次 load_delta 重放判断磁盘上已经留下什么。
    let mut file = files
        .open_name(&files.path, true)?
        .ok_or_else(|| failed("eventlog_file_missing"))?;
    verify_cached_file(&file, cache)?;
    if upgrade && cache.writer_version < JOURNAL_WRITER_VERSION {
        let header = encode_line(&JournalHeader::default())?;
        file.write_all(&header)
            .map_err(|e| io_error("eventlog_header_write_failed", e))?;
        file.sync_all()
            .map_err(|e| io_error("eventlog_header_sync_failed", e))?;
        files.sync_directory()?;
        cache.offset += header.len() as u64;
        cache.line += 1;
        cache.writer_version = JOURNAL_WRITER_VERSION;
    }
    file.write_all(bytes)
        .map_err(|e| io_error("eventlog_write_failed", e))?;
    file.flush()
        .map_err(|e| io_error("eventlog_flush_failed", e))?;
    file.sync_all()
        .map_err(|e| io_error("eventlog_sync_failed", e))?;
    files.sync_directory()?;
    files.verify_named_file(&files.path, &file)?;
    // remember_file 成功后才推进缓存 offset/line：游标一旦发布，后续增量读取就会从新位置开始。
    // 若先推进游标、之后又遇到可失败操作，Unknown 结果可能跳过已落盘帧，导致 command receipt
    // 不进入缓存。这里的顺序因此是恢复正确性的一部分，不能把这些更新随意交换。
    remember_file(&file, cache)?;
    cache.offset += bytes.len() as u64;
    cache.line += 1;
    cache.uncertain_write = false;
    Ok(())
}
fn verify_cached_file(file: &File, cache: &DiskCache) -> Result<(), PortError> {
    // 防止在缓存 offset 已代表旧内容时，把新打开的替换文件当成同一份日志继续追加。
    // Unix 同时比较 device/inode；其它平台没有可移植的同等身份元组，只能依赖其安全能力。
    let metadata = file
        .metadata()
        .map_err(|e| io_error("eventlog_metadata_failed", e))?;
    if metadata.len() != cache.offset {
        return Err(failed("eventlog_file_changed_during_commit"));
    }
    #[cfg(unix)]
    if cache
        .identity
        .is_some_and(|id| id != (metadata.dev(), metadata.ino()))
    {
        return Err(failed("eventlog_file_replaced"));
    }
    Ok(())
}
fn confirm_sync(files: &JournalFiles, cache: &DiskCache) -> Result<(), PortError> {
    // 对当前命名的日志再次校验长度/身份，再同步文件和目录，最后复核路径名称仍指向该文件。
    // 例如 read_command 命中 receipt 时会走这里，确保返回的恢复依据经过一次同步确认。
    let file = files
        .open_name(&files.path, false)?
        .ok_or_else(|| failed("eventlog_file_removed"))?;
    verify_cached_file(&file, cache)?;
    file.sync_all()
        .map_err(|e| io_error("eventlog_confirm_sync_failed", e))?;
    files.sync_directory()?;
    files.verify_named_file(&files.path, &file)
}
fn flush_store(files: &JournalFiles, cache: &DiskCache) -> Result<(), PortError> {
    // 有已知日志文件身份时同步该文件；尚未创建日志文件时只尝试同步父目录。
    if cache.identity.is_some() {
        confirm_sync(files, cache)
    } else {
        files.sync_directory()
    }
}
fn remember_file(file: &File, cache: &mut DiskCache) -> Result<(), PortError> {
    let metadata = file
        .metadata()
        .map_err(|e| io_error("eventlog_metadata_failed", e))?;
    #[cfg(unix)]
    {
        cache.identity = Some((metadata.dev(), metadata.ino()));
    }
    #[cfg(not(unix))]
    {
        cache.identity = Some((0, 0));
    }
    cache.modified = metadata.modified().ok();
    Ok(())
}
fn load_delta(
    files: &JournalFiles,
    file: &mut File,
    cache: &mut DiskCache,
) -> Result<(), PortError> {
    // 把上次验证的 byte offset 到当前文件末尾增量重放进缓存。调用者必须先持有进程间锁和
    // 缓存 Mutex；这里还检查文件身份、大小上限、已提交前缀以及每条记录的 schema/校验和。
    // 合法但缺少末尾换行的最后一行会补写换行；符合条件的 EOF 截断尾记录会回退到上一条
    // 已验证记录边界。中间损坏、旧前缀变化或校验不通过会返回错误，不会静默跳过。
    let original_offset = cache.offset;
    let metadata = file
        .metadata()
        .map_err(|e| io_error("eventlog_metadata_failed", e))?;
    if metadata.len() > MAX_JOURNAL_LOG_BYTES {
        return Err(failed("eventlog_disk_limit"));
    }
    #[cfg(unix)]
    if cache
        .identity
        .is_some_and(|id| id != (metadata.dev(), metadata.ino()))
    {
        return Err(failed("eventlog_file_replaced"));
    }
    if metadata.len() < cache.offset {
        return Err(failed("eventlog_committed_prefix_truncated"));
    }
    if metadata.len() == cache.offset
        && cache.offset > 0
        && cache.modified.is_some()
        && cache.modified != metadata.modified().ok()
        && !cache.uncertain_write
    {
        // 另一个进程可能只修剪了未提交的文件尾部，已提交前缀仍然相同。修改时间变化只能触发
        // 重新校验，不能直接证明内容遭到修改；因此从头重放并比较前缀事件和收据。
        let mut verified = DiskCache::default();
        load_delta(files, file, &mut verified)?;
        if verified.offset != cache.offset
            || verified.writer_version != cache.writer_version
            || verified.journal.events != cache.journal.events
            || verified.journal.commands != cache.journal.commands
        {
            return Err(failed("eventlog_committed_prefix_modified"));
        }
        *cache = verified;
        return Ok(());
    }
    file.seek(SeekFrom::Start(cache.offset))
        .map_err(|e| io_error("eventlog_seek_failed", e))?;
    let mut reader = BufReader::new(
        file.try_clone()
            .map_err(|e| io_error("eventlog_clone_failed", e))?,
    );
    loop {
        let bytes = read_bounded_line(&mut reader)?;
        if bytes.is_empty() {
            break;
        }
        let newline = bytes.last() == Some(&b'\n');
        let payload = if newline {
            &bytes[..bytes.len() - 1]
        } else {
            &bytes[..]
        };
        let line_number = cache.line + 1;
        if payload.iter().all(u8::is_ascii_whitespace) {
            cache.offset += bytes.len() as u64;
            cache.line += 1;
            continue;
        }
        let value = match parse_json_value_reject_duplicates(payload) {
            Ok(value) => value,
            Err(e)
                if !newline
                    && e.is_eof()
                    && (cache.writer_version == JOURNAL_WRITER_VERSION
                        || !cache.journal.events.is_empty()) =>
            {
                // 仅对新格式日志（或已有有效旧事件之后）容忍 EOF 截断：此时不完整尾行不可能
                // 是完整提交记录。将文件裁回本行起点，并同步修复；更早的损坏记录不能走此分支。
                file.set_len(cache.offset)
                    .map_err(|e| io_error("eventlog_repair_failed", e))?;
                file.sync_all()
                    .map_err(|e| io_error("eventlog_repair_sync_failed", e))?;
                files.sync_directory()?;
                cache.repaired_tail = true;
                cache.repaired_bytes = cache.repaired_bytes.saturating_add(bytes.len() as u64);
                break;
            }
            Err(_) => return Err(failed(format!("eventlog_corrupt:line={line_number}"))),
        };
        let schema = value.get("schema").and_then(serde_json::Value::as_str);
        if schema == Some(JOURNAL_HEADER_SCHEMA) {
            let header: JournalHeader =
                serde_json::from_value(value).map_err(|_| failed("eventlog_header_invalid"))?;
            header.validate().map_err(failed)?;
            if cache.writer_version == JOURNAL_WRITER_VERSION {
                return Err(failed("eventlog_writer_version_unsupported"));
            }
            cache.writer_version = JOURNAL_WRITER_VERSION;
        } else if schema == Some(JOURNAL_FRAME_SCHEMA) {
            if cache.writer_version != JOURNAL_WRITER_VERSION {
                return Err(failed("eventlog_frame_header_missing"));
            }
            let source_shape = canonical_journal_bytes(&value).map_err(failed)?;
            let frame: JournalFrame = serde_json::from_value(value)
                .map_err(|_| failed(format!("eventlog_frame_invalid:line={line_number}")))?;
            // 旧 RuntimeEvent 调用方仍按兼容 schema 读取；v2 authority frame 则必须保留所有
            // 当前结构能识别的字段。规范化前后形状不同，说明反序列化会丢字段或当前代码不认识
            // 该版本，因此拒绝而不是把不完整对象应用到 JournalState。
            if canonical_journal_bytes(&frame).map_err(failed)? != source_shape {
                return Err(failed("eventlog_frame_shape_unsupported"));
            }
            cache
                .journal
                .accept_frame(frame)
                .map_err(|e| failed(format!("eventlog_corrupt:line={line_number}:{e}")))?;
        } else if value.get("schema").is_some()
            || value.get("required").is_some()
            || value.get("writer_version").is_some()
        {
            return Err(failed("eventlog_required_record_unsupported"));
        } else {
            if cache.writer_version == JOURNAL_WRITER_VERSION {
                return Err(failed("eventlog_legacy_writer_after_upgrade"));
            }
            let event: RuntimeEvent = serde_json::from_value(value)
                .map_err(|_| failed(format!("eventlog_corrupt:line={line_number}")))?;
            let AppendPlan::Append(event) = cache.journal.plan_legacy(event, None, false)? else {
                unreachable!("legacy replay is not idempotent");
            };
            cache.journal.apply_legacy(event);
        }
        cache.offset += bytes.len() as u64;
        cache.line += 1;
        if !newline {
            // 这是一条可以完整解析的 JSON 记录，只是文件尾缺少 JSONL 分隔符。保留该记录，
            // 补上换行并同步；这与上面的“EOF 截断 JSON”分支不同，不能把合法事件一起丢掉。
            file.write_all(b"\n")
                .map_err(|e| io_error("eventlog_repair_newline_write_failed", e))?;
            file.sync_all()
                .map_err(|e| io_error("eventlog_repair_newline_sync_failed", e))?;
            files.sync_directory()?;
            cache.offset += 1;
            cache.repaired_tail = true;
            cache.repaired_bytes = cache.repaired_bytes.saturating_add(1);
            break;
        }
    }
    if cache.offset != original_offset || cache.uncertain_write {
        // 上一个进程可能在同步或提交响应阶段失联，磁盘上留下了当前进程尚未确认的记录。
        // 在向读取方暴露这些已重放的 authority frame 前，再建立文件与目录同步屏障；若同步
        // 失败，读取操作也失败，避免把尚未确认的恢复结果报告成稳定状态。
        cache.uncertain_write = true;
        file.sync_all()
            .map_err(|e| io_error("eventlog_recovery_sync_failed", e))?;
        files.sync_directory()?;
    }
    files.verify_named_file(&files.path, file)?;
    remember_file(file, cache)?;
    cache.uncertain_write = false;
    Ok(())
}
/// 将一行 JSON 解码成 `Value`，并拒绝任意层级对象中的重复键。
///
/// 【调用位置与数据流】`load_delta` 在判断 header、v2 frame 或兼容事件类型之前调用本函数；
/// 字节切片 → `StrictValueSeed`/`StrictValueVisitor` 递归构造 JSON 值 → 返回给 schema 分支
/// → 领域 DTO 反序列化 → frame 校验和与状态转换校验。
///
/// 【为什么不直接使用 `serde_json::from_slice::<Value>`】标准 `Value` map 对重复键会覆盖
/// 先前值，这会丢失原始日志中出现过的内容，使校验针对的值和写入文本不再一一对应。本解析器
/// 在本 adapter 接受记录之前拒绝重复键，减少本模块内部对同一行含义不一致的机会；它不宣称
/// 能约束仓库外的任意 JSON 工具，也不能代替帧校验和。
fn parse_json_value_reject_duplicates(
    bytes: &[u8],
) -> Result<serde_json::Value, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValueSeed.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

struct StrictValueSeed;

impl<'de> DeserializeSeed<'de> for StrictValueSeed {
    type Value = serde_json::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = serde_json::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value with unique object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let number =
            serde_json::Number::from_f64(value).ok_or_else(|| E::custom("invalid JSON number"))?;
        Ok(serde_json::Value::Number(number))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::String(value.to_owned()))
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(value)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::Null)
    }

    /// 递归处理 JSON 数组中的元素。
    ///
    /// serde 的访问者模式遇到数组时调用 `visit_seq`。元素可能包含嵌套数组或对象，因此每个
    /// 元素都重新经过 `StrictValueSeed`，使对象键检查覆盖整个 JSON 树，而不只是最外层。
    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValueSeed)? {
            values.push(value);
        }
        Ok(serde_json::Value::Array(values))
    }

    /// 递归构造 JSON 对象，并在发现重复键时拒绝整条记录。
    ///
    /// 【为什么在存入 `serde_json::Map` 前检查】map 本身只保留每个键的一个值；先查询已有键，
    /// 才能在覆盖发生前识别输入歧义。遇到重复键返回 serde 解析错误，最终由 `load_delta`
    /// 转成带行号的 `eventlog_corrupt`，不会让这一行继续进入 JournalState。
    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(StrictValueSeed)?;
            object.insert(key, value);
        }
        Ok(serde_json::Value::Object(object))
    }
}

fn read_bounded_line(reader: &mut impl BufRead) -> Result<Vec<u8>, PortError> {
    // 不使用 read_until 一次性无限增长：按 BufRead 缓冲块读取，一旦单行超过最大 frame
    // 字节数加 1 个换行符就拒绝。该边界保护重放过程，避免异常文件把内存占用拖到无上限。
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|e| io_error("eventlog_read_failed", e))?;
        if available.is_empty() {
            break;
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        if line.len().saturating_add(take) > MAX_JOURNAL_FRAME_BYTES + 1 {
            return Err(failed("eventlog_frame_size_limit"));
        }
        let ended = available[take - 1] == b'\n';
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if ended {
            break;
        }
    }
    Ok(line)
}
fn create_parent_directories(parent: &Path) -> Result<(), PortError> {
    // 从目标父目录向上找到最近的现存目录，再由浅到深补建缺失目录。Unix 新建目录权限为
    // 0o700（仅当前用户可访问），每建一级就同步该目录和它的父目录，降低崩溃后目录项消失
    // 的风险。发现已有路径不是实际目录（包括符号链接元数据）时拒绝继续。
    let mut missing = Vec::new();
    let mut current = parent;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(failed("eventlog_parent_not_directory"));
                }
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                current = current
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
            }
            Err(e) => return Err(io_error("eventlog_parent_metadata_failed", e)),
        }
    }
    for directory in missing.into_iter().rev() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        match builder.create(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_error("eventlog_create_failed", e)),
        }
        #[cfg(unix)]
        {
            open_directory(&directory)?
                .sync_all()
                .map_err(|e| io_error("eventlog_directory_sync_failed", e))?;
            if let Some(parent) = directory.parent() {
                open_directory(parent)?
                    .sync_all()
                    .map_err(|e| io_error("eventlog_directory_sync_failed", e))?;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_unix_storage_file(metadata: &std::fs::Metadata) -> Result<(), PortError> {
    // 日志和锁必须是普通文件、不可通过硬链接共享到另一位置，且 group/other 不得有权限。
    // 0o077 选出 group 与 other 的全部读/写/执行位；0o600 是生产文件创建时使用的私有权限。
    if !metadata.is_file() {
        return Err(failed("eventlog_not_regular_file"));
    }
    if metadata.nlink() > 1 {
        return Err(failed("eventlog_hardlink_rejected"));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(failed("eventlog_permissions_too_broad"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_non_unix_symlink(path: &Path) -> Result<(), PortError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(failed("eventlog_symlink_rejected"))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error("eventlog_metadata_failed", e)),
    }
}

#[cfg(unix)]
fn open_directory(path: &Path) -> Result<File, PortError> {
    // 将目录打开成稳定的文件描述符，供后续 openat、身份复核和目录同步复用。
    // O_NOFOLLOW 防止最后一级目录本身是符号链接；错误通过 PortError 返回给打开/写入调用方。
    let name = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| failed("eventlog_parent_invalid"))?;
    let fd = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io_error(
            "eventlog_open_failed",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

// Linux 测试夹具仍需用显式父目录文件描述符构造路径替换场景；这是测试辅助函数，生产写入
// 使用上面的 JournalFiles 封装。保留它可让安全回归用例继续覆盖同一类目录身份边界。
#[cfg(all(test, target_os = "linux"))]
fn open_eventlog_directory(path: &Path) -> Result<File, PortError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_name = std::ffi::CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| PortError::Failed("eventlog_parent_invalid".to_owned()))?;
    let fd = unsafe {
        libc::open(
            parent_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(PortError::Failed(format!(
            "eventlog_open_failed:{}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(all(test, target_os = "linux"))]
fn append_jsonl_line_at(
    path: &Path,
    event: &RuntimeEvent,
    directory: &File,
) -> Result<(), PortError> {
    let target_name = path
        .file_name()
        .ok_or_else(|| PortError::Failed("eventlog_path_required".to_owned()))?;
    let target_name = std::ffi::CString::new(target_name.as_bytes())
        .map_err(|_| PortError::Failed("eventlog_path_invalid".to_owned()))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            target_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(PortError::Failed(format!(
            "eventlog_open_failed:{}",
            std::io::Error::last_os_error()
        )));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut encoded = serde_json::to_string(event)
        .map_err(|error| PortError::Failed(format!("eventlog_encode_failed:{error}")))?;
    encoded.push('\n');
    file.write_all(encoded.as_bytes())
        .map_err(|error| PortError::Failed(format!("eventlog_write_failed:{error}")))?;
    file.flush()
        .map_err(|error| PortError::Failed(format!("eventlog_flush_failed:{error}")))?;
    file.sync_data()
        .map_err(|error| PortError::Failed(format!("eventlog_sync_failed:{error}")))?;
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn append_keeps_the_opened_parent_after_path_replacement() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-eventlog-parent-{}-{stamp}",
            std::process::id()
        ));
        let parent = root.join("sessions");
        let moved_parent = root.join("sessions-original");
        let path = parent.join("events.jsonl");
        fs::create_dir_all(&parent).unwrap();
        let directory = open_eventlog_directory(&path).unwrap();

        fs::rename(&parent, &moved_parent).unwrap();
        fs::create_dir(&parent).unwrap();
        let event = RuntimeEvent::new(RequestId::new(), 1, "run.accepted", Value::Null).unwrap();
        append_jsonl_line_at(&path, &event, &directory).unwrap();

        assert_eq!(
            fs::read_to_string(moved_parent.join("events.jsonl")).unwrap(),
            format!("{}\n", serde_json::to_string(&event).unwrap())
        );
        assert!(
            !path.exists(),
            "replacement parent must not receive the append"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
