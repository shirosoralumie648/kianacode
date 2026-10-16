//! CI-12 产品主链 deny-first/UAT 证据矩阵。
//!
//! # 文件职责与数据流
//!
//! 本文件定义十种安全场景的证据合同，验证“请求被拒绝时没有产生副作用”以及“未知、fake、
//! live 结果不能被夸大”。它只消费已有的结构化观察，不执行 provider/capability：
//!
//! ```text
//! CLI / Workbench / Web / Desktop 复用 DaemonHost
//!        -> ControlPlane / policy / approval / provider 产生事实
//!        -> fixture 或证据投影组装 Ci12Case
//!        -> Ci12ProductGate 聚合十场景
//!        -> 【当前文件】校验计数、receipt、approval/evidence、digest
//!        -> Core facade / CI guard
//! ```
//!
//! deny-first 是本合同的核心：认证、项目 trust、跨项目范围、过期审批、secret、TOCTOU 和
//! replay 被拒绝时，handler/provider/effect 三类计数必须全部为零。`ResultUnknown` 必须保留
//! receipt 和 reconciliation limitation；fake provider success 必须留在离线证据；live opt-in
//! 只有在 typed operator approval 与 provider evidence 同时存在时才可能被进一步消费，但本
//! 文件本身仍不把它提升为 live/physical 证明。
//!
//! ⚠ 这些字段是证据，不是事实源。不要在这里启动第二个模型循环、自动批准请求、调用
//! provider 或把 `digest` 当作签名；真正运行状态仍由 ControlPlane/EventLog/Receipt 主链拥有。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CI12_PRODUCT_GATE_SCHEMA: &str = "kiana.ci12-product-gate.v1";

/// 检查 limitation/引用等文本字段的基本形状。
///
/// 它拒绝空值、过长值和控制字符，但不验证文本来自可信服务器；来源、审批和权限仍由
/// 上层证据链负责。错误字符串由调用方作为稳定拒绝原因传播。
fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

/// 检查未加密钥 SHA-256 内容摘要的外形。
///
/// 通过只说明字符串可解析；各 case/gate 还会重新计算 canonical digest，digest 本身不是
/// provider 真实性、operator 身份或 live effect 的证明。
fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// CI-12 必须覆盖的十类安全/证据场景。
///
/// 前七类要求 deny-first 零副作用；`ResultUnknown` 要求保留未知和恢复线索；`FakeProviderSuccess`
/// 只允许离线 fixture；`LiveOptIn` 负责把 live 声称挡在审批与 provider evidence 之后。
/// 新增场景时必须同步 `Ci12Case::validate`、gate 的十项数量和 CI fixture。
pub enum Ci12Scenario {
    /// 缺少认证，应在 handler 之前拒绝。
    MissingAuthentication,
    /// 项目未通过信任检查。
    UntrustedProject,
    /// 请求访问了不属于当前范围的项目/资源。
    CrossProjectScope,
    /// approval 已过期或不可消费。
    ExpiredApproval,
    /// 发现秘密泄露风险，不能继续执行。
    SecretLeak,
    /// 检查到时间/版本漂移（TOCTOU），应 fail-closed。
    ToctouDrift,
    /// 重放请求，不能产生第二个 effect。
    Replay,
    /// effect 结果未知，需要 receipt 与 reconciliation 限制。
    ResultUnknown,
    /// fake provider 离线成功，仅作为结构化 fixture。
    FakeProviderSuccess,
    /// live opt-in 证据，仍受审批与独立 provider evidence 约束。
    LiveOptIn,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// CI-12 对一个场景给出的结果性质。
///
/// `Denied` 是明确拒绝，`Completed` 只适用于受控 fake/已满足合同的流程，`Unknown` 不能当
/// 成成功，`Unverified` 表示 live 相关证据尚不足以做真实性结论。状态会参与 digest，不能
/// 只在 UI 层改名绕过合同。
pub enum Ci12Disposition {
    /// 请求被拒绝且合同要求无 handler/provider/effect。
    Denied,
    /// 场景按其专属合同完成（fake success 仍是离线证明）。
    Completed,
    /// 结果未知，必须保留回执和恢复限制。
    Unknown,
    /// 尚未验证为 live/physical 结果。
    Unverified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 单个 CI-12 场景的结构化证据。
///
/// `handler_calls`、`provider_calls`、`effect_count` 不是性能统计，而是 deny-first 的副作用
/// 证明；`same_spine` 说明入口是否沿统一 DaemonHost/ControlPlane 主链，`receipt_bound` 说明
/// 结果是否绑定到结构化 receipt。可选 approval/provider evidence 只对相应场景有意义。
pub struct Ci12Case {
    /// case schema 版本。
    pub schema: String,
    /// 十类场景之一。
    pub scenario: Ci12Scenario,
    /// 场景结果性质。
    pub disposition: Ci12Disposition,
    /// handler 被调用的次数；deny 场景必须为 0。
    pub handler_calls: u32,
    /// provider 被调用的次数；deny 场景必须为 0。
    pub provider_calls: u32,
    /// 外部 effect 数量；deny 场景必须为 0。
    pub effect_count: u32,
    /// 证据中是否确认没有秘密文本泄露。
    pub secret_free: bool,
    /// 是否沿唯一 DaemonHost/ControlPlane 主链。
    pub same_spine: bool,
    /// 是否绑定了结构化 receipt。
    pub receipt_bound: bool,
    /// live opt-in 使用的 typed operator approval 引用。
    pub operator_approval_ref: Option<String>,
    /// live provider 独立证据的内容摘要。
    pub live_provider_evidence: Option<String>,
    /// 证据上限、未知结果或 fixture 限制说明，最多 32 条。
    pub limitations: Vec<String>,
    /// 对 case 字段重算出的 canonical 内容摘要。
    pub digest: String,
}

impl Ci12Case {
    /// 校验单个场景的公共字段、专属结果合同和 canonical digest。
    ///
    /// 【公共前提】
    /// schema 必须匹配，证据必须无 secret 并声称复用同一主链，limitation 数量和每条文本
    /// 受限；approval 引用与 live evidence 若存在也必须符合可解析格式。
    ///
    /// 【场景规则】
    /// - 缺认证、untrusted project、cross-project、expired approval、secret leak、TOCTOU、
    ///   replay：结果必须是 Denied，且 handler/provider/effect 全为 0；
    /// - ResultUnknown：必须保留 receipt、非零 effect 观察和 limitation；
    /// - FakeProviderSuccess：只允许一个 provider/effect、绑定 receipt，且不得混入 live
    ///   provider evidence；
    /// - LiveOptIn：除 Unverified 以外，必须带 approval 与 provider evidence；声称 Completed
    ///   时 approval 仍是必需字段。
    ///
    /// 【失败处理】
    /// 首个失败条件返回稳定错误码，不会在这里终止真实 handler、撤销 effect、补发 receipt 或
    /// 自动升级/降级 disposition。调用方必须把错误当作证据拒绝，而不能把它渲染成通过。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CI12_PRODUCT_GATE_SCHEMA
            || !self.secret_free
            || !self.same_spine
            || self.limitations.len() > 32
        {
            return Err("ci12_case_header_or_spine_invalid".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "ci12_limitation_invalid")?;
        }
        if let Some(reference) = &self.operator_approval_ref {
            if reference.trim() != reference || !reference.starts_with("approval:") {
                return Err("ci12_approval_ref_invalid".to_owned());
            }
        }
        if let Some(evidence) = &self.live_provider_evidence {
            digest(evidence, "ci12_live_provider_evidence_invalid")?;
        }
        match self.scenario {
            Ci12Scenario::MissingAuthentication
            | Ci12Scenario::UntrustedProject
            | Ci12Scenario::CrossProjectScope
            | Ci12Scenario::ExpiredApproval
            | Ci12Scenario::SecretLeak
            | Ci12Scenario::ToctouDrift
            | Ci12Scenario::Replay => {
                if self.disposition != Ci12Disposition::Denied
                    || self.handler_calls != 0
                    || self.provider_calls != 0
                    || self.effect_count != 0
                {
                    return Err("ci12_deny_case_effect_invalid".to_owned());
                }
            }
            Ci12Scenario::ResultUnknown => {
                if self.disposition != Ci12Disposition::Unknown
                    || !self.receipt_bound
                    || self.effect_count == 0
                    || self.limitations.is_empty()
                {
                    return Err("ci12_unknown_case_reconcile_invalid".to_owned());
                }
            }
            Ci12Scenario::FakeProviderSuccess => {
                if self.disposition != Ci12Disposition::Completed
                    || self.provider_calls != 1
                    || self.effect_count != 1
                    || !self.receipt_bound
                    || self.live_provider_evidence.is_some()
                {
                    return Err("ci12_fake_success_evidence_invalid".to_owned());
                }
            }
            Ci12Scenario::LiveOptIn => {
                if self.disposition != Ci12Disposition::Unverified
                    && (self.operator_approval_ref.is_none()
                        || self.live_provider_evidence.is_none())
                {
                    return Err("ci12_live_opt_in_proof_invalid".to_owned());
                }
                if self.disposition == Ci12Disposition::Completed
                    && self.operator_approval_ref.is_none()
                {
                    return Err("ci12_live_approval_required".to_owned());
                }
            }
        }
        digest(&self.digest, "ci12_case_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("ci12_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算不包含自身 `digest` 字段的 case canonical digest。
    ///
    /// 所有调用计数、secret/spine/receipt 标记、approval/evidence 引用和 limitation 都参与
    /// 摘要；只改其中一个字段而沿用旧 digest 会在 `validate()` 中失败。该摘要不是签名。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "scenario": self.scenario,
            "disposition": self.disposition,
            "handler_calls": self.handler_calls,
            "provider_calls": self.provider_calls,
            "effect_count": self.effect_count,
            "secret_free": self.secret_free,
            "same_spine": self.same_spine,
            "receipt_bound": self.receipt_bound,
            "operator_approval_ref": self.operator_approval_ref,
            "live_provider_evidence": self.live_provider_evidence,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 包含 CI-12 十个安全场景的完整产品链证据矩阵。
///
/// gate 固定为十条 case，并要求十种 `Ci12Scenario` 恰好各出现一次。它是 Domain 级静态
/// evidence contract；虽然名字包含 “Gate”，它不会在运行时准入请求，也不持有授权权威。
pub struct Ci12ProductGate {
    /// gate schema 版本。
    pub schema: String,
    /// 非零 fixture seed，作为该矩阵样本集的标识输入之一。
    pub seed: u64,
    /// 十类场景各一份的 evidence case。
    pub cases: Vec<Ci12Case>,
    /// 对 schema、seed 和 case 列表计算的 bundle digest。
    pub digest: String,
}

impl Ci12ProductGate {
    /// 构造 gate 并计算当前 case 列表的 canonical digest。
    ///
    /// 构造阶段不强制十场景完整、不拒绝重复项，也不验证 case；调用者组装完证据后必须再
    /// 调用 `validate()`。这样错误会以稳定 Domain reason 返回，而不是构造函数静默删改输入。
    pub fn new(seed: u64, cases: Vec<Ci12Case>) -> Self {
        let mut gate = Self {
            schema: CI12_PRODUCT_GATE_SCHEMA.to_owned(),
            seed,
            cases,
            digest: String::new(),
        };
        gate.digest = gate.canonical_digest();
        gate
    }

    /// 验证 gate 头部、bundle digest、每个 case 和十场景唯一覆盖。
    ///
    /// 先拒绝错误 schema/seed/数量，再校验 gate 摘要，随后逐 case 验证并拒绝重复 scenario。
    /// 注意顺序：gate digest 正确并不能替代内部 case 校验，因为调用方可以为错误内容重新
    /// 计算一个格式正确的未加密钥摘要。此方法只返回证据是否合法，不执行或授权任何动作。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CI12_PRODUCT_GATE_SCHEMA || self.seed == 0 || self.cases.len() != 10 {
            return Err("ci12_gate_header_invalid".to_owned());
        }
        digest(&self.digest, "ci12_gate_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("ci12_gate_digest_mismatch".to_owned());
        }
        let mut scenarios = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !scenarios.insert(case.scenario) {
                return Err("ci12_scenario_duplicate".to_owned());
            }
        }
        Ok(())
    }

    /// 计算不包含自身 `digest` 字段的完整 gate canonical digest。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "seed": self.seed,
            "cases": self.cases,
        }))
    }
}
