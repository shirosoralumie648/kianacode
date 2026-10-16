//! 工作区文件扫描结果与受限读取的领域合同，属于 `kiana-domain`。
//!
//! 【系统位置与预期数据流】
//! ```text
//! Query 扫描器接收 root、trust 声明和读取上限
//!     -> 读取文件前后身份并决定 Indexed/MetadataOnly/Skipped/Fenced
//!     -> 本文件把每个文件结果和整次扫描封装为可校验 DTO
//!     -> 后续 context/index adapter 可消费这些证据
//! ```
//! 当前仓库没有生产调用者把 `read_workspace_snapshot()` 接入模型上下文；该路径由 Query 测试直接调用。
//! Snapshot 只是一次文件读取尝试的证据，不是 EventLog/ControlPlane 权威状态，也不会因为 `instruction_safe` 为 true 就自动进入 prompt。
//! `trust` 由上层传入，本模块不会查询 ProjectTrust；调用者不能把“文件读取稳定”误当成“项目已受信任”。
//!
//! Serde 负责 DTO 与 JSON 等外部格式互转；`deny_unknown_fields` 使新旧 schema 不会静默忽略未知字段。

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

/// 单文件快照 DTO 使用的稳定 schema 名称。
pub const WORKSPACE_FILE_SNAPSHOT_SCHEMA: &str = "kiana.workspace-file-snapshot.v1";
/// 一次工作区扫描聚合 DTO 使用的稳定 schema 名称。
pub const WORKSPACE_SNAPSHOT_SCHEMA: &str = "kiana.workspace-snapshot.v1";
/// 工作区快照合同当前的主/次版本。
pub const WORKSPACE_SNAPSHOT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 单次快照允许声明的文件数硬上限，避免调用者用配置无限放大遍历集合。
pub const MAX_WORKSPACE_SNAPSHOT_FILES: u64 = 4_096;
/// limitation 原因条数的最大值；原因用于解释扫描为何不完整，不是错误详情日志。
pub const MAX_WORKSPACE_SNAPSHOT_LIMITATIONS: usize = 16;

/// 校验必填文本的非空、字节上限和 NUL 字符。
///
/// 返回字段名加 `_invalid` 的稳定领域错误；这里只限制字符串外形，不会验证其中的路径、身份或语义。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验 SHA-256 摘要文本是否为 `sha256:` 加 64 位十六进制字符。
///
/// 这只证明字符串形状正确，不读取文件，也不证明摘要由可信扫描器生成。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 调用方声明的工作区信任分类；不是本 crate 自行验证出的结论。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTrust {
    /// 上层认为该工作区可进行受信任读取；仍需通过身份和大小检查。
    Trusted,
    /// 上层明确判定为不可信；只能保留文件元数据，不能返回正文。
    Untrusted,
    /// 上层尚未建立信任结论；默认选项使用此值并采取 metadata-only 路径。
    Unknown,
}

/// 扫描器观察到的文件系统对象类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEntryKind {
    /// 普通文件；只有同时满足 trust、身份稳定和其他上限条件才可能被索引。
    File,
    /// 符号链接；扫描器不跟随链接读取目标内容。
    Symlink,
    /// 有多个硬链接的文件；扫描器只记录元数据，避免别名跨出 workspace 边界。
    Hardlink,
    /// 目录；用于遍历，不会作为文本内容返回。
    Directory,
}

/// 扫描器对单个路径采取的读取结果分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceReadDisposition {
    /// 正文通过当前合同并进入可供调用者消费的内容列表。
    Indexed,
    /// 只记录身份和原因，不把文件正文交给调用者。
    MetadataOnly,
    /// 文件因格式、大小等原因未被读取或索引。
    Skipped,
    /// 读取前后的文件身份不同；读取内容不得作为稳定输入使用。
    Fenced,
}

/// 一次扫描可以消耗的资源预算。
///
/// Query 扫描器在目录遍历和单文件读取处应用这些上限；Domain 负责拒绝零值、相互矛盾或过大的结构化配置。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshotLimits {
    /// 最多纳入快照结果的文件数，不包含目录条目本身。
    pub max_files: u64,
    /// 所有已读取正文允许累计的 UTF-8 字节预算。
    pub max_total_bytes: u64,
    /// 任意单个文件正文的最大字节数。
    pub max_file_bytes: u64,
    /// 从 root 开始计算的最大目录递归深度。
    pub max_depth: u32,
    /// 扫描经过的最大毫秒数；Query 在遍历边界检查，不会取消正在阻塞的单次文件 I/O。
    pub max_elapsed_ms: u64,
}

impl WorkspaceSnapshotLimits {
    /// 校验预算可用且内部一致。
    ///
    /// `max_files` 还受本模块 4096 项硬上限约束；单文件上限不能超过总正文预算。
    /// 失败返回 `workspace_snapshot_limits_invalid`，不会在这里分配扫描资源。
    pub fn validate(&self) -> Result<(), String> {
        if self.max_files == 0
            || self.max_files > MAX_WORKSPACE_SNAPSHOT_FILES
            || self.max_total_bytes == 0
            || self.max_file_bytes == 0
            || self.max_file_bytes > self.max_total_bytes
            || self.max_depth == 0
            || self.max_elapsed_ms == 0
        {
            return Err("workspace_snapshot_limits_invalid".to_owned());
        }
        Ok(())
    }
}

/// 单次读取前后用于判断“是否还是同一个文件”的元数据快照。
///
/// device/inode/link count 在部分平台可能不可用，因此使用 `Option`；它们为空并不表示文件不存在。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileIdentity {
    /// Unix 文件系统设备编号；其他平台通常没有此值。
    #[serde(default)]
    pub device: Option<u64>,
    /// Unix inode 编号；不等于跨文件系统或跨重启的全局文件 ID。
    #[serde(default)]
    pub inode: Option<u64>,
    /// 读取身份采样时文件系统报告的长度，单位为字节。
    pub size_bytes: u64,
    /// 文件修改时间距 Unix epoch 的毫秒数；不可取得或早于 epoch 时为空。
    #[serde(default)]
    pub modified_unix_ms: Option<u64>,
    /// 读取正文后的内容摘要；读取前身份通常没有它。
    #[serde(default)]
    pub content_digest: Option<String>,
    /// 文件硬链接数量；未知平台为空，值为 0 不符合文件系统身份合同。
    #[serde(default)]
    pub hard_link_count: Option<u64>,
}

impl WorkspaceFileIdentity {
    /// 校验可选摘要格式和硬链接数。
    ///
    /// 缺失的设备/inode/time/link count 会被接受，因为某些平台无法提供对应字段。
    pub fn validate(&self) -> Result<(), String> {
        if self.hard_link_count == Some(0) {
            return Err("workspace_file_identity_hard_link_count_invalid".to_owned());
        }
        if let Some(content_digest) = &self.content_digest {
            digest(content_digest, "workspace_file_identity_content_digest")?;
        }
        Ok(())
    }

    /// 比较读取前后的文件系统身份；刻意不比较 `content_digest`。
    ///
    /// 读取前通常没有内容摘要，读取后才会写入摘要；若把摘要纳入身份比较，任何成功读取都会被误判为文件替换。
    /// 比较的是调用方已采集的元数据，字段缺失时仍可能得到相等结果，不能替代文件描述符级锁定。
    pub fn same_file(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.size_bytes == other.size_bytes
            && self.modified_unix_ms == other.modified_unix_ms
            && self.hard_link_count == other.hard_link_count
    }
}

/// 单个相对路径的扫描证据。
///
/// 上游 Query adapter 根据 trust、文件类型、读取前后身份和大小决定 disposition；下游可据此选择是否消费内容。
/// 本 DTO 的自校验只检查字段之间的声明关系，不访问文件系统，也不认证这些字段的真实来源。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileSnapshot {
    /// 固定单文件 schema 名称。
    pub schema: String,
    /// 相对 canonical root 的 POSIX 风格路径，用于快照内定位，不是授权 token。
    pub path: String,
    /// File/Symlink/Hardlink/Directory 分类。
    pub kind: WorkspaceEntryKind,
    /// 调用方传入的 trust 声明。
    pub trust: WorkspaceTrust,
    /// 打开/读取正文前观察到的身份。
    pub identity_before: WorkspaceFileIdentity,
    /// 读取结束后重新观察到的身份；没有进行第二次采样时为空。
    #[serde(default)]
    pub identity_after: Option<WorkspaceFileIdentity>,
    /// 实际正文读取字节数；metadata-only/skipped 路径通常为 0。
    pub bytes_read: u64,
    /// 当前路径是否进入内容列表、只保留 metadata、跳过或被身份变化 fence。
    pub disposition: WorkspaceReadDisposition,
    /// 是否可作为可信指令材料的声明位；验证器只按 trust/kind/disposition/身份稳定性重算此位。
    pub instruction_safe: bool,
    /// MetadataOnly/Skipped 的原因；具体必填规则由 disposition 决定。
    #[serde(default)]
    pub reason: Option<String>,
    /// 文件内容或 metadata-only 路径标签的摘要，具体来源由 adapter 决定。
    pub file_digest: String,
}

impl WorkspaceFileSnapshot {
    /// 校验单文件快照的路径、身份、摘要和读取分类之间的约束。
    ///
    /// 【调用者】`WorkspaceSnapshot::validate()` 和 Query `record_file()`。
    /// 【输出/失败】合法返回 `Ok(())`；合同冲突返回稳定的 `workspace_file_snapshot_*` 错误。
    /// 【核心流程】先检查 schema/相对路径和可选身份摘要，再根据 trust、kind、disposition 与前后身份重算 safety 位，
    /// 最后检查 Indexed/Fenced/MetadataOnly/Skipped 各自要求的证据。
    /// 【边界】Indexed 只要求 after identity 带摘要；这里没有把该摘要与 `file_digest` 或 `bytes_read` 交叉比较，
    /// 也不会确认文件真实存在。`instruction_safe` 不是 sanitizer，也不是 ProjectTrust 证明。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKSPACE_FILE_SNAPSHOT_SCHEMA
            || self.path.trim().is_empty()
            || self.path.len() > 4_096
            || self.path.starts_with('/')
            || self
                .path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err("workspace_file_snapshot_header_invalid".to_owned());
        }
        self.identity_before.validate()?;
        if let Some(identity_after) = &self.identity_after {
            identity_after.validate()?;
        }
        digest(&self.file_digest, "workspace_file_snapshot_digest")?;
        if let Some(reason) = &self.reason {
            required(reason, "workspace_file_snapshot_reason", 256)?;
        }
        let identity_stable = self
            .identity_after
            .as_ref()
            .is_some_and(|after| self.identity_before.same_file(after));
        // instruction_safe 必须与这四个声明字段一致，避免反序列化 DTO 把不可信/不稳定行标成安全。
        let expected_safe = self.trust == WorkspaceTrust::Trusted
            && self.kind == WorkspaceEntryKind::File
            && self.disposition == WorkspaceReadDisposition::Indexed
            && identity_stable;
        if self.instruction_safe != expected_safe {
            return Err("workspace_file_snapshot_instruction_safety_mismatch".to_owned());
        }
        // 每种 disposition 表示不同的失败/消费边界；不能只看 `reason` 或 `instruction_safe` 单个字段。
        match self.disposition {
            WorkspaceReadDisposition::Indexed => {
                if !expected_safe
                    || self.identity_after.is_none()
                    || self
                        .identity_after
                        .as_ref()
                        .and_then(|value| value.content_digest.as_ref())
                        .is_none()
                {
                    return Err("workspace_file_snapshot_indexed_evidence_missing".to_owned());
                }
            }
            WorkspaceReadDisposition::Fenced => {
                if self.identity_after.is_none() || identity_stable {
                    return Err("workspace_file_snapshot_fence_invalid".to_owned());
                }
            }
            WorkspaceReadDisposition::MetadataOnly | WorkspaceReadDisposition::Skipped => {
                if self.reason.as_deref().is_none_or(str::is_empty) {
                    return Err("workspace_file_snapshot_reason_required".to_owned());
                }
            }
        }
        Ok(())
    }
}

/// 一次 workspace 扫描的聚合结果。
///
/// 它把 root、trust、预算、文件行及扫描是否触顶放在同一个可校验 DTO 中，为 Query 的 context/index 消费者提供快照输入。
/// Snapshot 是可重新扫描的证据，不是权威状态；`snapshot_digest` 是无密钥自摘要，不能证明由受信任扫描器生成。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    /// 固定聚合快照 schema 名称。
    pub schema: String,
    /// DTO 合同版本。
    pub version: SchemaVersion,
    /// Query adapter canonicalize 后的根目录展示路径；Domain 本身不 canonicalize。
    pub root: String,
    /// 整个扫描采用的调用方信任声明。
    pub trust: WorkspaceTrust,
    /// 本次扫描使用的资源预算。
    pub limits: WorkspaceSnapshotLimits,
    /// 按扫描器顺序收集的文件证据行。
    pub files: Vec<WorkspaceFileSnapshot>,
    /// 扫描器实际累计纳入文件行的正文字节数。
    pub total_bytes_read: u64,
    /// 是否至少有一个文件读取前后身份不一致。
    pub dirty: bool,
    /// 是否因某项扫描预算而提前停止或省略内容。
    pub scan_limited: bool,
    /// 触发扫描限制的原因标签；不会包含任意路径或文件正文。
    #[serde(default)]
    pub limitations: Vec<String>,
    /// 对聚合字段计算的无密钥 SHA-256 摘要。
    pub snapshot_digest: String,
}

impl WorkspaceSnapshot {
    /// 从已收集的文件行组装聚合快照并立即运行合同校验。
    ///
    /// 【调用者】Query 扫描器；目前生产源码没有调用此扫描路径，直接构造点是 Domain/Query fixtures。
    /// 【输入】root、trust、预算、文件行和 scan_limited/limitation 结果。
    /// 【输出】计算 `total_bytes_read`、`dirty` 和 self digest 后返回快照。
    /// 【失败情况】预算、重复路径、嵌套文件行或摘要不合法时返回稳定领域错误。
    /// 【副作用/设计原因】只在内存组装数据；统一从 files 推导 dirty 和初始总量，避免调用者为这两个字段各自填一份事实。
    /// ⚠ 聚合总量使用普通整数 `sum()`，再由 `validate()` 检查上限；极端的外部合成数值可能在求和阶段溢出。
    pub fn new(
        root: impl Into<String>,
        trust: WorkspaceTrust,
        limits: WorkspaceSnapshotLimits,
        files: Vec<WorkspaceFileSnapshot>,
        scan_limited: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let total_bytes_read = files.iter().map(|file| file.bytes_read).sum();
        let dirty = files
            .iter()
            .any(|file| file.disposition == WorkspaceReadDisposition::Fenced);
        let mut snapshot = Self {
            schema: WORKSPACE_SNAPSHOT_SCHEMA.to_owned(),
            version: WORKSPACE_SNAPSHOT_VERSION,
            root: root.into(),
            trust,
            limits,
            files,
            total_bytes_read,
            dirty,
            scan_limited,
            limitations,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// 校验聚合快照的 schema、上限、路径唯一性、dirty 标记和自摘要。
    ///
    /// 成功不证明 root 已 canonicalize、trust 已由 ProjectTrust 核验或文件仍与磁盘一致。
    /// 对反序列化 DTO，本方法也没有重算 `total_bytes_read` 与各 file.bytes_read 的总和。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKSPACE_SNAPSHOT_SCHEMA
            || self.version != WORKSPACE_SNAPSHOT_VERSION
            || self.root.trim().is_empty()
            || self.root.len() > 4_096
            || self.files.len() as u64 > self.limits.max_files
            || self.total_bytes_read > self.limits.max_total_bytes
            || self.limitations.len() > MAX_WORKSPACE_SNAPSHOT_LIMITATIONS
        {
            return Err("workspace_snapshot_header_invalid".to_owned());
        }
        self.limits.validate()?;
        if self.scan_limited && self.limitations.is_empty() {
            return Err("workspace_snapshot_limitation_reason_required".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "workspace_snapshot_limitation", 256)?;
        }
        let mut paths = BTreeSet::new();
        for file in &self.files {
            file.validate()?;
            if !paths.insert(file.path.clone()) {
                return Err("workspace_snapshot_duplicate_path".to_owned());
            }
            if file.bytes_read > self.limits.max_file_bytes {
                return Err("workspace_snapshot_file_limit_exceeded".to_owned());
            }
        }
        let dirty = self
            .files
            .iter()
            .any(|file| file.disposition == WorkspaceReadDisposition::Fenced);
        if self.dirty != dirty {
            return Err("workspace_snapshot_dirty_mismatch".to_owned());
        }
        digest(&self.snapshot_digest, "workspace_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("workspace_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 snapshot 的业务字段计算无密钥 SHA-256 摘要，不包含自身 `snapshot_digest`。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "root": self.root,
            "trust": self.trust,
            "limits": self.limits,
            "files": self.files,
            "total_bytes_read": self.total_bytes_read,
            "dirty": self.dirty,
            "scan_limited": self.scan_limited,
            "limitations": self.limitations,
        }))
    }
}
