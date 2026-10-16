//! 本文件把索引缓存差异或数据治理计划转换成可序列化的缓存决策报告。
//!
//! 在系统中的位置：`kiana-query/src/index.rs` 的持久索引构建器先生成当前索引，再比较旧报告
//! 与新报告，并调用 `ContextCacheDecision::from_report()` 把差异附到 `ContextIndexCacheReport`。
//! 另一个入口 `from_data_propagation()` 接收 Domain 的治理传播计划，推导缓存是否仍可作为候选。
//! 仓内尚未找到该治理入口的调用，也没有找到读取 `business_result_from_cache` 来决定实际业务
//! 返回值的生产调用；此处只创建决策 DTO，不读取缓存、不清除数据、不授权操作。
//!
//! ```text
//! 项目文件 -> index.rs 完整构建当前索引 -> 读取旧索引摘要 -> 比较 path/content_hash
//!          -> from_report() -> ContextIndexCacheReport.decision -> 随索引报告返回/持久化
//! DataPropagationPlan + 当前 epoch + payload 状态 -> from_data_propagation() -> 决策 DTO
//! ```
//!
//! 特别注意：当前索引路径是在读旧缓存前就已完整重建，因此“复用数大于零”只是差异统计，
//! 不证明当前扫描真的从旧缓存读取了业务结果。决策及其摘要都是描述性数据，不是权限或事实来源。

use kiana_domain::{json_digest, DataPayloadState, DataPropagationPlan, DataPropagationTarget};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONTEXT_CACHE_DECISION_SCHEMA: &str = "kiana.context-cache-decision.v1";

/// 缓存相对于当前输入的描述性状态。
///
/// Serde 将变体名称转换为 snake_case 文本存入 JSON；`Disabled` 当前没有被本文件两个构造器产生，
/// 但保留为合同状态供其他版本或调用方表达显式关闭。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCacheStatus {
    /// 旧缓存被报告为可用；是否真的用旧内容回答业务请求，仍需结合调用路径判断。
    Hit,
    /// 没有可复用的旧缓存，当前报告由新构建产生。
    Miss,
    /// 输入摘要变化，或治理条件表明旧缓存不再适合复用。
    Stale,
    /// 旧缓存损坏后被重建，或输入状态无法识别。
    Degraded,
    /// 显式禁用缓存的预留状态；本文件当前构造器不会产生它。
    Disabled,
}

/// 一次缓存状态判定及其自摘要。
///
/// 【系统角色】`index.rs` 把它放进持久索引报告；治理入口也能单独产生同一 DTO。
/// 这个结构记录“策略代码得出的描述”，不执行读取、失效、重建或业务授权。
/// 【数据格式】Serde 将字段编解码为 JSON；`deny_unknown_fields` 使本版本拒绝未知扩展字段。
/// 【信任边界】`decision_digest` 是无密钥自摘要，只能发现未同步更新摘要的改动，不能证明
/// 这份判定由可信组件生成。读取方仍须确认来源并执行实际的缓存读取/失效策略。
///
/// `Serialize` / `Deserialize` 来自 serde：分别支持结构体写入 JSON 和从 JSON 读回。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCacheDecision {
    /// 决策 DTO 的固定版本标识。
    pub schema: String,
    /// 由差异报告或治理输入映射出的状态。
    pub status: ContextCacheStatus,
    /// 面向日志/调用方解释状态的原因文本；当前只检查非空，不限制长度或原因集合。
    pub reason: String,
    /// 判定中是否声称读取了旧缓存；`from_report()` 仅根据差异计数设置，并不验证调用方是否实际读取旧结果。
    pub cache_read_used: bool,
    /// 判定中是否声称业务结果来自缓存；当前 `from_report()` 会在复用计数满足条件时置为 true，
    /// 但 `index.rs` 的现有构建顺序先完整重建当前索引，因此该标志并不等于已实际复用旧内容。
    pub business_result_from_cache: bool,
    /// 对 schema、状态、原因及两个布尔标志的无密钥自摘要；不包含自身。
    pub decision_digest: String,
}

impl ContextCacheDecision {
    /// 把索引缓存差异计数映射为稳定状态、原因和布尔标志。
    ///
    /// 【调用者】`kiana-query/src/index.rs` 的 `cache_report()` 在比较旧/新文件摘要后调用；
    /// PD-21 fixture 也直接传入合成计数。
    /// 【输入】`status` 是索引缓存报告文本，通常为 `created`、`recovered` 或 `updated`；
    /// 其余参数分别是旧新报告间相同、增加、内容变化和删除的文件数量。
    /// 【输出】总是返回一个已生成 `decision_digest` 的 DTO，不做 I/O，也不返回错误。
    /// 【核心映射】新建对应 Miss；旧缓存损坏并重建对应 Degraded；内容变化对应 Stale；只有
    /// `updated` 且没有增删改、并且 `reused > 0` 时对应 Hit；其他状态文本归为 Degraded。
    /// 【重要边界】当空项目没有文件时，完全无变化也会因 `reused == 0` 被分类为 Stale。
    /// 此外，`index.rs` 当前先从磁盘完整构建新索引，再读取旧报告计算这些计数；因此 Hit 和
    /// `cache_read_used=true` 目前表达的是报告推断，不是执行层已使用旧索引内容的证据。
    pub fn from_report(
        status: &str,
        reused: usize,
        added: usize,
        changed: usize,
        removed: usize,
    ) -> Self {
        let (status, reason, cache_read_used, business_result_from_cache) = match status {
            "created" => (ContextCacheStatus::Miss, "cache_missing", false, false),
            "recovered" => (
                ContextCacheStatus::Degraded,
                "cache_invalid_rebuilt",
                false,
                false,
            ),
            "updated" if added == 0 && changed == 0 && removed == 0 && reused > 0 => {
                (ContextCacheStatus::Hit, "cache_reused", true, true)
            }
            "updated" => (
                ContextCacheStatus::Stale,
                "cache_source_changed",
                false,
                false,
            ),
            _ => (
                ContextCacheStatus::Degraded,
                "cache_status_unknown",
                false,
                false,
            ),
        };
        let mut decision = Self {
            schema: CONTEXT_CACHE_DECISION_SCHEMA.to_owned(),
            status,
            reason: reason.to_owned(),
            cache_read_used,
            business_result_from_cache,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision
    }

    /// 验证决策合同、两个布尔字段之间的边界和自摘要。
    ///
    /// 【调用者】两个构造器在输出前调用；目前没有发现生产读入方调用此方法。
    /// 【输出/失败】通过返回 `Ok(())`；schema/reason 错误、布尔边界错误、摘要形状错误或摘要
    /// 不匹配时返回稳定错误字符串。错误只拒绝这份 DTO，不会执行重试、删除缓存或改动治理状态。
    /// 【易误改条件】这里要求两个布尔值一致，并禁止非 Hit 状态声称业务结果来自缓存；但没有
    /// 要求 Hit 必须把二者设为 true，所以一个自摘要正确的 Hit + false/false 对象仍能通过。
    /// 它也不限制 reason 长度或状态与 reason 的一一映射关系。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_CACHE_DECISION_SCHEMA || self.reason.trim().is_empty() {
            return Err("context_cache_decision_invalid".to_owned());
        }
        if self.business_result_from_cache != self.cache_read_used
            || (self.status != ContextCacheStatus::Hit && self.business_result_from_cache)
        {
            return Err("context_cache_business_result_boundary_invalid".to_owned());
        }
        if self
            .decision_digest
            .strip_prefix("sha256:")
            .is_none_or(|hex| hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("context_cache_decision_digest_invalid".to_owned());
        }
        if self.decision_digest != self.digest() {
            return Err("context_cache_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算判定字段的确定性 JSON 自摘要。
    ///
    /// `decision_digest` 不放入输入，避免递归摘要；这不是带密钥签名，知道字段内容的人也能
    /// 重新计算摘要。任何修改字段的代码都必须同步重算摘要，否则 `validate()` 会拒绝。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "status": self.status,
            "reason": self.reason,
            "cache_read_used": self.cache_read_used,
            "business_result_from_cache": self.business_result_from_cache,
        }))
    }

    /// 根据数据传播计划、当前数据 epoch 和 payload 状态决定治理后的缓存是否仍可用。
    ///
    /// 【调用者】当前仓内没有找到直接调用点；公开方法可由后续 Query 治理编排器调用。
    /// 【输入】`plan` 是 Domain 提供并带自校验摘要的数据传播计划；`current_data_epoch` 是调用方
    /// 观察到的当前版本；`payload_state` 表示缓存业务载荷当前为 Available、Expired、Revoked
    /// 或 Unknown。
    /// 【输出】找到 Cache 目标后返回已自摘要并通过 `validate()` 的 Hit 或 Stale 判定。
    /// 【核心流程】先验证整个计划，再找 Cache receipt；只有当前 epoch 不早于计划 epoch、缓存目标
    /// 状态为 `PreservedMetadata` 且 payload 为 `Available`，三个条件同时成立才判 Hit。
    /// 【失败情况】计划合同错误会原样向上传播；缺少 Cache 目标返回稳定错误。有效但不满足任一
    /// 可用条件时返回 Stale，而不是错误，因为这是一个明确的拒绝/失效判定。
    /// 【副作用与边界】本函数只读取传入 DTO，不读取缓存字节、不清理或重建缓存，也不证明该计划
    /// 来自可信 EventLog；cache miss/rebuild 仍由 Query 调用方负责，判定不能授权复用过期载荷。
    pub fn from_data_propagation(
        plan: &DataPropagationPlan,
        current_data_epoch: u64,
        payload_state: DataPayloadState,
    ) -> Result<Self, String> {
        plan.validate()?;
        let target = plan
            .targets
            .iter()
            .find(|receipt| receipt.target == DataPropagationTarget::Cache)
            .ok_or_else(|| "cache_propagation_target_missing".to_owned())?;
        let valid = current_data_epoch >= plan.data_epoch
            && target.state == kiana_domain::DataPropagationState::PreservedMetadata
            && payload_state == DataPayloadState::Available;
        let mut decision = Self {
            schema: CONTEXT_CACHE_DECISION_SCHEMA.to_owned(),
            status: if valid {
                ContextCacheStatus::Hit
            } else {
                ContextCacheStatus::Stale
            },
            reason: if valid {
                "cache_governance_current".to_owned()
            } else {
                "cache_governance_invalidated".to_owned()
            },
            cache_read_used: valid,
            business_result_from_cache: valid,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }
}
