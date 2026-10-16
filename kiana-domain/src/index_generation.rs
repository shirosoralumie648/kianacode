//! 多组件索引的 source manifest 与 generation 状态合同，属于 `kiana-domain`。
//!
//! 【系统位置与数据流】
//! ```text
//! Query builder 计算同一 source snapshot 下的组件摘要
//!     -> IndexGenerationState::begin() 暂存 Building manifest
//!     -> 全部组件完成后 commit() 转成 Ready
//!     -> Query adapter 原子发布 Ready manifest
//!     -> reader() 只拿到最近一次 Ready generation
//! ```
//! RepoMap、Exact、BM25、Dense 必须放在同一 generation，避免读者把不同源快照的组件拼成一个索引。
//! 状态对象本身是内存合同；真正构建组件、写文件、处理并发/进程恢复由上层 adapter 负责。本状态机不授权执行，也不是 EventLog 事实。
//!
//! Serde 将 DTO 和 JSON 等外部格式互转；`deny_unknown_fields` 拒绝当前合同未声明的字段。

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

/// Ready index manifest 的稳定 wire schema 名称。
pub const INDEX_MANIFEST_SCHEMA: &str = "kiana.index-manifest.v1";
/// generation 状态快照的稳定 wire schema 名称。
pub const INDEX_GENERATION_STATE_SCHEMA: &str = "kiana.index-generation-state.v1";
/// 两种 DTO 当前使用的 schema 版本。
pub const INDEX_GENERATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一个 generation 必须覆盖的组件种类数量。
pub const MAX_INDEX_COMPONENTS: usize = 4;

/// 检查摘要字符串是否具有固定 SHA-256 文本外形；不会读取组件或源 manifest。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验必须存在的原因等文本字段的非空、字节上限和 NUL 字符。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 必须作为同一索引代数发布的四种组件；名称是 manifest map 的 key。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexComponentKind {
    /// 仓库目录/结构图组件。
    RepoMap,
    /// 精确词项索引组件。
    Exact,
    /// BM25 词频相关性索引组件。
    Bm25,
    /// dense 向量索引组件。
    Dense,
}

impl IndexComponentKind {
    /// 固定完整组件集；缺少任一项时 manifest 不可发布。
    pub const ALL: [Self; MAX_INDEX_COMPONENTS] =
        [Self::RepoMap, Self::Exact, Self::Bm25, Self::Dense];

    /// 返回组件 map 使用的稳定小写 key。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepoMap => "repo_map",
            Self::Exact => "exact",
            Self::Bm25 => "bm25",
            Self::Dense => "dense",
        }
    }
}

/// 单个索引 generation 的生命周期阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexGenerationStatus {
    /// 组件摘要已登记，尚未允许 reader 读取。
    Building,
    /// 全部组件通过合同校验，可以成为当前 reader generation。
    Ready,
    /// 构建失败；失败原因需要保留供运维观察。
    Failed,
    /// 旧 generation 已退役；当前类型校验允许该状态，但没有专用状态转换方法。
    Retired,
}

/// 某个 generation 所有索引组件摘要的单一 manifest。
///
/// manifest 把组件集合绑到一个 `source_manifest_digest`，让读取方不用分别发现各组件再猜它们是否属于同一批构建。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexManifest {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// DTO schema 版本。
    pub version: SchemaVersion,
    /// 单调递增的索引代数；必须非零。
    pub generation: u64,
    /// 产生这些组件的 source manifest 摘要引用。
    pub source_manifest_digest: String,
    /// 组件名称到组件内容摘要的映射；必须恰好覆盖 `IndexComponentKind::ALL`。
    pub components: BTreeMap<String, String>,
    /// Building/Ready/Failed/Retired 生命周期状态。
    pub status: IndexGenerationStatus,
    /// Failed 状态必须有原因；Building/Ready 禁止附失败原因。
    #[serde(default)]
    pub failure_reason: Option<String>,
    /// 对上述业务字段计算的无密钥 SHA-256 摘要。
    pub manifest_digest: String,
}

impl IndexManifest {
    /// 创建 Building manifest；此时组件摘要已经齐全，但读者仍不能通过 generation state 取得它。
    ///
    /// 【调用者】`IndexGenerationState::begin()` 或负责汇总组件摘要的 Query adapter。
    /// 【输入/输出】接收非零 generation、source digest 和四个组件摘要，返回 Building DTO。
    /// 【失败情况】组件不完整/多余、digest 格式错误或 header 不合法时返回稳定错误。
    /// 【副作用/设计原因】只构造内存合同；将 Building 与 Ready 分开，防止未发布或失败组件被 reader 看见。
    pub fn building(
        generation: u64,
        source_manifest_digest: impl Into<String>,
        components: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut manifest = Self {
            schema: INDEX_MANIFEST_SCHEMA.to_owned(),
            version: INDEX_GENERATION_VERSION,
            generation,
            source_manifest_digest: source_manifest_digest.into(),
            components,
            status: IndexGenerationStatus::Building,
            failure_reason: None,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    /// 将当前 Building manifest 转成 Ready，并重新计算摘要/校验。
    ///
    /// 该值转换只表示领域合同满足；文件发布需由 Query adapter 之后另行完成。
    pub fn ready(mut self) -> Result<Self, String> {
        if self.status != IndexGenerationStatus::Building {
            return Err("index_manifest_not_building".to_owned());
        }
        self.status = IndexGenerationStatus::Ready;
        self.failure_reason = None;
        self.manifest_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    /// 将当前 Building manifest 转成带失败原因的 Failed DTO。
    ///
    /// 只有尚未终结的 Building 可失败；generation state 的 `fail()` 会调用此方法校验失败形状，
    /// 但当前会丢弃返回的 Failed manifest，只把原因记在 state.last_failure。
    pub fn failed(mut self, reason: impl Into<String>) -> Result<Self, String> {
        if self.status != IndexGenerationStatus::Building {
            return Err("index_manifest_not_building".to_owned());
        }
        self.status = IndexGenerationStatus::Failed;
        self.failure_reason = Some(reason.into());
        self.manifest_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    /// 检查 schema/version、四个组件 key/digest、失败原因与 manifest 自摘要。
    ///
    /// 成功不代表组件文件真实存在或对应摘要由可信 builder 生成；这些数据没有在此读取。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_MANIFEST_SCHEMA
            || self.version != INDEX_GENERATION_VERSION
            || self.generation == 0
            || self.components.len() != MAX_INDEX_COMPONENTS
        {
            return Err("index_manifest_header_invalid".to_owned());
        }
        digest(&self.source_manifest_digest, "index_source_manifest_digest")?;
        for component in IndexComponentKind::ALL {
            let Some(component_digest) = self.components.get(component.as_str()) else {
                return Err("index_manifest_component_missing".to_owned());
            };
            digest(component_digest, "index_component_digest")?;
        }
        if self.components.keys().any(|key| {
            !IndexComponentKind::ALL
                .iter()
                .any(|kind| kind.as_str() == key)
        }) {
            return Err("index_manifest_component_unknown".to_owned());
        }
        if let Some(reason) = &self.failure_reason {
            required(reason, "index_manifest_failure_reason", 256)?;
        }
        match self.status {
            IndexGenerationStatus::Building | IndexGenerationStatus::Ready => {
                if self.failure_reason.is_some() {
                    return Err("index_manifest_failure_on_active_status".to_owned());
                }
            }
            IndexGenerationStatus::Failed => {
                if self.failure_reason.is_none() {
                    return Err("index_manifest_failure_reason_required".to_owned());
                }
            }
            IndexGenerationStatus::Retired => {}
        }
        digest(&self.manifest_digest, "index_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("index_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 manifest 除 `manifest_digest` 外的所有字段计算无密钥 SHA-256 摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "generation": self.generation,
            "source_manifest_digest": self.source_manifest_digest,
            "components": self.components,
            "status": self.status,
            "failure_reason": self.failure_reason,
        }))
    }
}

/// 在内存中协调一个 Building generation 与一个最近 Ready generation。
///
/// builder 在新版本准备期间继续通过 `reader()` 获得旧 Ready；commit 成功后才切换。
/// 这保证单个 state snapshot 的读取语义，但不替代持久化 adapter 的文件锁/CAS/跨进程协调。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexGenerationState {
    /// 固定状态 DTO schema 名称。
    pub schema: String,
    /// DTO schema 版本。
    pub version: SchemaVersion,
    /// 下一个可用 generation；正在构建时也就是 Building manifest 的 generation。
    pub next_generation: u64,
    /// 最近一次成功发布的 manifest；新版本构建失败时必须保留它供 reader 使用。
    #[serde(default)]
    pub ready: Option<IndexManifest>,
    /// 当前唯一允许的 in-progress manifest，避免同一 state 并行开多个候选代数。
    #[serde(default)]
    pub building: Option<IndexManifest>,
    /// 最近一次失败的原因；不会替代旧 Ready manifest。
    #[serde(default)]
    pub last_failure: Option<String>,
    /// 对其余 generation state 字段计算的无密钥摘要。
    pub state_digest: String,
}

impl Default for IndexGenerationState {
    /// 默认从 generation 1 开始，且尚无 ready/building manifest。
    fn default() -> Self {
        Self::new()
    }
}

impl IndexGenerationState {
    /// 创建空状态；首个 Building manifest 将使用 generation 1。
    pub fn new() -> Self {
        let mut state = Self {
            schema: INDEX_GENERATION_STATE_SCHEMA.to_owned(),
            version: INDEX_GENERATION_VERSION,
            next_generation: 1,
            ready: None,
            building: None,
            last_failure: None,
            state_digest: String::new(),
        };
        state.state_digest = state.digest();
        state
    }

    /// 开始唯一一个 candidate generation，并把它登记为不可读取的 Building manifest。
    ///
    /// 【调用者】上层 generation builder；当前 `kiana-query::build_context_index_generation()` 会调用它。
    /// 【输入/输出】接收 source manifest 和四个组件摘要，返回候选 manifest；状态内同时保存一份相同值。
    /// 【失败情况】现有 Building 未结束、当前 state 无效或组件合同不完整时返回错误。
    /// 【副作用】只修改调用者持有的内存 state，不构建文件、不更新 ready reader。
    pub fn begin(
        &mut self,
        source_manifest_digest: impl Into<String>,
        components: BTreeMap<String, String>,
    ) -> Result<IndexManifest, String> {
        self.validate()?;
        if self.building.is_some() {
            return Err("index_generation_build_already_active".to_owned());
        }
        let manifest =
            IndexManifest::building(self.next_generation, source_manifest_digest, components)?;
        self.building = Some(manifest.clone());
        self.state_digest = self.digest();
        Ok(manifest)
    }

    /// 仅在传入 manifest 与当前 Building 值逐字段相同时提交为新的 Ready generation。
    ///
    /// 【为什么比较整个 manifest】它是简单的 in-memory fencing：防止调用者把另一个候选或被改过的摘要提交到此 state。
    /// 成功会保留旧值直到新 Ready 写入 state，再清除 Building 和 last_failure；返回的 Ready 还需要 Query 层原子发布。
    /// ⚠ 到 `next_generation == u64::MAX` 时 `saturating_add(1)` 会让新 Ready 的 generation 等于 next_generation，
    /// 与 `validate()` 要求 Ready generation 小于 next_generation 冲突；该极限转换当前没有在此处二次校验。
    pub fn commit(&mut self, manifest: IndexManifest) -> Result<IndexManifest, String> {
        self.validate()?;
        manifest.validate()?;
        if manifest.status != IndexGenerationStatus::Building
            || self.building.as_ref() != Some(&manifest)
        {
            return Err("index_generation_commit_fence_mismatch".to_owned());
        }
        let ready = manifest.ready()?;
        self.ready = Some(ready.clone());
        self.building = None;
        self.last_failure = None;
        self.next_generation = self.next_generation.saturating_add(1);
        self.state_digest = self.digest();
        Ok(ready)
    }

    /// 结束当前候选构建并记录失败原因，同时保留此前 Ready generation。
    ///
    /// 如果没有 active Building，当前实现仍会记录 `last_failure`；它不会改变已有 reader manifest。
    pub fn fail(&mut self, reason: impl Into<String>) -> Result<(), String> {
        self.validate()?;
        let reason = reason.into();
        required(&reason, "index_generation_failure_reason", 256)?;
        if let Some(building) = self.building.take() {
            let _ = building.failed(reason.clone())?;
        }
        self.last_failure = Some(reason);
        self.state_digest = self.digest();
        Ok(())
    }

    /// 读取最近一次 Ready manifest；构建中的候选不会从这个接口暴露。
    ///
    /// 状态无效时先返回校验错误；尚无成功版本时返回 `index_generation_no_ready_manifest`。
    pub fn reader(&self) -> Result<&IndexManifest, String> {
        self.validate()?;
        self.ready
            .as_ref()
            .ok_or_else(|| "index_generation_no_ready_manifest".to_owned())
    }

    /// 校验状态 header、ready/building 与 next_generation 关系、失败原因和自摘要。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_GENERATION_STATE_SCHEMA
            || self.version != INDEX_GENERATION_VERSION
            || self.next_generation == 0
        {
            return Err("index_generation_state_header_invalid".to_owned());
        }
        if let Some(ready) = &self.ready {
            ready.validate()?;
            if ready.status != IndexGenerationStatus::Ready
                || ready.generation >= self.next_generation
            {
                return Err("index_generation_ready_invalid".to_owned());
            }
        }
        if let Some(building) = &self.building {
            building.validate()?;
            if building.status != IndexGenerationStatus::Building
                || building.generation != self.next_generation
            {
                return Err("index_generation_building_invalid".to_owned());
            }
        }
        if let Some(failure) = &self.last_failure {
            required(failure, "index_generation_last_failure", 256)?;
        }
        digest(&self.state_digest, "index_generation_state_digest")?;
        if self.state_digest != self.digest() {
            return Err("index_generation_state_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对状态中除 `state_digest` 外的字段计算无密钥 SHA-256 摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "next_generation": self.next_generation,
            "ready": self.ready,
            "building": self.building,
            "last_failure": self.last_failure,
        }))
    }
}
