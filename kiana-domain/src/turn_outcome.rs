//! # 文件职责
//!
//! 本文件定义结构化输出合同（OutputContract）和一次 Run/Turn 如何结束的确定性判定器
//! （outcome reducer）。它把 Runner/Core 提供的输出、待处理工作、取消/失败/未知效果事实归并为
//! `TurnOutcome`；模型输出里写着“完成”并不能覆盖这些结构化事实。
//!
//! # 主要调用链
//!
//! ```text
//! kiana-runner -> RunnerEvent::Completed
//!   -> kiana-core/src/lifecycle.rs
//!   -> TurnOutcomeInput::completed(run_id, output)
//!   -> Core::propose_turn_outcome()
//!   -> TurnOutcome::decide() / validate()
//!   -> Core::annotate_output() 将 turn_outcome 加入结构化结果
//!   -> lifecycle 脱敏后以 run.completed 事实交给 terminal/receipt 路径
//! ```
//!
//! 当前源码调用扫描只找到 Core lifecycle 在 RunnerEvent::Completed 分支调用便捷构造器；该构造器
//! 没有 OutputContract、待处理 invocation/job、steering、approval 或 clarification 输入，相关字段均为
//! 默认值。因此，本文件确实实现并测试了完整优先级及可选输出合同，但不能据此宣称当前所有业务角色的
//! JSON 输出合同和 pending facts 已接入完成入口。failure/Unknown 等其他 Runner 事件由 lifecycle
//! 的独立终态分支处理。H27 baseline 将本切片证明上限定为 source；本 Domain 模块不写 EventLog、不重试、
//! 不对账，也不调用 Provider。
//!
//! 当前 Core 路径先在 Runner 输出中补 `run_id`，再把该 JSON 交给本判定器计算 `output_digest`；随后
//! 才附加 `turn_outcome` 并对将写入事件的完整 JSON 做脱敏。因此 `output_digest` 描述的是输入给
//! `decide()` 的那一版值，不一定等于最终 EventLog payload 的摘要，不能把它当成脱敏后事件字节的校验和。

// Serde 的 Serialize/Deserialize derive 负责 Rust 值与 JSON 等外部格式的互转；
// deny_unknown_fields 拒绝合同外字段，Option 上的 default/skip_serializing_if 控制缺省值的 wire 形式。
// 这些格式属性不验证数据由谁生成，也不认证状态真实性。

use crate::{json_digest, validate_json_limits, RunId, SchemaVersion, StepId, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 输出合同 DTO 的稳定外部 schema 名称。
pub const OUTPUT_CONTRACT_SCHEMA: &str = "kiana.output-contract.v1";
/// 一次 TurnOutcome DTO 的稳定外部 schema 名称。
pub const TURN_OUTCOME_SCHEMA: &str = "kiana.turn-outcome.v1";
/// 输出合同与 TurnOutcome 共用的格式版本。
pub const TURN_OUTCOME_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 输出合同最多描述 128 个必填键，也最多描述 128 个属性。
pub const MAX_OUTPUT_CONTRACT_FIELDS: usize = 128;
/// artifact、verification 和 blocker 列表各自最多容纳 128 项。
pub const MAX_TURN_OUTCOME_REFS: usize = 128;

/// OutputContract 中一个属性允许出现的 JSON 值类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputValueType {
    /// 任意 JSON 类型，包括 null。
    Any,
    /// JSON null。
    Null,
    /// JSON 布尔值。
    Boolean,
    /// JSON 数字。
    Number,
    /// JSON 字符串。
    String,
    /// JSON 对象。
    Object,
    /// JSON 数组。
    Array,
}

/// 对模型或 Runner 输出形状的有限约束，不是业务验收、权限或文件存在证明。
///
/// 当前合同只描述顶层对象的必填属性、单层属性类型和是否接受额外键；它不表达嵌套对象 schema、
/// 字符串格式、跨字段约束或现实副作用。合同应由可信上游选择，但本 DTO 本身不认证提供者。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    /// 此合同 DTO 的 schema 名称。
    pub schema: String,
    /// schema 兼容版本。
    pub version: SchemaVersion,
    /// 供日志或上游目录识别该输出合同的名称。
    pub name: String,
    /// 该合同自身的非零业务版本；与 DTO 的 `version` 是两种不同版本。
    pub contract_version: u32,
    /// 输出对象必须含有的键；每个键也必须在 `properties` 中声明。
    pub required: Vec<String>,
    /// 输出键到 JSON 顶层类型的映射；BTreeMap 使 key 顺序稳定。
    pub properties: BTreeMap<String, OutputValueType>,
    /// true 时允许对象带未列入 `properties` 的额外键，false 时拒绝。
    pub additional_properties: bool,
    /// 对其他合同字段的无密钥摘要，用于检测内容变化。
    pub contract_digest: String,
}

impl OutputContract {
    /// 组装当前版本的输出合同，计算自摘要并立即校验。
    ///
    /// 【调用者】定义角色/任务结果格式的服务端上游；当前 Core 完成事件的默认构造器没有传入合同。
    /// 【输入/输出】合同名、独立业务版本、必填键、属性类型表和额外键策略；成功返回完整合同。
    /// 【失败情况】名称/字段数量/版本/摘要无效时返回 `output_contract_invalid`；必填键重复、未声明
    /// 或名称无效时返回 `output_contract_required_invalid`；属性名无效时返回
    /// `output_contract_property_invalid`。
    /// 【副作用/设计】只创建内存 DTO；先计算不含摘要字段的 digest 再校验，确保构造值能通过同一合同。
    pub fn new(
        name: impl Into<String>,
        contract_version: u32,
        required: Vec<String>,
        properties: BTreeMap<String, OutputValueType>,
        additional_properties: bool,
    ) -> Result<Self, String> {
        let mut contract = Self {
            schema: OUTPUT_CONTRACT_SCHEMA.to_owned(),
            version: TURN_OUTCOME_VERSION,
            name: name.into(),
            contract_version,
            required,
            properties,
            additional_properties,
            contract_digest: String::new(),
        };
        contract.contract_digest = contract.digest();
        contract.validate()?;
        Ok(contract)
    }

    /// 校验合同 schema/version、摘要、属性数量和 required/property 之间的关系。
    ///
    /// schema 与 version 必须严格等于本文件的 v1.0 合同；必填键必须非空、最多 128 字节、互不重复且已在
    /// properties 登记；属性名最多 128 字节。
    /// `bounded()` 按 UTF-8 字节计数，并拒绝 NUL。该方法不检查业务语义，也不证明合同来自服务端。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OUTPUT_CONTRACT_SCHEMA
            || self.version != TURN_OUTCOME_VERSION
            || !bounded(&self.name, 256)
            || self.contract_version == 0
            || self.required.len() > MAX_OUTPUT_CONTRACT_FIELDS
            || self.properties.len() > MAX_OUTPUT_CONTRACT_FIELDS
            || !digest(&self.contract_digest)
            || self.contract_digest != self.digest()
        {
            return Err("output_contract_invalid".to_owned());
        }
        let mut required = std::collections::HashSet::new();
        for field in &self.required {
            if !bounded(field, 128)
                || !required.insert(field.as_str())
                || !self.properties.contains_key(field)
            {
                return Err("output_contract_required_invalid".to_owned());
            }
        }
        if self.properties.keys().any(|field| !bounded(field, 128)) {
            return Err("output_contract_property_invalid".to_owned());
        }
        Ok(())
    }

    /// 将一个 JSON 输出与本合同比较，只检查顶层对象键和 JSON 类型。
    ///
    /// 【调用者】`TurnOutcome::decide()` 在没有 Unknown/取消/失败/等待/未完成工作的优先状态时调用。
    /// 【输入/输出】输出必须是 JSON object；required 键必须出现，声明的键类型必须匹配。
    /// 【失败情况】先传播合同自校验错误和 `validate_json_limits()` 的复杂度/体积错误，再可能返回
    /// `turn_output_object_required`、`turn_output_required_missing:*`、`turn_output_field_unknown:*` 或
    /// `turn_output_field_type:*`。在 outcome 判定中，这些输出形状错误会成为 Failed outcome 的 reason。
    /// 【限制】`validate_json_limits()` 另限制 JSON 深度 32、单个数组/对象最多 4096 项、总节点最多
    /// 32768 个、序列化体积最多 512 KiB；本合同不递归校验嵌套属性，也不验证文件/Artifact 内容。
    pub fn validate_output(&self, output: &Value) -> Result<(), String> {
        self.validate()?;
        validate_json_limits(output)?;
        let Value::Object(object) = output else {
            return Err("turn_output_object_required".to_owned());
        };
        for field in &self.required {
            if !object.contains_key(field) {
                return Err(format!("turn_output_required_missing:{field}"));
            }
        }
        for (field, value) in object {
            let Some(expected) = self.properties.get(field) else {
                if self.additional_properties {
                    continue;
                }
                return Err(format!("turn_output_field_unknown:{field}"));
            };
            if !matches_output_type(*expected, value) {
                return Err(format!("turn_output_field_type:{field}"));
            }
        }
        Ok(())
    }

    /// 对 schema、合同业务版本、required、属性类型和额外属性策略计算摘要；排除 `contract_digest` 自身。
    ///
    /// 它使同样的合同内容产生同样摘要，但不是签名或身份凭据；调用者修改字段后也可以重新计算。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "name": self.name,
            "contract_version": self.contract_version,
            "required": self.required,
            "properties": self.properties,
            "additional_properties": self.additional_properties,
        }))
    }
}

/// Turn 最终判定的稳定类别；Serde 将其编码为 snake_case 字符串。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcomeKind {
    /// 该 Turn 给出了面向用户的回答，且没有更高优先级阻塞状态。
    Answered,
    /// 该 Turn 的工作已完成，但不是由 `answered` 标记为直接回答。
    Completed,
    /// 等待用户补充信息，可在后续输入后继续。
    AwaitingInput,
    /// 等待一次审批决定，可在审批流程后继续。
    AwaitingApproval,
    /// 仍有 invocation、后台 job 或 steering 等未完成工作。
    Blocked,
    /// 调用方已提供取消事实。
    Cancelled,
    /// 调用方提供失败原因，或输出违反合同。
    Failed,
    /// 外部效果已发生但结果不确定，需要独立 reconciliation；这不是成功。
    ResultUnknown,
}

impl TurnOutcomeKind {
    /// 返回 DTO 中使用的稳定状态字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Completed => "completed",
            Self::AwaitingInput => "awaiting_input",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::ResultUnknown => "result_unknown",
        }
    }

    /// 判断是否是本次 Turn 的终态。
    ///
    /// ResultUnknown 也在此返回 true：Runner 不能在外部副作用未明时假装本 Turn 可安全重跑；
    /// “terminal”不表示 Unknown 已经对账或现实结果已知。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Answered | Self::Completed | Self::Cancelled | Self::Failed | Self::ResultUnknown
        )
    }

    /// 判断是否属于明确的交互等待状态；Blocked 虽然可恢复，但不等同于等用户输入或等审批。
    pub const fn is_waiting(self) -> bool {
        matches!(self, Self::AwaitingInput | Self::AwaitingApproval)
    }
}

/// `TurnOutcome::decide()` 的输入事实，由 Runner/Core 而不是模型自述提供。
///
/// flags 描述调用者观察到的尚未完成事项。若调用方没有填入真实状态，判定器只能依据默认值做决定；
/// 此 struct 本身不读取 Harness queue、approval store、clarification inbox 或 job registry。
#[derive(Clone, Debug, PartialEq)]
pub struct TurnOutcomeInput {
    /// 该结果所属的 Run；必须是非 nil ID。
    pub run_id: RunId,
    /// 可选的模型会话轮次关联 ID；None 表示当前输入没有提供 Turn 绑定。
    pub turn_id: Option<TurnId>,
    /// 可选的工作步骤关联 ID；None 表示当前输入没有提供 Step 绑定。
    pub step_id: Option<StepId>,
    /// 要校验并摘要的结构化 JSON 输出；原始 Value 不会复制进 TurnOutcome。
    pub output: Value,
    /// 可选的输出形状合同；None 表示不做 OutputContract 字段/类型检查。
    pub output_contract: Option<OutputContract>,
    /// true 表示该终态属于直接回答；false 时无阻塞且有效的输出归为 Completed。
    pub answered: bool,
    /// 尚未返回结果的 capability invocation 数量；大于 0 时阻止完成。
    pub pending_invocations: u32,
    /// 尚未结束的后台 job 数量；大于 0 时阻止完成。
    pub pending_background_jobs: u32,
    /// 是否有未消费的用户 steering 指令。
    pub pending_steering: bool,
    /// 是否正在等待用户澄清；优先级高于 approval 和普通 outstanding work。
    pub pending_clarification: bool,
    /// 是否存在待决审批。
    pub pending_approval: bool,
    /// 调用方是否已确认取消。
    pub cancelled: bool,
    /// 是否有尚不能确定结果的外部效果；最高优先级，避免继续声称成功或安全重放。
    pub effect_unknown: bool,
    /// 已知失败的原因；None 或空字符串不构成失败，非空内容进入 Failed outcome。
    pub failure: Option<String>,
    /// 结果关联的 Artifact 引用，不等于文件存在或内容已验证的证明。
    pub artifact_refs: Vec<String>,
    /// 结果关联的验证证据引用，不等于验证一定通过。
    pub verification_refs: Vec<String>,
}

impl TurnOutcomeInput {
    /// 创建普通完成候选的便捷输入，其他 flags、合同和引用使用“没有观察到”的默认值。
    ///
    /// 【调用者】当前由 Core lifecycle 的 RunnerEvent::Completed 分支使用。
    /// 【输入/输出】Run ID 与输出 JSON；返回 turn/step、contract、pending、取消、Unknown、失败和引用
    /// 均未提供的输入结构。
    /// 【重要限制】这不是从 Runner queue 或 Core 状态读取的快照。若需要输出合同或等待/待处理判定，
    /// 上游必须改用完整 struct 填入事实；不能把此构造器的零值解释成外部系统已证明“没有待办”。
    pub fn completed(run_id: RunId, output: Value) -> Self {
        Self {
            run_id,
            turn_id: None,
            step_id: None,
            output,
            output_contract: None,
            answered: false,
            pending_invocations: 0,
            pending_background_jobs: 0,
            pending_steering: false,
            pending_clarification: false,
            pending_approval: false,
            cancelled: false,
            effect_unknown: false,
            failure: None,
            artifact_refs: Vec::new(),
            verification_refs: Vec::new(),
        }
    }
}

/// Domain reducer 为一次 Turn 生成的结构化、可序列化结束结果。
///
/// 它保留结果类别和引用，不保留完整输出正文。Core 把此值嵌入结构化 output，再交给生命周期事实/Receipt
/// 路径；摘要用于检查内容自洽，不能独立证明该值由 Core 生成。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnOutcome {
    /// 结果合同名称。
    pub schema: String,
    /// 结果格式版本。
    pub version: SchemaVersion,
    /// 结果所属 Run。
    pub run_id: RunId,
    /// 可选的 Turn 关联；None 表示没有提供。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,
    /// 可选的 Step 关联；None 表示没有提供。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<StepId>,
    /// 结构化结果类别。
    pub kind: TurnOutcomeKind,
    /// 与 `kind.as_str()` 一致的稳定 wire/display 状态。
    pub status: String,
    /// 由 kind 推导的终态标记；校验时必须与 `kind.is_terminal()` 一致。
    pub terminal: bool,
    /// 输入输出 JSON 的摘要；结果 DTO 不含原始输出，故 validate 只能检查摘要格式和自身绑定。
    pub output_digest: String,
    /// 使用的输出合同摘要；None 表示判定输入没有 output contract。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_digest: Option<String>,
    /// 输入提供的 Artifact 引用。
    pub artifact_refs: Vec<String>,
    /// 输入提供的验证证据引用。
    pub verification_refs: Vec<String>,
    /// 使本次 Turn 不能完成的结构化阻塞类别。
    pub blockers: Vec<String>,
    /// 人类可读或稳定 machine reason；具体内容由判定分支提供。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 对其他结果字段计算的无密钥摘要。
    pub outcome_digest: String,
}

impl TurnOutcome {
    /// 按固定优先级将输入事实归约为一份结果，并为其字段生成摘要。
    ///
    /// 【调用者】Core 的 `propose_turn_outcome()` 调用；Domain H27 合同测试直接调用。
    /// 【输入】Run/Turn/Step 身份、输出 JSON、可选合同、pending（待处理）/取消/失败/未知状态和证据引用。
    /// 【输出】合法输入返回 `TurnOutcome`；原始 output 只用于计算 `output_digest`，不会保留在结果中。
    /// 【错误】nil ID、结构不合法的合同、引用超限或引用为空时直接返回 Err；若只是输出不符合合同，
    /// 则在无更高优先级状态时返回 kind=Failed 的 outcome；生成后的字段若超过结果自身上限（例如 reason
    /// 超过 4096 字节）仍会由末尾 `validate()` 返回 Err。
    /// 【副作用】完全在内存中运行，不发信号、不重试、不禁用账号、不写 EventLog 或查询上游状态。
    /// 【设计原因】固定优先级让 Unknown、取消、失败和仍在等待/执行的事项不能被“输出看起来完成”覆盖。
    pub fn decide(input: TurnOutcomeInput) -> Result<Self, String> {
        if input.run_id.as_uuid().is_nil()
            || input.turn_id.is_some_and(|id| id.as_uuid().is_nil())
            || input.step_id.is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("turn_outcome_identity_invalid".to_owned());
        }
        if let Some(contract) = input.output_contract.as_ref() {
            contract.validate()?;
        }
        validate_refs(&input.artifact_refs, "artifact")?;
        validate_refs(&input.verification_refs, "verification")?;

        // 一份合同的自身结构无论最后生成哪种 outcome 都必须有效；但 output 内容只在所有更高优先级
        // 的未知、取消、失败、等待和未完工作都不存在时校验。第一个匹配分支决定最终 kind。
        let contract_digest = input
            .output_contract
            .as_ref()
            .map(|contract| contract.contract_digest.clone());
        let (kind, reason, blockers) = if input.effect_unknown {
            (
                TurnOutcomeKind::ResultUnknown,
                Some("effect_unknown_requires_reconciliation".to_owned()),
                vec!["result_unknown".to_owned()],
            )
        } else if input.cancelled {
            (
                TurnOutcomeKind::Cancelled,
                Some("cancelled".to_owned()),
                vec!["cancelled".to_owned()],
            )
        // 空 failure 字符串被视作“没有失败说明”，不会产生 Failed；非空失败保留为 reason。
        } else if let Some(failure) = input.failure.as_deref().filter(|value| !value.is_empty()) {
            (
                TurnOutcomeKind::Failed,
                Some(failure.to_owned()),
                vec!["failure".to_owned()],
            )
        } else if input.pending_clarification {
            (
                TurnOutcomeKind::AwaitingInput,
                Some("waiting_for_input".to_owned()),
                vec!["clarification".to_owned()],
            )
        } else if input.pending_approval {
            (
                TurnOutcomeKind::AwaitingApproval,
                Some("awaiting_approval".to_owned()),
                vec!["approval".to_owned()],
            )
        } else if input.pending_invocations > 0
            || input.pending_background_jobs > 0
            || input.pending_steering
        {
            let mut blockers = Vec::new();
            if input.pending_invocations > 0 {
                blockers.push("invocation".to_owned());
            }
            if input.pending_background_jobs > 0 {
                blockers.push("background_job".to_owned());
            }
            if input.pending_steering {
                blockers.push("steering".to_owned());
            }
            (
                TurnOutcomeKind::Blocked,
                Some("outstanding_work".to_owned()),
                blockers,
            )
        } else if let Some(contract) = input.output_contract.as_ref() {
            // 输出 schema 失败被转成一份可持久化的 Failed 判定；只有 schema 有效后才看 answered 标记。
            if let Err(error) = contract.validate_output(&input.output) {
                (
                    TurnOutcomeKind::Failed,
                    Some(format!("output_contract_invalid:{error}")),
                    vec!["output_contract".to_owned()],
                )
            } else if input.answered {
                (TurnOutcomeKind::Answered, None, Vec::new())
            } else {
                (TurnOutcomeKind::Completed, None, Vec::new())
            }
        } else if input.answered {
            (TurnOutcomeKind::Answered, None, Vec::new())
        } else {
            (TurnOutcomeKind::Completed, None, Vec::new())
        };

        let mut outcome = Self {
            schema: TURN_OUTCOME_SCHEMA.to_owned(),
            version: TURN_OUTCOME_VERSION,
            run_id: input.run_id,
            turn_id: input.turn_id,
            step_id: input.step_id,
            kind,
            status: kind.as_str().to_owned(),
            terminal: kind.is_terminal(),
            output_digest: json_digest(&input.output),
            contract_digest,
            artifact_refs: input.artifact_refs,
            verification_refs: input.verification_refs,
            blockers,
            reason,
            outcome_digest: String::new(),
        };
        // 摘要不含自身字段；validate 再检查构造结果的内部一致性，但不能认证传入事实的来源。
        outcome.outcome_digest = outcome.digest();
        outcome.validate()?;
        Ok(outcome)
    }

    /// 校验结果 DTO 的身份、状态派生关系、引用范围和自摘要。
    ///
    /// 【调用者】`decide()` 生成后调用；Core 的 wrapper/输出注入也会再次调用。
    /// 【限制】此值不保存 output 本文，因此无法重算 `output_digest`；digest 无密钥，调用者若能改字段并
    /// 重新计算摘要仍可通过。`contract_digest` 只做摘要外形检查，不会加载原合同重新计算；该方法也不
    /// 重新运行 `decide()`，不检查 blockers/reason 是否互相符合，更不证明它们来自真实运行态。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TURN_OUTCOME_SCHEMA
            || self.version != TURN_OUTCOME_VERSION
            || self.run_id.as_uuid().is_nil()
            || self.status != self.kind.as_str()
            || self.terminal != self.kind.is_terminal()
            || !digest(&self.output_digest)
            || self.artifact_refs.len() > MAX_TURN_OUTCOME_REFS
            || self.verification_refs.len() > MAX_TURN_OUTCOME_REFS
            || self.blockers.len() > MAX_TURN_OUTCOME_REFS
            || !digest(&self.outcome_digest)
            || self.outcome_digest != self.digest()
        {
            return Err("turn_outcome_invalid".to_owned());
        }
        if self.turn_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.step_id.is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("turn_outcome_identity_invalid".to_owned());
        }
        if let Some(contract_digest) = &self.contract_digest {
            if !digest(contract_digest) {
                return Err("turn_outcome_contract_digest_invalid".to_owned());
            }
        }
        validate_refs(&self.artifact_refs, "artifact")?;
        validate_refs(&self.verification_refs, "verification")?;
        if self.blockers.iter().any(|blocker| !bounded(blocker, 128)) {
            return Err("turn_outcome_blockers_invalid".to_owned());
        }
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| !bounded(reason, 4_096))
        {
            return Err("turn_outcome_reason_invalid".to_owned());
        }
        Ok(())
    }

    /// 对结果 schema、身份、状态、输出/合同摘要和所有引用/阻塞信息计算内容摘要。
    ///
    /// `outcome_digest` 自身不参与输入；数组顺序参与摘要，不能在重算前随意重排证据引用。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "kind": self.kind,
            "status": self.status,
            "terminal": self.terminal,
            "output_digest": self.output_digest,
            "contract_digest": self.contract_digest,
            "artifact_refs": self.artifact_refs,
            "verification_refs": self.verification_refs,
            "blockers": self.blockers,
            "reason": self.reason,
        }))
    }

    /// 返回此 outcome 是否仍是非终态，可继续处理或恢复。
    ///
    /// AwaitingInput、AwaitingApproval 和 Blocked 返回 true；ResultUnknown 返回 false，因为它不能安全
    /// 地重新执行，但“不可恢复此 Turn”不等同于“现实效果已完成对账”。
    pub fn is_resumable(&self) -> bool {
        !self.terminal
    }
}

/// 将声明的 OutputValueType 与一个 JSON 值做精确顶层类型匹配；Any 是唯一接受所有类型的分支。
fn matches_output_type(expected: OutputValueType, value: &Value) -> bool {
    matches!(
        (expected, value),
        (OutputValueType::Any, _)
            | (OutputValueType::Null, Value::Null)
            | (OutputValueType::Boolean, Value::Bool(_))
            | (OutputValueType::Number, Value::Number(_))
            | (OutputValueType::String, Value::String(_))
            | (OutputValueType::Object, Value::Object(_))
            | (OutputValueType::Array, Value::Array(_))
    )
}

/// 限制单个 Artifact/verification 引用列表的数量和每个引用的文本长度。
///
/// `kind` 由调用点传入固定的 `artifact` 或 `verification`，用于形成稳定错误码；每项最多 512 UTF-8
/// 字节，不能为空或包含 NUL。超出 128 项或任何一项不合格都会整份拒绝，不会截断列表。
fn validate_refs(refs: &[String], kind: &str) -> Result<(), String> {
    if refs.len() > MAX_TURN_OUTCOME_REFS || refs.iter().any(|reference| !bounded(reference, 512)) {
        return Err(format!("turn_outcome_{kind}_refs_invalid"));
    }
    Ok(())
}

/// 检查非空、字节上限和 NUL 字符；不裁剪或规范化原字符串。
fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

/// 检查字符串是否具有 SHA-256 摘要的文本外形，不执行哈希计算或来源认证。
fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
