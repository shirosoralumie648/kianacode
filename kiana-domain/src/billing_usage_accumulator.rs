//! 单个 model attempt 内的 snapshot/delta/final 用量累计 reducer。
//!
//! Provider 适配器或上层会把每个响应片段转换成 [`NormalizedUsage`]；本 reducer 按 attempt 和 sequence
//! 去重，再将累计快照、增量和最终快照折叠成当前 [`UsageVector`]：
//!
//! ```text
//! NormalizedUsage observation
//!     -> validate + attempt/sequence 检查
//!     -> Snapshot / Delta / Final 计算下一份累计 vector
//!     -> 成功后更新 latest、last_sequence、seen；Final 后封闭
//! ```
//!
//! 本文件是同步、内存中的纯 Domain reducer：不等待网络、不读取时钟/文件、不预留额度、不写 EventLog，也不是
//! durable accumulator。源码搜索目前仅找到 Domain fixture 直接使用 `UsageAccumulator`，尚未找到产品生产调用。
//! 多条记录必须属于同一个 attempt；这里不检查 run/provider/model 是否在所有观测间保持一致，这些身份约束由
//! 上游 attempt/lifecycle 合同负责。

use crate::{AttemptId, NormalizedUsage, UsageObservation, UsageVector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const USAGE_ACCUMULATOR_SCHEMA: &str = "kiana.usage-accumulator.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// `apply()` 对一条观测的处理结果。
pub enum UsageApplyOutcome {
    /// 新 sequence 成功折叠并更新累计状态。
    Applied,
    /// sequence 已出现且摘要完全相同；没有重复累计。
    Duplicate,
}

/// 一个 attempt 的临时累计状态。
///
/// 状态只存在当前对象内存中；没有持久化序列化接口。`seen` 保存已消费的 sequence→usage digest，以便
/// 识别重投递和同序号冲突，但本类型目前没有限制该 map 的大小。
pub struct UsageAccumulator {
    /// 当前 reducer 绑定的 attempt；不能混入其他 attempt 的记录。
    attempt_id: AttemptId,
    /// 最近成功应用后得到的累计 vector；第一条 Delta 前通常必须先有 Snapshot/Final 基线。
    latest: Option<UsageVector>,
    /// 最近一次成功应用的 observation sequence。
    last_sequence: Option<u64>,
    /// 所有已接收序号及其内容摘要；重复摘要返回 Duplicate，不同摘要返回冲突。
    seen: BTreeMap<u64, String>,
    /// 是否已经成功应用 Final；为 true 后所有新调用都会被拒绝。
    finalized: bool,
}

impl UsageAccumulator {
    /// 为一个非空 attempt ID 创建空累计器。
    ///
    /// 【调用者】需要在同一模型 attempt 中连续合并 usage observation 的 adapter/Domain 调用方。
    /// 【输入/输出】接收 attempt ID，成功返回 latest/sequence/seen 均为空且未 finalized 的内存状态。
    /// 【失败】nil UUID 返回 `usage_accumulator_attempt_invalid`。
    pub fn new(attempt_id: AttemptId) -> Result<Self, String> {
        if attempt_id.as_uuid().is_nil() {
            return Err("usage_accumulator_attempt_invalid".to_owned());
        }
        Ok(Self {
            attempt_id,
            latest: None,
            last_sequence: None,
            seen: BTreeMap::new(),
            finalized: false,
        })
    }

    /// 返回该累计器的 schema 标签，用于外层确认对象合同类型。
    pub fn schema(&self) -> &'static str {
        USAGE_ACCUMULATOR_SCHEMA
    }

    /// 返回最近一次成功接收的正 sequence；尚无记录时为 None。
    pub fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    /// 返回当前累计计量值的只读引用；尚未形成 Snapshot/Delta/Final 结果时为 None。
    pub fn latest(&self) -> Option<&UsageVector> {
        self.latest.as_ref()
    }

    /// 指示 Final 是否已成功应用；false 不代表当前值已经完整，只表示尚未终结。
    pub fn finalized(&self) -> bool {
        self.finalized
    }

    /// 将一条归一化观测折叠进本 attempt 的累计状态。
    ///
    /// 【调用者】上游在收到 provider/local usage 片段后；目前仓库内只见 Domain fixture 直接调用。
    /// 【输入】已构造并带摘要的 `NormalizedUsage`，其 attempt 必须与本 accumulator 相同。
    /// 【输出】`Applied` 表示新 sequence 已生效；`Duplicate` 表示同 sequence、同摘要已处理过。
    /// 【失败】记录无效、attempt 不同、Final 后调用、sequence 缺失/倒退/冲突、Delta 无基线、snapshot
    /// 不包含已知累计量或加法溢出时返回稳定错误。
    /// 【核心流程】先验证和去重，再根据 observation 类型计算一个完整的新 vector；只有计算成功才更新
    /// latest、last_sequence 和 seen，最后若是 Final 才封闭 accumulator。
    /// 【为什么用 checked arithmetic】用量会影响后续计费/配额，溢出成小数值会造成静默少计，因此溢出必须拒绝。
    /// 【容易误改】Final guard 目前先于 `seen` 去重检查；Final 应用后即使重投递完全相同的 Final 也返回
    /// `usage_accumulator_after_final`，而不会返回 Duplicate。若将来要求终态消息可幂等重投递，必须明确调整顺序。
    /// 同时，sequence 只要求递增，不要求连续；上游负责判断缺号是否可接受。
    pub fn apply(&mut self, usage: &NormalizedUsage) -> Result<UsageApplyOutcome, String> {
        // 输入必须先满足 BQ-02 的单条合同；之后才可按 attempt/sequence 做 BQ-03 累计。
        usage.validate()?;
        if usage.attempt_id != self.attempt_id {
            return Err("usage_accumulator_attempt_mismatch".to_owned());
        }
        // Final 是 attempt 的终结标记。当前分支在去重前运行，所以终态后包括旧消息重投递在内都会拒绝。
        if self.finalized {
            return Err("usage_accumulator_after_final".to_owned());
        }
        let sequence = usage
            .sequence
            .filter(|sequence| *sequence > 0)
            .ok_or_else(|| "usage_accumulator_sequence_required".to_owned())?;
        // 同一 sequence 且内容摘要相同是重投递；同一 sequence 携带不同值则不能选择性覆盖旧事实。
        if let Some(previous_digest) = self.seen.get(&sequence) {
            if previous_digest == &usage.usage_digest {
                return Ok(UsageApplyOutcome::Duplicate);
            }
            return Err("usage_sequence_conflict".to_owned());
        }
        // 只拒绝回退，不要求 sequence 恰好等于 last+1；日志是否有缺口需由上游恢复/完整性检查负责。
        if self.last_sequence.is_some_and(|last| sequence < last) {
            return Err("usage_sequence_regression".to_owned());
        }

        // Snapshot/Final 是“截至当前的总值”，必须包含当前已知各维度；Delta 才是相对已有基线的增量。
        let next = match usage.observation {
            UsageObservation::Snapshot => {
                if let Some(current) = &self.latest {
                    ensure_contains(current, &usage.vector)?;
                }
                usage.vector.clone()
            }
            UsageObservation::Delta => add_delta(self.latest.as_ref(), &usage.vector)?,
            UsageObservation::Final => {
                if let Some(current) = &self.latest {
                    ensure_contains(current, &usage.vector)?;
                }
                usage.vector.clone()
            }
        };
        // 所有校验和计算都成功后才提交内存状态，避免某个维度溢出时只更新了 vector 的一部分。
        self.latest = Some(next);
        self.last_sequence = Some(sequence);
        self.seen.insert(sequence, usage.usage_digest.clone());
        if usage.observation == UsageObservation::Final {
            self.finalized = true;
        }
        Ok(UsageApplyOutcome::Applied)
    }
}

/// 检查新累计快照是否至少包含前一累计值中的所有已知维度。
///
/// 【调用者】Snapshot 和 Final 分支；Delta 不走该比较，而由 `add_delta()` 逐维求和。
/// 【规则】旧值 Some(x) 时，新值不能变小或变回 None；旧值 None 时，新值可以首次变为已知。
/// 【失败】返回包含出问题维度名的 `usage_snapshot_not_containing:<field>` 或
/// `usage_snapshot_dropped_known:<field>`，上层必须拒绝该观测而不能覆盖已知累计数。
fn ensure_contains(current: &UsageVector, next: &UsageVector) -> Result<(), String> {
    for (name, before, after) in [
        ("input_tokens", current.input_tokens, next.input_tokens),
        ("output_tokens", current.output_tokens, next.output_tokens),
        (
            "cache_read_tokens",
            current.cache_read_tokens,
            next.cache_read_tokens,
        ),
        (
            "cache_write_tokens",
            current.cache_write_tokens,
            next.cache_write_tokens,
        ),
        (
            "reasoning_output_tokens",
            current.reasoning_output_tokens,
            next.reasoning_output_tokens,
        ),
        (
            "audio_input_tokens",
            current.audio_input_tokens,
            next.audio_input_tokens,
        ),
        (
            "audio_output_tokens",
            current.audio_output_tokens,
            next.audio_output_tokens,
        ),
    ] {
        if let (Some(before), Some(after)) = (before, after) {
            if after < before {
                return Err(format!("usage_snapshot_not_containing:{name}"));
            }
        } else if before.is_some() && after.is_none() {
            return Err(format!("usage_snapshot_dropped_known:{name}"));
        }
    }
    for (name, before, after) in [
        ("tool_calls", current.tool_calls, next.tool_calls),
        ("effect_count", current.effect_count, next.effect_count),
        ("wall_time_ms", current.wall_time_ms, next.wall_time_ms),
        ("output_bytes", current.output_bytes, next.output_bytes),
        (
            "artifact_bytes",
            current.artifact_bytes,
            next.artifact_bytes,
        ),
        ("storage_bytes", current.storage_bytes, next.storage_bytes),
    ] {
        if after < before {
            return Err(format!("usage_snapshot_not_containing:{name}"));
        }
    }
    Ok(())
}

/// 将一条 Delta 的每个用量维度安全加到已有累计向量上，生成新值而不修改 accumulator。
///
/// 【输入/输出】需要已有 Snapshot/Final 基线和 delta vector；成功返回新的累计 vector。
/// 【失败】没有基线返回 `usage_delta_without_snapshot`；任意计数溢出或“旧值未知但增量声称有数”时拒绝。
/// 【副作用】纯计算；只有调用方随后把完整返回值交给 accumulator，才会更新累计状态。
fn add_delta(current: Option<&UsageVector>, delta: &UsageVector) -> Result<UsageVector, String> {
    let Some(current) = current else {
        return Err("usage_delta_without_snapshot".to_owned());
    };
    Ok(UsageVector {
        schema: delta.schema.clone(),
        input_tokens: add_optional(current.input_tokens, delta.input_tokens, "input_tokens")?,
        output_tokens: add_optional(current.output_tokens, delta.output_tokens, "output_tokens")?,
        cache_read_tokens: add_optional(
            current.cache_read_tokens,
            delta.cache_read_tokens,
            "cache_read_tokens",
        )?,
        cache_write_tokens: add_optional(
            current.cache_write_tokens,
            delta.cache_write_tokens,
            "cache_write_tokens",
        )?,
        reasoning_output_tokens: add_optional(
            current.reasoning_output_tokens,
            delta.reasoning_output_tokens,
            "reasoning_output_tokens",
        )?,
        audio_input_tokens: add_optional(
            current.audio_input_tokens,
            delta.audio_input_tokens,
            "audio_input_tokens",
        )?,
        audio_output_tokens: add_optional(
            current.audio_output_tokens,
            delta.audio_output_tokens,
            "audio_output_tokens",
        )?,
        tool_calls: current
            .tool_calls
            .checked_add(delta.tool_calls)
            .ok_or_else(|| "usage_accumulator_overflow:tool_calls".to_owned())?,
        effect_count: current
            .effect_count
            .checked_add(delta.effect_count)
            .ok_or_else(|| "usage_accumulator_overflow:effect_count".to_owned())?,
        wall_time_ms: current
            .wall_time_ms
            .checked_add(delta.wall_time_ms)
            .ok_or_else(|| "usage_accumulator_overflow:wall_time_ms".to_owned())?,
        output_bytes: current
            .output_bytes
            .checked_add(delta.output_bytes)
            .ok_or_else(|| "usage_accumulator_overflow:output_bytes".to_owned())?,
        artifact_bytes: current
            .artifact_bytes
            .checked_add(delta.artifact_bytes)
            .ok_or_else(|| "usage_accumulator_overflow:artifact_bytes".to_owned())?,
        storage_bytes: current
            .storage_bytes
            .checked_add(delta.storage_bytes)
            .ok_or_else(|| "usage_accumulator_overflow:storage_bytes".to_owned())?,
    })
}

/// 按 None/Some 语义累计一个可选计量维度。
///
/// 已知基线加已知增量使用 `checked_add`；已知基线加未报告增量保留基线；两者都未知则仍未知；
/// 若基线未知但增量给出绝对数值，无法确认增量应加到什么值上，因此返回
/// `usage_delta_unknown_base:<field>`。
fn add_optional(
    current: Option<u64>,
    delta: Option<u64>,
    name: &str,
) -> Result<Option<u64>, String> {
    match (current, delta) {
        (Some(current), Some(delta)) => current
            .checked_add(delta)
            .map(Some)
            .ok_or_else(|| format!("usage_accumulator_overflow:{name}")),
        (Some(current), None) => Ok(Some(current)),
        (None, Some(_)) => Err(format!("usage_delta_unknown_base:{name}")),
        (None, None) => Ok(None),
    }
}
