//! 备份清单（snapshot manifest）的领域数据合同。
//!
//! 本文件把“某次备份声称包含什么”表示成可校验、可计算摘要的结构体，供创建备份的一侧
//! 填入文件清单、游标和数据代数（epoch），再交给恢复校验阶段绑定。数据大致沿下面的方向流动：
//!
//! ```text
//! 备份适配器收集的身份、游标和文件元数据
//!     ↓
//! SnapshotManifest::new() 排序文件清单、标记 Sealed、计算摘要
//!     ↓
//! SnapshotManifest::validate() 检查结构和摘要自洽
//!     ↓
//! Daemon verify_restore() 将清单身份与 RestoreVerificationFact 绑定
//! ```
//!
//! 当前工作区没有生产备份流程调用 `SnapshotManifest::new()`；直接构造点是测试，Daemon
//! 的公开恢复校验函数也没有生产调用者。因此这里描述的是已实现的数据合同，不代表备份
//! 文件已经被复制或持久化。
//!
//! ⚠ 本模块只验证调用方提供的元数据和摘要格式，不读取文件、不计算文件内容哈希、不停止
//! 写入者、不校验签名，也不激活恢复根目录。清单中的 `content_hash` 是调用方给出的事实；
//! 只有真正读取并核对文件字节的 adapter 才能证明内容与该值一致。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SNAPSHOT_MANIFEST_SCHEMA: &str = "kiana.snapshot-manifest.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 备份清单记录的是完整快照还是基于前一快照的增量快照。
///
/// 该值会进入 manifest digest；`validate()` 要求 Full 不带前序快照 ID，Incremental
/// 必须带前序快照 ID。它只描述数据关系，不会自行查找或读取前一份备份。
pub enum SnapshotMode {
    /// 清单独立描述一份完整快照，不允许引用前一份快照。
    Full,
    /// 清单描述增量快照，必须由调用方给出前一份快照 ID。
    Incremental,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 清单在备份流程中的阶段标签。
///
/// 目前 `SnapshotManifest::new()` 直接生成 Sealed 清单，`validate()` 只接受 Sealed；
/// Building 和 Failed 是 wire 类型可表达的状态，但不能通过当前的已封存清单校验。
pub enum SnapshotState {
    /// 仍在收集备份内容，尚不能作为已封存清单使用。
    Building,
    /// 文件元数据和清单摘要已固定，可进入后续的恢复事实绑定检查。
    Sealed,
    /// 备份创建过程已失败，不能作为有效快照输入。
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 调用方为快照中的一个文件提供的元数据记录。
///
/// 记录随 `SnapshotManifest.files` 一起参与清单摘要；这里只检查相对路径和哈希字符串
/// 的结构，不打开文件，也不核对 `size_bytes` 与实际字节数。
pub struct SnapshotFileSeal {
    /// 文件相对于快照根目录的路径；必须非空、不能以 `/` 开头，且不能包含 `..`。
    pub relative_path: String,
    /// 调用方声称的文件字节数；当前 validator 不限制其大小，包括允许 0。
    pub size_bytes: u64,
    /// 调用方声称的内容摘要，格式为 `sha256:` 加 64 位十六进制字符。
    pub content_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一份已经封存的备份清单。
///
/// 它将备份身份、所属范围、源数据位置和文件元数据放入同一份可摘要对象，使恢复阶段
/// 能检查恢复验证事实是否针对这份清单。字段是调用方提交的声明，不是对磁盘或数据库的
/// 实时查询结果；`manifest_digest` 也只是无密钥摘要，不能单独证明清单来源可信。
pub struct SnapshotManifest {
    /// 固定 wire schema 名称，用来拒绝不兼容或来源不明的清单形状。
    pub schema: String,
    /// 此次备份的稳定身份；不是恢复后新实例的 ID。
    pub snapshot_id: String,
    /// 快照所属的项目或其他 owner 范围，用于与恢复事实绑定。
    pub owner_scope: String,
    /// 产生快照的存储实例/存储域标识，用于阻止跨 store 错误套用。
    pub store_id: String,
    /// 创建此快照时的旧实例身份；恢复验证事实的 `previous_instance_id` 应与它一致。
    pub instance_id: String,
    /// 备份开始时活动数据根的路径字符串；本模块不规范化或访问该路径。
    pub active_root: String,
    /// 备份数据写入目标的路径字符串；当前只做字符串边界比较，不检查符号链接或文件系统。
    pub backup_root: String,
    /// 完整或增量模式；决定 `previous_snapshot_id` 是否必须为空。
    pub mode: SnapshotMode,
    /// 清单阶段；通过当前 validator 的清单必须为 Sealed。
    pub state: SnapshotState,
    /// 清单覆盖到的源事件/数据游标；必须大于 0，并在恢复事实中保持一致。
    pub source_cursor: u64,
    /// 备份时的数据代数；恢复后的数据 epoch 必须严格大于它。
    pub data_epoch: u64,
    /// 增量快照所依赖的上一份快照；Full 模式禁止填写。
    pub previous_snapshot_id: Option<String>,
    /// 调用方声明创建快照前已暂停写入；封存校验要求为 true。
    pub quiesced: bool,
    /// 调用方声明写前日志（WAL）已封存；封存校验要求为 true。
    pub wal_sealed: bool,
    /// 快照包含的文件元数据；排序后的顺序进入摘要，列表不能为空。
    pub files: Vec<SnapshotFileSeal>,
    /// 对除本字段外 manifest 内容计算的 `sha256:` JSON 摘要，用于检测载荷是否自洽。
    pub manifest_digest: String,
}

impl SnapshotManifest {
    /// 根据调用方收集的备份元数据，构造一份已封存清单并立即验证。
    ///
    /// 【调用者与位置】预期由备份适配器在文件清单、数据游标和 epoch 已收集后调用；
    /// 当前 workspace 只在 fixture 中直接调用，没有发现接入产品写入路径的生产调用者。
    ///
    /// 【输入】快照/owner/store/旧实例身份、活动根和备份根、快照模式、源游标、数据 epoch、
    /// 可选前序快照 ID、暂停与 WAL 封存声明，以及逐文件路径/大小/哈希元数据。
    ///
    /// 【输出】成功时返回排序过 `files`、状态为 Sealed 且 `manifest_digest` 已计算的清单。
    ///
    /// 【失败】任何参数组合不符合 `validate()` 时，把对应稳定错误字符串转换为 `String`；
    /// 因而路径重叠、未封存、游标无效和模式/前序快照不匹配都会在返回前被拒绝。
    ///
    /// 【副作用与设计原因】只改变传入的本地 `files` 向量并创建值对象，不做文件 I/O。
    /// 先按相对路径排序，保证相同文件集合以相同次序参与 JSON 摘要，避免输入顺序造成
    /// 不必要的摘要差异。封存清单在构造时一次性自校验，减少无效值流向恢复校验阶段。
    pub fn new(
        snapshot_id: impl Into<String>,
        owner_scope: impl Into<String>,
        store_id: impl Into<String>,
        instance_id: impl Into<String>,
        active_root: impl Into<String>,
        backup_root: impl Into<String>,
        mode: SnapshotMode,
        source_cursor: u64,
        data_epoch: u64,
        previous_snapshot_id: Option<String>,
        quiesced: bool,
        wal_sealed: bool,
        mut files: Vec<SnapshotFileSeal>,
    ) -> Result<Self, String> {
        // `files` 的顺序会影响 JSON 数组和摘要；排序让等价清单不依赖目录遍历顺序。
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut manifest = Self {
            schema: SNAPSHOT_MANIFEST_SCHEMA.to_owned(),
            snapshot_id: snapshot_id.into(),
            owner_scope: owner_scope.into(),
            store_id: store_id.into(),
            instance_id: instance_id.into(),
            active_root: active_root.into(),
            backup_root: backup_root.into(),
            mode,
            state: SnapshotState::Sealed,
            source_cursor,
            data_epoch,
            previous_snapshot_id,
            quiesced,
            wal_sealed,
            files,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    /// 检查清单是否符合当前 sealed-manifest 合同。
    ///
    /// 【输入/输出】读取 `self` 的所有字段；合法返回 `Ok(())`，否则返回首个失败规则对应
    /// 的错误字符串。检查顺序决定同一份无效输入首先报告哪个错误。
    ///
    /// 【核心流程】先校验 schema、封存/暂停/WAL/游标/文件清单；再验证身份文本和根路径
    /// 不重叠；随后检查 Full/Incremental 与前序快照关系、文件路径递增顺序及哈希格式；
    /// 最后重算摘要并与 `manifest_digest` 比较。
    ///
    /// 【边界】校验只针对对象字段，不读取路径、不核验文件长度或内容哈希，也不验证摘要
    /// 签名。若需要证明备份文件真实存在且字节未变，必须由上游存储 adapter 完成。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SNAPSHOT_MANIFEST_SCHEMA
            || self.state != SnapshotState::Sealed
            || !self.quiesced
            || !self.wal_sealed
            || self.source_cursor == 0
            || self.data_epoch == 0
            || self.files.is_empty()
        {
            return Err("snapshot_manifest_seal_invalid".to_owned());
        }
        for (value, field) in [
            (&self.snapshot_id, "snapshot_id"),
            (&self.owner_scope, "snapshot_owner_scope"),
            (&self.store_id, "snapshot_store_id"),
            (&self.instance_id, "snapshot_instance_id"),
            (&self.active_root, "snapshot_active_root"),
            (&self.backup_root, "snapshot_backup_root"),
        ] {
            if value.trim().is_empty() || value.len() > 4096 || value.contains('\0') {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.active_root == self.backup_root
            || self
                .backup_root
                .starts_with(&(self.active_root.clone() + "/"))
        {
            return Err("snapshot_backup_root_overlaps_active_root".to_owned());
        }
        if self.mode == SnapshotMode::Full && self.previous_snapshot_id.is_some() {
            return Err("snapshot_full_previous_forbidden".to_owned());
        }
        if self.mode == SnapshotMode::Incremental && self.previous_snapshot_id.is_none() {
            return Err("snapshot_incremental_previous_required".to_owned());
        }
        let mut previous = String::new();
        for file in &self.files {
            // 必须保持严格递增；这同时拒绝重复路径，并确保来自 `new()` 的排序格式稳定。
            if file.relative_path.is_empty()
                || file.relative_path.starts_with('/')
                || file.relative_path.contains("..")
                || file.relative_path <= previous
            {
                return Err("snapshot_file_path_invalid".to_owned());
            }
            if file.content_hash.strip_prefix("sha256:").is_none_or(|hex| {
                hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) {
                return Err("snapshot_file_hash_invalid".to_owned());
            }
            previous = file.relative_path.clone();
        }
        if self
            .manifest_digest
            .strip_prefix("sha256:")
            .is_none_or(|hex| hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("snapshot_manifest_digest_invalid".to_owned());
        }
        if self.manifest_digest != self.digest() {
            return Err("snapshot_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算当前 manifest 内容的确定性 JSON 摘要。
    ///
    /// `manifest_digest` 本身不放入输入对象，避免摘要依赖自身；清单字段和排序后的文件
    /// 列表都参与计算。摘要适合检测意外改动和绑定另一份 fact，但没有密钥，因此不是数字
    /// 签名或身份认证。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "snapshot_id": self.snapshot_id,
            "owner_scope": self.owner_scope,
            "store_id": self.store_id,
            "instance_id": self.instance_id,
            "active_root": self.active_root,
            "backup_root": self.backup_root,
            "mode": self.mode,
            "state": self.state,
            "source_cursor": self.source_cursor,
            "data_epoch": self.data_epoch,
            "previous_snapshot_id": self.previous_snapshot_id,
            "quiesced": self.quiesced,
            "wal_sealed": self.wal_sealed,
            "files": self.files,
        }))
    }
}
