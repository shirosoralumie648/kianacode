//! 本文件定义模型路由版本归因与 drift（运行指标偏移）报告使用的纯数据合同。
//!
//! Core 从 EventStore 已提交的 `run.model_turn` 事件读取审计元数据，由
//! [`RouteDecision::from_model_turn()`] 将 provider、model、提示词、路由、预算和运行时版本
//! 归一成 Domain 值，再交给 [`DriftReport`] 按完整版本键分桶。之后 Core 把报告作为只读命令响应返回：
//!
//! ```text
//! Runner 发出 ModelTurn
//!     -> Core 把 run.model_turn 写入 EventStore
//!     -> version.drift 读取已提交事件
//!     -> 本文件构造 RouteDecision 并累计 DriftBucket
//!     -> Core 校验、序列化 DriftReport
//!     -> 调用入口返回查询结果
//! ```
//!
//! 本文件不访问 EventStore、不调用模型、不验证 provider 身份、不授予执行权限，也不决定切换模型。
//! 版本字段是为历史指标归因准备的标签；报告只能说明被读取事件上记载了什么，不能证明这些元数据
//! 真实、完整或由可信 provider 签发。若把它当作授权凭证或自动路由指令，会越过 Core 的 authority chain。

use crate::{json_digest, EventId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const ROUTE_DECISION_SCHEMA: &str = "kiana.route-decision.v1";
pub const DRIFT_REPORT_SCHEMA: &str = "kiana.drift-report.v1";
pub const DRIFT_MEASUREMENT_OBSERVED_TURNS: &str = "observed_turns";
pub const DRIFT_COST_UNKNOWN: &str = "unknown";

/// 检查合同字符串是否非空、长度有界且不含 NUL 字节。
///
/// 【调用者】`RouteDecision::validate()` 用它检查 provider/model 等所有版本标签。
/// 【输入】待检查字符串与允许的最大字节数。
/// 【输出】满足三项形状条件时为 `true`；这里不返回具体失败原因。
/// 【设计原因】这些字段会进入 JSON 摘要和查询响应，长度上限限制单条元数据对内存和摘要输入的影响。
/// `len()` 计算 UTF-8 字节数，不是中文字符数。
/// 【边界】通过检查只代表字符串形状可接受，不代表值已知、真实或被外部系统认证。
fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

/// 按兼容顺序从审计 JSON 中读取第一个非空字符串字段。
///
/// 【调用者】`RouteDecision::from_model_turn()` 用它兼容不同版本 Runner 写入的顶层字段和
/// `/prepared/...` 嵌套字段。
/// 【输入】一条事件的 JSON payload、候选路径列表、字段缺失时的替代文本。
/// 【输出】第一个非空字符串的副本；如果路径不存在、值不是字符串或字符串为空，则尝试下一项，
/// 全部失败后复制 `fallback`。
/// 【失败/副作用】不因字段缺失而报错，也不修改 JSON；调用者需在后续显式校验构造结果。
/// 【设计原因】兼容旧 producer 的字段位置，缺字段仍进入 `unknown` 桶，避免静默丢掉已提交的 model turn。
fn field(data: &Value, paths: &[&str], fallback: &str) -> String {
    paths
        .iter()
        .filter_map(|path| {
            let value = if path.starts_with('/') {
                data.pointer(path)
            } else {
                data.get(*path)
            }?;
            value.as_str()
        })
        .find(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback.to_owned())
}

/// 一次模型调用所记载的路由、提示词、预算和运行时版本身份。
///
/// 【系统位置】由 Core 查询 EventLog 时从 `run.model_turn` payload 派生，并作为 drift 分桶维度。
/// 【下游】`version_key()` 将这些维度合成摘要，`DriftBucket` 用该摘要聚合可比较的调用记录。
/// 【重要边界】这是审计归因对象，不是 provider 请求、签名配置、授权许可或模型切换命令。字段来自
/// 事件元数据；本结构本身不重新查询 ModelProfile/PromptBundle，也不验证 hash 与原始内容一致。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDecision {
    /// wire schema 版本；当前必须等于 `ROUTE_DECISION_SCHEMA`。
    pub schema: String,
    /// provider 的稳定标识；序列化时对外字段名为 `provider`。
    #[serde(rename = "provider")]
    pub provider_id: String,
    /// provider 内部模型标识；序列化时对外字段名为 `model`。
    #[serde(rename = "model")]
    pub model_id: String,
    /// 调用时使用的模型配置档案标签，用于区分相同模型在不同角色/配置下的表现。
    pub model_profile: String,
    /// 提示词版本或摘要标签；不包含原始 prompt 正文。
    pub prompt_hash: String,
    /// 本次 route 的摘要标签；若事件未提供，会根据有限的 route 元数据计算。
    pub route_digest: String,
    /// 生成该次调用时所记载的配置修订标签。
    pub configuration_revision: String,
    /// 预算合同的 schema 标签，不是实际 token 成本。
    pub budget_schema: String,
    /// 调用 Core 传入的运行时版本字符串；当前生产调用传入编译期 Cargo 包版本。
    pub runtime_version: String,
}

impl RouteDecision {
    /// 从一条已提交 model-turn 的审计元数据构造版本归因对象。
    ///
    /// 【调用者】Core `version.drift` 分支为每个归属当前 actor/project 的 `run.model_turn` 调用。
    /// 【输入】事件 JSON payload 与 Core 提供的运行时版本字符串。
    /// 【输出】包含 provider、model、profile、prompt、route、配置、预算和运行时标签的对象；此函数
    /// 不自动调用 `validate()`。
    /// 【核心流程】各维度按显式优先级从顶层或兼容的嵌套路径读取；缺少字符串时记为 `unknown`；
    /// route digest 缺失时，对 provider/model/profile/configuration revision/streaming 组成的 JSON
    /// 求摘要；streaming 缺失或类型错误时按 `false` 参与摘要。
    /// 【失败/副作用】本函数不返回错误，也不改动输入；超长值会留到调用方显式校验时被拒绝。
    /// 【为什么保留 unknown】跳过缺字段事件会让报告只统计“字段齐全”的调用，造成选择性遗漏；使用
    /// 明确的 unknown 版本桶可使数据质量问题仍然可见。
    /// 【边界】摘要只固定输入内容，不证明审计字段来自可信 route 配置，也不包含请求正文或凭据。
    pub fn from_model_turn(data: &Value, runtime_version: impl Into<String>) -> Self {
        let provider_id = field(
            data,
            &["provider_id", "/prepared/route/provider_id"],
            "unknown",
        );
        let model_id = field(data, &["model_id", "/prepared/route/model_id"], "unknown");
        let model_profile = field(
            data,
            &["model_profile", "/prepared/route/profile"],
            "unknown",
        );
        let prompt_hash = field(
            data,
            &["prompt_hash", "prompt_version", "/prepared/prompt_version"],
            "unknown",
        );
        let configuration_revision = field(
            data,
            &[
                "configuration_revision",
                "/prepared/route/configuration_revision",
            ],
            "unknown",
        );
        let route_digest = field(data, &["route_digest", "/prepared/route_digest"], "");
        let route_digest = if route_digest.is_empty() {
            json_digest(&json!({
                "provider_id": provider_id,
                "model_id": model_id,
                "profile": model_profile,
                "configuration_revision": configuration_revision,
                "streaming": data
                    .get("streaming")
                    .or_else(|| data.pointer("/prepared/route/streaming"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }))
        } else {
            route_digest
        };
        Self {
            schema: ROUTE_DECISION_SCHEMA.to_owned(),
            provider_id,
            model_id,
            model_profile,
            prompt_hash,
            route_digest,
            configuration_revision,
            budget_schema: field(data, &["/budget/schema", "budget_schema"], "unknown"),
            runtime_version: runtime_version.into(),
        }
    }

    /// 检查 schema 版本和各字符串字段的有限长度/非空约束。
    ///
    /// 【调用者】Domain bucket 构造与校验、Core 返回报告前的整体校验。
    /// 【输入/输出】读取当前对象；满足约束返回 `Ok(())`，否则返回稳定错误码
    /// `route_decision_invalid`。
    /// 【失败影响】Core drift 命令会将错误映射为 `PortError::Failed` 并中止响应；不写状态、不重试模型。
    /// 【设计原因】严格 Serde 只拒绝未知字段，不能限制已知 JSON 字符串的空值和长度。
    /// 【边界】不核验 provider/model 是否存在，不重算 prompt/configuration 摘要，也不认证元数据来源。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != ROUTE_DECISION_SCHEMA
            || !bounded(&self.provider_id, 256)
            || !bounded(&self.model_id, 512)
            || !bounded(&self.model_profile, 256)
            || !bounded(&self.prompt_hash, 512)
            || !bounded(&self.route_digest, 256)
            || !bounded(&self.configuration_revision, 256)
            || !bounded(&self.budget_schema, 256)
            || !bounded(&self.runtime_version, 128)
        {
            return Err("route_decision_invalid");
        }
        Ok(())
    }

    /// 将版本维度编码成稳定的 drift 桶 key。
    ///
    /// 【调用者】`DriftReport::record()` 选择桶，`DriftReport::validate()` 核对 map key 与桶内容。
    /// 【输入/输出】读取八个归因字段，按 `json_digest()` 的对象键规范化规则生成 SHA-256 摘要；
    /// `schema` 不参与 key。
    /// 【设计原因】provider/model/profile/prompt/route/config/budget/runtime 任一纳入维度变化时会分桶，
    /// 避免只按 model 名称合并不同配置的指标。
    /// 【边界】本方法不先调用 `validate()`；无密钥摘要不是签名或真实性证明。
    pub fn version_key(&self) -> String {
        json_digest(&json!({
            "provider": self.provider_id,
            "model": self.model_id,
            "model_profile": self.model_profile,
            "prompt_hash": self.prompt_hash,
            "route_digest": self.route_digest,
            "configuration_revision": self.configuration_revision,
            "budget_schema": self.budget_schema,
            "runtime_version": self.runtime_version,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftBucket {
    /// 此桶共同归属的版本维度；同一桶中的调用应得到相同的 version_key。
    pub version: RouteDecision,
    /// 桶内被记录的 model-turn 事件数量。
    pub turns: u64,
    /// `run.model_turn` payload 的 `error` 字段非 null 的事件数量，不是质量评估结论。
    pub errors: u64,
    /// 事件 `elapsed_ms` 的无符号毫秒累计值；字段缺失或类型不匹配时 Core 以 0 记录。
    pub elapsed_ms: u64,
    /// 已计入本桶的 EventId，用于拒绝同桶重复记账；不是完整 EventStore 游标。
    pub event_ids: Vec<EventId>,
}

impl DriftBucket {
    /// 创建计数为零、且版本对象已通过基础形状校验的新桶。
    ///
    /// 【调用者】`DriftReport::record()` 在新版本 key 第一次出现时使用。
    /// 【输入/输出】接收版本身份；成功返回空桶，失败传播 `route_decision_invalid`。
    /// 【副作用】不访问 EventStore，也不创建持久记录。
    pub fn new(version: RouteDecision) -> Result<Self, &'static str> {
        version.validate()?;
        Ok(Self {
            version,
            turns: 0,
            errors: 0,
            elapsed_ms: 0,
            event_ids: Vec::new(),
        })
    }

    /// 把一个 model-turn 观测计入本桶。
    ///
    /// 【调用者】`DriftReport::record()` 在 Core 扫描历史事件时调用。
    /// 【输入】EventId、事件耗时（毫秒）以及事件是否带有非空错误字段。
    /// 【输出】成功时 turns 加一、elapsed_ms 加入耗时、错误事件的 errors 加一，并在最后保存 ID。
    /// 【失败】同桶重复 ID 返回 `drift_event_duplicate`；各计数器溢出时返回对应稳定错误码。Core 收到
    /// 错误会终止本次报告，不会写 EventStore 或重试模型请求。
    /// 【容易误改】更新按 turns → elapsed_ms → errors → event_ids 的顺序发生，不是原子事务。若较后阶段
    /// 溢出，前面已增加的计数不会回滚，而 ID 尚未追加；继续复用这个部分更新的桶可能导致 `validate()` 失败。
    /// 因此不能假定返回 `Err` 就表示对象保持原样。
    pub fn record(
        &mut self,
        event_id: EventId,
        elapsed_ms: u64,
        error: bool,
    ) -> Result<(), &'static str> {
        if self.event_ids.contains(&event_id) {
            return Err("drift_event_duplicate");
        }
        self.turns = self.turns.checked_add(1).ok_or("drift_turn_overflow")?;
        self.elapsed_ms = self
            .elapsed_ms
            .checked_add(elapsed_ms)
            .ok_or("drift_elapsed_overflow")?;
        if error {
            self.errors = self.errors.checked_add(1).ok_or("drift_error_overflow")?;
        }
        self.event_ids.push(event_id);
        Ok(())
    }

    /// 检查桶内版本、计数关系和本桶 EventId 唯一性。
    ///
    /// 【调用者】`DriftReport::validate()`。
    /// 【成功条件】版本字符串有效、key 可生成、turns 等于 ID 数量、errors 不大于 turns，且本桶 ID 无重复。
    /// 【失败】无效时返回稳定的版本/桶错误码，报告序列化前会因此被拒绝。
    /// 【边界】只检查当前桶；不会查询 EventStore，也不和其他桶比较 EventId，故不是全报告去重证明。
    pub fn validate(&self) -> Result<(), &'static str> {
        self.version.validate()?;
        if self.version.version_key().is_empty()
            || self.turns != self.event_ids.len() as u64
            || self.errors > self.turns
        {
            return Err("drift_bucket_invalid");
        }
        let mut ids = std::collections::BTreeSet::new();
        if self.event_ids.iter().any(|id| !ids.insert(*id)) {
            return Err("drift_event_duplicate");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftReport {
    /// 报告 wire schema；必须保持当前 drift report 版本。
    pub schema: String,
    /// 版本摘要到统计桶的排序映射；BTreeMap 使序列化遍历顺序稳定。
    pub buckets: BTreeMap<String, DriftBucket>,
    /// 本合同固定为 false：报告不具备自动切换模型的权限或行为。
    pub automatic_model_switch: bool,
    /// 指标口径；当前只统计实际观察到的 model turns。
    pub measurement: String,
    /// 成本口径；固定 unknown，避免把预算或耗时误报为货币成本。
    pub cost: String,
}

impl Default for DriftReport {
    /// 创建空的只读 drift 投影，并固定当前不支持的能力为保守值。
    ///
    /// 【调用者】Core `version.drift` 开始扫描之前。
    /// 【输出】空桶报告，measurement 为 observed turns、cost 为 unknown，且禁止自动模型切换。
    /// 【设计原因】即使当前没有可见 model turn，也能返回形状明确的空报告；报告存在不代表已经产生
    /// 质量告警、成本核算或路由处置。
    fn default() -> Self {
        Self {
            schema: DRIFT_REPORT_SCHEMA.to_owned(),
            buckets: BTreeMap::new(),
            automatic_model_switch: false,
            measurement: DRIFT_MEASUREMENT_OBSERVED_TURNS.to_owned(),
            cost: DRIFT_COST_UNKNOWN.to_owned(),
        }
    }
}

impl DriftReport {
    /// 按版本 key 找到或创建统计桶，再把一个事件计入桶中。
    ///
    /// 【调用者】Core drift 查询循环；每个通过 run owner 过滤的 `run.model_turn` 调用一次。
    /// 【输入】派生版本对象、EventId、耗时毫秒和错误标记。
    /// 【输出】更新内存中的报告；调用者之后仍须执行 `validate()`。
    /// 【失败】版本无效或桶计数/ID 检查失败时返回 Domain 稳定错误，Core 将它转为命令失败。
    /// 【设计原因】完整版本 key 将同配置事件聚在一起，EventId 阻止同一个事件在同一桶重复记账。
    /// 【容易误改】`or_insert(...)` 的参数会在调用前求值；即使桶已存在，传入版本仍会先经过
    /// `DriftBucket::new()` 校验。不要误以为新桶构造是惰性执行的。
    pub fn record(
        &mut self,
        version: RouteDecision,
        event_id: EventId,
        elapsed_ms: u64,
        error: bool,
    ) -> Result<(), &'static str> {
        let key = version.version_key();
        let bucket = self
            .buckets
            .entry(key)
            .or_insert(DriftBucket::new(version)?);
        bucket.record(event_id, elapsed_ms, error)
    }

    /// 验证报告固定口径、每个桶自身有效，且 map key 与桶版本摘要一致。
    ///
    /// 【调用者】Core 将 Domain 报告序列化并返回前。
    /// 【成功条件】schema/measurement/cost 是合同规定值、自动切换为 false、每个桶通过校验并位于
    /// 对应版本 key 下。
    /// 【失败】不一致时返回报告错误、桶错误或 key 不匹配错误，Core 不会返回成功报告。
    /// 【边界/待注意】EventId 唯一性目前只在单个桶内检查；相同 ID 若被放进两个版本桶，本 validator
    /// 不会发现。当前 Core 单次扫描对每个事件派生一个 RouteDecision，但这个全局唯一性依赖生产者，
    /// 没有由报告合同强制保证。报告也没有全局桶数或事件数上限。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_REPORT_SCHEMA
            || self.automatic_model_switch
            || self.measurement != DRIFT_MEASUREMENT_OBSERVED_TURNS
            || self.cost != DRIFT_COST_UNKNOWN
        {
            return Err("drift_report_invalid");
        }
        for (key, bucket) in &self.buckets {
            bucket.validate()?;
            if key != &bucket.version.version_key() {
                return Err("drift_bucket_key_mismatch");
            }
        }
        Ok(())
    }
}
