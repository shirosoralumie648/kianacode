//! ContextIndex source fingerprint、统一 generation envelope 和 manifest 原子发布 adapter，属于 `kiana-query`。
//!
//! 【系统位置与数据流】
//! ```text
//! build_context_index() 扫描项目文本文件
//!     -> ContextIndexSourceManifest 绑定 root/files/ignore/config/model
//!     -> IndexGenerationState 暂存并提交一个 Domain Ready manifest
//!     -> ContextIndexGenerationEnvelope 绑定 source、index 和 manifest
//!     -> 临时文件写入 + sync + rename 发布
//!     -> read_context_index_generation() 重新解码并校验
//! ```
//! 本模块是 Query adapter，负责同步文件 I/O 和内容 envelope；Domain generation state 负责 Ready/Building 决策。
//! 它不接入 EventLog/ControlPlane，也不授予检索或执行权限。当前仓库没有生产调用这些 generation API 的调用点，主要覆盖来自 fixture。
//! 文件发布的 rename 是单文件原子可见边界，不提供跨进程锁或 compare-and-swap；Unix 会 sync 父目录，非 Unix 平台的父目录 sync helper 是空操作。
//!
//! Serde 的 Serialize/Deserialize 把 DTO 与 JSON 互转；`deny_unknown_fields` 避免 reader 静默忽略不认识的字段。

use crate::index::{build_context_index, ContextIndex, ContextIndexOptions};
use anyhow::{anyhow, Context, Result};
use kiana_domain::{
    json_digest, Freshness, IndexComponentKind, IndexGenerationState, IndexManifest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const CONTEXT_INDEX_SOURCE_MANIFEST_SCHEMA: &str = "kiana.context-index-source-manifest.v1";
/// source/index/Domain manifest 一起发布时使用的 envelope schema。
pub const CONTEXT_INDEX_GENERATION_ENVELOPE_SCHEMA: &str =
    "kiana.context-index-generation-envelope.v1";

/// 校验 `sha256:` 加 64 位十六进制字符串外形，不读取 source/index 数据。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验必须存在的 root/model 等文字字段的非空、字节长度和 NUL。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// ContextIndex 本次构建使用的 source/config/model 指纹。
///
/// 它把文件清单、忽略规则与 builder 参数变成一个可比较的输入快照；不是文件内容本身，也不是索引的授权来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextIndexSourceManifest {
    /// 固定 source manifest schema 名称。
    pub schema: String,
    /// ContextIndex 扫描后 canonicalize 的根路径。
    pub root: String,
    /// 该 source manifest 所属的索引 generation。
    pub generation: u64,
    /// root、排序文件路径/大小/content hash、ignore rules 和 config 的合成指纹。
    pub source_fingerprint: String,
    /// repo map 忽略规则的摘要；忽略规则改变会使旧索引 stale。
    pub ignore_rules_digest: String,
    /// max_bytes/model 等构建参数的摘要。
    pub config_digest: String,
    /// 使用的本地 embedding/model 标签；该字符串本身不启动在线模型。
    pub model_id: String,
    /// 新构建时固定为 Current；freshness 比较结果会作为返回值提供，不回写本结构。
    pub freshness: Freshness,
    /// 对其余 source manifest 字段计算的无密钥摘要。
    pub manifest_digest: String,
}

impl ContextIndexSourceManifest {
    /// 从一份已构建 ContextIndex 和其选项生成稳定 source manifest。
    ///
    /// 【调用者】`build_context_index_generation()` 在文件扫描完成后调用。
    /// 【输入/输出】接收 index、目标 generation、读取选项、ignore digest 和 model label；成功返回可和旧 source 比较的清单。
    /// 【核心流程】按相对 path 排序文件 → 计算 config digest → 将 root/files/ignore/config 算成 source fingerprint → 填字段并校验自摘要。
    /// 【失败情况】generation、root、ignore digest、model 或生成摘要格式不合约时返回稳定 `String` 错误。
    /// 【限制】这个 fingerprint 绑定扫描结果和声明参数，不证明扫描期间文件未在读取后变化，也不证明真实 embedding 实现与 model label 一致。
    pub fn from_index(
        index: &ContextIndex,
        generation: u64,
        options: &ContextIndexOptions,
        ignore_rules_digest: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Result<Self, String> {
        let ignore_rules_digest = ignore_rules_digest.into();
        let model_id = model_id.into();
        if generation == 0 {
            return Err("context_index_generation_invalid".to_owned());
        }
        required(&index.root, "context_index_root", 4096)?;
        digest(&ignore_rules_digest, "context_index_ignore_rules_digest")?;
        required(&model_id, "context_index_model_id", 256)?;
        let mut files = index
            .files
            .iter()
            .map(|file| (&file.path, &file.bytes, &file.content_hash))
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(right.0));
        let config_digest = json_digest(&json!({
            "max_bytes_per_file": options.max_bytes_per_file,
            "model_id": &model_id,
        }));
        // 排序后的 path 序列确保同一组文件无论目录遍历顺序怎样，都得到一致指纹。
        let source_fingerprint = json_digest(&json!({
            "root": &index.root,
            "files": files,
            "ignore_rules_digest": &ignore_rules_digest,
            "config_digest": &config_digest,
        }));
        let mut manifest = Self {
            schema: CONTEXT_INDEX_SOURCE_MANIFEST_SCHEMA.to_owned(),
            root: index.root.clone(),
            generation,
            source_fingerprint,
            ignore_rules_digest,
            config_digest,
            model_id,
            freshness: Freshness::Current,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    /// 比较两份 source/config/model 输入，返回当前清单相对另一份是否仍新鲜。
    ///
    /// generation 不参与 Freshness 比较：只要 root、文件指纹、ignore/config 和 model 都相同，不同发布代数也可表示同一内容。
    /// 任一输入不同返回 Stale；错误 manifest 返回校验错误。
    pub fn compare(&self, other: &Self) -> Result<Freshness, String> {
        self.validate()?;
        other.validate()?;
        Ok(
            if self.root == other.root
                && self.source_fingerprint == other.source_fingerprint
                && self.ignore_rules_digest == other.ignore_rules_digest
                && self.config_digest == other.config_digest
                && self.model_id == other.model_id
            {
                Freshness::Current
            } else {
                Freshness::Stale
            },
        )
    }

    /// 校验 schema、非零代数、Current 初始标记、各摘要外形和自摘要。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_INDEX_SOURCE_MANIFEST_SCHEMA
            || self.generation == 0
            || self.freshness != Freshness::Current
        {
            return Err("context_index_source_manifest_invalid".to_owned());
        }
        required(&self.root, "context_index_root", 4096)?;
        digest(&self.source_fingerprint, "context_index_source_fingerprint")?;
        digest(
            &self.ignore_rules_digest,
            "context_index_ignore_rules_digest",
        )?;
        digest(&self.config_digest, "context_index_config_digest")?;
        required(&self.model_id, "context_index_model_id", 256)?;
        digest(&self.manifest_digest, "context_index_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("context_index_source_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 source manifest 除 `manifest_digest` 外的字段计算 SHA-256 内容指纹。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "root": self.root,
            "generation": self.generation,
            "source_fingerprint": self.source_fingerprint,
            "ignore_rules_digest": self.ignore_rules_digest,
            "config_digest": self.config_digest,
            "model_id": self.model_id,
            "freshness": self.freshness,
        }))
    }
}

/// 一次完整的 ContextIndex 发布单元，避免读者把不同 source/generation 的部件拼在一起。
///
/// envelope 将 source metadata、扫描出来的 index 和 Domain index manifest 放在一个 JSON 载体中；当前验证是 DTO 级绑定，摘要没有密钥。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextIndexGenerationEnvelope {
    /// 固定 envelope schema 名称。
    pub schema: String,
    /// source root/files/config/model 的生成快照。
    pub source: ContextIndexSourceManifest,
    /// 实际序列化的工作区文本索引内容。
    pub index: ContextIndex,
    /// Domain 中该 generation 的 Ready-only 组件 manifest。
    pub index_manifest: IndexManifest,
    /// 对其余 envelope 字段计算的无密钥 SHA-256 摘要。
    pub envelope_digest: String,
}

impl ContextIndexGenerationEnvelope {
    /// 验证嵌套清单、Ready 状态、代数/root 和 envelope 自摘要。
    ///
    /// ⚠ 当前只检查 `index_manifest` 为 Ready、generation 与 source 相同、index.root 与 source.root 相同；
    /// 没有比较 `index_manifest.source_manifest_digest` 与 `source.manifest_digest`，也没有用 embedded index 重算 source/component digest。
    /// 因此此方法不能被描述成完整验证 index 内容与 Domain component manifest 的来源绑定。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_INDEX_GENERATION_ENVELOPE_SCHEMA {
            return Err("context_index_generation_envelope_schema_invalid".to_owned());
        }
        self.source.validate()?;
        self.index_manifest.validate()?;
        if self.index_manifest.status != kiana_domain::IndexGenerationStatus::Ready
            || self.index_manifest.generation != self.source.generation
            || self.index.root != self.source.root
        {
            return Err("context_index_generation_envelope_binding_invalid".to_owned());
        }
        digest(&self.envelope_digest, "context_index_envelope_digest")?;
        if self.envelope_digest != self.digest() {
            return Err("context_index_generation_envelope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 envelope 的 source、index 和 index manifest 计算内容摘要，不包含自身摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source": self.source,
            "index": self.index,
            "index_manifest": self.index_manifest,
        }))
    }
}

/// 扫描 root、生成 source 指纹、通过 Domain 状态机分配 generation 并组装 Ready envelope。
///
/// 【调用者】Query 层索引构建编排；当前对外导出但仓内没有生产调用者，PD-19 fixture 直接调用。
/// 【输入/输出】接收项目根、扫描选项、上一代 state、ignore digest、model label；返回 envelope 和更新后的内存 generation state。
/// 【核心流程】验证 previous → 调用已有 `build_context_index()` 扫描文件 → 生成 source manifest 与 whole-index digest →
/// 派生四个组件摘要 → 对 cloned state 执行 begin/commit → 验证 generation 及 envelope。
/// 【副作用/失败】会由 `build_context_index()` 读取工作区文件，但不写发布文件；失败转换为 String，调用者随后可选择是否持久化。
/// 【实现边界】当前四个 component digest 都由同一个 serialized ContextIndex digest 加 kind/source fingerprint 派生，
/// 并非本函数分别构建 repo-map/exact/BM25/dense 四个物理索引文件。
pub fn build_context_index_generation(
    root: impl AsRef<Path>,
    options: ContextIndexOptions,
    previous: &IndexGenerationState,
    ignore_rules_digest: impl Into<String>,
    model_id: impl Into<String>,
) -> Result<(ContextIndexGenerationEnvelope, IndexGenerationState), String> {
    previous.validate()?;
    let index = build_context_index(root, options).map_err(|error| error.to_string())?;
    let generation = previous.next_generation;
    let source = ContextIndexSourceManifest::from_index(
        &index,
        generation,
        &options,
        ignore_rules_digest,
        model_id,
    )?;
    let index_digest = json_digest(
        &serde_json::to_value(&index).map_err(|_| "context_index_encode_failed".to_owned())?,
    );
    let components = IndexComponentKind::ALL
        .into_iter()
        .map(|kind| {
            // 为 Domain 提供完整四类 key；这里的 component hash 是同一整体 ContextIndex 的分类指纹。
            (
                kind.as_str().to_owned(),
                json_digest(&json!({
                    "kind": kind,
                    "source_fingerprint": &source.source_fingerprint,
                    "index_digest": &index_digest,
                })),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut next = previous.clone();
    let building = next.begin(source.manifest_digest.clone(), components)?;
    let ready = next.commit(building)?;
    if ready.generation != source.generation {
        return Err("context_index_generation_binding_invalid".to_owned());
    }
    let mut envelope = ContextIndexGenerationEnvelope {
        schema: CONTEXT_INDEX_GENERATION_ENVELOPE_SCHEMA.to_owned(),
        source,
        index,
        index_manifest: ready,
        envelope_digest: String::new(),
    };
    envelope.envelope_digest = envelope.digest();
    envelope.validate()?;
    Ok((envelope, next))
}

/// 校验 envelope 后，把整个 generation envelope 原子写入目标 JSON 文件。
///
/// 【调用者】发布 generation 的 Query adapter；当前由测试直接调用。
/// 【失败情况】DTO 校验或父目录/temp/write/sync/rename 出错时返回带稳定标签的 String；失败时 best-effort 删除临时文件。
/// 【副作用】创建目录并写文件，成功后目标路径只出现完整 envelope；不会同时持久化 `IndexGenerationState`。
pub fn write_context_index_generation_atomic(
    path: impl AsRef<Path>,
    envelope: &ContextIndexGenerationEnvelope,
) -> Result<(), String> {
    envelope.validate()?;
    atomic_write_json(path.as_ref(), envelope, "context_index_generation")
}

/// 从磁盘读取完整 envelope，并在返回前重新执行 DTO 校验。
///
/// 文件不存在/不可读时用 `anyhow::Context` 保留路径上下文，JSON decode 或 envelope 校验失败则返回错误。
/// 此函数没有当前 workspace snapshot 参数，所以不会检查磁盘内容是否仍与 source fingerprint 新鲜匹配。
pub fn read_context_index_generation(
    path: impl AsRef<Path>,
) -> Result<ContextIndexGenerationEnvelope> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)
        .with_context(|| format!("context index generation open failed: {}", path.display()))?;
    let envelope: ContextIndexGenerationEnvelope = serde_json::from_str(&contents)
        .map_err(|_| anyhow!("context index generation decode failed"))?;
    envelope.validate().map_err(|error| anyhow!(error))?;
    Ok(envelope)
}

/// 将单独的 Domain Ready manifest 以临时文件 + sync + rename 方式发布。
///
/// Building/Failed manifest 在创建临时文件前就被拒绝；读回时 `read_index_manifest()` 也只返回 Ready。
pub fn write_index_manifest_atomic(
    path: impl AsRef<Path>,
    manifest: &IndexManifest,
) -> Result<(), String> {
    manifest.validate()?;
    if manifest.status != kiana_domain::IndexGenerationStatus::Ready {
        return Err("index_manifest_publish_requires_ready".to_owned());
    }
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| "index_manifest_parent_missing".to_owned())?;
    fs::create_dir_all(parent).map_err(|_| "index_manifest_parent_create_failed".to_owned())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "index_manifest_filename_missing".to_owned())?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|_| "index_manifest_encode_failed".to_owned())?;
    let result = (|| -> Result<(), String> {
        // create_new 防止覆盖同名临时文件；临时文件与目标位于同一父目录，确保 rename 在同一文件系统。
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "index_manifest_temp_create_failed".to_owned())?;
        file.write_all(&bytes)
            .map_err(|_| "index_manifest_temp_write_failed".to_owned())?;
        file.sync_all()
            .map_err(|_| "index_manifest_temp_sync_failed".to_owned())?;
        // 在 rename 前目标文件仍保持旧版本；rename 后读者看到的是完整新 manifest，而非半写入 JSON。
        fs::rename(&temporary, path)
            .map_err(|_| "index_manifest_atomic_rename_failed".to_owned())?;
        sync_parent(parent).map_err(|_| "index_manifest_parent_sync_failed".to_owned())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// 将已校验对象用统一的临时文件流程原子写成 JSON；供 generation envelope 复用。
///
/// `label` 只为各处错误码加前缀。临时文件名含进程 ID，create_new 冲突会失败；此函数不加锁/CAS，
/// 多进程同时发布到同一目标仍由最后成功 rename 的写者决定最终版本。
pub(crate) fn atomic_write_json<T: Serialize>(
    path: &Path,
    value: &T,
    label: &str,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{label}_parent_missing"))?;
    fs::create_dir_all(parent).map_err(|_| format!("{label}_parent_create_failed"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("{label}_filename_missing"))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| format!("{label}_encode_failed"))?;
    let result = (|| -> Result<(), String> {
        // 临时文件先独占创建，再完整写入并 sync；这避免同名的未完成文件被复用。
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| format!("{label}_temp_create_failed"))?;
        file.write_all(&bytes)
            .map_err(|_| format!("{label}_temp_write_failed"))?;
        file.sync_all()
            .map_err(|_| format!("{label}_temp_sync_failed"))?;
        fs::rename(&temporary, path).map_err(|_| format!("{label}_atomic_rename_failed"))?;
        sync_parent(parent).map_err(|_| format!("{label}_parent_sync_failed"))?;
        Ok(())
    })();
    if result.is_err() {
        // 错误路径只尽力清理未 rename 的临时文件；清理本身失败不会覆盖最初的写入错误。
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// 读取独立 manifest，解码后重新校验，并拒绝仍处于 Building/Failed 的版本。
///
/// 文件打开/读取错误附带路径上下文；JSON 解码失败统一映射为 anyhow 错误，manifest 合同错误保留领域错误文本。
pub fn read_index_manifest(path: impl AsRef<Path>) -> Result<IndexManifest> {
    let path = path.as_ref();
    let mut contents = String::new();
    File::open(path)
        .with_context(|| format!("index manifest open failed: {}", path.display()))?
        .read_to_string(&mut contents)
        .with_context(|| format!("index manifest read failed: {}", path.display()))?;
    let manifest: IndexManifest =
        serde_json::from_str(&contents).map_err(|_| anyhow!("index manifest decode failed"))?;
    manifest.validate().map_err(|error| anyhow!(error))?;
    if manifest.status != kiana_domain::IndexGenerationStatus::Ready {
        return Err(anyhow!("index manifest is not ready"));
    }
    Ok(manifest)
}

/// 在支持目录句柄同步的平台上刷新 parent directory，帮助 rename 后的目录项跨崩溃保留。
///
/// 当前 Unix 分支打开目录并 `sync_all()`；非 Unix 分支不做等价同步，返回成功但耐久性保证较弱。
fn sync_parent(parent: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

/// 把相对 manifest 路径拼到 root 下；绝对路径原样返回。
///
/// ⚠ 绝对路径不会被 root 约束，也没有路径穿越检查；调用者不能把此 helper 当作项目根目录 sandbox。
pub fn manifest_path(root: impl AsRef<Path>, relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    if relative.is_absolute() {
        relative.to_path_buf()
    } else {
        root.as_ref().join(relative)
    }
}
