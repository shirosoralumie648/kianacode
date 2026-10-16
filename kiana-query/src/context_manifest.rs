//! 本文件负责把 RepoMap、上下文 artifact 集合和启发式依赖图组合成可写入磁盘的上下文材料清单。
//!
//! 在整个系统中的位置：这些索引数据由 `kiana-query` 从项目文件派生，调用方可将结果写成
//! JSON，之后再读回并检查结构与摘要。仓内目前可见的构建、写入和读取调用位于 PD-20
//! fixture；`kiana-query/src/lib.rs` 公开了这些 API，但这不等于它已经接入生产请求路径。
//!
//! 主要数据流如下：
//!
//! ```text
//! 项目根目录 + RepoMapOptions + ContextArtifactOptions + 调用方提供的 source_cursor/tool_version
//!     -> build_repo_map() 与 build_context_artifact_store() 分别扫描项目
//!     -> ContextMaterialManifest 组合两份派生报告并计算 manifest_digest
//!     -> validate() 检查清单合同
//!     -> atomic_write_json() 持久化，或 read_context_material_manifest() 解码后再校验
//! ```
//!
//! 清单记录的是索引输入和报告摘要，不会把摘要变成文件真实性证明，也不会替代事件日志、
//! ControlPlane 或其他执行授权来源。构建过程会读取项目文件；写入过程只写清单 JSON，
//! 不会激活 artifact、执行工作流或改变项目源文件。

use crate::index::{build_context_artifact_store, ContextArtifactOptions, ContextArtifactStore};
use crate::index_generation::atomic_write_json;
use crate::repo_map::{build_repo_map, RepoMap, RepoMapOptions};
use anyhow::{anyhow, Context, Result};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

pub const CONTEXT_MATERIAL_MANIFEST_SCHEMA: &str = "kiana.context-material-manifest.v1";

/// 检查调用方提供的非空文本是否满足本文件约定的字节数和 NUL 字符边界。
///
/// 【调用者】`ContextMaterialManifest::build()` 和 `validate()` 用它检查工具版本与根路径。
/// 【输入/输出】输入字段值、错误码前缀和最大 UTF-8 字节数；通过时返回 `Ok(())`，否则
/// 返回稳定的字段级错误字符串，供上层继续传播。
/// 【为什么使用字节数】Rust 的 `str::len()` 返回 UTF-8 字节数，不是中文字符数；这里的
/// 上限因此约束序列化输入大小，而不是用户看到的字符数量。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 只检查 `sha256:` 摘要的文本形状，不在这里重新读取文件或计算摘要。
///
/// 调用者分别负责提供已计算的文件摘要或 manifest 摘要；此函数要求前缀后恰有 64 个
/// 十六进制字符。通过形状检查不代表摘要来自可信来源，也不代表它与磁盘上的内容相符。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// RepoMap、artifact store 与依赖图的持久化组合清单。
///
/// 【系统角色】它是 `kiana-query` 派生上下文材料的磁盘边界：构建时聚合报告，读回时
/// 验证外层清单合同。Domain/Query 的业务调用方提供 source cursor 和工具版本；当前仓内
/// 直接构建、读写 API 的调用点是 PD-20 fixture，crate 根模块另行 re-export 这些接口。
/// 【数据流】项目文件分别进入 RepoMap 和 artifact store 扫描器，结果及根目录被放入本结构，
/// `manifest_digest` 再覆盖清单主体；写入前和读取后都会调用 `validate()`。
/// 【边界】Serde 负责 Rust 值与 JSON 的双向转换；`deny_unknown_fields` 使读者拒绝本版本
/// 未认识的额外字段，避免把未知合同静默当成已理解的数据。摘要是无密钥的自摘要，不是
/// 签名或认证；读取清单也不会重新扫描磁盘来证明各文件仍与摘要相符。
///
/// 以下 `Serialize` / `Deserialize` 来自 serde：前者供 JSON 写出，后者供 JSON 读回。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMaterialManifest {
    /// 固定版本标识；不匹配时拒绝按当前字段合同解释清单。
    pub schema: String,
    /// RepoMap 与 artifact store 共同声称对应的 canonical 项目根路径。
    pub root: String,
    /// 调用方提供的非零来源游标，用于标记生成材料对应的上游位置；本模块不查询 EventLog
    /// 来证明它真实或新鲜。
    pub source_cursor: u64,
    /// 调用方提供的生成工具版本文本，最多 128 个 UTF-8 字节。
    pub tool_version: String,
    /// 从项目路径、语言和文件预算派生的轻量文件地图。
    pub repo_map: RepoMap,
    /// 文件 artifact、角色汇总及启发式依赖图的组合报告。
    pub artifact_store: ContextArtifactStore,
    /// 对 schema、根路径、来源游标、工具版本及两份报告做 JSON 摘要后的结果；自身不参与
    /// 被摘要对象，避免出现递归定义。
    pub manifest_digest: String,
}

impl ContextMaterialManifest {
    /// 从一个项目根目录重新构建两份上下文报告并组合成经过校验的清单。
    ///
    /// 【调用者】仓内当前直接调用点是 `kiana-query/tests/pd20_context_material_manifest.rs`；
    /// crate 公共 API 也允许外部调用方使用，但尚未从本仓生产路由中找到调用。
    /// 【输入】`root` 是待扫描目录；两个 options 分别控制 RepoMap 和 artifact 扫描；
    /// `source_cursor` 与 `tool_version` 来自调用方，不由本函数查询或推导。
    /// 【输出】成功时返回根路径一致、摘要已计算且通过 `validate()` 的组合清单。
    /// 【副作用】通过两个 builder 读取项目文件；本函数不写清单文件、不改项目文件、不访问网络。
    /// 【失败】游标为零、版本文本不合约、任一 builder 失败、两次扫描得到不同根路径，或
    /// 最终清单未通过验证时返回错误字符串。底层 builder 错误在这里转成字符串继续传播。
    /// 【核心流程】先拒绝零游标并校验版本，再分别构建 RepoMap 与 artifact store，检查根路径，
    /// 组装字段，计算自摘要，最后调用 `validate()`。
    /// 【设计原因】RepoMap 与 artifact store 描述不同索引视图，清单把它们绑定到同一个根和
    /// 一组来源元数据，方便一起持久化；但底层会分开扫描，不能据此声称它们来自原子文件快照。
    pub fn build(
        root: impl AsRef<Path>,
        repo_options: RepoMapOptions,
        artifact_options: ContextArtifactOptions,
        source_cursor: u64,
        tool_version: impl Into<String>,
    ) -> Result<Self, String> {
        if source_cursor == 0 {
            return Err("context_material_source_cursor_invalid".to_owned());
        }
        let tool_version = tool_version.into();
        required(&tool_version, "context_material_tool_version", 128)?;
        let repo_map = build_repo_map(&root, repo_options).map_err(|error| error.to_string())?;
        let artifact_store = build_context_artifact_store(&root, artifact_options)
            .map_err(|error| error.to_string())?;
        if repo_map.root != artifact_store.root {
            return Err("context_material_root_mismatch".to_owned());
        }
        let mut manifest = Self {
            schema: CONTEXT_MATERIAL_MANIFEST_SCHEMA.to_owned(),
            root: repo_map.root.clone(),
            source_cursor,
            tool_version,
            repo_map,
            artifact_store,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    /// 检查外层清单的版本、来源、根路径、路径/摘要文本形状和自摘要。
    ///
    /// 【调用者】`build()` 在返回前调用；原子写入前和 JSON 读回后也调用。
    /// 【输入/输出】读取 `self` 中的所有外层字段及两个报告中的文件路径/内容摘要；成功返回
    /// `Ok(())`，失败返回稳定错误字符串，读 API 再将它包装成 `anyhow::Error`。
    /// 【失败的影响】写入 API 在失败时不会进入原子写 helper；读取 API 不会把无效对象交给调用方。
    /// 这些错误不触发重试、账号封禁或外部状态修改。
    /// 【校验顺序】先检查版本与非零游标，再检查根路径和版本文本，再绑定两个报告的顶层根；
    /// 随后逐个检查 RepoMap/artifact 路径与内容摘要形状，最后检查 manifest 摘要格式及其值。
    /// 【边界】这里没有重新打开项目文件重算内容摘要，也没有逐项校验依赖图的 root、节点/边
    /// 引用、计数或角色汇总是否彼此一致；调用方不能把 `Ok(())` 当成新鲜性或来源真实性证明。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_MATERIAL_MANIFEST_SCHEMA || self.source_cursor == 0 {
            return Err("context_material_manifest_header_invalid".to_owned());
        }
        required(&self.root, "context_material_root", 4096)?;
        required(&self.tool_version, "context_material_tool_version", 128)?;
        if self.repo_map.root != self.root || self.artifact_store.root != self.root {
            return Err("context_material_root_mismatch".to_owned());
        }
        for file in &self.repo_map.files {
            if file.path.starts_with('/') || file.path.contains("..") {
                return Err("context_material_repo_path_invalid".to_owned());
            }
            digest(&file.content_hash, "context_material_repo_content_hash")?;
        }
        for artifact in &self.artifact_store.artifacts.artifacts {
            if artifact.path.starts_with('/') || artifact.path.contains("..") {
                return Err("context_material_artifact_path_invalid".to_owned());
            }
            digest(
                &artifact.content_hash,
                "context_material_artifact_content_hash",
            )?;
        }
        digest(&self.manifest_digest, "context_material_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("context_material_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算清单主体的确定性 JSON 摘要。
    ///
    /// `manifest_digest` 故意不放进 JSON 输入，避免摘要包含自身而无法稳定计算。`json_digest`
    /// 提供无密钥摘要；若有人同时改动内容并重新计算此字段，单靠它不能识别伪造，因此不能
    /// 代替签名、受信任存储或重新核对源文件。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "root": self.root,
            "source_cursor": self.source_cursor,
            "tool_version": self.tool_version,
            "repo_map": self.repo_map,
            "artifact_store": self.artifact_store,
        }))
    }
}

/// 校验清单后，将完整 JSON 报告通过共享原子写 helper 发布到指定路径。
///
/// 【调用者】仓内当前由 PD-20 round-trip fixture 调用；实现对 crate 外公开。
/// 【输入】目标文件路径和已构建/修改的 manifest 引用。
/// 【输出】成功返回 `Ok(())`；验证或文件系统错误以字符串返回。
/// 【副作用】helper 会创建父目录、创建同目录临时文件、写入并同步文件，再 rename 到目标，
/// 最后同步父目录；出错时尽力删除临时文件。它不写项目索引源文件。
/// 【并发与失败】原子替换避免读者看到半份 JSON，但 helper 没有跨进程锁或 CAS；临时名冲突
/// 会失败，同时写同一路径的多个进程最终由最后成功 rename 的写者决定版本。
/// 【为什么先 validate】防止本地构造的字段不一致对象被发布为看似有效的清单；写成功仍只证明
/// 序列化和本地文件替换完成，不提升摘要的真实性或内容新鲜度。
pub fn write_context_material_manifest_atomic(
    path: impl AsRef<Path>,
    manifest: &ContextMaterialManifest,
) -> Result<(), String> {
    manifest.validate()?;
    atomic_write_json(path.as_ref(), manifest, "context_material_manifest")
}

/// 读取 JSON 清单并在交给调用方前重新执行外层合同校验。
///
/// 【调用者】仓内当前由 PD-20 round-trip fixture 调用；公共接口可供其他 Query 使用者调用。
/// 【输入/输出】输入清单路径；成功返回反序列化且通过 `validate()` 的对象。
/// 【数据流】路径 → `read_to_string` → serde JSON 解码（未知字段会被结构体属性拒绝）→
/// manifest 验证 → 调用方。文件读取错误带路径上下文；解码错误统一成可读错误；合同错误
/// 保留领域错误字符串并包装为 anyhow 错误。
/// 【副作用/失败】只读取文件，不写磁盘、不扫描项目、不重算源文件内容 hash。任何阶段失败都会
/// 返回错误，不会给调用方部分解码结果，也不会自动重试或更改清单。
pub fn read_context_material_manifest(path: impl AsRef<Path>) -> Result<ContextMaterialManifest> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)
        .with_context(|| format!("context material manifest open failed: {}", path.display()))?;
    let manifest: ContextMaterialManifest = serde_json::from_str(&contents)
        .map_err(|_| anyhow!("context material manifest decode failed"))?;
    manifest.validate().map_err(|error| anyhow!(error))?;
    Ok(manifest)
}
