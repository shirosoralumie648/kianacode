//! 服务商无关的用量向量与归一化观测合同。
//!
//! Provider 回包字段各不相同；Provider adapter 把原始字段转换成 [`UsageVector`] 和 [`NormalizedUsage`]，
//! 后续的 attempt 生命周期、成本和结算合同再消费同一种结构。本文件只描述并验证“单条观测是什么”，不负责
//! 把多条流事件累计、不负责扣预算、算钱或写持久事件。sequence 顺序、snapshot/delta/final 累计由相邻的
//! `billing_usage_accumulator.rs` 定义。
//!
//! ```text
//! provider / 本地执行器 / 派生统计提供观测
//!     -> UsageVector（多个计量维度；None 与 Some(0) 分开）
//!     -> NormalizedUsage（attempt/run 来源、观测类型、sequence、置信度、摘要）
//!     -> attempt / pricing / settlement 等下游 Domain 合同
//! ```
//!
//! `Serialize`/`Deserialize` 由 serde 提供，用于 Rust 值与 JSON 等 wire/persistence 形式互转。字段的摘要
//! 是无密钥内容校验值，不会认证 Provider 身份或证明原始回包真实存在。

use crate::{
    json_digest, AttemptId, BillingUnknownReason, CellId, OrganizationId, ProjectId, RunId,
    SchemaVersion, UsageId,
};
use serde::{Deserialize, Serialize};

pub const USAGE_VECTOR_SCHEMA: &str = "kiana.usage-vector.v1";
pub const NORMALIZED_USAGE_SCHEMA: &str = "kiana.normalized-usage.v1";
pub const USAGE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 对一个可选用量字段进行不丢信息的三态解释。
pub enum UsagePresence {
    /// 对应字段没有出现在观测里，因此当前不知道其数值。
    Absent,
    /// 来源明确报告了 0；这与“没有报告”不同。
    ExplicitZero,
    /// 来源报告了非零数值。
    Present,
}

impl UsagePresence {
    /// 把 `Option<u64>` 映射为三种业务含义，不把 `None` 偷换成零。
    pub const fn of(value: Option<u64>) -> Self {
        match value {
            None => Self::Absent,
            Some(0) => Self::ExplicitZero,
            Some(_) => Self::Present,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 多种执行活动共同使用的计量向量。
///
/// 可选 token 维度必须保留“未报告”和“报告为零”的差别；其它计数使用非负整数。注意 derive 出来的
/// `Default` 会留下空 schema，不能直接作为合法向量使用；新建明确的全零向量应调用 `zero()`。
pub struct UsageVector {
    /// 向量合同 schema；validator 目前只检查这个版本标记。
    pub schema: String,
    /// 提交给模型的输入 token 数；`None` 未知/缺失，`Some(0)` 是明确零。
    pub input_tokens: Option<u64>,
    /// 模型生成的输出 token 数，语义同 input_tokens。
    pub output_tokens: Option<u64>,
    /// 从缓存读取的 token 数；不是 input_tokens 的别名。
    pub cache_read_tokens: Option<u64>,
    /// 写入缓存的 token 数。
    pub cache_write_tokens: Option<u64>,
    /// 推理过程专用输出 token 数；可能是 output_tokens 的组成部分，需由价格/适配规则决定是否重叠。
    pub reasoning_output_tokens: Option<u64>,
    /// 音频输入 token 数。
    pub audio_input_tokens: Option<u64>,
    /// 音频输出 token 数。
    pub audio_output_tokens: Option<u64>,
    /// 本次观测的工具调用数。
    pub tool_calls: u64,
    /// 本次观测的 effect 数量。
    pub effect_count: u64,
    /// 本次观测的墙钟耗时，单位毫秒。
    pub wall_time_ms: u64,
    /// 输出文本/数据的字节数。
    pub output_bytes: u64,
    /// 生成或传输 artifact 的字节数。
    pub artifact_bytes: u64,
    /// 存储占用/变更字节数。
    pub storage_bytes: u64,
}

impl UsageVector {
    /// 创建 schema 正确且所有维度都显式为零的合法向量。
    ///
    /// 【调用者】适配器、预留估算和 fixture 在需要“已知为零”的基线时使用。
    /// 【重要区别】可选 token 字段设为 `Some(0)`，而不是 `None`；调用方若实际不知道某一维度，必须再把
    /// 对应字段设为 `None`。
    pub fn zero() -> Self {
        Self {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(0),
            output_tokens: Some(0),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_output_tokens: Some(0),
            audio_input_tokens: Some(0),
            audio_output_tokens: Some(0),
            tool_calls: 0,
            effect_count: 0,
            wall_time_ms: 0,
            output_bytes: 0,
            artifact_bytes: 0,
            storage_bytes: 0,
        }
    }

    /// 检查 UsageVector schema 标记。
    /// 当前不检查维度之间是否重复计数，也不推断 confidence；这些语义由价格/归一化层处理。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != USAGE_VECTOR_SCHEMA {
            return Err("usage_vector_schema_invalid".to_owned());
        }
        Ok(())
    }

    /// 返回 input token 字段的 absent/explicit-zero/present 语义。
    pub const fn input_presence(&self) -> UsagePresence {
        UsagePresence::of(self.input_tokens)
    }

    /// 返回 output token 字段的 absent/explicit-zero/present 语义。
    pub const fn output_presence(&self) -> UsagePresence {
        UsagePresence::of(self.output_tokens)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 说明一条观测的数值从哪里来；它是声明标签，不单独证明该来源可信。
pub enum UsageSource {
    /// 上游模型服务商的回包。
    Provider,
    /// 本地执行器直接测量的数据。
    LocalExecutor,
    /// 从其他已知事实推导出的数据。
    Derived,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 说明本条观测在一条用量序列中的数值形态。
pub enum UsageObservation {
    /// 截至当前的累计值，可作为序列基线或后续累计快照。
    Snapshot,
    /// 相对已有基线新增的量，需要 accumulator 加到已知总值上。
    Delta,
    /// 最终累计值；成功应用后该 attempt 不再接受后续观测。
    Final,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 表示观测中的计量信息完整程度，而不是一次请求是否成功。
pub enum UsageConfidence {
    /// 调用方声明核心计量足够完整；validator 只要求没有 unknown_reason，不检查 vector 字段是否齐全。
    Known,
    /// 调用方声明部分维度可用；validator 不检查是否至少存在一个非空维度。
    Partial,
    /// 调用方声明没有可确认用量；validator 要求提供 reason，但不要求 vector 全部为空。
    Unknown,
}

/// 检查必须提供的来源文字是否非空、字节数有界且不含 NUL/换行。
fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
}

/// 验证带 `sha256:` 前缀的 64 位十六进制标签；不验证它是否真由指定原文/来源算出。
fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 将一个来源的原始计量信息转成跨 Provider 通用的单条用量观测。
///
/// 本结构把稳定身份、请求/实际服务模型、路由、用量数值、观测次序、置信度和来源依据放在同一摘要输入中，
/// 下游可据此判断该记录能否用于计费或 attempt 结算。`new()` 给常用字段提供基础构造，但调用方可填充
/// public 的可选关联字段后必须重新计算摘要并再次 `validate()`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedUsage {
    /// wire schema，区分归一化用量记录合同。
    pub schema: String,
    /// 用量记录合同版本。
    pub version: SchemaVersion,
    /// 单条观测的稳定 ID。
    pub usage_id: UsageId,
    /// 产生这条观测的模型调用 attempt；累计器按它拒绝混入其他 attempt 的数据。
    pub attempt_id: AttemptId,
    /// 可选的模型调用 invocation 关联 ID。
    pub invocation_id: Option<crate::InvocationId>,
    /// 所属 run ID，由服务端执行上下文提供，不应由 provider 回包指定。
    pub run_id: RunId,
    /// 可选执行 Cell 关联。
    pub cell_id: Option<CellId>,
    /// 可选项目关联。
    pub project_id: Option<ProjectId>,
    /// 可选组织关联。
    pub organization_id: Option<OrganizationId>,
    /// 可选 provider 标识；`None` 表示该观测没有声明服务商 ID。
    pub provider_id: Option<String>,
    /// 调用方原本请求的模型标识。
    pub requested_model_id: String,
    /// provider 回包声明实际服务的模型；可能缺失或与请求模型不同。
    pub served_model_id: Option<String>,
    /// 选择的连接/路由标识，便于追溯该次调用经过的路线。
    pub route_id: String,
    /// 本次 attempt 在同一调用重试组中的序号；本合同记录它，但不自行调度 retry。
    pub retry_ordinal: u32,
    /// 本次观察到的各计量维度。
    pub vector: UsageVector,
    /// 数值来源标签：provider、本地执行器或派生计算。
    pub source: UsageSource,
    /// 累计快照、增量或终态快照。
    pub observation: UsageObservation,
    /// 此 attempt 的观测序号；Delta 必须提供正数，其他观察形态由 accumulator 再作序号检查。
    pub sequence: Option<u64>,
    /// 当前用量能否完整用于下游计价的判断标签。
    pub confidence: UsageConfidence,
    /// Partial/Unknown 的原因；Known 记录不能携带此字段。
    pub unknown_reason: Option<BillingUnknownReason>,
    /// 描述抽取/测量依据的短标签，例如 adapter schema 与协议名。
    pub basis: String,
    /// 原始回包或输入依据的摘要标签；不把原文保存在本结构中。
    pub raw_digest: String,
    /// 对当前所有语义字段重算的摘要；自身不参加摘要输入。
    pub usage_digest: String,
}

impl NormalizedUsage {
    /// 以最常见的 provider/local 观测字段构造记录，生成摘要后立即执行 Domain 验证。
    ///
    /// 【调用者】Provider usage normalizer、测试和需要构造派生观测的下游层。
    /// 【输入】稳定的 usage/attempt/run ID、provider 与请求模型/路由、vector、来源/观测/sequence、置信度、
    /// 未知原因、basis 和 raw digest。
    /// 【输出】成功时是 schema/version 正确且 `usage_digest` 已填的记录；`invocation_id`、cell/project/org、
    /// `served_model_id` 默认 None，`retry_ordinal` 默认 0。Provider adapter 若随后填这些字段，必须重算摘要。
    /// 【失败】必填字段、摘要、Delta 序号、置信度/未知原因组合不符合合同则返回稳定字符串，不会进入下游账本。
    /// 【副作用】只构造内存值，不写 EventLog、不计费、不重试。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        usage_id: UsageId,
        attempt_id: AttemptId,
        run_id: RunId,
        provider_id: Option<String>,
        requested_model_id: impl Into<String>,
        route_id: impl Into<String>,
        vector: UsageVector,
        source: UsageSource,
        observation: UsageObservation,
        sequence: Option<u64>,
        confidence: UsageConfidence,
        unknown_reason: Option<BillingUnknownReason>,
        basis: impl Into<String>,
        raw_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut usage = Self {
            schema: NORMALIZED_USAGE_SCHEMA.to_owned(),
            version: USAGE_SCHEMA_VERSION,
            usage_id,
            attempt_id,
            invocation_id: None,
            run_id,
            cell_id: None,
            project_id: None,
            organization_id: None,
            provider_id,
            requested_model_id: requested_model_id.into(),
            served_model_id: None,
            route_id: route_id.into(),
            retry_ordinal: 0,
            vector,
            source,
            observation,
            sequence,
            confidence,
            unknown_reason,
            basis: basis.into(),
            raw_digest: raw_digest.into(),
            usage_digest: String::new(),
        };
        usage.usage_digest = usage.digest();
        usage.validate()?;
        Ok(usage)
    }

    /// 校验记录身份/文本边界、摘要、vector schema、Delta 序号和置信度/原因组合。
    ///
    /// 【调用者】构造完成后的 `new()`、provider adapter 填完额外字段后，以及下游 attempt/billing reducer。
    /// 【失败影响】调用方应停止将该观测加入累计或结算，并保留原始稳定错误便于定位。
    /// 【边界】该 validator 不校验 `confidence` 是否与 vector 的字段完整度一致，不核验 raw digest 的原文，
    /// 不限制 retry_ordinal，也不把 UnknownReason 与 source 做一一匹配；摘要一致只代表字段内部一致。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NORMALIZED_USAGE_SCHEMA
            || !self.version.is_compatible_with(&USAGE_SCHEMA_VERSION)
            || self.usage_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self
                .provider_id
                .as_deref()
                .is_some_and(|id| !bounded(id, 256))
            || !bounded(&self.requested_model_id, 256)
            || self
                .served_model_id
                .as_deref()
                .is_some_and(|id| !bounded(id, 256))
            || !bounded(&self.route_id, 256)
            || !bounded(&self.basis, 512)
            || !valid_digest(&self.raw_digest)
            || !valid_digest(&self.usage_digest)
            || self.usage_digest != self.digest()
        {
            return Err("normalized_usage_header_invalid".to_owned());
        }
        self.vector.validate()?;
        if matches!(self.observation, UsageObservation::Delta)
            && self.sequence.is_none_or(|sequence| sequence == 0)
        {
            return Err("normalized_usage_delta_sequence_required".to_owned());
        }
        match self.confidence {
            UsageConfidence::Known if self.unknown_reason.is_some() => {
                return Err("normalized_usage_known_reason_invalid".to_owned())
            }
            UsageConfidence::Partial | UsageConfidence::Unknown
                if self.unknown_reason.is_none() =>
            {
                return Err("normalized_usage_unknown_reason_required".to_owned())
            }
            _ => {}
        }
        Ok(())
    }

    /// 对 usage 身份、关联字段、向量、观测/序号、置信度和来源依据生成无密钥摘要。
    /// 修改 public 字段后必须重算它；它用于检测意外漂移，不是 provider 签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "usage_id": self.usage_id,
            "attempt_id": self.attempt_id,
            "invocation_id": self.invocation_id,
            "run_id": self.run_id,
            "cell_id": self.cell_id,
            "project_id": self.project_id,
            "organization_id": self.organization_id,
            "provider_id": self.provider_id,
            "requested_model_id": self.requested_model_id,
            "served_model_id": self.served_model_id,
            "route_id": self.route_id,
            "retry_ordinal": self.retry_ordinal,
            "vector": self.vector,
            "source": self.source,
            "observation": self.observation,
            "sequence": self.sequence,
            "confidence": self.confidence,
            "unknown_reason": self.unknown_reason,
            "basis": self.basis,
            "raw_digest": self.raw_digest,
        }))
    }
}
