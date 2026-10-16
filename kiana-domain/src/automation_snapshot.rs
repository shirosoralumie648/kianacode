//! AUT-22 自动化工作、触发器、Receipt 和 incident 查询视图的领域 DTO 与静态校验。
//!
//! # 本文件的职责和调用位置
//!
//! 当前实际能确认的契约路径是：调用方构造 `AutomationSnapshot`
//!     -> Core `validate_automation_snapshot()` 转发
//!     -> 本文件检查快照头部、嵌套对象、跨对象 ID 绑定和摘要
//!     -> 返回 `Ok(())` 或具体字段错误字符串
//!
//! 当前没有发现生产快照 builder 或生产代码调用该 validator；AUT-22 baseline 将它定位为只读 projection
//! contract，真实的 planner/queue/Receipt/incident owners 仍负责各自状态事实。因此不能把此 DTO 的存在或
//! 校验通过解释成 UI 已获得 EventStore 实时快照、scheduler 已运行，或 claim/approval/effect 已执行。
//!
//! 注意区分同名概念：协议的 `AUTOMATION_SNAPSHOT` 命令进入 Core `workflow_snapshot()`，通过
//! `load_workflows()` 读取并折叠 durable workflow 事件，返回 workflow state DTO；那条生产读路径不是
//! 本文件的 AUT-22 `AutomationSnapshot`，也不会调用本文件的 validator。
//!
//! Serde（Rust 序列化/反序列化库）的 `deny_unknown_fields` 让结构体拒绝未声明 JSON 键；本文件中的
//! SHA-256 摘要使用 Domain 的规范 JSON helper，只能证明对象字段与随附摘要一致，不能认证来源。
//! 校验过程不读取持久化，不 claim queue lease、不消费 approval、不 dispatch capability、不修改 incident。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

/// AUT-22 自动化投影视图的固定 wire schema 名称。
pub const AUT22_SNAPSHOT_SCHEMA: &str = "kiana.aut22-automation-snapshot.v1";

/// 检查必需文本的形状，并用调用者传入的字段名返回稳定错误。
///
/// 当前规则是 trim 后非空、最多 4096 UTF-8 字节、不得含 NUL/CR/LF；不会验证 ID 是否存在、字符串
/// 是否属于特定命名空间，也不要求除首尾空白外的内容无空格。
fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}
/// 检查 SHA-256 摘要文本的外部格式；不计算它引用的业务数据。
///
/// 前缀必须精确是小写 `sha256:`，后面恰好 64 个 ASCII 十六进制字符；十六进制字母接受大小写。
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

/// AUT-22 UI/query projection 对 workflow 运行状态的简化分类。
///
/// 这组值不是 durable workflow 的完整状态机：durable planner 另有 Failed、Cancelled、Paused、
/// WaitingApproval 等状态，目前没有本文件中的映射代码说明它们应如何折叠到这五种状态。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWorkStatus {
    /// 查询视图认为工作已到执行时间或等待开始。
    Due,
    /// 查询视图认为工作暂时不能继续；reason 说明调用方提供的原因。
    Blocked,
    /// 查询视图认为工作正在运行。
    Running,
    /// 外部事实不足以确定工作结果；workflow view 必须附至少一项 evidence 引用。
    Unknown,
    /// 查询视图认为运行完成；不代表业务结果已经确认。
    Completed,
}

/// 将触发工作流的来源简化成手动、事件或时间间隔三类。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTriggerSource {
    /// 人或入口显式发起。
    Manual,
    /// 由已发生的事件触发。
    Event,
    /// 由间隔/定时计划触发。
    Interval,
}

/// 单个 workflow 在 AUT-22 查询视图中的投影。
///
/// 这是只读结果对象，不保存 planner 状态、不持有 queue lease。`digest` 覆盖其余字段，用于发现单条视图
/// 内容变化；快照校验还会确认 workflow ID 在同一快照中唯一。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationWorkflowView {
    /// durable workflow 的逻辑 ID；本 DTO 要求同一 snapshot 中不重复。
    pub workflow_id: String,
    /// 映射后的简化运行状态；当前没有 durable 状态到此 enum 的生产映射器。
    pub status: AutomationWorkStatus,
    /// 人类可读原因或状态解释；这里只要求非空。
    pub reason: String,
    /// 支持此视图结论的引用。此类列表最多 64 项，逐项要求有效文本，但不会打开或解析引用目标。
    pub evidence_refs: Vec<String>,
    /// `Due` 工作的 Unix 毫秒截止/到期时间；非 Due 状态可为空。
    pub due_at_unix_ms: Option<u64>,
    /// 对 workflow_id/status/reason/evidence/deadline 计算的摘要，不含自身。
    pub digest: String,
}

impl AutomationWorkflowView {
    /// 校验单条 workflow 投影视图的字段、局部状态条件和 digest。
    ///
    /// 【调用者/输入】由 `AutomationSnapshot::validate()` 遍历调用；接收一条由快照构造者提供的视图。
    ///
    /// 【核心流程】要求 ID/reason 非空；evidence 最多 64 项并逐项应用 `required()`；Due 必须带 deadline；
    /// Unknown 必须至少带一项 evidence；然后检查 digest 格式和 canonical digest 相等。
    ///
    /// 【输出/失败】成功返回 `Ok(())`；错误码分别标识缺 ID、缺 reason、证据无效/超量、Due 缺 deadline、
    /// Unknown 缺证据或摘要无效/不匹配。无 I/O、无副作用；不会验证 evidence 的真实性。
    fn validate(&self) -> Result<(), String> {
        required(&self.workflow_id, "aut22_workflow_id_required")?;
        required(&self.reason, "aut22_workflow_reason_required")?;
        if self.evidence_refs.len() > 64 {
            return Err("aut22_workflow_evidence_limit".to_owned());
        }
        for reference in &self.evidence_refs {
            required(reference, "aut22_workflow_evidence_invalid")?;
        }
        if self.status == AutomationWorkStatus::Due && self.due_at_unix_ms.is_none() {
            return Err("aut22_due_workflow_deadline_missing".to_owned());
        }
        if self.status == AutomationWorkStatus::Unknown && self.evidence_refs.is_empty() {
            return Err("aut22_unknown_workflow_evidence_missing".to_owned());
        }
        digest(&self.digest, "aut22_workflow_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_workflow_digest_mismatch".to_owned());
        }
        Ok(())
    }
    /// 按当前视图的五个语义字段计算规范 JSON 摘要，刻意不把 digest 自身放入摘要输入。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "workflow_id": self.workflow_id,
            "status": self.status,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
            "due_at_unix_ms": self.due_at_unix_ms,
        }))
    }
}

/// 某次 workflow 触发条件在查询快照中的只读视图。
///
/// `occurrence_key` 表示触发来源中的某次发生实例；本 DTO 只要求其文本非空，不验证它是否是唯一幂等键。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationTriggerView {
    /// 触发器记录 ID；同一快照内必须唯一。
    pub trigger_id: String,
    /// 触发器所属 workflow；快照 validator 要求引用本快照已列出的 workflow。
    pub workflow_id: String,
    /// 手动、事件或间隔触发来源。
    pub source: AutomationTriggerSource,
    /// 此次发生的键；用于标识哪一次触发，但当前 validator 不做 occurrence 去重。
    pub occurrence_key: String,
    /// 此次触发的简化状态。
    pub status: AutomationWorkStatus,
    /// 触发状态或拒绝原因；必须为有效文本。
    pub reason: String,
    /// 触发证据引用；Unknown 至少要求非空，但当前不逐项验证内容或限制列表长度。
    pub evidence_refs: Vec<String>,
    /// 除摘要自身以外所有触发字段的 canonical digest。
    pub digest: String,
}

impl AutomationTriggerView {
    /// 校验触发器的身份文本、Unknown 证据要求及内容摘要。
    ///
    /// workflow 是否存在、trigger_id 是否重复在外层 `AutomationSnapshot::validate()` 检查。本方法对
    /// evidence_refs 只检查 Unknown 时列表非空，不检查列表成员是否为空、引用目标是否存在或发生键是否唯一。
    fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (&self.trigger_id, "aut22_trigger_id_required"),
            (&self.workflow_id, "aut22_trigger_workflow_required"),
            (&self.occurrence_key, "aut22_trigger_occurrence_required"),
            (&self.reason, "aut22_trigger_reason_required"),
        ] {
            required(value, field)?;
        }
        if self.status == AutomationWorkStatus::Unknown && self.evidence_refs.is_empty() {
            return Err("aut22_unknown_trigger_evidence_missing".to_owned());
        }
        digest(&self.digest, "aut22_trigger_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_trigger_digest_mismatch".to_owned());
        }
        Ok(())
    }
    /// 对该视图除 digest 外的语义字段计算规范 JSON 摘要。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "trigger_id": self.trigger_id,
            "workflow_id": self.workflow_id,
            "source": self.source,
            "occurrence_key": self.occurrence_key,
            "status": self.status,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
        }))
    }
}

/// 某个 workflow 运行结果及业务结果证据在快照中的只读 Receipt 视图。
///
/// `runtime_status` 讲的是运行时状态，`business_outcome_confirmed` 讲的是现实/业务结果是否另有证据确认；
/// 两者必须分开读取，Completed 本身不会自动把业务结果置为已确认。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationReceiptView {
    /// Receipt 的稳定 ID；同一快照内必须唯一。
    pub receipt_id: String,
    /// Receipt 所属 workflow；必须引用同一快照里的 workflow。
    pub workflow_id: String,
    /// workflow runtime 的简化状态，不等于业务成功。
    pub runtime_status: AutomationWorkStatus,
    /// 调用方是否声称业务结果已确认；若为 true，必须附 business evidence。
    pub business_outcome_confirmed: bool,
    /// 支持 runtime 状态的证据列表；当前只要求列表非空，不逐项检查字符串或限制数量。
    pub runtime_evidence_refs: Vec<String>,
    /// 支持 business outcome 的独立证据；只有 confirmed=true 时要求列表非空。
    pub business_evidence_refs: Vec<String>,
    /// 对当前证明能力的限制说明；只校验列表非空，不校验每一项内容。
    pub limitations: Vec<String>,
    /// 除自身外所有 Receipt 字段的 canonical digest。
    pub digest: String,
}

impl AutomationReceiptView {
    /// 校验 Receipt 身份、证据是否存在以及自摘要一致性。
    ///
    /// 本方法要求 runtime evidence 和 limitations 两个列表非空；business outcome 标为 confirmed 时，
    /// business evidence 也必须非空。列表元素、外部引用及证据是否真实不在此处验证，跨 workflow 绑定和
    /// receipt_id 去重由 snapshot 层处理。
    fn validate(&self) -> Result<(), String> {
        required(&self.receipt_id, "aut22_receipt_id_required")?;
        required(&self.workflow_id, "aut22_receipt_workflow_required")?;
        if self.runtime_evidence_refs.is_empty() || self.limitations.is_empty() {
            return Err("aut22_receipt_evidence_or_limitations_missing".to_owned());
        }
        if self.business_outcome_confirmed && self.business_evidence_refs.is_empty() {
            return Err("aut22_business_outcome_evidence_missing".to_owned());
        }
        digest(&self.digest, "aut22_receipt_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }
    /// 对 Receipt 的运行状态与业务结果分离字段一并计算摘要，不含 digest 自身。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "receipt_id": self.receipt_id,
            "workflow_id": self.workflow_id,
            "runtime_status": self.runtime_status,
            "business_outcome_confirmed": self.business_outcome_confirmed,
            "runtime_evidence_refs": self.runtime_evidence_refs,
            "business_evidence_refs": self.business_evidence_refs,
            "limitations": self.limitations,
        }))
    }
}

/// 与 workflow 关联的异常/未知结果及待 reconcile 声明。
///
/// 此对象只描述查询视图中的 incident，不创建工单、不执行补偿，也不真的对账；`unknown` 为 true 时，
/// 要求同时声明 reconcile_required 并至少包含一项 evidence。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationIncidentView {
    /// incident 身份文本；当前快照校验不要求不同 incident_id 唯一。
    pub incident_id: String,
    /// 关联 workflow ID；必须引用本快照中的 workflow。
    pub workflow_id: String,
    /// 调用方是否认为效果/结果仍未知。
    pub unknown: bool,
    /// 是否需要人工或系统 reconcile；unknown=true 时必须为 true。
    pub reconcile_required: bool,
    /// 发生未知/异常的理由，必须是有效文本。
    pub reason: String,
    /// 支持 incident 声明的引用；unknown=true 时只要求列表非空，不验证每项。
    pub evidence_refs: Vec<String>,
    /// 对 incident 其余字段的 canonical digest。
    pub digest: String,
}

impl AutomationIncidentView {
    /// 校验 incident 的 ID/reason、Unknown 与 reconcile/evidence 关系及 digest。
    ///
    /// 关联 workflow 是否存在在 snapshot 外层检查；incident_id 是否重复、evidence 是否真实或引用是否
    /// 有界，当前代码不检查。失败统一返回相应的 `aut22_incident_*` 字段错误字符串。
    fn validate(&self) -> Result<(), String> {
        required(&self.incident_id, "aut22_incident_id_required")?;
        required(&self.workflow_id, "aut22_incident_workflow_required")?;
        required(&self.reason, "aut22_incident_reason_required")?;
        if self.unknown && (!self.reconcile_required || self.evidence_refs.is_empty()) {
            return Err("aut22_unknown_incident_reconcile_missing".to_owned());
        }
        digest(&self.digest, "aut22_incident_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_incident_digest_mismatch".to_owned());
        }
        Ok(())
    }
    /// 对 incident 的状态声明、原因和 evidence 引用计算摘要，不包含 digest 字段本身。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "incident_id": self.incident_id,
            "workflow_id": self.workflow_id,
            "unknown": self.unknown,
            "reconcile_required": self.reconcile_required,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
        }))
    }
}

/// 把 workflow、trigger、Receipt、incident 的只读查询视图绑定到一组源/投影/授权版本的聚合快照。
///
/// 结构体本身不生成 projection，也不定义实际 EventStore reader。source cursor、projection version 和
/// authority epoch 均由上游提供；当前 validator 只检查非零，不检查它们是否对应同一时刻的真实存储快照。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSnapshot {
    /// 必须等于本模块 AUT-22 schema 常量。
    pub schema: String,
    /// 本次快照自身身份；只要求有效文本，不要求从 source cursor 派生。
    pub snapshot_id: String,
    /// 调用方声明的源事实读取位置；必须非零，但本 DTO 不解释它是全局 cursor 还是单流版本。
    pub source_cursor: u64,
    /// 投影版本号；必须非零，不在此处校验投影实现或新旧兼容关系。
    pub projection_version: u64,
    /// 生成快照时引用的 authority epoch；必须非零，不读取实时 authority。
    pub authority_epoch: u64,
    /// 同一查询结果中的 workflow 简化视图；ID 必须唯一。
    pub workflows: Vec<AutomationWorkflowView>,
    /// 触发记录视图；每项必须绑定现有 workflow，trigger_id 必须唯一。
    pub triggers: Vec<AutomationTriggerView>,
    /// runtime/business evidence 分离后的收据；每项须绑定 workflow 且 receipt_id 唯一。
    pub receipts: Vec<AutomationReceiptView>,
    /// 未知效果或异常的查询视图；每项须绑定 workflow，但当前不按 incident_id 去重。
    pub incidents: Vec<AutomationIncidentView>,
    /// 对整份视图的限制说明；仅校验列表非空，不逐项检查文本。
    pub limitations: Vec<String>,
    /// 对除自身以外的所有快照字段和嵌套 DTO（包括嵌套 digest）计算的 canonical digest。
    pub digest: String,
}

impl AutomationSnapshot {
    /// 按头部、嵌套对象、跨对象 ID 绑定和整份摘要的顺序验证快照。
    ///
    /// 【调用者】Core 的只读 `validate_automation_snapshot()` facade 转调；当前未找到生产 builder 或生产
    /// 调用者，Domain fixture 会直接手工构造对象。
    ///
    /// 【输入/输出】借用整份快照；成功只表示本地 DTO 规则通过，失败返回字段级静态错误字符串，原对象不变。
    ///
    /// 【核心流程】要求 schema 正确、source cursor/projection version/authority epoch 非零、limitations
    /// 列表非空；再检查 snapshot_id。随后逐条调用各 view 的 validator：workflow ID 唯一；trigger 必须
    /// 引用已有 workflow 且 trigger ID 唯一；Receipt 必须引用已有 workflow 且 receipt ID 唯一；incident
    /// 必须引用已有 workflow。最后校验顶层 digest 格式并重算比较。
    ///
    /// 【失败处理/副作用】第一个失败条件即返回对应错误字符串，不聚合多条错误、不重试、不读 EventStore、
    /// 不写查询索引、不消费 lease/approval、不修改任何 workflow 或 incident。
    ///
    /// ⚠ cursor/version/epoch 只要求非零，不与当前权威状态比对；只有 workflow evidence 有数量上限并校验
    /// 每项文本，其他 evidence/list 元素与顶层列表没有相同的完整性/容量约束。状态 enum 也尚未涵盖 durable
    /// workflow 的所有终态/等待态；当前没有映射器，不能自行把缺少的状态映射成 Completed 或 Unknown。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUT22_SNAPSHOT_SCHEMA
            || self.source_cursor == 0
            || self.projection_version == 0
            || self.authority_epoch == 0
            || self.limitations.is_empty()
        {
            return Err("aut22_snapshot_header_invalid".to_owned());
        }
        required(&self.snapshot_id, "aut22_snapshot_id_required")?;
        let mut workflows = BTreeSet::new();
        for workflow in &self.workflows {
            workflow.validate()?;
            if !workflows.insert(workflow.workflow_id.clone()) {
                return Err("aut22_workflow_duplicate".to_owned());
            }
        }
        let mut triggers = BTreeSet::new();
        for trigger in &self.triggers {
            trigger.validate()?;
            if !workflows.contains(&trigger.workflow_id)
                || !triggers.insert(trigger.trigger_id.clone())
            {
                return Err("aut22_trigger_binding_invalid".to_owned());
            }
        }
        let mut receipts = BTreeSet::new();
        for receipt in &self.receipts {
            receipt.validate()?;
            if !workflows.contains(&receipt.workflow_id)
                || !receipts.insert(receipt.receipt_id.clone())
            {
                return Err("aut22_receipt_binding_invalid".to_owned());
            }
        }
        for incident in &self.incidents {
            incident.validate()?;
            if !workflows.contains(&incident.workflow_id) {
                return Err("aut22_incident_binding_invalid".to_owned());
            }
        }
        digest(&self.digest, "aut22_snapshot_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对快照头部、全部嵌套视图及 limitations 计算规范 JSON 摘要，不包含顶层 digest 本身。
    ///
    /// 嵌套视图中的 digest 是各嵌套对象字段的一部分，也包含在顶层摘要输入中；因此修改一个引用或
    /// 子对象摘要都会使整份快照的摘要变化。内容摘要仍不认证生产者。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "snapshot_id": self.snapshot_id,
            "source_cursor": self.source_cursor,
            "projection_version": self.projection_version,
            "authority_epoch": self.authority_epoch,
            "workflows": self.workflows,
            "triggers": self.triggers,
            "receipts": self.receipts,
            "incidents": self.incidents,
            "limitations": self.limitations,
        }))
    }
}
