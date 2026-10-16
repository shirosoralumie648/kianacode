//! CP-30 产品流程与证据收口合同（Domain 事实模型）。
//!
//! # 文件职责与数据流
//!
//! 本文件定义的是“如何检查已提交的流程证据”，不是“如何执行流程”。真实执行链仍然由
//! 同一个 `DaemonHost` 组合根驱动：
//!
//! ```text
//! CLI / Workbench / Web / Desktop 请求
//!        -> DaemonHost
//!        -> ControlPlane（授权、关卡、审批、生命周期）
//!        -> KianaHarness / CapabilityBroker
//!        -> EventStore / Receipt
//!        -> 形成 Cp30ProductFlowEvidence
//!        -> 四个界面各自产生 Cp30SurfaceReceipt
//!        -> Cp30ProductFlowBundle
//!        -> 当前文件：校验结构、摘要、流程不变量和 surface parity
//! ```
//!
//! CP-30 要求证据包同时覆盖三种流程：低风险只读、审批后写入、取消/恢复；每个流程又要
//! 在 CLI、Workbench、Web、Desktop 四个界面上共享相同的 flow ID、authority digest、事件
//! 游标、终态和 receipt digest。Core 只提供一个薄校验门面，CI fixture 或适配层通过该门面
//! 消费这里的规则。
//!
//! # 证明边界
//!
//! 这里的字段是对已有观察结果的结构化描述。`digest` 是 canonical JSON 的未加密钥 SHA-256
//! 内容一致性指纹，不是签名、身份认证、审批票据或外部副作用证明；`event_cursor` 是引用
//! 事实流位置的非零游标，也不单独证明跨进程 durable 或 live 执行。CI 中使用的 fake model、
//! fake effect 和本地 receipt 只能证明 source/fixture 契约，不能伪装成 physical 结果。
//!
//! ⚠ 注意：不要在本文件增加第二个模型循环、Broker、UI loop 或自动审批分支。它必须保持
//! 为 Domain 层的单一证据规则，否则四个入口可能各自维护一套“看起来完成”的状态，绕过
//! `ControlPlane` 和 EventLog 事实源。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CONTROL_PLANE_PRODUCT_FLOW_SCHEMA: &str = "kiana.control-plane-product-flow.v1";
pub const CONTROL_PLANE_PRODUCT_BUNDLE_SCHEMA: &str = "kiana.control-plane-product-bundle.v1";

/// 检查一个需要非空、无控制字符且长度有上限的文本字段。
///
/// 该 helper 用于 flow ID 和 limitation 等人类/外部输入字段。它只做形状校验，不验证
/// 名称是否来自某个数据库或当前调用者是否有权使用它；身份和授权由上层协议、ControlPlane
/// 和事实流负责。返回的字符串是调用方预先定义的稳定字段错误码。
fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

/// 检查 `sha256:<64 位十六进制>` 的摘要外形。
///
/// 这里只确认摘要字符串能被 canonical digest 合同解析，不能证明摘要确实由可信来源计算，
/// 也不能替代 `canonical_digest()` 对当前结构内容的重算比较。因此所有调用者仍需在形状
/// 校验后执行“保存的 digest 等于当前字段重算结果”的一致性检查。
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
/// CP-30 证据包中被验证的三种产品流程。
///
/// - `ReadOnly`：低风险读取，不能带有 Broker effect 或审批消费；
/// - `ApprovalWrite`：写入只有在审批消费后才能完成并产生 receipt；
/// - `CancelRecovery`：取消、结果未知或恢复后的结果必须能回到已有事实并完成缓存清理。
///
/// 这个枚举是合同分派键；新增枚举值时必须同步 `Cp30ProductFlowEvidence::validate`、bundle
/// 的完整集合检查和相关 CI fixtures，否则旧消费者可能把新流程误判成已覆盖。
pub enum Cp30FlowKind {
    /// 只读流程，不能声称发生了 effect 或审批消费。
    ReadOnly,
    /// 审批后写入流程。
    ApprovalWrite,
    /// 取消、结果未知和基于事实恢复的流程。
    CancelRecovery,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// CP-30 流程的终态或等待态。
///
/// `AwaitingApproval` 不是成功：它表示审批还没有被消费，不能出现 Broker effect。`ResultUnknown`
/// 表示不能安全断言外部结果，`Reconciled` 表示已经用事实完成对账；两者都属于取消/恢复
/// 证据分支，而不是普通完成。状态名会进入 canonical digest，不能只修改显示文本。
pub enum Cp30FlowStatus {
    /// 流程已经满足对应的完成不变量。
    Completed,
    /// 流程停在等待审批阶段，尚未消费审批或执行 effect。
    AwaitingApproval,
    /// 已确认取消，且本合同要求不能带有 effect。
    Cancelled,
    /// 结果无法安全确认，需要保留未知状态并等待恢复/对账。
    ResultUnknown,
    /// 已从事实恢复并完成对账。
    Reconciled,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 需要保持证据一致性的四个产品界面。
///
/// 这里的 surface 只是同一运行事实的展示/适配面，不是四条独立执行路径。bundle 要求每个
/// flow 恰好出现一次四面 receipt，并比较它们的共享身份、游标、状态和摘要。
pub enum Cp30Surface {
    /// 命令行界面。
    Cli,
    /// Workbench 工作台界面。
    Workbench,
    /// Web 工作台界面。
    Web,
    /// Desktop 壳界面。
    Desktop,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个产品界面看到的 CP-30 流程收据投影。
///
/// 它把同一流程的关键对账键带到每个界面：`flow_id` 说明属于哪个流程，`authority_digest`
/// 说明使用了哪份授权事实，`event_cursor` 说明对应事实流的位置，`status` 和
/// `receipt_digest` 说明终态/收据是否一致。它是证据投影，不会反向触发执行或审批。
pub struct Cp30SurfaceReceipt {
    /// 固定的单界面证据 schema 版本。
    pub schema: String,
    /// 产生这份展示收据的界面。
    pub surface: Cp30Surface,
    /// 所属流程 ID；必须与父 evidence 相同。
    pub flow_id: String,
    /// 授权事实摘要；必须与父 evidence 相同。
    pub authority_digest: String,
    /// 关联 EventLog/事实流的非零游标。
    pub event_cursor: u64,
    /// 该界面看到的流程状态；必须与父 evidence 相同。
    pub status: Cp30FlowStatus,
    /// 可选最终 receipt 摘要；只读流程通常没有，审批写入/恢复流程可有。
    pub receipt_digest: Option<String>,
    /// 对本收据除自身 digest 外的字段重算出的内容摘要。
    pub digest: String,
}

impl Cp30SurfaceReceipt {
    /// 构造一份界面收据，并立即按当前字段计算 canonical digest。
    ///
    /// 【调用者】
    /// Domain fixture、surface adapter 或证据投影层在已经知道流程身份/状态后调用。它不会
    /// 查询 EventStore，也不会检查四面是否齐全；跨 surface 的一致性由父 evidence 验证。
    ///
    /// 【输入与输出】
    /// 参数分别是界面、流程 ID、授权摘要、事件游标、状态和可选 receipt 摘要；返回的结构
    /// 固定使用 `CONTROL_PLANE_PRODUCT_FLOW_SCHEMA`，并把摘要计算放在构造阶段，避免调用方
    /// 忘记初始化 `digest`。
    ///
    /// 【失败/副作用】
    /// 构造函数不返回 `Result`，也不验证字段形状；空 ID、坏 digest 或零游标会在后续
    /// `validate()` 中被拒绝。它只在内存中创建值，不写日志、不发请求、不生成真实收据。
    pub fn new(
        surface: Cp30Surface,
        flow_id: impl Into<String>,
        authority_digest: impl Into<String>,
        event_cursor: u64,
        status: Cp30FlowStatus,
        receipt_digest: Option<String>,
    ) -> Self {
        let mut receipt = Self {
            schema: CONTROL_PLANE_PRODUCT_FLOW_SCHEMA.to_owned(),
            surface,
            flow_id: flow_id.into(),
            authority_digest: authority_digest.into(),
            event_cursor,
            status,
            receipt_digest,
            digest: String::new(),
        };
        receipt.digest = receipt.canonical_digest();
        receipt
    }

    /// 验证单个界面收据的 schema、字段形状和 canonical digest。
    ///
    /// 这是父 `Cp30ProductFlowEvidence::validate` 使用的内部第一层检查。它只负责“这份
    /// receipt 自己是否自洽”；它不负责检查四个 surface 是否齐全，也不负责比较父 evidence
    /// 的 flow/status/receipt 字段。任何失败都返回稳定错误码，由父层继续向调用方传播。
    fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_PRODUCT_FLOW_SCHEMA || self.event_cursor == 0 {
            return Err("cp30_surface_receipt_header_invalid".to_owned());
        }
        required(&self.flow_id, "cp30_surface_flow_required")?;
        digest(
            &self.authority_digest,
            "cp30_surface_authority_digest_invalid",
        )?;
        if let Some(receipt) = &self.receipt_digest {
            digest(receipt, "cp30_surface_receipt_digest_invalid")?;
        }
        digest(&self.digest, "cp30_surface_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp30_surface_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 以不包含自身 `digest` 字段的 canonical JSON 计算内容指纹。
    ///
    /// JSON 字段集合和枚举序列化形状是跨界面对账合同的一部分。该方法只计算一致性摘要，
    /// 不提供密码学签名、来源认证或 replay protection；验证时必须用当前字段重新计算并
    /// 与保存的 digest 比较。
    fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "surface": self.surface,
            "flow_id": self.flow_id,
            "authority_digest": self.authority_digest,
            "event_cursor": self.event_cursor,
            "status": self.status,
            "receipt_digest": self.receipt_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个 CP-30 流程在四个界面上的完整证据。
///
/// 该结构是从 EventLog/Receipt 等事实投影出来的“证据包”，不是运行时状态机。字段中的
/// 计数用于检查流程不变量：只读必须有模型调用但不能有 Broker/effect，审批写入完成时必须
/// 消费审批并有一个 Broker/effect/receipt，取消恢复必须清理缓存且能从事实恢复。
pub struct Cp30ProductFlowEvidence {
    /// 单流程 schema 版本。
    pub schema: String,
    /// 稳定流程 ID；用于把四个 surface receipt 归并到同一流程。
    pub flow_id: String,
    /// 流程类别，决定后续使用哪组不变量。
    pub kind: Cp30FlowKind,
    /// 当前流程终态或等待审批状态。
    pub status: Cp30FlowStatus,
    /// 产生该证据时使用的授权事实摘要。
    pub authority_digest: String,
    /// 关联事实流游标；零值被视为没有可核对事实。
    pub event_cursor: u64,
    /// 记录的模型调用次数；只读流程必须大于零。
    pub model_calls: u32,
    /// 记录的 Broker 调用次数；审批写入完成目前要求恰好一次。
    pub broker_calls: u32,
    /// 记录的外部 effect 数量；只读必须为零，完成写入目前要求恰好一次。
    pub effect_count: u32,
    /// 是否已经消费审批；等待审批状态必须为 false，完成写入必须为 true。
    pub approval_consumed: bool,
    /// 取消/恢复流程是否移除了旧缓存，防止盲目复用未确认结果。
    pub cache_removed: bool,
    /// 取消/恢复流程是否根据已有事实恢复，而不是从 UI/transcript 猜测。
    pub restored_from_facts: bool,
    /// CLI、Workbench、Web、Desktop 四面收据集合。
    pub surfaces: Vec<Cp30SurfaceReceipt>,
    /// 可选最终收据摘要；完成写入时必须存在。
    pub receipt_digest: Option<String>,
    /// 已知限制或证据上限，最多保存 32 条非空文本。
    pub limitations: Vec<String>,
    /// 对本证据除自身 digest 外字段计算出的内容摘要。
    pub digest: String,
}

impl Cp30ProductFlowEvidence {
    /// 根据流程观察值构造 evidence，并计算其 canonical digest。
    ///
    /// 【调用者】
    /// 主要由 CI fixture、适配层或事实投影在已经拥有流程计数和四面 receipt 后调用。构造
    /// 不会执行模型、Broker、审批或恢复动作，也不会替调用方补充缺失 surface。
    ///
    /// 【为什么保留多个计数】
    /// `model_calls`、`broker_calls`、`effect_count` 和 `approval_consumed` 分别对应不同的
    /// 权限/副作用阶段；只保留一个“成功”布尔值会掩盖“审批未消费却发生 effect”或“只读
    /// 流程声称写入”等安全错误。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        flow_id: impl Into<String>,
        kind: Cp30FlowKind,
        status: Cp30FlowStatus,
        authority_digest: impl Into<String>,
        event_cursor: u64,
        model_calls: u32,
        broker_calls: u32,
        effect_count: u32,
        approval_consumed: bool,
        cache_removed: bool,
        restored_from_facts: bool,
        surfaces: Vec<Cp30SurfaceReceipt>,
        receipt_digest: Option<String>,
        limitations: Vec<String>,
    ) -> Self {
        let mut evidence = Self {
            schema: CONTROL_PLANE_PRODUCT_FLOW_SCHEMA.to_owned(),
            flow_id: flow_id.into(),
            kind,
            status,
            authority_digest: authority_digest.into(),
            event_cursor,
            model_calls,
            broker_calls,
            effect_count,
            approval_consumed,
            cache_removed,
            restored_from_facts,
            surfaces,
            receipt_digest,
            limitations,
            digest: String::new(),
        };
        evidence.digest = evidence.canonical_digest();
        evidence
    }

    /// 验证一条流程证据及其四面 receipt 的全部 CP-30 不变量。
    ///
    /// 【检查顺序】
    /// 1. schema、游标、四面数量和限制数量；
    /// 2. flow/authority/receipt/digest 文本形状；
    /// 3. 每个 surface receipt 自洽且与父 evidence 一致，并且四个界面不重复；
    /// 4. 根据 `kind` 检查只读、审批写入或取消恢复的专属计数/状态；
    /// 5. 最后重算 evidence digest，拒绝过期或手工篡改的字段。
    ///
    /// 【失败处理】
    /// 返回稳定错误码；不会自动修复、重试、撤销 effect、删除 cache、写失败事件或改变
    /// ControlPlane 状态。上层可以把它作为 source/fixture guard 的拒绝依据，但不能把错误
    /// 转换为“流程成功”。
    ///
    /// ⚠ 易误改：不要把 `AwaitingApproval` 当作完成，也不要为了接收“未知”数据而放宽
    /// `ResultUnknown`/`Reconciled` 的恢复条件；这些分支用于防止在没有事实时声称外部动作
    /// 已完成。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_PRODUCT_FLOW_SCHEMA
            || self.event_cursor == 0
            || self.surfaces.len() != 4
            || self.limitations.len() > 32
        {
            return Err("cp30_product_flow_header_invalid".to_owned());
        }
        required(&self.flow_id, "cp30_flow_id_required")?;
        digest(&self.authority_digest, "cp30_authority_digest_invalid")?;
        if let Some(receipt) = &self.receipt_digest {
            digest(receipt, "cp30_receipt_digest_invalid")?;
        }
        for limitation in &self.limitations {
            required(limitation, "cp30_limitation_invalid")?;
        }
        let mut surfaces = BTreeSet::new();
        for receipt in &self.surfaces {
            receipt.validate()?;
            if receipt.flow_id != self.flow_id
                || receipt.authority_digest != self.authority_digest
                || receipt.event_cursor != self.event_cursor
                || receipt.status != self.status
                || receipt.receipt_digest != self.receipt_digest
                || !surfaces.insert(receipt.surface)
            {
                return Err("cp30_surface_parity_mismatch".to_owned());
            }
        }
        let expected = [
            Cp30Surface::Cli,
            Cp30Surface::Workbench,
            Cp30Surface::Web,
            Cp30Surface::Desktop,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if surfaces != expected {
            return Err("cp30_surface_set_incomplete".to_owned());
        }
        match self.kind {
            Cp30FlowKind::ReadOnly => {
                if self.status != Cp30FlowStatus::Completed
                    || self.effect_count != 0
                    || self.broker_calls != 0
                    || self.approval_consumed
                    || self.model_calls == 0
                {
                    return Err("cp30_read_only_flow_effect_or_approval_invalid".to_owned());
                }
            }
            Cp30FlowKind::ApprovalWrite => {
                if self.status == Cp30FlowStatus::Completed
                    && (!self.approval_consumed
                        || self.broker_calls != 1
                        || self.effect_count != 1
                        || self.receipt_digest.is_none())
                {
                    return Err("cp30_approval_write_requires_consumed_receipt".to_owned());
                }
                if self.status == Cp30FlowStatus::AwaitingApproval
                    && (self.approval_consumed || self.broker_calls != 0 || self.effect_count != 0)
                {
                    return Err("cp30_pending_approval_has_effect".to_owned());
                }
            }
            Cp30FlowKind::CancelRecovery => {
                if !matches!(
                    self.status,
                    Cp30FlowStatus::Cancelled
                        | Cp30FlowStatus::ResultUnknown
                        | Cp30FlowStatus::Reconciled
                ) || !self.cache_removed
                    || !self.restored_from_facts
                {
                    return Err("cp30_cancel_recovery_evidence_incomplete".to_owned());
                }
                if self.status == Cp30FlowStatus::Cancelled && self.effect_count > 0 {
                    return Err("cp30_cancelled_effect_count_invalid".to_owned());
                }
            }
        }
        digest(&self.digest, "cp30_flow_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp30_flow_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算不包含自身 `digest` 字段的 evidence canonical digest。
    ///
    /// `surfaces`、限制文本和所有计数字段都会进入摘要，因此修改任何证据内容后都必须通过
    /// 正确的构造/重算流程生成新摘要；仅把旧 digest 原样复制会在 `validate()` 中失败。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "flow_id": self.flow_id,
            "kind": self.kind,
            "status": self.status,
            "authority_digest": self.authority_digest,
            "event_cursor": self.event_cursor,
            "model_calls": self.model_calls,
            "broker_calls": self.broker_calls,
            "effect_count": self.effect_count,
            "approval_consumed": self.approval_consumed,
            "cache_removed": self.cache_removed,
            "restored_from_facts": self.restored_from_facts,
            "surfaces": self.surfaces,
            "receipt_digest": self.receipt_digest,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// CP-30 三类流程的总证据包。
///
/// bundle 是 Core/CI 消费的最小完整集合：必须恰好有一条只读、一条审批写入和一条取消/恢复
/// 证据，且流程种类与 ID 均不得重复。它聚合的是事实投影，不是第二个执行状态机。
pub struct Cp30ProductFlowBundle {
    /// bundle schema 版本。
    pub schema: String,
    /// 三条流程证据；数量和种类由 `validate` 固定检查。
    pub flows: Vec<Cp30ProductFlowEvidence>,
    /// 对 schema 和 flows 计算出的 bundle 内容摘要。
    pub digest: String,
}

impl Cp30ProductFlowBundle {
    /// 构造流程 bundle，并计算其 canonical digest。
    ///
    /// 构造阶段不检查是否已经包含三种流程；空列表、重复流程或缺少某类流程会在
    /// `validate()` 中失败。这样允许调用方先组装事实投影，再用同一个公开校验入口得到
    /// 明确的拒绝错误，而不是在构造阶段偷偷丢弃数据或自动补造流程。
    pub fn new(flows: Vec<Cp30ProductFlowEvidence>) -> Self {
        let mut bundle = Self {
            schema: CONTROL_PLANE_PRODUCT_BUNDLE_SCHEMA.to_owned(),
            flows,
            digest: String::new(),
        };
        bundle.digest = bundle.canonical_digest();
        bundle
    }

    /// 验证 bundle 的完整性、流程唯一性和三类流程覆盖。
    ///
    /// 【输入】
    /// 调用方提供已经构造的流程列表；本方法不读取 EventLog、不比较外部账单，也不验证
    /// authority digest 对应的签名。它只检查本文件定义的结构和每条 evidence 的验证结果。
    ///
    /// 【失败情况】
    /// bundle schema/数量/digest 不正确，某条 evidence 失败，流程 kind 或 flow ID 重复，或
    /// 三类流程集合不完整时返回稳定错误码。错误不会触发执行、审批或恢复。
    ///
    /// 【为什么要求完整集合】
    /// 只有同时看到 read-only、approval/write、cancel/recovery，CI 和上层报告才能说明产品
    /// 主路径的三个安全边界都被覆盖；仅有一条绿色 fixture 不能证明整个 ControlPlane 产品流。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_PRODUCT_BUNDLE_SCHEMA || self.flows.len() != 3 {
            return Err("cp30_product_bundle_header_invalid".to_owned());
        }
        digest(&self.digest, "cp30_bundle_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp30_bundle_digest_mismatch".to_owned());
        }
        let mut kinds = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for flow in &self.flows {
            flow.validate()?;
            if !kinds.insert(flow.kind) || !ids.insert(flow.flow_id.clone()) {
                return Err("cp30_product_flow_duplicate".to_owned());
            }
        }
        let expected = [
            Cp30FlowKind::ReadOnly,
            Cp30FlowKind::ApprovalWrite,
            Cp30FlowKind::CancelRecovery,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if kinds != expected {
            return Err("cp30_product_flow_set_incomplete".to_owned());
        }
        Ok(())
    }

    /// 计算不包含自身 `digest` 字段的 bundle canonical digest。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "flows": self.flows,
        }))
    }
}
