//! 增量索引失效计划、变更证据和缓存 key 的领域合同。
//!
//! 【预期数据流】可信 workspace snapshot 前后两版
//!     -> 本文件按相对路径、稳定身份、内容摘要和读取 disposition 派生 Added/Changed/Removed/Renamed
//!     -> IndexInvalidationPlan 收集需作废路径与 tombstone
//!     -> Query/cache 层把 plan 作为下一 generation 的重建线索
//!
//! 这里不扫描目录、不删除缓存、不重建索引，也不授予文件访问权限。mtime 只保存在 snapshot identity 中，内容变化主要依据摘要；
//! rename/delete 要显式留下旧路径证据，避免旧索引命中因为路径被移除而继续存活。
//! `IndexCacheKey` 则把根/worktree/branch/dirty manifest 和 parser/chunker/config 绑定在一个 key 内，防止算法输入不同却复用同一缓存。
//!
//! Serde 把版本化 DTO 与 JSON 等格式互转；`deny_unknown_fields` 拒绝未声明字段。摘要是无密钥指纹，不认证 snapshot 来源。

use crate::{
    json_digest, SchemaVersion, WorkspaceFileSnapshot, WorkspaceReadDisposition, WorkspaceSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// 完整索引缓存输入的稳定 schema 名称。
pub const INDEX_CACHE_KEY_SCHEMA: &str = "kiana.index-cache-key.v1";
/// 单文件变更与聚合 invalidation plan 共用的稳定 schema 名称。
pub const INDEX_INVALIDATION_SCHEMA: &str = "kiana.index-invalidation-plan.v1";
/// 当前 cache key/plan 的 schema 版本。
pub const INDEX_INVALIDATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 单次 source snapshot 比较最多保留的变更条数。
pub const MAX_INDEX_CHANGES: usize = 4_096;

/// 校验必填路径或标签的非空、UTF-8 字节上限和 NUL 字符。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 对使用正斜杠分隔的相对路径做词法检查。
///
/// 这里只拒绝 POSIX 根路径、空路径段、`.` 和 `..`；不 canonicalize、不检查磁盘，也不是跨平台文件系统授权。
/// ⚠ 当前实现没有单独拒绝 Windows 盘符或反斜杠写法；不要把通过此 helper 的字符串直接当作受限磁盘路径。
fn path(value: &str, field: &str) -> Result<(), String> {
    required(value, field, 4_096)?;
    if value.starts_with('/')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 验证 `sha256:` 加 64 个十六进制字符的摘要外形，不访问摘要对象。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 两份 WorkspaceSnapshot 比较后可能出现的路径变化类型。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceChangeKind {
    /// 目标 snapshot 新出现的相对路径。
    Added,
    /// 同一路径的内容摘要、读取 disposition 或 instruction_safe 状态发生变化。
    Changed,
    /// 旧 snapshot 中存在、目标 snapshot 中消失的路径。
    Removed,
    /// 旧路径消失且在新路径发现相同 effective content digest。
    Renamed,
}

/// 单一路径变化的输入/输出证据行。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceChange {
    /// 与 plan 共用的稳定 change schema 名称。
    pub schema: String,
    /// Added/Changed/Removed/Renamed 分类。
    pub kind: WorkspaceChangeKind,
    /// 目标路径；rename 时是新路径。
    pub path: String,
    /// rename 的旧路径；其他变化通常为 None。
    #[serde(default)]
    pub previous_path: Option<String>,
    /// 旧版本文件的 effective content digest；新建时为 None。
    #[serde(default)]
    pub before_digest: Option<String>,
    /// 新版本文件的 effective content digest；删除时为 None。
    #[serde(default)]
    pub after_digest: Option<String>,
    /// 对 change 业务字段计算的无密钥 SHA-256 摘要。
    pub change_digest: String,
}

impl WorkspaceChange {
    /// 内部构造 change 行，计算自摘要并立即检查变化类型所需的证据字段。
    fn new(
        kind: WorkspaceChangeKind,
        path: String,
        previous_path: Option<String>,
        before_digest: Option<String>,
        after_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut change = Self {
            schema: INDEX_INVALIDATION_SCHEMA.to_owned(),
            kind,
            path,
            previous_path,
            before_digest,
            after_digest,
            change_digest: String::new(),
        };
        change.change_digest = change.digest();
        change.validate()?;
        Ok(change)
    }

    /// 验证路径、摘要外形、变化类型所需 before/after 证据和自摘要。
    ///
    /// 此处只验证单行结构，不知道来源 snapshot，也不确认路径是否真的发生了对应变化。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_INVALIDATION_SCHEMA {
            return Err("workspace_change_schema_invalid".to_owned());
        }
        path(&self.path, "workspace_change_path")?;
        if let Some(previous_path) = &self.previous_path {
            path(previous_path, "workspace_change_previous_path")?;
        }
        for (value, field) in [
            (&self.before_digest, "workspace_change_before_digest"),
            (&self.after_digest, "workspace_change_after_digest"),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        match self.kind {
            WorkspaceChangeKind::Added if self.after_digest.is_none() => {
                return Err("workspace_change_after_digest_required".to_owned())
            }
            WorkspaceChangeKind::Changed
                if self.before_digest.is_none() || self.after_digest.is_none() =>
            {
                return Err("workspace_change_digest_pair_required".to_owned())
            }
            WorkspaceChangeKind::Removed if self.before_digest.is_none() => {
                return Err("workspace_change_before_digest_required".to_owned())
            }
            WorkspaceChangeKind::Renamed
                if self.previous_path.is_none()
                    || self.before_digest.is_none()
                    || self.after_digest.is_none() =>
            {
                return Err("workspace_change_rename_evidence_required".to_owned())
            }
            _ => {}
        }
        if self.kind == WorkspaceChangeKind::Renamed
            && self.previous_path.as_deref() == Some(self.path.as_str())
        {
            return Err("workspace_change_rename_same_path".to_owned());
        }
        digest(&self.change_digest, "workspace_change_digest")?;
        if self.change_digest != self.digest() {
            return Err("workspace_change_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 change 除 `change_digest` 外的字段计算无密钥内容摘要。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "kind": self.kind,
            "path": self.path,
            "previous_path": self.previous_path,
            "before_digest": self.before_digest,
            "after_digest": self.after_digest,
        }))
    }
}

/// 把 source、解析器和切块算法输入压成一个完整索引缓存身份。
///
/// 任一字段变化都会改变 `key_digest`，避免同一 branch/root 下因 dirty 文件或 parser/chunker 配置不同而错误复用缓存。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexCacheKey {
    /// 固定 key schema 名称。
    pub schema: String,
    /// canonical project root 的摘要。
    pub root_digest: String,
    /// worktree/source identity 的摘要。
    pub worktree_digest: String,
    /// 当前分支名；它是 key 的组成字段，不等同于 root/worktree identity。
    pub branch: String,
    /// 未提交工作树状态清单的摘要。
    pub dirty_manifest_digest: String,
    /// 解析器及版本配置摘要。
    pub parser_digest: String,
    /// chunker 及切分参数摘要。
    pub chunker_digest: String,
    /// 其余索引构建配置摘要。
    pub config_digest: String,
    /// 对以上输入计算的无密钥 cache key 摘要。
    pub key_digest: String,
}

impl IndexCacheKey {
    /// 从所有影响索引结果的输入构造 cache key，并校验字段与 digest。
    ///
    /// 【调用者】索引缓存 adapter；当前直接由 Domain fixtures 验证，Query API 暴露该合同。
    /// 【失败情况】任一摘要格式、branch 文本或自摘要不合法时返回稳定错误。
    /// 【设计原因】不能只用文件内容做缓存 key，因为 parser、chunker、branch、dirty state 或 config 改变也会改变索引结果。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root_digest: impl Into<String>,
        worktree_digest: impl Into<String>,
        branch: impl Into<String>,
        dirty_manifest_digest: impl Into<String>,
        parser_digest: impl Into<String>,
        chunker_digest: impl Into<String>,
        config_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut key = Self {
            schema: INDEX_CACHE_KEY_SCHEMA.to_owned(),
            root_digest: root_digest.into(),
            worktree_digest: worktree_digest.into(),
            branch: branch.into(),
            dirty_manifest_digest: dirty_manifest_digest.into(),
            parser_digest: parser_digest.into(),
            chunker_digest: chunker_digest.into(),
            config_digest: config_digest.into(),
            key_digest: String::new(),
        };
        key.key_digest = key.digest();
        key.validate()?;
        Ok(key)
    }

    /// 验证 key schema、所有摘要字段、branch 文本及 key_digest 一致性。
    ///
    /// 无密钥 digest 可检测内容变化，但不能认证 key 是可信 builder 生成的。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_CACHE_KEY_SCHEMA {
            return Err("index_cache_key_schema_invalid".to_owned());
        }
        for (value, field) in [
            (&self.root_digest, "index_cache_root_digest"),
            (&self.worktree_digest, "index_cache_worktree_digest"),
            (
                &self.dirty_manifest_digest,
                "index_cache_dirty_manifest_digest",
            ),
            (&self.parser_digest, "index_cache_parser_digest"),
            (&self.chunker_digest, "index_cache_chunker_digest"),
            (&self.config_digest, "index_cache_config_digest"),
        ] {
            digest(value, field)?;
        }
        required(&self.branch, "index_cache_branch", 256)?;
        digest(&self.key_digest, "index_cache_key_digest")?;
        if self.key_digest != self.digest() {
            return Err("index_cache_key_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 schema 和所有索引输入字段计算 cache key；不包含 `key_digest` 自身。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "root_digest": self.root_digest,
            "worktree_digest": self.worktree_digest,
            "branch": self.branch,
            "dirty_manifest_digest": self.dirty_manifest_digest,
            "parser_digest": self.parser_digest,
            "chunker_digest": self.chunker_digest,
            "config_digest": self.config_digest,
        }))
    }
}

/// 从两个 workspace snapshot 派生的增量失效计划。
///
/// 计划记录源/目标 generation、snapshot digest、变更行、失效路径和删除墓碑；消费方仍须负责真正重建和提交下一个索引 generation。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexInvalidationPlan {
    /// 固定 plan schema 名称。
    pub schema: String,
    /// plan schema 版本。
    pub version: SchemaVersion,
    /// 计划基于的旧 generation；无旧 snapshot 时允许从 0 开始。
    pub source_generation: u64,
    /// 计划目标 generation，必须严格大于 source generation。
    pub target_generation: u64,
    /// 旧 snapshot digest；首代索引可能没有旧 snapshot。
    #[serde(default)]
    pub source_snapshot_digest: Option<String>,
    /// 目标 snapshot digest。
    pub target_snapshot_digest: String,
    /// 按路径/kind 稳定排序的逐路径变更行。
    pub changes: Vec<WorkspaceChange>,
    /// 下一代索引必须失效或重算的路径集合。
    pub invalidated_paths: Vec<String>,
    /// 已消失的旧路径；下游需要用墓碑防止 cache/index 保留旧内容。
    pub tombstones: Vec<String>,
    /// 对其余计划字段计算的无密钥 SHA-256 摘要。
    pub plan_digest: String,
}

impl IndexInvalidationPlan {
    /// 比较旧/新 workspace snapshot，生成确定性 path change、invalidation 列表和 tombstones。
    ///
    /// 【调用者】Query `plan_index_invalidation()` adapter，或负责增量索引编排的上层。
    /// 【输入/输出】`previous=None` 表示首次索引；传入旧/新 generation 和 snapshots，返回不修改 snapshot/index 的纯计划。
    /// 【核心流程】校验 current → 按路径建立 map → 对移除/新增项按 effective digest 配对 rename → 检查同路径的内容/disposition/safety 变化 →
    /// 排序 change 行 → 从旧/新路径集合生成 invalidated_paths 与 tombstones → 计算 plan digest。
    /// 【失败情况】当前 snapshot 无效、generation 不递增、变更超过 4096 条、路径/摘要不合法时返回 `String`。
    /// 【易误改边界】当前只调用 `current.validate()`，没有校验 `previous`；调用者必须先保证旧 snapshot 有效，否则重复 path 会在 BTreeMap 收集时折叠。
    /// rename 仅按 effective digest 相等匹配，不核验 inode，因此相同内容也可能成为 rename 证据；该计划不是文件删除/写入授权。
    pub fn from_snapshots(
        previous: Option<&WorkspaceSnapshot>,
        current: &WorkspaceSnapshot,
        source_generation: u64,
        target_generation: u64,
    ) -> Result<Self, String> {
        current.validate()?;
        if target_generation <= source_generation {
            return Err("index_invalidation_generation_order_invalid".to_owned());
        }
        let previous_files = previous
            .map(|snapshot| {
                snapshot
                    .files
                    .iter()
                    .map(|file| (file.path.clone(), file))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let current_files = current
            .files
            .iter()
            .map(|file| (file.path.clone(), file))
            .collect::<BTreeMap<_, _>>();
        let mut removed = previous_files
            .iter()
            .filter(|(path, _)| !current_files.contains_key(*path))
            .map(|(path, file)| (path.to_string(), effective_digest(file)))
            .collect::<Vec<_>>();
        let mut added = current_files
            .iter()
            .filter(|(path, _)| !previous_files.contains_key(*path))
            .map(|(path, file)| (path.to_string(), effective_digest(file)))
            .collect::<Vec<_>>();
        removed.sort_by(|left, right| left.0.cmp(&right.0));
        added.sort_by(|left, right| left.0.cmp(&right.0));
        let mut changes = Vec::new();
        // 记录已被 rename 配对消耗的新路径，避免同一个 Added 行同时匹配多个旧文件。
        let mut consumed_added = BTreeSet::new();
        for (old_path, before_digest) in &removed {
            if let Some((new_path, after_digest)) = added
                .iter()
                .find(|(path, digest)| !consumed_added.contains(path) && digest == before_digest)
            {
                consumed_added.insert(new_path.clone());
                changes.push(WorkspaceChange::new(
                    WorkspaceChangeKind::Renamed,
                    new_path.clone(),
                    Some(old_path.clone()),
                    Some(before_digest.clone()),
                    Some(after_digest.clone()),
                )?);
            } else {
                changes.push(WorkspaceChange::new(
                    WorkspaceChangeKind::Removed,
                    old_path.clone(),
                    None,
                    Some(before_digest.clone()),
                    None,
                )?);
            }
        }
        for (new_path, after_digest) in &added {
            if !consumed_added.contains(new_path) {
                changes.push(WorkspaceChange::new(
                    WorkspaceChangeKind::Added,
                    new_path.clone(),
                    None,
                    None,
                    Some(after_digest.clone()),
                )?);
            }
        }
        for (path, before) in &previous_files {
            if let Some(after) = current_files.get(path) {
                let before_digest = effective_digest(before);
                let after_digest = effective_digest(after);
                if before_digest != after_digest
                    || before.disposition != after.disposition
                    || before.instruction_safe != after.instruction_safe
                {
                    changes.push(WorkspaceChange::new(
                        WorkspaceChangeKind::Changed,
                        path.clone(),
                        None,
                        Some(before_digest),
                        Some(after_digest),
                    )?);
                }
            }
        }
        changes.sort_by(|left, right| left.path.cmp(&right.path).then(left.kind.cmp(&right.kind)));
        if changes.len() > MAX_INDEX_CHANGES {
            return Err("index_invalidation_change_limit".to_owned());
        }
        let mut invalidated = BTreeSet::new();
        let mut tombstones = BTreeSet::new();
        // BTreeSet 一边去重一边提供稳定排序；rename 需要同时失效新旧路径并 tombstone 旧路径。
        for change in &changes {
            invalidated.insert(change.path.clone());
            if let Some(previous_path) = &change.previous_path {
                invalidated.insert(previous_path.clone());
                tombstones.insert(previous_path.clone());
            }
            if change.kind == WorkspaceChangeKind::Removed {
                tombstones.insert(change.path.clone());
            }
        }
        let mut plan = Self {
            schema: INDEX_INVALIDATION_SCHEMA.to_owned(),
            version: INDEX_INVALIDATION_VERSION,
            source_generation,
            target_generation,
            source_snapshot_digest: previous.map(|snapshot| snapshot.snapshot_digest.clone()),
            target_snapshot_digest: current.snapshot_digest.clone(),
            changes,
            invalidated_paths: invalidated.into_iter().collect(),
            tombstones: tombstones.into_iter().collect(),
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    /// 验证 plan header、generation 顺序、嵌套 change、路径集合和自摘要。
    ///
    /// 反序列化后没有原始 snapshots 可供重算，所以本方法不会证明 `changes`、`invalidated_paths` 和 `tombstones`
    /// 确实由这两份 snapshot 推导；它们的 digest 是无密钥的，调用者重算后仍可构造另一份结构自洽计划。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_INVALIDATION_SCHEMA
            || self.version != INDEX_INVALIDATION_VERSION
            || self.target_generation <= self.source_generation
            || self.changes.len() > MAX_INDEX_CHANGES
        {
            return Err("index_invalidation_header_invalid".to_owned());
        }
        if let Some(source) = &self.source_snapshot_digest {
            digest(source, "index_invalidation_source_snapshot_digest")?;
        }
        digest(
            &self.target_snapshot_digest,
            "index_invalidation_target_snapshot_digest",
        )?;
        let mut change_digests = BTreeSet::new();
        for change in &self.changes {
            change.validate()?;
            if !change_digests.insert(change.change_digest.clone()) {
                return Err("index_invalidation_duplicate_change".to_owned());
            }
        }
        validate_paths(&self.invalidated_paths, "index_invalidation_path")?;
        validate_paths(&self.tombstones, "index_invalidation_tombstone")?;
        digest(&self.plan_digest, "index_invalidation_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("index_invalidation_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 plan 除 `plan_digest` 外的字段计算内容摘要。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_generation": self.source_generation,
            "target_generation": self.target_generation,
            "source_snapshot_digest": self.source_snapshot_digest,
            "target_snapshot_digest": self.target_snapshot_digest,
            "changes": self.changes,
            "invalidated_paths": self.invalidated_paths,
            "tombstones": self.tombstones,
        }))
    }
}

/// 按顺序校验一组相对路径，并拒绝重复项。
fn validate_paths(paths: &[String], field: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for value in paths {
        path(value, field)?;
        if !seen.insert(value) {
            return Err(format!("{field}_duplicate"));
        }
    }
    Ok(())
}

/// 选择快照中用于内容比较的摘要：优先使用扫描后 identity 的 content digest，缺少时退回 file_digest。
///
/// 这样可以把读取前后 identity 及实际读入内容的证据纳入差异判断，而不是单靠 mtime。
fn effective_digest(file: &WorkspaceFileSnapshot) -> String {
    file.identity_after
        .as_ref()
        .and_then(|identity| identity.content_digest.clone())
        .unwrap_or_else(|| file.file_digest.clone())
}

// 这个 helper 当前没有被 from_snapshots() 调用，保留作未来实际索引筛选 adapter 的合同提示。
// 当前 invalidation 仍把不可信/仅元数据行当作变化证据；此函数本身不改变计划中的行。
#[allow(dead_code)]
fn is_indexable(file: &WorkspaceFileSnapshot) -> bool {
    file.disposition == WorkspaceReadDisposition::Indexed && file.instruction_safe
}
