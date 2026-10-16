//! 本文件定义可跨事件存储和 wire 协议保存的持久化 workflow / trigger 数据契约。
//!
//! # 在系统中的位置
//!
//! ```text
//! CLI / Web / Workbench 的通用 command
//!     -> RequestEnvelope.arguments 反序列化为 AutomationCommandRequest
//!     -> Core Automation 读取历史、建立 AutomationAuthority / AutomationProof
//!     -> kiana-workflow::durable 纯规划出新 AutomationState 和 WorkflowEffect
//!     -> Core 先 CAS 追加 AutomationEvent，再沿 Company / Capability 路径消费 effect
//!     -> EventStore 重放 AutomationEvent.request + authority + proof 重建状态
//! ```
//!
//! 这里仅定义共享类型和确定性 interval 计算，不负责入口路由、数据库/EventStore、权限决策、
//! timer、网络或执行节点。`WorkflowDefinition` 是可复用模板，`WorkflowInstance` 是一次运行，
//! `AutomationCommandRequest` 是一次状态转换请求，`AutomationEvent` 是 Core 写入事实流的载荷。
//! `WorkflowEffect` 是 planner 返回给 Core 的待办描述，不是许可；是否执行仍由 ControlPlane
//! 按当前调用者、policy、gate、approval 和 Broker 流程决定。
//!
//! Serde（Rust 数据类型与 JSON 等 wire 格式互转）派生让这些结构可被请求解析和事件重放。
//! `deny_unknown_fields` 会拒绝不认识的 JSON 字段，避免旧 schema 静默接收含义不明的新数据；
//! 新增字段时还要考虑历史事件兼容，不能只看当前 Rust 构造代码。
use crate::{json_digest, CapabilityRequest, CoreResponse, RequestContext, RequestId, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Core 通用命令路由使用的 durable workflow 命令名。
pub const AUTOMATION_COMMAND: &str = "workflow.command.v1";
/// Core 通用命令路由使用的只读 workflow 快照命令名。
pub const AUTOMATION_SNAPSHOT: &str = "workflow.snapshot.v1";
/// 命令请求 DTO 的版本标识；不是命令路由名称。
pub const AUTOMATION_SCHEMA: &str = "kiana.workflow-command.v1";
/// EventStore 中 workflow 事件载荷的版本标识。
pub const AUTOMATION_EVENT_SCHEMA: &str = "kiana.workflow-event.v1";
/// WorkflowDefinition 内容摘要纳入的 schema 版本。
pub const WORKFLOW_DEFINITION_SCHEMA: &str = "kiana.workflow-definition.v1";
/// TriggerDefinition 内容摘要纳入的 schema 版本。
pub const TRIGGER_DEFINITION_SCHEMA: &str = "kiana.trigger-definition.v1";
/// 事件 envelope 的版本标识。
pub const AUTOMATION_EVENT_ENVELOPE_SCHEMA: &str = "kiana.workflow-event-envelope.v1";
/// 纯 planner intent 摘要的版本标识。
pub const WORKFLOW_PLAN_INTENT_SCHEMA: &str = "kiana.workflow-plan-intent.v1";

/// 一个不可变版本的 workflow 模板，描述输入输出契约、准入角色、预算和节点依赖图。
///
/// 注册时由 `kiana-workflow::durable::validate_definition()` 检查静态结构；启动实例时按
/// `(definition_id, version)` 查找这个版本。实例保留这两个字段，因此模板升级应登记新版本，
/// 不能覆盖已有版本后再用新内容解释旧事件。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDefinition {
    /// 工作流模板的稳定逻辑名称；与 version 合起来构成版本身份。
    pub definition_id: String,
    /// 单调升级的模板版本号；必须大于 0。
    pub version: u64,
    /// 启动时必须提供的输入 key；当前实例创建会检查这些 key 存在，但允许额外输入。
    pub input_keys: Vec<String>,
    /// 普通实例成功前必须出现在 WorkflowInstance.outputs 中的 key。
    pub output_keys: Vec<String>,
    /// 可以创建该定义实例的 role ID 列表；它只是 planner 准入条件之一，不代替后续能力授权。
    pub allowed_roles: Vec<String>,
    /// 从实例创建时间开始计算的总运行时限，单位为毫秒；不是单个节点的 timeout。
    pub max_duration_ms: u64,
    /// 实例可启动的最大节点步数，防止长时间推进无界增长。
    pub max_steps: u32,
    /// 以节点 ID 排序存储的工作流 DAG；BTreeMap 的顺序也会影响 planner 的稳定选择顺序。
    pub nodes: BTreeMap<String, WorkflowNode>,
    /// 逻辑 artifact 的依赖/输出描述。`serde(default)` 让缺少该字段的旧载荷按空表读取。
    #[serde(default)]
    pub artifacts: BTreeMap<String, WorkflowArtifact>,
}

impl WorkflowDefinition {
    /// 按固定字段生成模板内容摘要，以便 intent/事件等结构关联一个具体定义版本。
    ///
    /// 摘要包含 schema、版本、输入输出、角色、预算、节点和 artifact。它是 SHA-256 内容摘要，
    /// 不含密钥，所以不是签名，也不能证明模板经过审批。当前 planner 用它填充 intent；Core
    /// 主路径目前接收后丢弃 intent，因此不能据此推断该摘要已成为持久化事件的强制绑定。
    /// `validate_digest()` 当前未发现生产调用者，新增消费者前需确认它在哪个准入边界验证。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": WORKFLOW_DEFINITION_SCHEMA,
            "definition_id": self.definition_id,
            "version": self.version,
            "input_keys": self.input_keys,
            "output_keys": self.output_keys,
            "allowed_roles": self.allowed_roles,
            "max_duration_ms": self.max_duration_ms,
            "max_steps": self.max_steps,
            "nodes": self.nodes,
            "artifacts": self.artifacts,
        }))
    }

    /// 比较外部提供的期望摘要与当前模板重新计算的摘要。
    ///
    /// 不匹配时返回稳定字符串错误 `workflow_definition_digest_mismatch`；函数不查询 registry、
    /// 不读取 EventStore，也不判断调用者是否有权运行该模板。
    pub fn validate_digest(&self, expected: &str) -> Result<(), String> {
        if expected != self.digest() {
            return Err("workflow_definition_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

/// 一个 DAG 节点的配置：依赖关系、执行种类、节点 lease 和可选补偿节点。
///
/// planner 会在定义校验阶段检查依赖图；节点真正运行时，`timeout_ms` 转成该次执行的 lease，
/// 而 `retry_limit` 只限制显式允许的纯节点重试，不代表所有网络/Provider 请求都可重试。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNode {
    /// 必须先成功的节点 ID；决定 DAG 的可执行顺序，不代表并行授权。
    pub dependencies: Vec<String>,
    /// 决定 planner 本地计算、等待人/信号，还是向 Core 返回外部 effect。
    pub kind: WorkflowNodeKind,
    /// 单次节点执行的最长 lease 时长，单位为毫秒；与整个实例 deadline 分开检查。
    pub timeout_ms: u64,
    /// 允许的重试次数，省略旧 JSON 字段时默认为 0。实际可重试节点还受 planner 的种类限制。
    #[serde(default)]
    pub retry_limit: u32,
    /// 该节点失败后可选择执行的补偿节点 ID；这声明关联，不会自动执行补偿。
    pub compensation: Option<String>,
}

/// Workflow 节点的带标签联合类型；Serde 使用 `type` 字段和 snake_case 名称编码具体分支。
///
/// `Literal`、`CopyInput`、`Gate`、`FanOut`、`FanIn` 可在 planner 内做确定性状态计算；
/// `Approval`/`WaitSignal` 进入等待；`AgentTask`/`Capability` 只产生交给 Core 的 effect；
/// `SubWorkflow` 建立另一个实例。枚举成员本身不会授予权限，尤其 Capability 仍需经过 Core 的
/// Company、Policy、Gate、审批和 Broker 检查。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowNodeKind {
    Literal {
        /// 此节点在执行时直接写入实例 outputs 的静态值。
        values: BTreeMap<String, Value>,
    },
    CopyInput {
        /// 输出 key 到输入 key 的映射；缺少来源输入会使 Advance 返回 workflow_input_missing。
        mapping: BTreeMap<String, String>,
    },
    Gate {
        /// 本地比较的输入 key。
        key: String,
        /// 输入必须与此 JSON 值相等；不匹配时节点记为 Failed。
        equals: Value,
    },
    AgentTask {
        /// 目标项目 ID；Core 后续用它建立 Company task/run 上下文。
        project_id: String,
        /// 目标 WorkPacket ID；实际权限和 packet 绑定仍由 Core 检查。
        packet_id: String,
        /// 可选 sandbox profile 名称；None 表示不在 DTO 层指定名称，并不表示绕过 sandbox 策略。
        sandbox: Option<String>,
    },
    Capability {
        /// 请求的能力描述；通过 workflow guard 只代表来源匹配，再继续走常规 capability 审批链。
        request: CapabilityRequest,
    },
    Approval {
        /// 需要回答此节点的角色 ID；实际决定还要求 owner actor 和独立 session 等条件。
        role_id: String,
    },
    WaitSignal {
        /// 等待 Core 已验证的 signal 名称；收到值后写入对应节点输出。
        signal: String,
    },
    FanOut,
    FanIn,
    SubWorkflow {
        /// 被嵌套运行的模板 ID；定义校验要求被引用版本已登记。
        definition_id: String,
        /// 固定子定义版本，避免父实例运行期间被替换模板内容。
        version: u64,
    },
}

/// Artifact 在 workflow 数据契约中的逻辑描述，不代表文件已经创建或存在。
///
/// Planner 校验 generator 节点和依赖关系，并在实例完成时检查 `output_key` 已存在；实际文件、
/// 存储位置和 ArtifactReceipt 由更下游的 artifact 能力处理，本 DTO 不访问文件系统。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowArtifact {
    /// 负责生成此 artifact 的节点 ID。
    pub generated_by: String,
    /// 生成该 artifact 前必须具备的其他 artifact ID。
    pub requires: Vec<String>,
    /// generator 将逻辑 artifact 引用写入实例 outputs 时使用的 key。
    pub output_key: String,
}
/// 一次 workflow 实例的整体阶段；由节点状态汇总或明确的 Pause/Cancel 命令推进。
///
/// `ResultUnknown` 也被 `terminal()` 视为终态，表示自动推进停止并需要保留未知结果事故；它不表示
/// 上游动作肯定没有发生。当前 durable planner 显式使用 Ready/Running/Waiting/Paused、
/// CancelRequested、Retrying、Compensating 及完成/失败状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowInstanceStatus {
    /// 实例已创建，但还没有正在运行或等待的节点。
    Ready,
    /// 至少有一个节点已预留给 Core 执行。
    Running,
    /// 一个或多个 Approval 节点等待符合要求的角色决定。
    WaitingApproval,
    /// 一个或多个 WaitSignal 节点等待命名信号。
    WaitingSignal,
    /// 显式暂停；节点现状保留，外部运行不因此自动取消。
    Paused,
    /// planner 为纯节点创建新尝试后的实例标记。
    Retrying,
    /// 当前实例是仅运行选定 compensation 节点的补偿运行。
    Compensating,
    /// 已提出取消请求，但仍有节点未被确认终止。
    CancelRequested,
    /// 所有选中节点成功，且普通实例满足输出 key 契约。
    Succeeded,
    /// 至少一个节点失败，或实例成功条件不满足。
    Failed,
    /// 取消请求已收尾，所有节点都已终结。
    Cancelled,
    /// 至少一个 effect 的结果未知；自动推进停止，但外部副作用可能已经发生。
    ResultUnknown,
}
impl WorkflowInstanceStatus {
    /// 返回状态是否会被 planner 当作已结束，从而阻止普通后续推进。
    ///
    /// 特别注意：`ResultUnknown` 包含在终态集合内，是为了避免未经人工/新契约判断就自动重发；
    /// 它与“已证明执行结束且没有副作用”不是同一个含义。
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::ResultUnknown
        )
    }
}
/// 一次节点执行的阶段。实例整体状态是对这组节点记录的汇总，不应与节点状态混为一谈。
///
/// `Pending` 在 DTO 中保留，但当前 `kiana-workflow::durable` planner 没有写入它的生产分支；
/// `Reserved` 表示 Core 已记录要派发、但最终运行结果尚待观测。`ResultUnknown` 虽然属于终态，
/// 仍表示外部结果不确定，且当前 observe reducer 会拒绝再次更新它。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeStatus {
    /// 领域状态中表示尚未开始；当前 durable planner 没有生产分支写入它。
    Pending,
    /// 节点已由 planner 预留、等待 Core dispatch 或运行结果观测。
    Reserved,
    /// 节点等待满足角色/会话条件的 Approval 决定。
    WaitingApproval,
    /// 节点等待对应名称的 signal。
    WaitingSignal,
    /// 节点执行事实已确认成功。
    Succeeded,
    /// 节点有确定失败结果；可以按定义和命令规则检查是否允许纯节点 Retry。
    Failed,
    /// 节点已确认取消。
    Cancelled,
    /// 运行结果未知；保留 incident 并阻止自动重试。
    ResultUnknown,
}
impl WorkflowNodeStatus {
    /// 判断此节点状态是否被状态汇总当作已经结束。
    ///
    /// Waiting 与 Reserved 都非终态；Succeeded、Failed、Cancelled 和 ResultUnknown 为终态。
    /// ResultUnknown 进入终态集合是重试保护，并不证明外部 effect 已停止。
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::ResultUnknown
        )
    }
}
/// 一次节点尝试在事件重放状态中的记录，供 Advance、Reconcile、取消与 Receipt 关联。
///
/// planner 在 Advance 为外部节点分配 execution/session 身份并创建 Reserved；Core 执行 effect，
/// 再把 ControlPlaneResponse、证据引用和服务端 receipt 放入 proof，由 observe reducer 填充结果。
/// 这些字段是可重放状态，不代表它们单独就能证明调用者有权限。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowNodeExecution {
    /// 这一次尝试的 RequestId；也是匹配 Core Response 与运行事件的关联键，不是实例 ID。
    pub execution_id: RequestId,
    /// 创建/执行此尝试时绑定的 session，用于和审批者 session 做独立性检查。
    pub session_id: SessionId,
    /// 该节点第几次尝试；与实例的 retry_counts 配合区分显式重试。
    pub attempt: u32,
    /// 该次尝试的状态，不是整个 WorkflowInstance 的汇总状态。
    pub status: WorkflowNodeStatus,
    /// planner/Core 记录的尝试开始时间，Unix 毫秒时间戳。
    pub started_at: u64,
    /// 当前节点 lease 截止时间，超过后 Decide/Signal 会被拒绝；它不会自动终结等待节点。
    pub lease_expires_at: u64,
    /// 终态被记录时的时间；仍运行或等待时为空。其具体来源由更新节点状态的调用方决定。
    pub ended_at: Option<u64>,
    /// 启动节点时对合并输入的内容指纹，用于计划/回执关联，不是安全签名。
    pub input_digest: String,
    /// 该尝试产生的 JSON 输出；尚无已记录输出时仍有占位值，需结合 output_recorded 判断。
    pub output: Value,
    /// output 的内容摘要；默认值兼容较早的事件形状。
    #[serde(default)]
    pub output_digest: String,
    /// 显式区分“空 JSON 输出已记录”与“目前没有输出记录”。
    #[serde(default)]
    pub output_recorded: bool,
    /// 稳定错误码或 Core response error；为空表示没有记录该类错误。
    pub error_code: Option<String>,
    /// 指向经 Core 验证的事件事实；planner 校验 proof 引用，但不自行读取事件库。
    pub evidence_refs: Vec<String>,
    /// Core 派生的运行 receipt，说明运行结果事实；业务结果是否验收是另一层判断。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_receipt: Option<crate::RuntimeReceiptRef>,
    /// ResultUnknown 时关联的 incident ID；仅保存 ID 不代表 incident 已被解决。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    /// SubWorkflow 节点创建的子实例 ID；非子 workflow 节点时通常为空。
    pub child_instance_id: Option<String>,
}

/// 某个 WorkflowDefinition 的一次持久运行及其当前可重放投影。
///
/// 输入在启动时被冻结；每次 Advance/观测把节点记录和输出推进；Core 将命令事实写入 EventStore，
/// 后续由 automation replay 重新折叠得到同样实例。父子 ID 用于嵌套 workflow，trigger ID 标记
/// 自动触发来源；它不是执行权或 EventStore stream cursor。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInstance {
    /// 一次运行的稳定 ID；不同于模板 definition_id 和单节点 execution_id。
    pub instance_id: String,
    /// 此实例引用的不可变模板逻辑 ID。
    pub definition_id: String,
    /// 实例固定使用的模板版本，重放时不可换成最新版本。
    pub definition_version: u64,
    /// 创建实例时绑定的 actor ID；后续命令仍需匹配它。
    pub owner_id: String,
    /// 启动时的 role ID 快照；planner 按定义角色和命令类型校验。
    pub role_id: String,
    /// 实例创建时刻，Unix 毫秒。
    pub created_at: u64,
    /// 整体运行 deadline，通常为 created_at + definition.max_duration_ms。
    pub deadline: u64,
    /// 调用者在 Start/trigger 时提交并验证必需 key 的输入。
    pub inputs: BTreeMap<String, Value>,
    /// 已提交节点产生的共享输出；后续节点输入由 inputs 与 outputs 合并形成。
    pub outputs: BTreeMap<String, Value>,
    /// 由 refresh 汇总，或由显式 Pause/Cancel 分支暂时改变的实例整体状态。
    pub status: WorkflowInstanceStatus,
    /// 已消耗的节点步骤计数，用于和 definition.max_steps 比较。
    pub steps_used: u32,
    /// 实例层稳定失败原因；为空表示没有实例级错误记录。
    #[serde(default)]
    pub error_code: Option<String>,
    /// 按节点 ID 保存的尝试/结果事实；不同节点可有不同 session、lease 和 evidence。
    pub nodes: BTreeMap<String, WorkflowNodeExecution>,
    /// 已消费的信号值。相同 signal key 只能存一次，可在 WaitSignal 节点启动前先写入。
    pub signals: BTreeMap<String, Value>,
    /// 创建本实例的 DurableTrigger ID；手动 Start 通常为空。
    pub trigger_id: Option<String>,
    /// 创建本实例的父 workflow ID；用于 subworkflow 和祖先取消检查。
    pub parent_instance_id: Option<String>,
    /// 从根实例起算的 subworkflow 深度；`serde(default)` 用于兼容旧事件。
    #[serde(default)]
    pub depth: u32,
    /// 补偿实例仅运行此列表里的节点；None 表示普通实例执行非补偿节点。
    #[serde(default)]
    pub selected_nodes: Option<Vec<String>>,
    /// 每个节点的显式纯计算 retry 计数；key 是 definition node ID。
    #[serde(default)]
    pub retry_counts: BTreeMap<String, u32>,
}
/// Trigger 有活动实例时，如何处理新 occurrence。
///
/// Reject 返回冲突；Queue 保存等待项；Coalesce 保留最新等待项；Replace 标记旧实例取消并排队。
/// 这些是 planner 的状态策略，特别是 Replace 是否实际发送取消，还取决于 Core 收到的 effect。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerConcurrency {
    /// 已有未终结实例时拒绝新 occurrence。
    Reject,
    /// 把新 occurrence 加到有界 pending 队列，待现有实例结束后处理。
    Queue,
    /// 请求替换现有实例并排队新 occurrence；实际取消链路还需看 Core effect，当前 planner 有缺口。
    Replace,
    /// 清掉先前 pending occurrence，只保留最新到来的 occurrence。
    Coalesce,
}

/// interval tick 晚到并积累多个到期时间点时，如何处理错过的 occurrence。
///
/// Skip 只在恰有一个到期项时发出它；FireOnce 把多个错过项折为一个；CatchUp 分批发出并保留
/// backlog。精确计算见 `plan_interval_due()`，此枚举本身不创建 timer。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedSchedulePolicy {
    /// 到期项恰好一个时发出；积压超过一个时全部跳过。
    Skip,
    /// 有一个或多个到期项时折成一次 firing，并越过所有已错过时间点。
    FireOnce,
    /// 每 tick 至多排出 32 项，剩余已到期项保留在 cursor backlog。
    CatchUp,
}

/// 单次 interval tick 最多排出的 CatchUp occurrence 数，剩余项留给后续 tick 继续消费。
pub const MAX_INTERVAL_CATCH_UP: u64 = 32;

/// 一次 interval 计算的结果，供 durable trigger 保存游标和审计本次跳过/积压情况。
///
/// `next_at` 是下一个尚未消费的计划时刻；CatchUp 达到单批上限时它仍可能早于或等于 now，
/// 后续 Tick 可从该处继续。字段全部由确定性计算产生，不含 I/O 或 wall clock。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntervalDueBatch {
    /// 本次实际排出的 occurrence key，格式包含准确的计划时间戳。
    pub occurrence_keys: Vec<String>,
    /// 计算后的游标位置；Skip/FireOnce 会越过所有已到期项，CatchUp 只越过实际发出的项。
    pub next_at: u64,
    /// 从输入游标到 now 一共到期的计划次数。
    pub total_due: u64,
    /// 本批次实际排出的次数。
    pub emitted: u64,
    /// Skip/FireOnce 主动跳过的次数；CatchUp 不把未处理 backlog 记成 skipped。
    pub skipped: u64,
    /// 仍可由后续 Tick 处理的到期数量。
    pub backlog_remaining: u64,
    /// CatchUp 因单批 32 项上限而仍有 backlog 时为 true。
    pub catch_up_limited: bool,
}

/// 根据持久游标与调用方提供的当前时间，纯计算一次 interval trigger tick 的 occurrence 批次。
///
/// 【调用者】`kiana-workflow::durable::fire_trigger()` 在显式 Tick 命令中调用；生产 host 是否
/// 定时提交 Tick 是更外层调度职责，本函数不读取系统时钟，也不启动后台任务。
/// 【输入】`next_at` 为下一个计划时刻，`every_ms` 为间隔，`now_ms` 是 Core authority 的当前时间，
/// `policy` 决定漏 tick 后如何处理。
/// 【输出】每个 key 带确切计划时间，结果同时报告总到期、已发出、跳过和积压数。
/// 【失败】时间为 0、间隔少于 1000 毫秒或任何游标乘加溢出时返回错误；调用方因此不会保存回绕
/// 后的错误游标，也不会误把未来某一时刻当成下一次 occurrence。
///
/// Skip 在仅有一项到期时会发这一项，超过一项则一项都不发并跳过全部；FireOnce 始终最多发一项；
/// CatchUp 单次最多发 `MAX_INTERVAL_CATCH_UP` 项并留下 backlog。这个行为由代码明确决定，不能
/// 只凭策略名字推断出“跳过当前所有欠账”或“无限追赶”。
pub fn plan_interval_due(
    next_at: u64,
    every_ms: u64,
    now_ms: u64,
    policy: MissedSchedulePolicy,
) -> Result<IntervalDueBatch, String> {
    if next_at == 0 || every_ms < 1_000 || now_ms == 0 {
        return Err("trigger_interval_cursor_invalid".to_owned());
    }
    if now_ms < next_at {
        return Ok(IntervalDueBatch {
            occurrence_keys: Vec::new(),
            next_at,
            total_due: 0,
            emitted: 0,
            skipped: 0,
            backlog_remaining: 0,
            catch_up_limited: false,
        });
    }
    let total_due = (now_ms - next_at)
        .checked_div(every_ms)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())?;
    let emitted = match policy {
        MissedSchedulePolicy::Skip if total_due > 1 => 0,
        MissedSchedulePolicy::Skip | MissedSchedulePolicy::FireOnce => 1,
        MissedSchedulePolicy::CatchUp => total_due.min(MAX_INTERVAL_CATCH_UP),
    };
    let advance = match policy {
        MissedSchedulePolicy::CatchUp => emitted,
        MissedSchedulePolicy::Skip | MissedSchedulePolicy::FireOnce => total_due,
    };
    let first_due = next_at;
    let next_at = next_at
        .checked_add(
            every_ms
                .checked_mul(advance)
                .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())?,
        )
        .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())?;
    let occurrence_keys = (0..emitted)
        .map(|offset| {
            let step = every_ms
                .checked_mul(offset)
                .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())?;
            first_due
                .checked_add(step)
                .map(|scheduled_at| format!("schedule:{scheduled_at}"))
                .ok_or_else(|| "trigger_interval_cursor_overflow".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let skipped = match policy {
        MissedSchedulePolicy::Skip | MissedSchedulePolicy::FireOnce => total_due - emitted,
        MissedSchedulePolicy::CatchUp => 0,
    };
    let backlog_remaining = match policy {
        MissedSchedulePolicy::CatchUp => total_due - emitted,
        MissedSchedulePolicy::Skip | MissedSchedulePolicy::FireOnce => 0,
    };
    Ok(IntervalDueBatch {
        occurrence_keys,
        next_at,
        total_due,
        emitted,
        skipped,
        backlog_remaining,
        catch_up_limited: policy == MissedSchedulePolicy::CatchUp && backlog_remaining > 0,
    })
}

/// 决定 durable trigger occurrence 来自哪里；Interval 只描述时间参数，不是正在运行的 timer。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerSchedule {
    /// 只允许显式 Fire 命令产生 occurrence。
    Manual,
    /// 由调用方提供 event proof 的事件种类触发；仅有 kind 文本本身不能证明事件存在。
    Event { kind: String },
    /// 以 Unix 毫秒绝对时间 `first_at` 开始，之后每隔 `every_ms` 计算一次。
    Interval { every_ms: u64, first_at: u64 },
}

/// 将一个已批准的触发来源绑定到固定模板版本、固定输入和并发/过期策略。
///
/// Core 注册时除这里的 shape 检查外，还要校验 owner、approval event、role 和模板版本；触发器
/// 之后创建的 workflow 实例复用保存的输入和身份。领域 DTO 不能自行启动 workflow 或证明 approval。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerDefinition {
    /// trigger 的稳定 ID，也是 AutomationState.triggers 中的 key。
    pub trigger_id: String,
    /// 被创建实例使用的 workflow 模板 ID。
    pub definition_id: String,
    /// 被创建实例固定使用的模板版本。
    pub definition_version: u64,
    /// 每次 firing 都复制到新实例的固定输入；不是由 firing event 任意追加的输入。
    pub inputs: BTreeMap<String, Value>,
    /// 创建 trigger 的 actor；后续 Tick/Fire 也必须以该 owner 操作。
    pub owner_id: String,
    /// 允许该 trigger 操作/执行时使用的 role 快照。
    pub role_id: String,
    /// trigger 可接受新 occurrence 的截止时间，Unix 毫秒。
    pub expires_at: u64,
    /// trigger 生命周期内最多启动多少个 workflow 实例。
    pub max_firings: u32,
    /// 有实例运行时的新 occurrence 策略。
    pub concurrency: TriggerConcurrency,
    /// interval tick 欠账的处理策略；非 interval trigger 不会用到此值。
    pub missed_schedule: MissedSchedulePolicy,
    /// occurrence 来源及其参数。
    pub schedule: TriggerSchedule,
    /// 经批准事件的引用；形状检查只要求 `event:` 前缀，真实性/归属由 Core 查询验证。
    pub approval_ref: String,
}

impl TriggerDefinition {
    /// 检查 trigger DTO 自身的字段边界；不查询模板、项目、EventStore 或当前用户权限。
    ///
    /// 【调用者】Core 在登记前调用；随后 planner/Core 还要核实 owner/role、审批事件和 definition。
    /// 【范围】检查 ID 文本、approval 引用形状、过期时间最多 365 天、firing 数为 1..=1024、
    /// 输入项最多 256 个且序列化后不超过 64 KiB，并检查 schedule 的基本数值/文本。
    /// 文本长度使用 `str::len()`，单位是 UTF-8 字节。
    ///
    /// 注意：Interval 的 `first_at` 只要求大于 0，本函数没有要求它早于 `expires_at`；若首个时刻
    /// 已在有效期之后，shape 检查仍可能通过，但正常触发时会因 expires_at 被拒绝。
    pub fn validate_shape(&self, now_ms: u64) -> Result<(), String> {
        fn bounded(value: &str, max: usize) -> bool {
            !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
        }
        if !bounded(&self.trigger_id, 128)
            || !bounded(&self.definition_id, 128)
            || self.definition_version == 0
            || !bounded(&self.owner_id, 256)
            || !bounded(&self.role_id, 128)
            || !bounded(&self.approval_ref, 512)
            || !self.approval_ref.starts_with("event:")
            || now_ms == 0
            || self.expires_at <= now_ms
            || self.expires_at - now_ms > 31_536_000_000
            || !(1..=1_024).contains(&self.max_firings)
            || self.inputs.len() > 256
            || serde_json::to_vec(&self.inputs).map_or(true, |bytes| bytes.len() > 64 * 1024)
        {
            return Err("trigger_definition_shape_invalid".to_owned());
        }
        for key in self.inputs.keys() {
            if !bounded(key, 128) {
                return Err("trigger_input_key_invalid".to_owned());
            }
        }
        match &self.schedule {
            TriggerSchedule::Manual => {}
            TriggerSchedule::Event { kind } => {
                if !bounded(kind, 128) {
                    return Err("trigger_event_kind_invalid".to_owned());
                }
            }
            TriggerSchedule::Interval { every_ms, first_at } => {
                if *every_ms < 1_000 || *first_at == 0 {
                    return Err("trigger_schedule_invalid".to_owned());
                }
                self.expires_at
                    .checked_add(*every_ms)
                    .ok_or_else(|| "trigger_schedule_overflow".to_owned())?;
            }
        }
        Ok(())
    }

    /// 对 trigger 的所有影响行为字段生成 SHA-256 内容摘要；这只是身份指纹，不是审批签名。
    /// 当前生产调用点尚未发现对该 digest 的验证，不能把它当成 trigger 注册已校验的证明。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": TRIGGER_DEFINITION_SCHEMA,
            "trigger_id": self.trigger_id,
            "definition_id": self.definition_id,
            "definition_version": self.definition_version,
            "inputs": self.inputs,
            "owner_id": self.owner_id,
            "role_id": self.role_id,
            "expires_at": self.expires_at,
            "max_firings": self.max_firings,
            "concurrency": self.concurrency,
            "missed_schedule": self.missed_schedule,
            "schedule": self.schedule,
            "approval_ref": self.approval_ref,
        }))
    }
}
/// DurableTrigger 是 trigger 的可重放运行态；与不可变的 TriggerDefinition 分开保存。
///
/// Core 通过事件折叠恢复这些游标和去重表。`enabled=false` 会阻止新 occurrence；pending 队列是
/// 已接收但因并发策略尚未创建实例的 key；fired 记录 key 到实例 ID 的映射，防止重放重复创建。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DurableTrigger {
    /// 触发配置快照；注册后运行状态依此固定模板、输入、owner 和策略。
    pub definition: TriggerDefinition,
    /// 是否接受新的 Tick/Fire；false 不会自动取消已经启动的实例。
    pub enabled: bool,
    /// Interval 的下一个计划毫秒时间；Manual/Event schedule 没有时间游标时为空。
    pub next_at: Option<u64>,
    /// 已创建实例的次数，用来分配后续 trigger instance ID 并检查 firing budget。
    pub firings_used: u32,
    /// 因 Queue/Coalesce/Replace 尚未启动的 occurrence key，顺序即后续处理顺序。
    pub pending_keys: Vec<String>,
    /// 每个 pending key 对应的来源摘要；缺少历史摘要时按默认空表读取。
    #[serde(default)]
    pub pending_digests: BTreeMap<String, String>,
    /// 已消费 occurrence key 到已创建 instance ID 的映射，是重放幂等的重要状态。
    pub fired: BTreeMap<String, String>,
    /// 每个已 fire key 的来源摘要，用于发现同 key 被不同事件内容复用。
    #[serde(default)]
    pub fired_digests: BTreeMap<String, String>,
}

/// 一次交给 Core 的版本化 workflow 命令请求。
///
/// 入口把通用命令 JSON 解码为该类型；Core 将 `expected_revision` 与事件折叠出的当前 revision
/// 比较，并用 idempotency key 防止重放相同提交重复产生 effect，随后由 `command` 决定转换。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCommandRequest {
    /// 必须匹配 `AUTOMATION_SCHEMA`；错误版本会在 Core command handler 被拒绝。
    pub schema: String,
    /// 调用者观察到的状态 revision，用于乐观并发控制；过期时不能覆盖并发提交的新状态。
    pub expected_revision: u64,
    /// 请求去重身份；Core 将它与 workflow stream 事实绑定，避免同一请求重复执行 effect。
    pub idempotency_key: String,
    /// 这次希望对 workflow/trigger 执行的领域操作。
    pub command: AutomationCommand,
}

/// durable workflow 对外表达的领域操作；实际请求还要被包进 AutomationCommandRequest。
///
/// Serde 的内部标签 `type` 与 `deny_unknown_fields` 让 wire 载荷明确表示一个且仅一个操作。
/// Core 会拒绝外部提供的 RecordObservation，因为运行结果必须来自 ControlPlane 内部 proof。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationCommand {
    /// 登记一个不可变的模板版本；同一 `(definition_id, version)` 不能被覆盖。
    RegisterDefinition { definition: WorkflowDefinition },
    /// 基于已登记模板创建实例；instance_id 由调用者给出，inputs 随实例冻结。
    Start {
        instance_id: String,
        definition_id: String,
        version: u64,
        inputs: BTreeMap<String, Value>,
    },
    /// 按依赖与节点状态选取并推进一个节点；外部节点会产生 effect 供 Core 在提交后执行。
    Advance { instance_id: String },
    /// 让 Core 重新从 Company/Capability 等事实来源构造 proof，再尝试收敛指定节点状态。
    Reconcile {
        instance_id: String,
        node_id: String,
    },
    /// 由符合定义角色和独立 session 的 owner 决定 Approval 节点，并引用已验证事件。
    Decide {
        instance_id: String,
        node_id: String,
        approve: bool,
        evidence_ref: String,
    },
    /// 记录经 Core proof 校验的命名信号；可唤醒所有等待同名 signal 的节点。
    Signal {
        instance_id: String,
        signal: String,
        value: Value,
        evidence_ref: String,
    },
    /// 暂停此实例的后续推进；不意味着已经运行的外部 effect 被停止。
    Pause { instance_id: String },
    /// 请求恢复暂停实例；当前 planner 对非终态实例仍会经 refresh 保持 Paused，见 durable.rs 风险注释。
    Resume { instance_id: String },
    /// 请求取消实例和真实嵌套 descendants；外部动作是否停止仍需 Core 取消和 Reconcile 事实。
    Cancel { instance_id: String, reason: String },
    /// 为已知失败且属于允许类型的节点建立一次新尝试；不是通用 HTTP/provider retry。
    Retry {
        instance_id: String,
        node_id: String,
    },
    /// 为已知失败/取消的源实例创建独立补偿实例，不把原实例改写成成功。
    Compensate {
        instance_id: String,
        compensation_instance_id: String,
    },
    /// 登记有 owner、批准引用和有限 budget 的自动启动配置。
    RegisterTrigger { trigger: TriggerDefinition },
    /// 禁止 trigger 接受新 occurrence，并清空未启动的排队项；不取消现有实例。
    DisableTrigger { trigger_id: String },
    /// 用 Core 提供的 now_ms 推进 interval 游标并处理 pending occurrences。
    Tick { trigger_id: String },
    /// 提交一个手动或事件 schedule occurrence；event schedule 必须同时提供可验证 event_ref。
    Fire {
        trigger_id: String,
        firing_key: String,
        event_ref: Option<String>,
    },
    /// 仅 ControlPlane 内部使用的观测命令；外部 wire caller 不得直接提交运行结果。
    ///
    /// 如果允许请求者自行构造此分支，就能伪造 AgentTask/Capability 的 Completed 或 Failed 事实，
    /// 因此 Core handler 会单独拒绝来自外部 command 的该 variant。
    RecordObservation {
        instance_id: String,
        node_id: String,
    },
}
impl AutomationCommand {
    /// 返回明确绑定到单个实例的命令所携带的实例 ID。
    ///
    /// RegisterDefinition、Start、RegisterTrigger、DisableTrigger、Tick 和 Fire 属于模板/trigger
    /// 范围而不是现有实例范围，所以返回 None。当前实现也因此无法从 Start command 推导出新建的
    /// instance_id；`durable::derive_plan_intent()` 的 Start intent 缺失实例身份已单独注明。
    pub fn instance_id(&self) -> Option<&str> {
        match self {
            Self::Advance { instance_id }
            | Self::Reconcile { instance_id, .. }
            | Self::Decide { instance_id, .. }
            | Self::Signal { instance_id, .. }
            | Self::Pause { instance_id }
            | Self::Resume { instance_id }
            | Self::Cancel { instance_id, .. }
            | Self::Retry { instance_id, .. }
            | Self::Compensate { instance_id, .. }
            | Self::RecordObservation { instance_id, .. } => Some(instance_id),
            _ => None,
        }
    }
}
/// EventStore 中 workflow command 事件按序折叠出的完整状态快照。
///
/// 这是可丢弃的重放结果，不是第二个独立事实源；事实仍是 AutomationEvent。每条新命令先在副本
/// 上规划，然后 Core 以 expected revision CAS 追加事件，下一次读取再由事件重建本结构。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AutomationState {
    /// 此聚合的状态 revision；每次成功 planner 转换只增加一次，与底层所有 EventStore 流的
    /// 全局 cursor 不是同一个计数器。
    pub revision: u64,
    /// 按 `definition_id@version` 索引的不可变模板。
    pub definitions: BTreeMap<String, WorkflowDefinition>,
    /// 按 instance_id 索引的运行实例投影。
    pub instances: BTreeMap<String, WorkflowInstance>,
    /// 按 trigger_id 索引的定义和运行游标。
    pub triggers: BTreeMap<String, DurableTrigger>,
    /// ResultUnknown 运行结果对应的 incident 事实；它保留待人工/恢复处理的问题，不等于已解决。
    #[serde(default)]
    pub incidents: BTreeMap<String, crate::WorkflowIncident>,
}

/// planner 每次转换使用的 Core 身份与时间快照。
///
/// 它从请求上下文和 ControlPlane 生成；`now_ms` 被显式传入，使 planner 不读时钟而保持确定性。
/// execution_id/session_id 供本次新节点尝试绑定身份，不是用户可自行选择的权限凭证。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationAuthority {
    /// 当前 command 经入口/Core 解析后的 actor、role、project 与 session 上下文。
    pub context: RequestContext,
    /// Core 在本次转换采用的 Unix 毫秒时间快照。
    pub now_ms: u64,
    /// Core 为本次执行尝试分配的关联 ID。
    pub execution_id: RequestId,
    /// 本次调用实际关联的 session；用于独立审批和运行身份校验。
    pub session_id: SessionId,
}

/// Core 为 planner 整理的执行/事件证明材料；它本身不负责读取或认证这些来源。
///
/// planner 根据响应、引用、receipt 和 incident 做结构及 ID 一致性检查；由谁验证引用确实来自
/// 当前 EventStore/Company 状态，是 Core 调用前的职责。默认值支持只含 command 身份的纯转换。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AutomationProof {
    /// 外部 AgentTask/Capability 的 ControlPlane 响应；本地 Literal/Gate 转换通常不需要它。
    pub response: Option<CoreResponse>,
    /// 响应或批准/信号所依赖的事件 ID 列表；这里只保存引用，不是事件内容。
    pub evidence_refs: Vec<String>,
    /// Event schedule 的来源事件类型，须与 trigger 要求匹配。
    pub event_kind: Option<String>,
    /// ControlPlane 为终态运行观测生成的 receipt；不能由任意调用者当作批准凭证。
    #[serde(default)]
    pub runtime_receipt: Option<crate::RuntimeReceiptRef>,
    /// 运行结果未知时必须提供的 incident 详情，并绑定实例/节点/execution/evidence。
    #[serde(default)]
    pub incident: Option<crate::WorkflowIncident>,
}

/// 为已提交 workflow command 事实附加聚合、并发、幂等和摘要绑定信息。
///
/// RuntimeEvent 仍是事实来源，envelope 只是同一载荷内显式保存关联元数据，方便 replay 时交叉
/// 校验；它不替代 EventStore CAS，也不自行查询存储。当前构造把 `source_cursor` 设成聚合
/// `stream_version`，不是全局事件 cursor。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationEventEnvelope {
    /// 必须等于当前 envelope schema 常量。
    pub schema: String,
    /// 聚合类型，当前固定为 `workflow`。
    pub aggregate_type: String,
    /// 此次命令作用的 workflow 聚合 ID。
    pub aggregate_id: String,
    /// workflow 聚合内的版本号，用于验证 expected revision/CAS 顺序。
    pub stream_version: u64,
    /// 来源 cursor；当前契约要求等于 stream_version，不代表 EventStore 全局排序游标。
    pub source_cursor: u64,
    /// 与 RuntimeEvent/request 关联的 ControlPlane 请求 ID。
    pub request_id: RequestId,
    /// 命令去重 key，须与外层 AutomationCommandRequest 一致。
    pub idempotency_key: String,
    /// 规范化 command 内容的 `sha256:` 摘要。
    pub command_digest: String,
    /// 同一事件中 proof/payload 内容的 `sha256:` 摘要。
    pub payload_digest: String,
    /// 对 envelope 其余绑定字段重新计算的 `sha256:` 摘要；不是数字签名。
    pub envelope_digest: String,
}

impl AutomationEventEnvelope {
    /// 创建 workflow envelope，计算自身摘要并立即执行字段/摘要格式检查。
    ///
    /// `source_cursor` 在当前格式中直接取 `stream_version`。如果后续需要表达全局 cursor，必须
    /// 一并调整 schema/校验和所有生产者消费者，不能只改变该字段的解释。
    pub fn new(
        aggregate_id: impl Into<String>,
        stream_version: u64,
        request_id: RequestId,
        idempotency_key: impl Into<String>,
        command_digest: impl Into<String>,
        payload_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut envelope = Self {
            schema: AUTOMATION_EVENT_ENVELOPE_SCHEMA.to_owned(),
            aggregate_type: "workflow".to_owned(),
            aggregate_id: aggregate_id.into(),
            stream_version,
            source_cursor: stream_version,
            request_id,
            idempotency_key: idempotency_key.into(),
            command_digest: command_digest.into(),
            payload_digest: payload_digest.into(),
            envelope_digest: String::new(),
        };
        envelope.envelope_digest = envelope.digest();
        envelope.validate()?;
        Ok(envelope)
    }

    /// 检查 envelope 的 schema、身份、版本、文本边界和摘要形状/一致性。
    ///
    /// 这里仅验证载荷内部自洽；哈希没有密钥，无法鉴别伪造者，也不能证明 envelope 真从
    /// EventStore 读取。实际事件与聚合绑定还要由 `validate_against()` 或 Core replay 完成。
    pub fn validate(&self) -> Result<(), String> {
        fn digest(value: &str) -> bool {
            value
                .strip_prefix("sha256:")
                .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        }
        if self.schema != AUTOMATION_EVENT_ENVELOPE_SCHEMA
            || self.aggregate_type != "workflow"
            || self.aggregate_id.trim().is_empty()
            || self.aggregate_id.len() > 512
            || self.stream_version == 0
            || self.source_cursor != self.stream_version
            || self.request_id.as_uuid().is_nil()
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.len() > 256
            || !digest(&self.command_digest)
            || !digest(&self.payload_digest)
            || !digest(&self.envelope_digest)
            || self.envelope_digest != self.digest()
        {
            return Err("automation_event_envelope_invalid".to_owned());
        }
        Ok(())
    }

    /// 同时验证 envelope 自身完整性和它是否绑定到调用方提供的事件字段。
    ///
    /// 任一 aggregate/version/request/idempotency/command/payload 字段不同都会返回
    /// `automation_event_envelope_binding_mismatch`，以免拿一份格式正确的 envelope 附到另一事件。
    pub fn validate_against(
        &self,
        aggregate_id: &str,
        stream_version: u64,
        request_id: RequestId,
        idempotency_key: &str,
        command_digest: &str,
        payload_digest: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.aggregate_id != aggregate_id
            || self.stream_version != stream_version
            || self.request_id != request_id
            || self.idempotency_key != idempotency_key
            || self.command_digest != command_digest
            || self.payload_digest != payload_digest
        {
            return Err("automation_event_envelope_binding_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对固定字段计算 envelope 摘要；摘要不包含 `envelope_digest` 本身，避免自引用。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "aggregate_type": self.aggregate_type,
            "aggregate_id": self.aggregate_id,
            "stream_version": self.stream_version,
            "source_cursor": self.source_cursor,
            "request_id": self.request_id,
            "idempotency_key": self.idempotency_key,
            "command_digest": self.command_digest,
            "payload_digest": self.payload_digest,
        }))
    }
}

/// Core 为 planner 输入及转换结果形成的一条可重放命令事实载荷。
///
/// 它保存原始命令请求、生成时的身份/时间 authority、Core 验证后的 proof，以及可选 envelope。
/// 历史事件有可能没有 envelope，所以缺省值兼容旧形状；有 envelope 时 Core replay 再校验绑定。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationEvent {
    /// 必须为 `AUTOMATION_EVENT_SCHEMA`。
    pub schema: String,
    /// 触发状态变化的版本化请求。
    pub request: AutomationCommandRequest,
    /// 当时使用的 owner/session/role 和时间快照，供确定性重放。
    pub authority: AutomationAuthority,
    /// 当时使用的事件/运行结果证据快照。
    pub proof: AutomationProof,
    /// 新事件可附带的跨字段绑定；旧事件允许省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<AutomationEventEnvelope>,
}

/// planner 返回给 Core 的进程内待办描述；Core 先提交 workflow 事实，再按 effect 类型调用既有入口。
///
/// 此 enum 没有 Serialize/Deserialize，当前不作为外部 wire 请求或 durable 事件载荷。Dispatch 的
/// `kind` 仍是 WorkflowNodeKind 配置，Core 会按 AgentTask/Capability 等分支处理；Cancel 也需要
/// 经过 Core 取消路径。单独构造/持有 effect 不会获得执行许可。
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowEffect {
    /// 请求 Core 尝试运行一个已预留的节点。
    Dispatch {
        /// workflow 实例身份。
        instance_id: String,
        /// 模板中的节点身份。
        node_id: String,
        /// 该节点的不可变执行配置。
        kind: WorkflowNodeKind,
        /// 此次具体运行尝试的关联 ID。
        execution_id: RequestId,
        /// 运行时要绑定的 session 身份。
        session_id: SessionId,
    },
    /// 请求 Core 对该实例及其受管理运行/待审批操作传播取消。
    Cancel { instance_id: String },
}

/// `WorkflowPlanIntent.kind` 对本次规划结果的有限分类，不代表运行时节点状态本身。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowIntentKind {
    /// Planner 返回了需要 Core 尝试执行的 Dispatch effect。
    Dispatch,
    /// 节点已写成 reservation，但 effect 不由该摘要本身执行。
    Reserve,
    /// 状态转换进入人工审批或信号等待。
    Wait,
    /// 实例进入终态。
    Terminal,
    /// planner 请求 Core 传播取消。
    Cancel,
    /// 未从本命令状态变化推导出更具体类别。
    Noop,
}

/// 纯 planner 的可序列化结果摘要，用于队列/诊断等边界传递“基于什么快照规划了什么”。
///
/// 它是 projection/intent 而非事实、批准或执行许可。Core 仍须以 CAS 持久化命令事实，并在已有
/// 权限链后消费 WorkflowEffect。摘要绑定输入/定义/authority 快照，但没有密钥，不能证明来源可信。
/// 当前 Core 主命令路径计算后未使用这个 intent；且 `Start` 因 `instance_id()` 返回 None，生成的
/// Start intent 无法准确绑定新实例，需结合其后续真实消费者理解。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPlanIntent {
    /// 必须匹配 `WORKFLOW_PLAN_INTENT_SCHEMA`。
    pub schema: String,
    /// 本次变化是派发、预留、等待、终态、取消还是没有可识别动作。
    pub kind: WorkflowIntentKind,
    /// 受影响 workflow instance；无 effect 命令可能根据 command 推导，Start 当前可能为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// 受影响节点；对新建 reservation 通常存在，对实例级命令可为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// 规划前 AutomationState.revision，用于让下游知道使用了哪个快照。
    pub expected_revision: u64,
    /// 规划后 revision，正常转换比 expected_revision 大 1。
    pub next_revision: u64,
    /// 本次实例引用 definition 的内容摘要；非实例级命令可为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_digest: Option<String>,
    /// 若可定位到节点，则记录它使用的输入摘要；不是输入值本身。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_digest: Option<String>,
    /// 供 queue 侧去重/关联的 workflow-instance/node/revision 字符串，不是授权 token。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_key: Option<String>,
    /// 规划所用 actor/session/role/project/time 快照的摘要。
    pub authority_digest: String,
    /// 对 intent 其他字段重新计算的完整内容摘要。
    pub intent_digest: String,
}

impl WorkflowPlanIntent {
    /// 检查 schema、revision 单步递增和摘要字段形状/一致性。
    ///
    /// 它不访问 AutomationState、EventStore 或 authority provider，因此只说明 intent 自洽，不说明
    /// expected_revision 仍是当前版本，也不说明用户有权执行其中描述的 action。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_PLAN_INTENT_SCHEMA
            || self.next_revision != self.expected_revision.saturating_add(1)
            || self.expected_revision == u64::MAX
            || self.authority_digest.strip_prefix("sha256:").is_none()
            || self.authority_digest.len() != 71
            || !self.authority_digest[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.intent_digest != self.digest()
        {
            return Err("workflow_plan_intent_invalid".to_owned());
        }
        Ok(())
    }

    /// 重新计算 intent 自身摘要；摘要字段本身不包含在被摘要内容中。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "kind": self.kind,
            "instance_id": self.instance_id,
            "node_id": self.node_id,
            "expected_revision": self.expected_revision,
            "next_revision": self.next_revision,
            "definition_digest": self.definition_digest,
            "input_digest": self.input_digest,
            "queue_key": self.queue_key,
            "authority_digest": self.authority_digest,
        }))
    }
}

/// Workflow JSON 值的领域别名，帮助读者识别 inputs/outputs/signals 使用统一 JSON 值承载。
pub type WorkflowValue = serde_json::Value;

/// 对按 key 排序的 JSON inputs 生成确定性 FNV-1a 64 位内容指纹。
///
/// 该值适用于 replay identity/内容关联，不是 SHA-256，也不是签名或抗碰撞证明；不能用于授权、
/// secret 完整性校验或确认调用者身份。键顺序由 BTreeMap 固定，所以相同映射可稳定序列化。
pub fn workflow_input_digest(inputs: &BTreeMap<String, Value>) -> String {
    let bytes = serde_json::to_vec(inputs).unwrap_or_default();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}
