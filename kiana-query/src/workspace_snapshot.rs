//! 工作区扫描与受限文件读取 adapter，属于 `kiana-query`。
//!
//! 【预期数据流】
//! ```text
//! 调用方提供 root + trust 声明 + WorkspaceSnapshotLimits
//!     -> 本文件 canonicalize root、排序遍历并跳过已知生成/存储目录
//!     -> 只有 Trusted、普通文件、非 hardlink 且满足大小限制的文件会读正文
//!     -> 读取前后身份相同且 UTF-8 有效时返回 WorkspaceReadContent
//!     -> 所有观察结果交给 kiana-domain::WorkspaceSnapshot 校验
//! ```
//! `WorkspaceReadOutcome` 是可供 context/index 层使用的读取证据，不会授予 capability，也不会自动成为模型指令。
//! 当前 Rust 生产源码没有调用 `read_workspace_snapshot()`；实际直接调用点在 CM-07 fixtures，
//! 所以不能把此模块描述成已经接入普通请求的 workspace prompt 路径。
//! `trust` 由调用方传入，默认值是 Unknown；本 adapter 没有 ProjectTrust resolver，也不查询 ControlPlane/EventLog。
//! 身份前后比较可以发现一部分并发变化，但文件读取仍通过普通路径 `File::open`，不是文件描述符级快照或完整 TOCTOU 防护。

use anyhow::{anyhow, Context, Result};
use kiana_domain::{
    json_digest, WorkspaceEntryKind, WorkspaceFileIdentity, WorkspaceFileSnapshot,
    WorkspaceReadDisposition, WorkspaceSnapshot, WorkspaceSnapshotLimits, WorkspaceTrust,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 扫描器用于 Unknown/untrusted 场景的默认最大文件数量。
const DEFAULT_MAX_FILES: u64 = 4_096;
/// 默认整次扫描可返回的正文预算，单位为 UTF-8 字节。
const DEFAULT_MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
/// 默认单文件正文预算，超过时保留 Skipped 证据而不返回正文。
const DEFAULT_MAX_FILE_BYTES: u64 = 128 * 1024;
/// 默认目录递归深度，root 作为深度 0。
const DEFAULT_MAX_DEPTH: u32 = 16;
/// 默认扫描时限；只在遍历边界检查，不能中断已经开始的阻塞 I/O。
const DEFAULT_MAX_ELAPSED_MS: u64 = 5_000;

/// 单次 Query workspace 读取的信任声明和资源限制。
///
/// 此类型本身不决定谁有权信任项目；构造者负责把正确的 trust 值交给 adapter。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshotOptions {
    /// 调用方对整个 workspace 的信任分类；默认 Unknown 会关闭正文读取。
    pub trust: WorkspaceTrust,
    /// 文件数、总量、单文件、深度和扫描时间预算。
    pub limits: WorkspaceSnapshotLimits,
}

impl Default for WorkspaceSnapshotOptions {
    /// 使用保守预算并将 trust 设为 Unknown，防止未提供信任判定时把项目文本交给后续调用者。
    fn default() -> Self {
        Self {
            trust: WorkspaceTrust::Unknown,
            limits: WorkspaceSnapshotLimits {
                max_files: DEFAULT_MAX_FILES,
                max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
                max_file_bytes: DEFAULT_MAX_FILE_BYTES,
                max_depth: DEFAULT_MAX_DEPTH,
                max_elapsed_ms: DEFAULT_MAX_ELAPSED_MS,
            },
        }
    }
}

/// 通过信任和身份检查后可交给上层的单文件正文。
///
/// `path` 是相对 canonical root 的路径；`content_digest` 绑定 UTF-8 文本的原始字节，不代表文本已做 prompt 注入净化。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReadContent {
    /// 相对 root 的 POSIX 风格路径。
    pub path: String,
    /// 通过 UTF-8 解码的原始文件内容。
    pub text: String,
    /// `text` 对应的 SHA-256 字节摘要。
    pub content_digest: String,
}

/// 一次读取操作返回给调用方的双视图：聚合证据與被允许返回的正文。
///
/// `snapshot.files` 可包含 MetadataOnly/Skipped/Fenced 行；`contents` 只包含正文真正通过检查的文件。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReadOutcome {
    /// 全部被记录的路径证据和预算结果。
    pub snapshot: WorkspaceSnapshot,
    /// 实际返回的受信任 UTF-8 正文集合，与 snapshot 行通过 path 关联。
    pub contents: Vec<WorkspaceReadContent>,
}

// `WorkspaceSnapshot` 的字段本身均实现 Eq，因此这里为 Query wrapper 声明 Eq；不需要为 Domain DTO 扩大 derive 范围。
impl Eq for WorkspaceReadOutcome {}

/// 单次递归扫描的可变工作区状态；只在 `read_workspace_snapshot()` 内部流转，不跨请求共享。
struct ScanState {
    /// 已 canonicalize 的根，用于生成相对路径。
    root: PathBuf,
    /// 本次扫描使用的调用方声明和预算。
    options: WorkspaceSnapshotOptions,
    /// 遍历开始时间，用于在目录/条目访问边界估算经过时间。
    started: std::time::Instant,
    /// 已记录的文件证据行；数量同时用作 max_files 计数。
    files: Vec<WorkspaceFileSnapshot>,
    /// 已通过检查并准备返回给上层的正文。
    contents: Vec<WorkspaceReadContent>,
    /// 已触发的预算原因，重复原因只保留一份。
    limitations: Vec<String>,
    /// 是否因资源上限未扫描完整。
    scan_limited: bool,
}

impl ScanState {
    /// 判断全局扫描时限是否已经经过；该检查不能取消正在执行的系统调用。
    fn elapsed(&self) -> bool {
        self.started.elapsed() >= Duration::from_millis(self.options.limits.max_elapsed_ms)
    }

    /// 标记扫描结果不完整，并按原因去重。
    fn limit(&mut self, reason: &str) {
        self.scan_limited = true;
        if !self.limitations.iter().any(|item| item == reason) {
            self.limitations.push(reason.to_owned());
        }
    }

    /// 在开始访问下一个目录/条目前检查时间与文件行上限。
    ///
    /// 目录条目本身不会增加 `files.len()`；单次 `read_dir`、open 或 read 执行期间也没有中断点。
    fn can_visit(&mut self) -> bool {
        if self.elapsed() {
            self.limit("max_elapsed_ms");
            return false;
        }
        if self.files.len() as u64 >= self.options.limits.max_files {
            self.limit("max_files");
            return false;
        }
        true
    }
}

/// 对真实目录执行有界 metadata/content 扫描，并返回快照证据与允许读取的正文。
///
/// 【调用者】当前直接调用点是 CM-07 Query fixtures；仓内没有生产调用者。
/// 【输入】root 路径以及 trust/资源预算；默认 options 使用 Unknown，因此默认不读正文。
/// 【输出】`WorkspaceReadOutcome` 中的完整扫描证据和 Indexed 正文列表。
/// 【副作用】读取目录和文件 metadata/正文，不写文件、不执行文件内容，也不更改信任状态。
/// 【失败情况】预算非法、root 不存在/不是实际目录、目录读取失败或关键 metadata/I/O 失败时返回 `anyhow::Error`；
/// 单条 I/O 错误会结束整次调用，不会把部分结果作为成功 Outcome 返回。
/// 【核心流程】先拒绝 symlink/non-directory root，再 canonicalize；之后排序递归扫描，最后把行交给 Domain 构造/校验。
/// 【设计原因】分开 `snapshot` 与 `contents`，让上层可以审计未读取或被 fence 的路径，同时只把通过读取约束的文本交给调用者。
pub fn read_workspace_snapshot(
    root: impl AsRef<Path>,
    options: WorkspaceSnapshotOptions,
) -> Result<WorkspaceReadOutcome> {
    options.limits.validate().map_err(|error| anyhow!(error))?;
    let requested_root = root.as_ref();
    let root_metadata = fs::symlink_metadata(requested_root).with_context(|| {
        format!(
            "workspace root metadata failed: {}",
            requested_root.display()
        )
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(anyhow!("workspace root must be a real directory"));
    }
    // 将 root 规范化后再计算相对路径；这建立扫描范围基准，但不把传入 root 的 trust 自动升级为 Trusted。
    let root = fs::canonicalize(requested_root).with_context(|| {
        format!(
            "workspace root canonicalize failed: {}",
            requested_root.display()
        )
    })?;
    let mut state = ScanState {
        root: root.clone(),
        options,
        started: std::time::Instant::now(),
        files: Vec::new(),
        contents: Vec::new(),
        limitations: Vec::new(),
        scan_limited: false,
    };
    walk_directory(&mut state, &root, 0)?;
    let snapshot = WorkspaceSnapshot::new(
        root.to_string_lossy().replace('\\', "/"),
        state.options.trust,
        state.options.limits,
        state.files,
        state.scan_limited,
        state.limitations,
    )
    .map_err(|error| anyhow!(error))?;
    Ok(WorkspaceReadOutcome {
        snapshot,
        contents: state.contents,
    })
}

/// 按确定性目录顺序递归遍历一个目录，并把文件交给 `read_file()` 分类。
///
/// `.git`、`.kiana`、`target`、`node_modules` 被作为生成/存储目录跳过；symlink 用 `symlink_metadata()` 识别后不跟随。
/// 达到 depth/file/time 限制时保留 limitation 并停止相应分支，返回的 snapshot 可能是有明确限制标记的部分结果。
fn walk_directory(state: &mut ScanState, directory: &Path, depth: u32) -> Result<()> {
    if !state.can_visit() {
        return Ok(());
    }
    if depth > state.options.limits.max_depth {
        state.limit("max_depth");
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("workspace directory read failed: {}", directory.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| {
            format!(
                "workspace directory enumeration failed: {}",
                directory.display()
            )
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if !state.can_visit() {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // 这些目录通常含版本库、应用缓存或构建产物；排除它们可避免把内部状态当作项目正文扫描。
        if matches!(name.as_str(), ".git" | ".kiana" | "target" | "node_modules") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("workspace entry metadata failed: {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            record_metadata_only(
                state,
                &path,
                &metadata,
                WorkspaceEntryKind::Symlink,
                "symlink_not_read",
            )?;
        } else if metadata.is_dir() {
            // 在进入下一层之前检查深度；超过预算时不递归，但已读取的兄弟条目仍可保留。
            if depth >= state.options.limits.max_depth {
                state.limit("max_depth");
            } else {
                walk_directory(state, &path, depth + 1)?;
            }
        } else if metadata.is_file() {
            read_file(state, &path, &metadata)?;
        }
    }
    Ok(())
}

/// 根据调用方 trust 和读取前后身份决定一个普通文件是否可以返回正文。
///
/// 【调用者】`walk_directory()` 只对 metadata 判定为普通文件的路径调用。
/// 【输入/输出】输入路径与第一次 metadata；成功时把 evidence 行和可选正文追加到 ScanState。
/// 【核心流程】先计算相对路径并采集身份；hardlink 与非 Trusted 直接 metadata-only；再做读前长度检查，
/// 限量读取最多 `max_file_bytes + 1` 字节以侦测读取期间增长；随后重采身份，变化则 Fenced；稳定且 UTF-8 才进入 contents。
/// 【失败情况】open/read/post-read metadata 错误会带路径上下文并中止整次 scan；超限、非 UTF-8 或身份变化则转为 Skipped/Fenced 行。
/// 【副作用/边界】只读文件；前后 metadata 比较能拒绝部分变化，但 `File::open(path)` 仍可能跟随读取前后的路径替换，
/// 本函数没有用目录句柄相对 open 或禁止跟随 symlink 的系统调用，所以不能宣称抵御所有 TOCTOU 情况。
fn read_file(state: &mut ScanState, path: &Path, metadata_before: &fs::Metadata) -> Result<()> {
    let relative = relative_path(&state.root, path)?;
    let identity_before = identity(metadata_before, None);
    let hard_linked = hard_link_count(metadata_before).is_some_and(|count| count > 1);
    if hard_linked {
        return record_metadata_only(
            state,
            path,
            metadata_before,
            WorkspaceEntryKind::Hardlink,
            "hardlink_not_read",
        );
    }
    if state.options.trust != WorkspaceTrust::Trusted {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before.clone(),
            Some(identity_before),
            0,
            WorkspaceReadDisposition::MetadataOnly,
            false,
            Some(match state.options.trust {
                WorkspaceTrust::Untrusted => "untrusted_metadata_only",
                WorkspaceTrust::Unknown => "unknown_trust_metadata_only",
                WorkspaceTrust::Trusted => unreachable!(),
            }),
            digest_bytes(path.to_string_lossy().as_bytes()),
            None,
        );
    }
    if metadata_before.len() > state.options.limits.max_file_bytes {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before.clone(),
            Some(identity_before),
            0,
            WorkspaceReadDisposition::Skipped,
            false,
            Some("max_file_bytes"),
            digest_bytes(path.to_string_lossy().as_bytes()),
            None,
        );
    }
    let mut file = fs::File::open(path)
        .with_context(|| format!("workspace file open failed: {}", path.display()))?;
    file.seek(SeekFrom::Start(0))?;
    // 多读 1 byte 用于识别文件在第一次 metadata 后增长并越过单文件上限的情况。
    let read_limit = state.options.limits.max_file_bytes.saturating_add(1) as usize;
    let mut bytes = Vec::with_capacity(read_limit.min(metadata_before.len() as usize + 1));
    file.take(read_limit as u64).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > state.options.limits.max_file_bytes {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before.clone(),
            Some(identity_before),
            0,
            WorkspaceReadDisposition::Skipped,
            false,
            Some("max_file_bytes_after_open"),
            digest_bytes(path.to_string_lossy().as_bytes()),
            None,
        );
    }
    let metadata_after = fs::symlink_metadata(path).with_context(|| {
        format!(
            "workspace file post-read metadata failed: {}",
            path.display()
        )
    })?;
    let identity_after = identity(&metadata_after, None);
    let content_digest = digest_bytes(&bytes);
    // 前后身份不一致时，哪怕已经读到字节也不把它放入 contents，避免不稳定内容被当作可信快照。
    if !identity_before.same_file(&identity_after) {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before,
            Some(identity_after),
            0,
            WorkspaceReadDisposition::Fenced,
            false,
            Some("workspace_change_between_scan_and_read"),
            content_digest,
            None,
        );
    }
    let Ok(text) = String::from_utf8(bytes.clone()) else {
        return record_file(
            state,
            relative,
            WorkspaceEntryKind::File,
            identity_before,
            Some(identity_after),
            bytes.len() as u64,
            WorkspaceReadDisposition::Skipped,
            false,
            Some("non_utf8_content"),
            content_digest,
            None,
        );
    };
    // 只有稳定身份和有效 UTF-8 同时成立才生成 Indexed evidence 与可供调用者使用的正文。
    let mut identity_after = identity_after;
    identity_after.content_digest = Some(content_digest.clone());
    record_file(
        state,
        relative.clone(),
        WorkspaceEntryKind::File,
        identity_before,
        Some(identity_after),
        text.len() as u64,
        WorkspaceReadDisposition::Indexed,
        true,
        None,
        content_digest.clone(),
        Some(WorkspaceReadContent {
            path: relative,
            text,
            content_digest,
        }),
    )
}

/// 为不读取的 symlink/hardlink 记录相对路径、元数据和原因，不打开目标内容。
fn record_metadata_only(
    state: &mut ScanState,
    path: &Path,
    metadata: &fs::Metadata,
    kind: WorkspaceEntryKind,
    reason: &'static str,
) -> Result<()> {
    record_file(
        state,
        relative_path(&state.root, path)?,
        kind,
        identity(metadata, None),
        None,
        0,
        WorkspaceReadDisposition::MetadataOnly,
        false,
        Some(reason),
        digest_bytes(path.to_string_lossy().as_bytes()),
        None,
    )
}

/// 校验单文件证据，再按总内容预算决定是否把正文和证据行加入扫描结果。
///
/// 对超出累计正文预算的候选内容，当前实现先标记 `max_total_bytes` 然后直接返回，既不加入正文也不加入该文件的 snapshot 行；
/// 因而调用者只能从 `scan_limited/limitations` 知道结果不完整，不能从 files 获得该候选路径的失败记录。
#[allow(clippy::too_many_arguments)]
fn record_file(
    state: &mut ScanState,
    path: String,
    kind: WorkspaceEntryKind,
    identity_before: WorkspaceFileIdentity,
    identity_after: Option<WorkspaceFileIdentity>,
    bytes_read: u64,
    disposition: WorkspaceReadDisposition,
    instruction_safe: bool,
    reason: Option<&str>,
    file_digest: String,
    content: Option<WorkspaceReadContent>,
) -> Result<()> {
    let file = WorkspaceFileSnapshot {
        schema: kiana_domain::WORKSPACE_FILE_SNAPSHOT_SCHEMA.to_owned(),
        path,
        kind,
        trust: state.options.trust,
        identity_before,
        identity_after,
        bytes_read,
        disposition,
        instruction_safe,
        reason: reason.map(str::to_owned),
        file_digest,
    };
    file.validate().map_err(|error| anyhow!(error))?;
    if let Some(content) = content {
        if state.total_content_bytes() + content.text.len() as u64
            > state.options.limits.max_total_bytes
        {
            state.limit("max_total_bytes");
            return Ok(());
        }
        state.contents.push(content);
    }
    state.files.push(file);
    Ok(())
}

impl ScanState {
    /// 统计已接受正文的 UTF-8 字节数，用于下一条内容的总预算判断。
    fn total_content_bytes(&self) -> u64 {
        self.contents
            .iter()
            .map(|content| content.text.len() as u64)
            .sum()
    }
}

/// 把遍历路径转成相对 canonical root 的 POSIX 风格路径，并拒绝 root 之外的结果。
///
/// 这是一项扫描范围检查，不负责判断调用方是否有权读取 root。
fn relative_path(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| anyhow!("workspace path escaped canonical root"))?
        .to_string_lossy()
        .replace('\\', "/");
    if relative.is_empty() {
        return Err(anyhow!("workspace relative path empty"));
    }
    Ok(relative)
}

/// 从文件系统 metadata 生成可选平台身份字段；content digest 由读正文后补入。
fn identity(metadata: &fs::Metadata, content_digest: Option<String>) -> WorkspaceFileIdentity {
    WorkspaceFileIdentity {
        device: device(metadata),
        inode: inode(metadata),
        size_bytes: metadata.len(),
        modified_unix_ms: metadata.modified().ok().and_then(unix_ms),
        content_digest,
        hard_link_count: hard_link_count(metadata),
    }
}

/// 将文件修改时间转换为 Unix epoch 毫秒；系统时间早于 epoch 或换算溢出时返回 None。
fn unix_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

/// 对任意字节计算 `sha256:` 前缀的 SHA-256 文本摘要。
fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// Unix 平台提供的设备号；其他平台返回 None，避免伪造可用身份。
#[cfg(unix)]
fn device(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.dev())
}

/// 非 Unix 平台不提供本实现需要的设备号。
#[cfg(not(unix))]
fn device(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

/// Unix 平台提供的 inode 编号。
#[cfg(unix)]
fn inode(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.ino())
}

/// 非 Unix 平台没有通过此 helper 采集 inode。
#[cfg(not(unix))]
fn inode(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

/// Unix 平台提供的硬链接计数；大于 1 时 `read_file()` 拒绝读取正文。
#[cfg(unix)]
fn hard_link_count(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.nlink())
}

/// 非 Unix 平台未采集硬链接计数，因此此字段保持未知。
#[cfg(not(unix))]
fn hard_link_count(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

/// 将序列化后的整个快照计算为稳定摘要，供上层作为 provenance 字段使用。
///
/// 与 `WorkspaceSnapshot::digest()` 不同，此 helper 序列化对象本身，因此输入也包含已有的 `snapshot_digest`；
/// 它不会先调用 `validate()`，也不会查文件系统。序列化失败时当前实现以 JSON null 作为 hash 输入。
pub fn snapshot_digest(snapshot: &WorkspaceSnapshot) -> String {
    json_digest(&serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null))
}
