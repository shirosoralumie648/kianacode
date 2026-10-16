//! AUT-24 durable/live proof ceiling 的发布证据声明 DTO 与字段校验。
//!
//! # 本文件在系统中的位置
//!
//! 当前可确认的数据流只有：调用方构造或反序列化 `Aut24ReleaseGate`
//!     -> 可选地经 Core `validate_automation_release_gate()` 转发
//!     -> 本文件检查声明字段、重复 step 和自摘要
//!     -> 返回 `Ok(())` 或 `aut24_*` 错误字符串
//!
//! 当前没有生产 Rust 调用者、发布动作、EventStore 读写或真实证据文件读取。通过校验不证明
//! `snapshot_digest` 对应某个真实 snapshot，不证明 `workflow_ref`/`evidence_ref` 文件存在，不证明审批
//! 已发生，也不会把源码证据自动提升为 durable/live/physical。这个结构是 proof handoff 索引声明，
//! 不是发布 gate 执行器。要了解 Core 的实际对外边界，继续读 `kiana-core::automation_release_evidence`。
//!
//! Serde（Rust 序列化/反序列化库）的 `deny_unknown_fields` 会拒绝未知 JSON 字段；本文件的 digest 是
//! 无密钥 canonical JSON SHA-256 内容指纹，只验证字段与自带摘要一致，不能认证证据来源或授权人。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

/// AUT-24 证据 gate DTO 的固定序列化 schema 名称。
pub const AUT24_RELEASE_GATE_SCHEMA: &str = "kiana.aut24-release-gate.v1";

/// 校验必需文本是否非空且不含不允许的控制字符。
///
/// 字符串长度上限是 4096 UTF-8 字节，拒绝 NUL/CR/LF；此 helper 不解析路径、URL、artifact 身份，且
/// 不要求整串没有空格，因此调用者传入的业务含义仍需其他边界核验。
fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}
/// 校验 `sha256:` 加 64 位十六进制的外部格式，不重新计算它声称覆盖的对象。
fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

/// 实现方对一个 roadmap step 的功能完成度声明。
///
/// 这是报告字段，不会由 validator 从源码、测试或 CURRENT_STATUS 推导出来。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aut24FeatureStatus {
    /// 调用方声明该 step 已实现；validator 只额外拒绝精确的 `evidence_ref == "none"`。
    Implemented,
    /// 调用方声明仅完成一部分。
    Partial,
    /// 当前选择延后处理。
    Deferred,
    /// 当前明确不支持。
    NotSupported,
}

/// 证据声称达到的最强证明层级。
///
/// variant 声明顺序同时决定 `Ord` 比较顺序；当前校验中的 `proof_level >= Live` 因而表示 Live 或
/// Physical。若未来重排 variant，比较语义也会改变，不能把 derive 出来的顺序当成无关实现细节。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aut24ProofLevel {
    /// 只有源代码/静态结构可供检查。
    Source,
    /// 本地可重复执行的行为证据。
    LocalBehavior,
    /// 经过持久化/重启边界验证的证据。
    Durable,
    /// 与真实外部服务交互取得的证据。
    Live,
    /// 目标物理设备或现实环境取得的证据。
    Physical,
}

/// 一项 roadmap step 的实现状态、证明级别、下一关卡和限制声明。
///
/// validator 只比较这些调用方提供的值和摘要，不自行运行测试、读取文件或判断证据真实性。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aut24EvidenceRow {
    /// roadmap step 的文本 ID；同一 gate 内按原始大小写/空格精确去重。
    pub step: String,
    /// 该 step 的功能状态声明。
    pub feature_status: Aut24FeatureStatus,
    /// 该 step 声称的最高证据层级；Live/Physical 必须同时声称 Implemented。
    pub proof_level: Aut24ProofLevel,
    /// 指向证据的字符串引用；当前不打开路径，只拒绝精确字符串 `"none"`（且仅对 Implemented）。
    pub evidence_ref: String,
    /// 后续应满足的 gate/step 描述；只做必填文本检查。
    pub next_gate: String,
    /// 证明当前能力限制的说明；列表及每项都必须非空，但列表数量无上限。
    pub limitations: Vec<String>,
    /// 对本行除 digest 外字段计算的 SHA-256 canonical 摘要。
    pub digest: String,
}

impl Aut24EvidenceRow {
    /// 校验一行证据声明的字段、状态/证明层级组合及自摘要。
    ///
    /// 【调用者/输入】由 `Aut24ReleaseGate::validate()` 对每一行调用；所有状态和引用都由 gate 构造者
    /// 提供，validator 不从 roadmap/CURRENT_STATUS 推导真值。
    ///
    /// 【检查流程】要求 step、evidence_ref、next_gate 通过 `required()`；limitations 至少一项且逐项必填；
    /// Live/Physical 的 proof_level 要求 feature_status=Implemented；Implemented 只拒绝精确值 `"none"`；
    /// 最后检查 digest 字符串格式及其 canonical 重算值。
    ///
    /// 【输出/失败】第一个不满足的条件返回稳定 `aut24_*` 错误字符串，Core facade 会原样传播。无重试、
    /// 日志、文件读取、审批验证或状态改变。
    ///
    /// ⚠ `required()` 允许非空占位文本，且 `"none "` 不等于精确 `"none"`；所以通过不证明 evidence_ref
    /// 指向真实证据。限制说明项数也没有封顶。此处仅记录已实现条件，不改变校验。
    fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (&self.step, "aut24_step_required"),
            (&self.evidence_ref, "aut24_evidence_ref_required"),
            (&self.next_gate, "aut24_next_gate_required"),
        ] {
            required(value, field)?;
        }
        if self.limitations.is_empty() {
            return Err("aut24_limitations_required".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "aut24_limitation_invalid")?;
        }
        if self.proof_level >= Aut24ProofLevel::Live
            && self.feature_status != Aut24FeatureStatus::Implemented
        {
            return Err("aut24_live_proof_requires_implemented_status".to_owned());
        }
        if self.feature_status == Aut24FeatureStatus::Implemented && self.evidence_ref == "none" {
            return Err("aut24_implemented_evidence_missing".to_owned());
        }
        digest(&self.digest, "aut24_row_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut24_row_digest_mismatch".to_owned());
        }
        Ok(())
    }
    /// 对该行所有声明字段（不包括 digest 自身）计算规范 JSON 摘要。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "step": self.step,
            "feature_status": self.feature_status,
            "proof_level": self.proof_level,
            "evidence_ref": self.evidence_ref,
            "next_gate": self.next_gate,
            "limitations": self.limitations,
        }))
    }
}

/// 汇总一组不同 step 的证据声明，并绑定 snapshot/workflow 的外部引用。
///
/// 该 gate 的 `blanket_completion` 必须为 false，用于防止“一项总完成声明”替代逐 step 证据；validator
/// 仍只检查传入字段的结构，不能证明被引用的 snapshot/workflow 真实存在或已获人工批准。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aut24ReleaseGate {
    /// 必须与 `AUT24_RELEASE_GATE_SCHEMA` 完全相等。
    pub schema: String,
    /// 被引用 snapshot 的 digest 文本；只校验格式，不加载 snapshot 并重新比对。
    pub snapshot_digest: String,
    /// workflow/baseline 的引用文本；只做必填字符串检查，不打开文件。
    pub workflow_ref: String,
    /// 逐 step 的 evidence row；要求 1 到 256 行且 step ID 原样不重复。
    pub rows: Vec<Aut24EvidenceRow>,
    /// 禁止用一个 blanket completion 结论取代逐 step 证据；必须为 false。
    pub blanket_completion: bool,
    /// 除自身外所有 gate 字段与完整 rows 计算出的 SHA-256 canonical 摘要。
    pub digest: String,
}

impl Aut24ReleaseGate {
    /// 按 header、外部引用形状、row 内容/唯一性和顶层 digest 顺序校验 gate。
    ///
    /// 【调用者】Core 的 `validate_automation_release_gate()` facade 转发到这里；全仓未找到生产调用者，
    /// 现有测试仅直接构造/验证 DTO 或通过静态源码 guard 查 marker。
    ///
    /// 【核心流程】schema 必须匹配；rows 数量为 1..=256；blanket_completion 必须为 false；snapshot_digest
    /// 只检查 digest 形状；workflow_ref 只检查必填文本。然后逐行校验，并用原始 step 字符串在 BTreeSet
    /// 中拒绝重复；最后比较 gate 自身的 digest。
    ///
    /// 【失败/副作用】返回首个字段或重复 step 的 `aut24_*` 错误字符串；不打开引用文件、不验证 approval、
    /// 不读取测试回执、不更改 roadmap proof status，也不执行 release/publish。
    ///
    /// 【设计原因】逐行声明限制“整个项目已完成”的笼统证明，把最高证据层级、下一关和当前限制逐项保留；
    /// 但真正的 proof promotion 必须由外部权威流程根据证据审核，这个 DTO 校验不是该审核本身。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUT24_RELEASE_GATE_SCHEMA
            || self.rows.is_empty()
            || self.rows.len() > 256
            || self.blanket_completion
        {
            return Err("aut24_gate_header_or_blanket_completion_invalid".to_owned());
        }
        digest(&self.snapshot_digest, "aut24_snapshot_digest_invalid")?;
        required(&self.workflow_ref, "aut24_workflow_ref_required")?;
        let mut steps = BTreeSet::new();
        for row in &self.rows {
            row.validate()?;
            if !steps.insert(row.step.clone()) {
                return Err("aut24_step_duplicate".to_owned());
            }
        }
        digest(&self.digest, "aut24_gate_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut24_gate_digest_mismatch".to_owned());
        }
        Ok(())
    }
    /// 对完整 gate（包括 rows 各自的 digest）计算规范 JSON 摘要，不包含自身 digest。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "snapshot_digest": self.snapshot_digest,
            "workflow_ref": self.workflow_ref,
            "rows": self.rows,
            "blanket_completion": self.blanket_completion,
        }))
    }
}
