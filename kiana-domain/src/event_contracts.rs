//! # 文件职责
//!
//! 本文件登记运行事件（`RuntimeEvent`）的事件类型、版本、必需 ID、允许字段、终态属性和
//! 旧格式迁移规则。事件载荷是事件日志中的持久数据；这里的登记表是服务端解释这些数据
//! 的权威契约，防止不同生产者和投影器各自猜测字段含义。
//!
//! # 在系统中的位置与数据流
//!
//! ```text
//! ControlPlane / Runner / 其他事件生产者
//!   -> RuntimeEvent（身份字段 + data 载荷）
//!   -> EventStore::validate_event_for_storage()
//!      -> stream 元数据检查
//!      -> 现代格式（带聚合/版本元数据且 data 是对象）调用本文件 validate_runtime_event()
//!      -> 后续再检查身份关联、审计、治理和秘密信息
//!   -> 所有写入边界通过后才可进入持久事实日志；任何失败都会阻止追加
//!
//! 读取旧事件 -> resolve_event_payload()
//!   -> Current：按当前契约校验后的载荷
//!   -> Migrated：经具名迁移器解释的旧载荷（原始事件字节不被改写）
//!   -> Opaque：可检查但不能进入权威/执行投影的未知可选事件
//! ```
//!
//! `EventKindSpec` 把每种事件与所属聚合、必需 ID、字段白名单、是否终态及迁移规则绑定；
//! `EVENT_KIND_SPECS` 是这张登记表。未知但属于关键产品事件族的类型会失败关闭，避免
//! 投影器把自己不认识的事件当作授权或执行事实；允许保持不透明的类型只能供查询查看。
//! 较早的 legacy event frame 可能缺少聚合/版本信封或使用非对象载荷，会暂时走兼容写入路径，
//! 不经过此处的现代事件登记校验；不要把“本函数验证通过”等同于 EventStore 整体完成验证。
//! 当前明确迁移入口由测试覆盖，workspace 生产读取代码尚未直接调用
//! `resolve_event_payload()`。

use crate::{RuntimeEvent, SchemaVersion, UnknownEventPolicy, UNKNOWN_EVENT_POLICY};
// `Serialize` 来自 serde，用于让静态事件契约可以被转换成 JSON 等外部格式，供检查工具或
// API 查看；实际的事件载荷使用 serde_json::Value 承载，并在写入前按本文件的白名单验证。
use serde::Serialize;
use serde_json::Value;

pub const RUNTIME_EVENT_SCHEMA: &str = "kiana.runtime-event.v1";
pub const RUNTIME_EVENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

// 顶层 RuntimeEvent.request_id 才是统一事件信封中的请求身份；载荷字段白名单不重复放入
// request_id，避免两份相同身份在后续演变中出现互相矛盾的值。
const REQUEST_IDS: &[&str] = &[];
const SESSION_ASSIGNMENT_IDS: &[&str] = &["session_id"];
const RUN_IDS: &[&str] = &["run_id"];
const RUN_AUTHORIZED_FIELDS: &[&str] = &[
    "run_id",
    "session_id",
    "actor_id",
    "project_root",
    "role_id",
    "department_id",
    "harness",
    "sandbox",
    "capability_mode",
    "max_steps_per_turn",
    "runtime_budget",
    "authority_revision",
    "authority_epoch",
    "turn_id",
    "turn",
    "role_prompt_hash",
    "model_profile",
    "role_spec_schema",
    "role_version",
    "role_catalog_schema",
    "role_catalog_version",
    "role_input_schema",
    "role_output_schema",
    "decision",
];
const RUN_STARTED_FIELDS: &[&str] = &["run_id", "sequence"];
const RUN_PROMPT_FIELDS: &[&str] = &["run_id", "session_id", "turn_id", "turn", "text"];
const RUN_DELTA_FIELDS: &[&str] = &["run_id", "text"];
const RUN_CANCELLING_FIELDS: &[&str] = &[
    "run_id",
    "reason",
    "cancellation_state",
    "cancellation_reason",
    "cancel_actor_id",
    "cancellation_targets",
    "cancellation_at_unix_ms",
    "cancellation_fact",
    "project_root",
];
const RUN_CAPABILITY_REQUESTED_FIELDS: &[&str] = &[
    "run_id",
    "request_id",
    "capability_request_id",
    "capability",
    "operation",
    "risk",
    "cell_id",
    "capability_grant_id",
    "budget_lease_id",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
    "action_digest",
    "turn_id",
    "step_id",
    "invocation_id",
    "arguments",
    "execution_scope",
];
const RUN_CAPABILITY_BLOCKED_FIELDS: &[&str] = &[
    "run_id",
    "capability_request_id",
    "error",
    "reason",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
];
const RUN_AWAITING_APPROVAL_IDS: &[&str] = &["run_id", "capability_request_id", "approval_id"];
const RUN_AWAITING_APPROVAL_FIELDS: &[&str] = &[
    "run_id",
    "approval_id",
    "capability_request_id",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
    "resume_binding",
];
const RUN_SNAPSHOT_FIELDS: &[&str] = &["run_id", "snapshot"];
const RUN_INPUT_ACCEPTED_FIELDS: &[&str] = &[
    "run_id",
    "input_id",
    "source",
    "target",
    "target_turn_id",
    "text",
    "received_sequence",
    "disposition",
];
const RUN_INPUT_CLAIMED_FIELDS: &[&str] = &[
    "run_id",
    "input_id",
    "source",
    "target",
    "target_turn_id",
    "received_sequence",
    "disposition",
    "error",
];
const RUN_CLARIFICATION_REQUESTED_FIELDS: &[&str] = &[
    "run_id",
    "interaction_id",
    "turn_id",
    "request",
    "wait",
    "status",
];
const INVOCATION_IDS: &[&str] = &["run_id", "capability_request_id"];
const INVOCATION_DIRECT_IDS: &[&str] = &["capability_request_id"];
const CAPABILITY_DECISION_IDS: &[&str] = &[];
const EXECUTION_PREPARED_IDS: &[&str] = &[
    "execution_id",
    "invocation_id",
    "capability_request_id",
    "action_digest",
    "attempt",
];
const EXECUTION_PREPARED_DIRECT_IDS: &[&str] = &[
    "execution_id",
    "invocation_id",
    "capability_request_id",
    "action_digest",
    "attempt",
];
const INPUT_IDS: &[&str] = &["run_id", "input_id"];
const CLARIFICATION_IDS: &[&str] = &["run_id", "interaction_id", "turn_id"];
const APPROVAL_IDS: &[&str] = &["approval_id"];
const ACTION_IDS: &[&str] = &["request_id", "action_digest"];
const COMMUNICATION_IDS: &[&str] = &["message"];
const COMMUNICATION_LIFECYCLE_IDS: &[&str] = &["message_id"];
const SWARM_TRANSITION_IDS: &[&str] = &["swarm_plan_id"];
const QUALITY_IDS: &[&str] = &["request_id"];
const RECOVERY_IDS: &[&str] = &["run_id"];
const MODEL_EVENT_IDS: &[&str] = &[
    "run_id",
    "session_id",
    "turn_id",
    "step_id",
    "model_call_id",
    "model_attempt_id",
];
const MODEL_ATTEMPT_LIFECYCLE_IDS: &[&str] = &[
    "run_id",
    "turn_id",
    "model_attempt_id",
    "attempt_id",
    "reservation_id",
];
const SETTLEMENT_FOLD_IDS: &[&str] =
    &["run_id", "model_attempt_id", "attempt_id", "reservation_id"];
const COST_IDS: &[&str] = &["run_id", "attempt_id", "usage_digest"];
const COST_LEDGER_IDS: &[&str] = &["run_id", "entry_id", "source_event_id"];
const COST_CORRECTION_IDS: &[&str] = &[
    "run_id",
    "correction_id",
    "target_entry_id",
    "target_entry_digest",
];
const COST_ALLOCATION_IDS: &[&str] = &["run_id", "allocation_id", "usage_id"];

/// 一种运行事件在服务端的静态契约。
///
/// 事件生产者把 `kind` 和 JSON 载荷写入 `RuntimeEvent`；本结构由本 crate 维护的
/// `EVENT_KIND_SPECS` 提供，供 EventStore 写入校验及 schema 版本解析使用。它不是事件本身，
/// 而是说明事件应该如何被解释的规则：归属哪个聚合、哪些身份字段必须出现、载荷允许哪些键、
/// 该事件是否结束其所属聚合，以及旧版本是否有显式迁移器。
///
/// 修改白名单会直接改变哪些持久事件可以进入日志；修改 `terminal` 或聚合归属也可能影响
/// 读取方如何归并事件。因此扩展事件格式时，应同时检查其生产者、写入边界和投影器，而不能
/// 只在这里添加一个字段就假定整条数据流已经理解该字段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EventKindSpec {
    /// 持久化事件的稳定类型名，例如 `run.completed`。
    pub kind: &'static str,
    /// 载荷所使用的 schema 名称；通常与运行事件信封 schema 相同。
    pub schema: &'static str,
    /// 此类型当前登记的主、次版本。
    pub version: SchemaVersion,
    /// 对事件契约负责的 crate 名称，供审计和工具查询所有权边界。
    pub owner_crate: &'static str,
    /// 事件改变或描述的聚合类别，例如 run、approval 或 execution permit。
    pub aggregate_type: &'static str,
    /// 载荷中必须非空的身份键。它们用于把事件关联到对应对象或尝试。
    pub required_ids: &'static [&'static str],
    /// 此事件载荷允许出现的键集合；未知键会被拒绝，避免未经登记的数据被误当成事实。
    pub allowed_fields: &'static [&'static str],
    /// 此事件是否结束它所代表的生命周期单元；有些事件只结束子尝试，不代表外层 run 结束。
    pub terminal: bool,
    /// 对秘密数据的写入策略标记；实际扫描与拒绝逻辑位于事件写入边界。
    pub secret_policy: &'static str,
    /// 可处理旧主版本的迁移器名称；为 `None` 表示没有登记该类型的主版本迁移。
    pub migration: Option<&'static str>,
}

impl EventKindSpec {
    /// 判断传入的 schema 版本能否按该事件类型的当前主版本规则继续读取。
    ///
    /// 【作用】次版本号只允许向前兼容；主版本必须完全相同，较旧的次版本不能冒充当前契约。
    /// 通过这里只代表版本号可继续处理，载荷字段仍会由 `validate_event_payload()` 独立检查。
    ///
    /// 【调用者】主要由 `validate_version()` 和事件 schema 解析流程调用。
    ///
    /// 【输入】`incoming` 是持久事件信封声明的版本；`self.version` 是登记表中的当前版本。
    ///
    /// 【输出】兼容时返回 `true`，主版本不符或次版本低于当前契约时返回 `false`。
    ///
    /// 【为什么这样设计】次版本用于兼容增加字段等演进；破坏性主版本变化必须经过具名迁移，
    /// 不能仅因 JSON 看起来还能解析，就把旧事件当成当前事件解释。
    pub fn accepts_version(&self, incoming: &SchemaVersion) -> bool {
        self.version.major == incoming.major && incoming.minor >= self.version.minor
    }

    /// 校验 schema 版本，并把不兼容情况转换为稳定的事件错误码。
    ///
    /// 【调用者】由版本解析函数间接使用；调用方通常再把错误转换为写入或读取失败。
    ///
    /// 【输入】事件生产者或存储记录携带的 `incoming` 版本。
    ///
    /// 【输出】兼容时返回 `Ok(())`；较旧次版本返回
    /// `event_schema_version_downgrade`，其余不兼容主版本返回
    /// `event_schema_version_incompatible`。
    ///
    /// 【副作用】无。该函数只检查版本号，不读取或修改事件载荷。
    ///
    /// 【为什么这么设计】把“版本可否读取”和“JSON 内容是否合法”拆开，使版本迁移与字段
    /// 白名单各自承担清楚的责任；不能将成功结果当成载荷已验证。
    pub fn validate_version(&self, incoming: &SchemaVersion) -> Result<(), String> {
        if self.version.major == incoming.major && incoming.minor < self.version.minor {
            return Err("event_schema_version_downgrade".to_owned());
        }
        if !self.accepts_version(incoming) {
            return Err("event_schema_version_incompatible".to_owned());
        }
        Ok(())
    }
}

/// 一个事件载荷经过服务端契约登记表解析后的结果。
///
/// 【数据流】读取方把事件类型、版本和载荷交给 `resolve_event_payload()`；解析成功后得到以下
/// 三种状态之一。该枚举只描述“如何解释已存数据”，不会重写事件日志，也不会授予执行权限。
///
/// `Current` 表示类型已登记，版本通过检查，且载荷通过当前字段契约。
/// `Migrated` 表示旧主版本经具名迁移器转换成可读取形状；同时保留迁移器和源/目标版本，
/// 以便调用方知道这不是原样的当前格式。
/// `Opaque` 表示类型属于允许保留的未知事件；原始载荷可供查询和审计，但不能并入授权或执行
/// 投影，因为本程序并不知道它表达什么状态变化。
///
/// ⚠ 不要把 `Opaque` 简化成普通 `Current`。那会让未理解的事件绕过类型语义边界，
/// 使投影器可能把未知数据误解为系统事实。
#[derive(Clone, Debug, PartialEq)]
pub enum EventSchemaResolution {
    Current {
        payload: Value,
    },
    Migrated {
        payload: Value,
        migration: &'static str,
        from: SchemaVersion,
        to: SchemaVersion,
    },
    Opaque {
        kind: String,
        payload: Value,
    },
}

impl EventSchemaResolution {
    /// 以只读方式取得三种解析结果中携带的载荷。
    ///
    /// 调用者通常用于展示、审计或后续已知类型处理。返回引用而不是克隆载荷，避免大型
    /// JSON 对象被无必要地复制；此访问器不表示载荷已经取得执行或授权含义。
    pub fn payload(&self) -> &Value {
        match self {
            Self::Current { payload }
            | Self::Migrated { payload, .. }
            | Self::Opaque { payload, .. } => payload,
        }
    }

    /// 判断解析结果是否仍是未知事件的只读不透明形式。
    ///
    /// 投影器可用此标记决定是否跳过状态归并；未知载荷虽然能检查，但不能作为已知事实应用。
    pub const fn is_opaque(&self) -> bool {
        matches!(self, Self::Opaque { .. })
    }
}

// 以下数组是各类事件的 JSON 载荷键白名单。它们按生命周期/领域分组复用：
// `required_ids` 只规定关联对象所需的身份键，`allowed_fields` 则限制整个载荷中可以出现的键。
// 将两者分开，才能允许可选上下文存在，同时仍拒绝拼写错误或未审计的新字段。
const RUN_FIELDS: &[&str] = &[
    "run_id",
    "schema",
    "sequence",
    "session_id",
    "actor_id",
    "project_root",
    "turn_id",
    "turn",
    "text",
    "steps",
    "stop_reason",
    "step",
    "step_id",
    "error",
    "reason",
    "outcome",
    "result",
    "snapshot",
    "harness",
    "sandbox",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
    "invocation_id",
    "capability_request_id",
    "call_id",
    "request_id",
    "capability",
    "operation",
    "risk",
    "action_digest",
    "arguments",
    "cell_id",
    "capability_grant_id",
    "budget_lease_id",
    "execution_scope",
    "resume_binding",
    "cancellation_state",
    "cancellation_reason",
    "cancellation_targets",
    "cancellation_at_unix_ms",
    "stop_confirmed",
    "cancellation_fact",
    "cancel_actor_id",
    "input_id",
    "source",
    "target",
    "target_turn_id",
    "received_sequence",
    "disposition",
    "interaction_id",
    "question",
    "options",
    "required",
    "expires_at_unix_ms",
    "cancel_policy",
    "created_by_role",
    "responder_roles",
    "status",
    "request",
    "wait",
    "answer",
    "answer_id",
    "request_digest",
    "answer_digest",
    "waiting_for_input",
    "turn_outcome",
    "contract_digest",
    "output_digest",
    "artifact_refs",
    "verification_refs",
    "blockers",
    "outcome_digest",
    "terminal",
    "kind",
    "effect_usage",
    "owner_digest",
    "lease_digest",
    "resource_digest",
];
// run.completed 可以保留 Harness 交付的模型标识、token 用量以及凭证引用。
// 这些数据只在完成事件中有明确语义，所以不能直接扩充通用 RUN_FIELDS，
// 否则 failed/cancelled 等其他终态也会意外接受这些字段。
const RUN_COMPLETED_FIELDS: &[&str] = &[
    "run_id",
    "schema",
    "sequence",
    "session_id",
    "actor_id",
    "project_root",
    "turn_id",
    "turn",
    "text",
    "steps",
    "stop_reason",
    "step",
    "step_id",
    "error",
    "reason",
    "outcome",
    "result",
    "snapshot",
    "harness",
    "sandbox",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
    "invocation_id",
    "capability_request_id",
    "call_id",
    "request_id",
    "capability",
    "operation",
    "risk",
    "action_digest",
    "arguments",
    "cell_id",
    "capability_grant_id",
    "budget_lease_id",
    "execution_scope",
    "resume_binding",
    "cancellation_state",
    "cancellation_reason",
    "cancellation_targets",
    "cancellation_at_unix_ms",
    "stop_confirmed",
    "cancellation_fact",
    "cancel_actor_id",
    "input_id",
    "source",
    "target",
    "target_turn_id",
    "received_sequence",
    "disposition",
    "interaction_id",
    "question",
    "options",
    "required",
    "expires_at_unix_ms",
    "cancel_policy",
    "created_by_role",
    "responder_roles",
    "status",
    "request",
    "wait",
    "answer",
    "answer_id",
    "request_digest",
    "answer_digest",
    "waiting_for_input",
    "turn_outcome",
    "contract_digest",
    "output_digest",
    "artifact_refs",
    "verification_refs",
    "blockers",
    "outcome_digest",
    "terminal",
    "kind",
    "effect_usage",
    "owner_digest",
    "lease_digest",
    "resource_digest",
    "model_id",
    "usage",
    "secret_ref",
];
const RUN_REJECTED_FIELDS: &[&str] = &[
    "run_id",
    "reason",
    "command",
    "session_id",
    "packet_id",
    "status",
    "author_session_id",
    "reviewer_session_id",
    "author_role_id",
];

const RUN_TOOL_CALL_FIELDS: &[&str] = &[
    "run_id",
    "capability_request_id",
    "call_id",
    "turn_id",
    "step_id",
    "invocation_id",
    "execution_scope",
    "tool",
    "operation",
];
const RUN_TOOL_RESULT_FIELDS: &[&str] = &[
    "run_id",
    "capability_request_id",
    "call_id",
    "result",
    "cancelled",
    "not_executed",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "stop_confirmed",
    "fenced",
];
const RUN_PREDECESSOR_FIELDS: &[&str] = &[
    "run_id",
    "previous_run_id",
    "turn_id",
    "turn",
    "session_id",
    "semantics",
];
const RUN_RESUME_PREPARED_FIELDS: &[&str] = &[
    "run_id",
    "session_id",
    "actor_id",
    "snapshot_event_id",
    "turn_id",
    "turn",
];
const RUN_COMPACTED_FIELDS: &[&str] = &[
    "schema",
    "run_id",
    "tokens_before",
    "tokens_after",
    "summary_present",
];
const RUN_RECEIPT_FIELDS: &[&str] = &[
    "schema",
    "run_id",
    "session_id",
    "harness",
    "sandbox",
    "actor_id",
    "role_id",
    "department_id",
    "role_resolution",
    "role_spec_schema",
    "role_version",
    "role_catalog_schema",
    "role_catalog_version",
    "input_schema",
    "output_schema",
    "model_profile",
    "max_steps_per_turn",
    "prompt_hash",
    "model_turns",
    "cost_ledger",
    "files_changed",
    "memory_hits",
    "retrieval_receipts",
    "memory_proposals",
    "invocations",
    "invocation_projection_error",
    "run_receipt",
    "receipt_data_binding",
    "projection",
    "execution_receipts",
    "aggregation",
    "cost_breakdown",
    "effect_usage",
    "compact",
    "capabilities",
    "observability",
    "output",
    "work_packet_id",
    "input",
];

// request 与 session assignment 保存入口请求和“会话绑定到哪个 actor / 项目 / 角色”的快照。
// 此处定义的是事件载荷可出现的字段；真正对项目路径和角色权限的授权判断属于 ControlPlane，
// 不能把字段通过 schema 校验误当成权限已获批准。
const REQUEST_FIELDS: &[&str] = &[
    "command",
    "arguments",
    "run_id",
    "session_id",
    "turn_id",
    "turn",
    "actor_id",
    "project_root",
    "role_id",
    "department_id",
    "project_trusted",
    "cell_id",
    "work_packet_id",
    "harness",
    "anti_meeting",
    "capability",
    "operation",
    "risk",
    "error",
    "reason",
    "action_digest",
    "request_id",
];
const SESSION_ASSIGNMENT_FIELDS: &[&str] = &[
    "schema",
    "session_id",
    "actor_id",
    "project_root",
    "role_id",
    "department_id",
    "prompt_hash",
    "model_profile",
    "role_spec_schema",
    "role_version",
    "role_catalog_schema",
    "role_catalog_version",
    "role_input_schema",
    "role_output_schema",
    "authority_epoch",
    "principal",
    "project_identity",
    "assignment",
];

// communication 和 delegation 记录跨角色交接、回执、事件升级及计划状态转换。
// 生命周期事件使用专门白名单，是为了区分“发出消息”和“对消息/事件作出处理”的证据形状。
const COMMUNICATION_FIELDS: &[&str] = &[
    "message",
    "message_id",
    "lifecycle",
    "accepted",
    "reason",
    "evidence_refs",
    "authority_granted",
    "project_root",
    "actor_id",
    "session_id",
    "request_id",
];
const COMMUNICATION_SEND_FIELDS: &[&str] = &[
    "message",
    "message_id",
    "lifecycle",
    "authority_granted",
    "project_root",
    "actor_id",
    "session_id",
    "request_id",
];
const COMMUNICATION_HANDOFF_LIFECYCLE_FIELDS: &[&str] = &[
    "message",
    "message_id",
    "lifecycle",
    "accepted",
    "reason",
    "authority_granted",
    "project_root",
    "actor_id",
    "session_id",
    "request_id",
];
const COMMUNICATION_INCIDENT_LIFECYCLE_FIELDS: &[&str] = &[
    "message",
    "message_id",
    "lifecycle",
    "evidence_refs",
    "reason",
    "authority_granted",
    "project_root",
    "actor_id",
    "session_id",
    "request_id",
];
const SWARM_TRANSITION_FIELDS: &[&str] = &[
    "swarm_plan_id",
    "partition_id",
    "attempt_id",
    "child_cell_id",
    "from",
    "to",
    "revision",
    "authority_epoch",
    "correlation_id",
    "causation_event_id",
    "review_complete",
    "event_digest",
];
const QUALITY_FIELDS: &[&str] = &[
    "request_id",
    "command",
    "dataset_id",
    "suite_id",
    "case_id",
    "experiment_id",
    "result_id",
    "candidate_id",
    "gate_id",
    "decision_id",
    "feedback_id",
    "trace_id",
    "source_cursor",
    "source_digest",
    "payload_digest",
    "reason",
    "status",
    "verdict",
    "evidence_refs",
    "candidate_digest",
    "gate_decision_digest",
    "approval_ref",
    "approval_request_hash",
    "scope_digest",
    "authority_epoch",
    "operation",
];
const RECOVERY_FIELDS: &[&str] = &["run_id", "recovery"];
macro_rules! invocation_fields {
    // invocation_fields! 给 invocation / capability / execution / result-delivery 事件提供共享的
    // 关联身份和执行效果字段；具体事件再通过宏参数加入自己的专属字段。复用的是字段形状，
    // 不代表这些事件可以互换：各自的 EventKindSpec 仍单独指定必需 ID、聚合和终态语义。
    ($($extra:literal),* $(,)?) => {
        &[
            "run_id",
            "turn_id",
            "invocation_id",
            "execution_id",
            "capability_request_id",
            "call_id",
            "operation",
            "capability",
            "attempt",
            "action_digest",
            "args_fingerprint",
            "result",
            "effect_started",
            "effect_known",
            "zero_effect",
            "stop_state",
            "stop_requested",
            "stop_confirmed",
            "fenced",
            "started",
            "boundary",
            "decision_id",
            "permit",
            "permit_digest",
            "invocation",
            "cell_reservation",
            "reservation_digest",
            "effect_usage",
            "owner_digest",
            "lease_digest",
            "resource_digest",
            $($extra,)*
        ]
    };
}
const INVOCATION_FIELDS: &[&str] = invocation_fields!();
const CAPABILITY_DECISION_FIELDS: &[&str] = invocation_fields!("policy", "gate");
const CAPABILITY_BLOCKED_FIELDS: &[&str] = &[
    "error",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
];
const CAPABILITY_RESULT_FIELDS: &[&str] = invocation_fields!(
    "session_id",
    "cell_id",
    "capability_grant_id",
    "budget_lease_id",
    "stdout",
    "outcome",
    "result_receipt",
    "result_source"
);
const EXECUTION_RESULT_FIELDS: &[&str] =
    invocation_fields!("outcome_state", "outcome_ready", "result_receipt");
const RESULT_DELIVERY_FIELDS: &[&str] = invocation_fields!(
    "result_digest",
    "receipt_digest",
    "outcome_state",
    "outcome_ready",
    "delivery_policy"
);
const MODEL_ATTEMPT_FIELDS: &[&str] = &[
    "run_id",
    "session_id",
    "turn_id",
    "step",
    "step_id",
    "step_identity",
    "model_call_id",
    "model_request_id",
    "model_attempt_id",
    "attempt_identity",
    "attempt",
    "schema",
    "provider_id",
    "model_id",
    "configuration_revision",
    "provider_request_id",
    "provider_response_id",
    "provider_request_id_status",
    "provider_response_id_status",
    "prepared",
    "route_digest",
    "prompt_version",
    "purpose",
    "streaming",
    "budget",
    "reserved_tokens",
    "prompt_sources",
    "usage",
    "usage_complete",
    "attempted",
    "elapsed_ms",
    "finish",
    "retry_class",
    "outcome",
    "assistant",
    "error",
    "cache_usage",
    "harness_budget",
    "cell_budget",
    "usage_accounting",
    "budget_error",
    "provider_timing",
    "retry_reservation",
    "stop_reason_normalized",
    "tool_catalog_digest",
];
const MODEL_EVENT_FIELDS: &[&str] = &[
    "schema",
    "version",
    "kind",
    "session_id",
    "run_id",
    "turn_id",
    "step_id",
    "model_call_id",
    "model_attempt_id",
    "attempt",
    "invocation_id",
    "execution_id",
    "tool_call_id",
    "tool_name",
    "provider_id",
    "actual_model_id",
    "configuration_revision",
    "prompt_digest",
    "schema_digest",
    "route_digest",
    "config_digest",
    "tool_catalog_digest",
    "provider_request",
    "provider_response",
    "retry",
    "finish",
    "usage",
    "usage_correction",
    "trace",
    "sequences",
    "correlation",
    "source_event_id",
    "source_cursor",
    "event_digest",
];
const MODEL_BUDGET_PREPARED_FIELDS: &[&str] = &["permit", "prepared", "authority_versions"];
const MODEL_BUDGET_AUDIT_FIELDS: &[&str] = &[
    "schema",
    "model_call_id",
    "model_request_id",
    "model_attempt_id",
    "step_id",
    "run_id",
    "step",
    "purpose",
    "route",
    "route_digest",
    "prompt_version",
    "request_hash",
    "tool_catalog_hash",
    "budget",
    "streaming",
    "deadline_unix_ms",
    "provider_account",
    "credential_revision",
];
const MODEL_BUDGET_DISPATCH_FIELDS: &[&str] =
    &["run_id", "model_request_id", "request_hash", "permit_id"];
const MODEL_BUDGET_SETTLED_FIELDS: &[&str] = &[
    "run_id",
    "model_request_id",
    "charged_tokens",
    "reported_tokens",
    "usage_known",
    "reservation_exceeded",
    "at_unix_ms",
    "settlement_fact",
];
const CONNECTOR_HEALTH_IDS: &[&str] = &["request_id", "connector_id", "binding_id"];
const CONNECTOR_HEALTH_FIELDS: &[&str] = &[
    "schema",
    "request_id",
    "connector_id",
    "binding_id",
    "status",
    "health",
    "probe_kind",
    "checked_at_unix_ms",
    "project_root",
    "actor_id",
    "authorization_id",
    "request_fingerprint",
];
const CONNECTOR_MCP_HANDSHAKE_IDS: &[&str] = &["request_id", "connector_id", "binding_id"];
const CONNECTOR_MCP_HANDSHAKE_FIELDS: &[&str] = &[
    "schema",
    "request_id",
    "connector_id",
    "binding_id",
    "server",
    "actor_id",
    "project_root",
    "authorization_id",
    "handshake",
    "proof_level",
];
const MODEL_ATTEMPT_LIFECYCLE_FIELDS: &[&str] = &[
    "schema",
    "version",
    "kind",
    "run_id",
    "turn_id",
    "model_attempt_id",
    "attempt_id",
    "reservation_id",
    "permit_id",
    "usage",
    "receipt_id",
    "error",
    "prepared_flushed",
    "flush_sequence",
    "revision",
    "event_id",
    "event_digest",
];
const SETTLEMENT_FOLD_FIELDS: &[&str] = &[
    "schema",
    "version",
    "kind",
    "event_id",
    "run_id",
    "model_attempt_id",
    "attempt_id",
    "reservation_id",
    "reservation_digest",
    "usage_class",
    "usage_digest",
    "receipt_id",
    "reserved",
    "consumed",
    "released",
    "unknown_reason",
    "reconciliation_required",
    "source_refs",
    "revision",
    "event_digest",
];

// billing / cost 字段组记录用量到费用的关联、账本追加、修正与分摊。
// ID 和 digest 把账单结果绑定到源事件/条目；不能把它们当作任意备注字段删去或换名。
const COST_FIELDS: &[&str] = &[
    "schema",
    "version",
    "run_id",
    "attempt_id",
    "usage_digest",
    "usage_confidence",
    "rate_card_id",
    "rate_card_version",
    "kind",
    "lines",
    "breakdown_digest",
];
const COST_LEDGER_FIELDS: &[&str] = &[
    "schema",
    "version",
    "entry_id",
    "kind",
    "run_id",
    "attempt_id",
    "usage_ref",
    "reservation_ref",
    "allocation_ref",
    "amount",
    "estimated_cost",
    "measured_cost",
    "currency",
    "rate_card_id",
    "rate_card_version",
    "provider_receipt_ref",
    "created_at_unix_ms",
    "source_event_id",
    "source_cursor",
    "revision",
    "source_digest",
    "entry_digest",
];
const COST_CORRECTION_FIELDS: &[&str] = &[
    "schema",
    "version",
    "correction_id",
    "command_id",
    "command_digest",
    "target_entry_id",
    "target_entry_digest",
    "target_run_id",
    "target_attempt_id",
    "target_source_cursor",
    "target_revision",
    "delta_estimated",
    "delta_measured",
    "reason",
    "evidence_refs",
    "requested_by",
    "idempotency_key",
    "approval_id",
    "approver_id",
    "approved_at_unix_ms",
    "approval_digest",
    "created_at_unix_ms",
    "correction_digest",
    "run_id",
];
const COST_ALLOCATION_FIELDS: &[&str] = &[
    "schema",
    "version",
    "allocation_id",
    "usage_id",
    "source_project_id",
    "scope",
    "organization_id",
    "project_id",
    "workflow_id",
    "cell_id",
    "run_id",
    "sharing_grant",
    "cost",
    "source_event_id",
    "source_cursor",
    "revision",
    "source_digest",
    "allocation_digest",
];

// approval 字段组分别描述审批请求、决策、过期、撤销、消费等生命周期事实。
// 各状态使用精确白名单，避免一个宽泛联合结构把“已请求”载荷里的字段带入“已批准”事件。
const APPROVAL_FIELDS: &[&str] = &[
    "approval_id",
    "request_hash",
    "session_id",
    "actor_id",
    "subject_request_id",
    "run_id",
    "operation",
    "expires_at_unix_ms",
    "decision",
    "scope",
    "capability_request_id",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
    "resume_binding",
    "error",
];
const APPROVAL_STAGED_FIELDS: &[&str] =
    &["schema", "approval_id", "subject", "state", "at_unix_ms"];
const APPROVAL_ACTIVATED_FIELDS: &[&str] = &[
    "schema",
    "approval_id",
    "previous_state",
    "state",
    "request_hash",
    "at_unix_ms",
    "activation_command_id",
];
const APPROVAL_REQUESTED_FIELDS: &[&str] = &[
    "approval_id",
    "request_hash",
    "session_id",
    "actor_id",
    "run_id",
    "expires_at_unix_ms",
    "capability_request_id",
    "attempt",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
    "resume_binding",
    "action_digest",
];
const APPROVAL_APPROVED_FIELDS: &[&str] = &[
    "schema",
    "approval_id",
    "previous_state",
    "state",
    "request_hash",
    "at_unix_ms",
    "decision",
    "decision_command_id",
    "decided_by",
    "decision_fact",
];
const APPROVAL_DENIED_FIELDS: &[&str] = &[
    "schema",
    "approval_id",
    "previous_state",
    "state",
    "request_hash",
    "at_unix_ms",
    "decision",
    "decision_command_id",
    "decided_by",
    "decision_fact",
];
// 人工审批响应会在具类型的审批状态转换后，再发出绑定 subject request 的直接决策事件。
// 这个直接信封与 journal transition 是两种生产格式，因此分开登记，不能取两组字段的并集；
// 否则任一生产者都能夹带另一条链路才允许的字段。
const APPROVAL_DIRECT_DECISION_IDS: &[&str] = &["approval_id", "subject_request_id", "session_id"];
const APPROVAL_DIRECT_DECISION_FIELDS: &[&str] = &[
    "approval_id",
    "request_hash",
    "session_id",
    "actor_id",
    "decision",
    "subject_request_id",
    "operation",
    "expires_at_unix_ms",
    "scope",
    "run_id",
];
const APPROVAL_EXPIRED_FIELDS: &[&str] = &[
    "schema",
    "approval_id",
    "previous_state",
    "state",
    "request_hash",
    "at_unix_ms",
    "reason",
];
const APPROVAL_CANCELLED_FIELDS: &[&str] = &[
    "schema",
    "approval_id",
    "previous_state",
    "state",
    "request_hash",
    "at_unix_ms",
    "reason",
    "revoked_by",
    "source",
];
const APPROVAL_CONSUMED_FIELDS: &[&str] = &[
    "schema",
    "approval_id",
    "previous_state",
    "state",
    "request_hash",
    "at_unix_ms",
    "dispatch_command_id",
    "decision_command_id",
    "decided_by",
    "consumption_fact",
];
const APPROVAL_CONTINUATION_UNAVAILABLE_IDS: &[&str] = &["approval_id", "run_id"];
const APPROVAL_CONTINUATION_UNAVAILABLE_FIELDS: &[&str] = &["approval_id", "run_id", "error"];
const ACTION_FIELDS: &[&str] = &[
    "request_id",
    "action_digest",
    "authority_version",
    "authority_key",
    "actor_id",
    "run_id",
    "turn_id",
    "invocation_id",
    "execution_id",
    "capability_request_id",
    "call_id",
    "operation",
    "attempt",
    "result",
    "effect_started",
    "effect_known",
    "zero_effect",
    "stop_state",
    "fenced",
];

macro_rules! spec {
    // 统一填充每个事件都会共享的 envelope schema、owner 和秘密策略；调用点只声明该事件
    // 自己的聚合类型、必需 ID、载荷白名单、终态和迁移器，便于集中审计而不复制默认值。
    ($kind:literal, $aggregate:literal, $ids:expr, $fields:expr, $terminal:expr, $migration:expr) => {
        EventKindSpec {
            kind: $kind,
            schema: RUNTIME_EVENT_SCHEMA,
            version: RUNTIME_EVENT_SCHEMA_VERSION,
            owner_crate: "kiana-domain",
            aggregate_type: $aggregate,
            required_ids: $ids,
            allowed_fields: $fields,
            terminal: $terminal,
            secret_policy: "redact_event_value_or_reject",
            migration: $migration,
        }
    };
}

/// 服务端认可的事件类型登记表。
///
/// 这是事件契约的单一静态来源：验证函数按 `kind` 查找这里的规格，再检查 schema 版本、
/// 必需身份字段及字段白名单。事件生产者和读取方应共享此表，不要在入口、UI 或投影模块
/// 另写一份“差不多”的允许字段列表，否则写入成功的数据可能无法被其他模块一致解释。
///
/// `terminal` 表示该事件关闭它所代表的生命周期单元；有些事件结束整个 run，有些只结束一次
/// capability 尝试。必须结合 `kind` 和具体业务事件理解，不能仅凭该标记就认定外层 run 结束。
pub const EVENT_KIND_SPECS: &[EventKindSpec] = &[
    spec!(
        "request.accepted",
        "request",
        REQUEST_IDS,
        REQUEST_FIELDS,
        false,
        None
    ),
    spec!(
        "request.rejected",
        "request",
        REQUEST_IDS,
        REQUEST_FIELDS,
        true,
        None
    ),
    spec!(
        "communication.chat",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_SEND_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.command",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_SEND_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.handoff",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_SEND_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.decision",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_SEND_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.status_report",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_SEND_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.evidence",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_SEND_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.incident",
        "communication",
        COMMUNICATION_IDS,
        COMMUNICATION_SEND_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.handoff_acknowledged",
        "communication",
        COMMUNICATION_LIFECYCLE_IDS,
        COMMUNICATION_HANDOFF_LIFECYCLE_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.handoff_rejected",
        "communication",
        COMMUNICATION_LIFECYCLE_IDS,
        COMMUNICATION_HANDOFF_LIFECYCLE_FIELDS,
        false,
        None
    ),
    spec!(
        "communication.incident_escalated",
        "communication",
        COMMUNICATION_LIFECYCLE_IDS,
        COMMUNICATION_INCIDENT_LIFECYCLE_FIELDS,
        false,
        None
    ),
    spec!(
        "delegation.swarm_state",
        "swarm",
        SWARM_TRANSITION_IDS,
        SWARM_TRANSITION_FIELDS,
        false,
        None
    ),
    spec!(
        "delegation.partition_state",
        "swarm",
        SWARM_TRANSITION_IDS,
        SWARM_TRANSITION_FIELDS,
        false,
        None
    ),
    spec!(
        "delegation.attempt_state",
        "swarm",
        SWARM_TRANSITION_IDS,
        SWARM_TRANSITION_FIELDS,
        false,
        None
    ),
    spec!(
        "eval.run",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "eval.capture",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "eval.compare",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "eval.explain",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "eval.list",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "quality.feedback",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "quality.promote",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "quality.rollback",
        "quality",
        QUALITY_IDS,
        QUALITY_FIELDS,
        false,
        None
    ),
    spec!(
        "connector.health_checked",
        "connector",
        CONNECTOR_HEALTH_IDS,
        CONNECTOR_HEALTH_FIELDS,
        false,
        None
    ),
    spec!(
        "connector.mcp_handshake",
        "connector",
        CONNECTOR_MCP_HANDSHAKE_IDS,
        CONNECTOR_MCP_HANDSHAKE_FIELDS,
        false,
        None
    ),
    spec!(
        "run.authorized",
        "run",
        RUN_IDS,
        RUN_AUTHORIZED_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.predecessor",
        "run",
        RUN_IDS,
        RUN_PREDECESSOR_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.started",
        "run",
        RUN_IDS,
        RUN_STARTED_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.rejected",
        "request",
        &[],
        RUN_REJECTED_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.queued",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.prompt",
        "run",
        RUN_IDS,
        RUN_PROMPT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.delta",
        "run",
        RUN_IDS,
        RUN_DELTA_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.tool_call",
        "run",
        INVOCATION_IDS,
        RUN_TOOL_CALL_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.capability_requested",
        "run",
        INVOCATION_IDS,
        RUN_CAPABILITY_REQUESTED_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    // 此终态仅关闭这次 capability 尝试，不代表它所属的整个 run 结束。
    spec!(
        "run.capability_blocked",
        "run",
        INVOCATION_IDS,
        RUN_CAPABILITY_BLOCKED_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.tool_result",
        "run",
        INVOCATION_IDS,
        RUN_TOOL_RESULT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.awaiting_approval",
        "run",
        RUN_AWAITING_APPROVAL_IDS,
        RUN_AWAITING_APPROVAL_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.cancelling",
        "run",
        RUN_IDS,
        RUN_CANCELLING_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.resume_prepared",
        "run",
        RUN_IDS,
        RUN_RESUME_PREPARED_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.compacted",
        "run",
        RUN_IDS,
        RUN_COMPACTED_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.receipt",
        "run",
        RUN_IDS,
        RUN_RECEIPT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.completed",
        "run",
        RUN_IDS,
        RUN_COMPLETED_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.failed",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.cancelled",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.result_unknown",
        "run",
        RUN_IDS,
        RUN_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.model_turn",
        "run",
        RUN_IDS,
        MODEL_ATTEMPT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "model.reserved",
        "model_budget",
        &["run_id", "model_request_id"],
        &[
            "run_id",
            "model_request_id",
            "tokens",
            "at_unix_ms",
            "reservation_fact",
            "request_count",
            "charged_and_reserved_tokens",
            "limit",
            "basis",
        ],
        false,
        None
    ),
    spec!(
        "model.prepared",
        "model_call",
        MODEL_EVENT_IDS,
        MODEL_EVENT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "model.denied",
        "model_call",
        MODEL_EVENT_IDS,
        MODEL_EVENT_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "model.attempt_started",
        "model_call",
        MODEL_EVENT_IDS,
        MODEL_EVENT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "model.retry_scheduled",
        "model_call",
        MODEL_EVENT_IDS,
        MODEL_EVENT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "model.finished",
        "model_call",
        MODEL_EVENT_IDS,
        MODEL_EVENT_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "model.usage_correction",
        "model_call",
        MODEL_EVENT_IDS,
        MODEL_EVENT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "model.observed",
        "model_attempt",
        MODEL_ATTEMPT_LIFECYCLE_IDS,
        MODEL_ATTEMPT_LIFECYCLE_FIELDS,
        false,
        None
    ),
    spec!(
        "model.dispatching",
        "model_attempt",
        MODEL_ATTEMPT_LIFECYCLE_IDS,
        MODEL_ATTEMPT_LIFECYCLE_FIELDS,
        false,
        None
    ),
    spec!(
        "model.settled",
        "model_attempt",
        MODEL_ATTEMPT_LIFECYCLE_IDS,
        MODEL_ATTEMPT_LIFECYCLE_FIELDS,
        true,
        None
    ),
    spec!(
        "model.unknown",
        "model_attempt",
        MODEL_ATTEMPT_LIFECYCLE_IDS,
        MODEL_ATTEMPT_LIFECYCLE_FIELDS,
        true,
        None
    ),
    spec!(
        "usage.reserved",
        "billing_attempt",
        SETTLEMENT_FOLD_IDS,
        SETTLEMENT_FOLD_FIELDS,
        false,
        None
    ),
    spec!(
        "usage.observed",
        "billing_attempt",
        SETTLEMENT_FOLD_IDS,
        SETTLEMENT_FOLD_FIELDS,
        false,
        None
    ),
    spec!(
        "usage.settled",
        "billing_attempt",
        SETTLEMENT_FOLD_IDS,
        SETTLEMENT_FOLD_FIELDS,
        false,
        None
    ),
    spec!(
        "usage.released",
        "billing_attempt",
        SETTLEMENT_FOLD_IDS,
        SETTLEMENT_FOLD_FIELDS,
        true,
        None
    ),
    spec!(
        "usage.unknown",
        "billing_attempt",
        SETTLEMENT_FOLD_IDS,
        SETTLEMENT_FOLD_FIELDS,
        true,
        None
    ),
    spec!(
        "usage.cost_estimated",
        "billing_attempt",
        COST_IDS,
        COST_FIELDS,
        false,
        None
    ),
    spec!(
        "usage.cost_measured",
        "billing_attempt",
        COST_IDS,
        COST_FIELDS,
        false,
        None
    ),
    spec!(
        "usage.cost_unknown",
        "billing_attempt",
        COST_IDS,
        COST_FIELDS,
        false,
        None
    ),
    spec!(
        "cost.ledger_entry",
        "cost_ledger",
        COST_LEDGER_IDS,
        COST_LEDGER_FIELDS,
        false,
        None
    ),
    spec!(
        "cost.corrected",
        "cost_ledger",
        COST_CORRECTION_IDS,
        COST_CORRECTION_FIELDS,
        false,
        None
    ),
    spec!(
        "cost.allocation",
        "cost_allocation",
        COST_ALLOCATION_IDS,
        COST_ALLOCATION_FIELDS,
        false,
        None
    ),
    spec!(
        "run.snapshot",
        "run",
        RUN_IDS,
        RUN_SNAPSHOT_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.input.accepted",
        "run",
        INPUT_IDS,
        RUN_INPUT_ACCEPTED_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.input.claimed",
        "run",
        INPUT_IDS,
        RUN_INPUT_CLAIMED_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "run.clarification.requested",
        "run",
        CLARIFICATION_IDS,
        RUN_CLARIFICATION_REQUESTED_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.decision",
        "run",
        CAPABILITY_DECISION_IDS,
        CAPABILITY_DECISION_FIELDS,
        false,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.blocked",
        "request",
        &[],
        CAPABILITY_BLOCKED_FIELDS,
        true,
        None
    ),
    spec!(
        "capability.completed",
        "run",
        INVOCATION_IDS,
        CAPABILITY_RESULT_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.failed",
        "run",
        INVOCATION_IDS,
        CAPABILITY_RESULT_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.cancelled",
        "run",
        INVOCATION_IDS,
        CAPABILITY_RESULT_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "capability.result_unknown",
        "run",
        INVOCATION_IDS,
        CAPABILITY_RESULT_FIELDS,
        true,
        Some("legacy_run_event_v0_to_v1")
    ),
    spec!(
        "approval.staged",
        "approval",
        APPROVAL_IDS,
        APPROVAL_STAGED_FIELDS,
        false,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.requested",
        "approval",
        APPROVAL_IDS,
        APPROVAL_REQUESTED_FIELDS,
        false,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.activated",
        "approval",
        APPROVAL_IDS,
        APPROVAL_ACTIVATED_FIELDS,
        false,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.approved",
        "approval",
        APPROVAL_IDS,
        APPROVAL_APPROVED_FIELDS,
        false,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.denied",
        "approval",
        APPROVAL_IDS,
        APPROVAL_DENIED_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.expired",
        "approval",
        APPROVAL_IDS,
        APPROVAL_EXPIRED_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.cancelled",
        "approval",
        APPROVAL_IDS,
        APPROVAL_CANCELLED_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.consumed",
        "approval",
        APPROVAL_IDS,
        APPROVAL_CONSUMED_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "approval.continuation_unavailable",
        "approval",
        APPROVAL_CONTINUATION_UNAVAILABLE_IDS,
        APPROVAL_CONTINUATION_UNAVAILABLE_FIELDS,
        true,
        Some("legacy_approval_event_v0_to_v1")
    ),
    spec!(
        "invocation.dispatching",
        "execution_permit",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        false,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "invocation.executing",
        "execution_permit",
        INVOCATION_IDS,
        INVOCATION_FIELDS,
        false,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "execution.prepared",
        "execution_permit",
        EXECUTION_PREPARED_IDS,
        INVOCATION_FIELDS,
        false,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "execution.result_committed",
        "execution_permit",
        INVOCATION_IDS,
        EXECUTION_RESULT_FIELDS,
        true,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "action.authority_pinned",
        "action",
        ACTION_IDS,
        ACTION_FIELDS,
        false,
        Some("legacy_action_event_v0_to_v1")
    ),
    spec!(
        "result.delivery_claimed",
        "result_delivery",
        INVOCATION_IDS,
        RESULT_DELIVERY_FIELDS,
        false,
        Some("legacy_invocation_event_v0_to_v1")
    ),
    spec!(
        "session.assigned",
        "session_assignment",
        SESSION_ASSIGNMENT_IDS,
        SESSION_ASSIGNMENT_FIELDS,
        false,
        Some("legacy_session_event_v0_to_v1")
    ),
    spec!(
        "recovery.credential",
        "run",
        RECOVERY_IDS,
        RECOVERY_FIELDS,
        false,
        None
    ),
];

/// 事件主版本迁移清单，按事件族、来源版本、目标版本和迁移器名称登记。
///
/// 这里的记录必须与 `EventKindSpec.migration` 及 `upcast_event_payload()` 中的实际分发相符；
/// 清单只表示某个版本方向被允许，不会单独实现转换。当前条目是 v0 到 v1 的 envelope 兼容，
/// 对应 payload 保持原形状，并仍需要通过当前字段契约。
pub const EVENT_MIGRATIONS: &[(&str, u32, u32, &str)] = &[
    ("run", 0, 1, "legacy_run_event_v0_to_v1"),
    ("approval", 0, 1, "legacy_approval_event_v0_to_v1"),
    ("invocation", 0, 1, "legacy_invocation_event_v0_to_v1"),
    ("action", 0, 1, "legacy_action_event_v0_to_v1"),
    ("session", 0, 1, "legacy_session_event_v0_to_v1"),
];

/// 按稳定类型名查找该事件的服务端契约。
///
/// 【调用者】版本检查、未知事件策略和 payload 解析函数均从此表查找；测试也使用它核对表项。
/// 【输入】事件信封中的 `kind` 字符串。
/// 【输出】已登记时返回静态规格引用；未知类型返回 `None`，由调用方进一步区分可保留的可选
/// 事件和必须认识的产品事件。
/// 【副作用/失败】无副作用；找不到类型本身不直接报错，因为是否报错取决于事件所属类别。
pub fn event_kind_spec(kind: &str) -> Option<&'static EventKindSpec> {
    EVENT_KIND_SPECS.iter().find(|spec| spec.kind == kind)
}

/// 判断未知事件类型是否落在必须由当前产品版本理解的关键事件族中。
///
/// 前缀集合覆盖请求、运行、授权、执行、会话、模型、费用等会影响状态或审计的事实。
/// 【调用者】`unknown_event_policy()` 用它对未登记类型执行失败关闭判断。
/// 【输出】命中任何受管前缀时为 `true`；其他未知命名空间可被保留为不透明数据。
/// 【为什么这么设计】允许未知类型被检查有助于兼容外围扩展；但关键执行事件如果本版本
/// 不认识，继续投影可能误授予权限或伪造状态，因此必须拒绝。
pub fn event_kind_is_required(kind: &str) -> bool {
    [
        "request.",
        "run.",
        "capability.",
        "approval.",
        "invocation.",
        "execution.",
        "action.",
        "result.",
        "session.",
        "communication.",
        "eval.",
        "quality.",
        "recovery.",
        "model.",
        "usage.",
        "cost.",
    ]
    .iter()
    .any(|prefix| kind.starts_with(prefix))
}

/// 为已登记、关键未知和可选未知类型决定是否能继续处理。
///
/// 【调用者】`validate_runtime_event()` 与 `resolve_event_payload()` 在没有事件规格时调用。
/// 【输入】事件的稳定类型名。
/// 【输出】已登记类型及可选未知类型返回 `PreserveOpaqueWithoutExecution` 策略；关键事件族的
/// 未知类型返回 `unknown_required_event_kind`。
/// 【失败后果】事件存储边界会把错误转换为写入失败；不会自动重试、禁用账号或执行回退。
/// 【设计原因】可选事件可以留作查询证据，但关键事实必须先有明确契约，不能猜测它的含义。
pub fn unknown_event_policy(kind: &str) -> Result<UnknownEventPolicy, String> {
    if event_kind_spec(kind).is_some() {
        return Ok(UNKNOWN_EVENT_POLICY);
    }
    if event_kind_is_required(kind) {
        return Err("unknown_required_event_kind".to_owned());
    }
    Ok(UNKNOWN_EVENT_POLICY)
}

/// 检查某种已登记事件能否使用传入的 schema 版本。
///
/// 【调用者】由 `resolve_event_payload()` 在校验当前版本载荷前调用；迁移旧主版本则走
/// 显式迁移分支。目前 workspace 中该读取解析入口还没有生产调用者。
/// 【输入】类型名和事件信封上携带的 `SchemaVersion`。
/// 【输出/失败】版本兼容返回 `Ok(())`；未知关键事件、未知类型或版本不兼容返回稳定错误码。
/// 【副作用】无。它不转换载荷，也不写入或修复日志。
pub fn check_event_schema_version(kind: &str, incoming: &SchemaVersion) -> Result<(), String> {
    let spec = event_kind_spec(kind).ok_or_else(|| {
        unknown_event_policy(kind)
            .err()
            .unwrap_or_else(|| "event_kind_opaque".to_owned())
    })?;
    spec.validate_version(incoming)
}

/// 查找某个事件主版本转换所允许使用的迁移器名称。
///
/// 【输入】事件 kind，以及来源和目标主版本号。
/// 【输出】只有事件规格与本文件的显式迁移清单都匹配时才返回迁移器名；否则为 `None`。
/// 【设计原因】规格里的迁移字段与版本对照清单必须同时匹配，避免只登记了名字却没有批准
/// 该事件族/版本方向。具体执行还要由 `upcast_event_payload()` 匹配实现。
pub fn event_migration(kind: &str, from_major: u32, to_major: u32) -> Option<&'static str> {
    event_kind_spec(kind).and_then(|spec| {
        spec.migration.filter(|_| {
            EVENT_MIGRATIONS.iter().any(|(family, from, to, _)| {
                kind.starts_with(&format!("{family}.")) && *from == from_major && *to == to_major
            })
        })
    })
}

/// 应用一个已登记的旧版事件载荷迁移器，不修改事件日志中的原始字节。
///
/// 【作用】把指定旧主版本的载荷转换为当前读取方可接受的形状。目前登记的 v0→v1 家族载荷
/// 形状保持不变，因此实现是 identity transform（原样返回）；显式调度仍然存在，以保证未来
/// 真正需要改形状时必须落实对应实现。
///
/// 【调用者】由 `resolve_event_payload()` 在遇到已登记的旧主版本时调用；当前 workspace 的
/// 生产读取路径尚未直接调用 `resolve_event_payload()`，行为由领域契约测试覆盖。
/// 【输入】事件类型、来源/目标主版本和待迁移载荷（所有权转移进来，成功后原样或转换后返回）。
/// 【输出】迁移成功并通过当前载荷校验后返回载荷。
/// 【失败情况】无迁移规则时返回 `event_migration_unavailable`；规则有登记但没有实现时返回
/// `event_migration_unimplemented`；载荷不符合当前有界契约时返回 payload 校验错误。
/// 【副作用】无，不会重写事件日志，也不会改变系统状态。
///
/// ⚠ 不要因为现有迁移器目前是原样返回，就删除版本分发和 payload 校验。版本边界仍用于
/// 阻止未登记的旧格式被投影；未来新增字段语义变化时还需要在这里提供真正的转换。
pub fn upcast_event_payload(
    kind: &str,
    from_major: u32,
    to_major: u32,
    payload: Value,
) -> Result<Value, String> {
    let migration = event_migration(kind, from_major, to_major)
        .ok_or_else(|| "event_migration_unavailable".to_owned())?;
    validate_event_payload(kind, &payload)?;
    match migration {
        "legacy_run_event_v0_to_v1"
        | "legacy_approval_event_v0_to_v1"
        | "legacy_invocation_event_v0_to_v1"
        | "legacy_action_event_v0_to_v1"
        | "legacy_session_event_v0_to_v1" => Ok(payload),
        _ => Err("event_migration_unimplemented".to_owned()),
    }
}

/// 通过事件登记表解析载荷，并保留它来自当前格式、旧格式还是未知类型的信息。
///
/// 【作用】为读取方提供统一入口：当前版事件按当前契约校验；旧版事件走具名迁移；可选未知
/// 事件保留为 `Opaque`，仅供查询检查。
/// 【调用者】目前由 `kiana-domain` 的契约测试调用；workspace 生产代码还没有直接接入此解析
/// 函数。写入事件时使用的是下面的 `validate_runtime_event()`。
/// 【输入】事件 `kind`、信封上的版本和 JSON payload。payload 会按值传入，成功结果拥有该值。
/// 【输出】`Current`、`Migrated` 或 `Opaque` 三类之一。
/// 【失败情况】关键事件族的未知 kind、未来不兼容主版本、未登记迁移、缺少必需 ID、未知字段
/// 或无效 payload 均返回错误；失败数据不能作为权威投影输入。
/// 【副作用】无；尤其不会回写旧事件，也不会执行事件表示的动作。
///
/// 数据流：存储事件 → 读取者提供 kind/version/payload → 本函数检查登记表 → 返回已验证载荷
/// 或只读不透明载荷 → 上层读取逻辑决定如何展示/投影。
pub fn resolve_event_payload(
    kind: &str,
    incoming: &SchemaVersion,
    payload: Value,
) -> Result<EventSchemaResolution, String> {
    let Some(spec) = event_kind_spec(kind) else {
        unknown_event_policy(kind)?;
        return Ok(EventSchemaResolution::Opaque {
            kind: kind.to_owned(),
            payload,
        });
    };

    if incoming.major == spec.version.major {
        check_event_schema_version(kind, incoming)?;
        validate_event_payload(kind, &payload)?;
        return Ok(EventSchemaResolution::Current { payload });
    }

    if incoming.major > spec.version.major {
        return Err("event_schema_version_incompatible".to_owned());
    }

    let migration = event_migration(kind, incoming.major, spec.version.major)
        .ok_or_else(|| "event_schema_version_incompatible".to_owned())?;
    let payload = upcast_event_payload(kind, incoming.major, spec.version.major, payload)?;
    Ok(EventSchemaResolution::Migrated {
        payload,
        migration,
        from: *incoming,
        to: spec.version,
    })
}

/// 按事件类型验证 JSON payload 的身份键、字段集合及类型专属语义。
///
/// 【调用者】EventStore 的现代信封校验经 `validate_runtime_event()` 间接调用；旧版 schema
/// 解析器和领域测试也直接调用。ACP adapter 在读取待审批快照时会用它检查每个
/// `approval.requested` 项，再额外核对 Run、Session 与审批/能力请求 ID 的绑定。
/// 【输入】事件类型名和 JSON 值。已知事件必须是 JSON object；未知类型按关键/可选策略处理。
/// 【输出】所有契约通过时返回 `Ok(())`，否则返回稳定的错误字符串供上游转换为存储错误。
/// 【副作用】无。此处只拒绝不合约数据，不会记录日志、重试请求、禁用账号或修复数据。
/// 【失败后果】通过 EventStore 写入时，错误会被映射为 `PortError::Failed`，该事件不会进入
/// 持久事实日志；是否向最终用户展示何种错误由更上层入口决定。
///
/// 【核心流程】先选出该 kind 对应的具体 payload 契约（部分事件存在两种生产信封），再验证
/// 关联身份、必需 ID、字段白名单，最后执行 run/model 等类型专属检查。
///
/// ⚠ 不要把不同生产者的字段白名单合并成宽松并集。相同事件名可能有直接请求信封和具 schema
/// 的 journal 信封；区分来源形状可以阻止一种生产者伪装成另一种事实。
pub fn validate_event_payload(kind: &str, payload: &Value) -> Result<(), String> {
    let Some(spec) = event_kind_spec(kind) else {
        return unknown_event_policy(kind).map(|_| ());
    };
    let object = payload
        .as_object()
        .ok_or_else(|| "event_payload_object_required".to_owned())?;
    // 审批的状态 journal 与直接决策响应共享 approved/denied 名称，但 schema 字段不同，
    // 因而必须分别选取精确契约。类似地，没有 run_id 的执行事件来自直接调用链，不能套用
    // 把 run 作为父级上下文的 invocation 规格；这里仅在确实缺少 run_id 时切换 identity 集合。
    let (required_ids, allowed_fields) = if matches!(kind, "approval.approved" | "approval.denied")
    {
        approval_decision_payload_contract(object, spec)?
    } else if matches!(
        kind,
        "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "invocation.dispatching"
            | "invocation.executing"
            | "execution.prepared"
            | "execution.result_committed"
    ) && object.get("run_id").is_none_or(Value::is_null)
    {
        (
            if kind == "execution.prepared" {
                EXECUTION_PREPARED_DIRECT_IDS
            } else {
                INVOCATION_DIRECT_IDS
            },
            spec.allowed_fields,
        )
    } else {
        model_payload_contract(kind, object, spec)?
    };
    // capability.decision 的 run_id 与 capability_request_id 要么同时出现，要么同时省略；
    // 只出现一个会让审计记录无法明确绑定到具体运行上下文，因此作为不完整身份拒绝。
    if kind == "capability.decision" {
        match (object.get("run_id"), object.get("capability_request_id")) {
            (None, None) => {}
            (Some(run_id), Some(capability_request_id))
                if !run_id.is_null() && !capability_request_id.is_null() => {}
            _ => return Err("event_capability_identity_pair_incomplete".to_owned()),
        }
    }
    if kind == "run.rejected" {
        // 拒绝事件本身必须说明拒绝理由；否则事件虽然能进入日志，却无法向请求者或审计读取者
        // 解释是哪条前置条件不成立。
        let reason = object
            .get("reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.trim().is_empty())
            .ok_or_else(|| "event_rejected_reason_required".to_owned())?;
        let _ = reason;
    }

    // required_ids 是身份约束，不等同于“字段可以缺省”：这里要求键存在且 JSON 值非 null。
    // ID 类型更细的格式校验由相应领域值对象/其他验证器负责。
    for id in required_ids {
        if !object.get(*id).is_some_and(|value| !value.is_null()) {
            return Err(format!("event_required_id_missing:{id}"));
        }
    }
    // 精确白名单是持久化兼容边界：未知键可能是拼写错误，也可能是尚未经过审计的新语义。
    // 两种情况都不能在没有更新契约的前提下静默进入事实日志。
    if object
        .keys()
        .any(|key| !allowed_fields.iter().any(|allowed| allowed == key))
    {
        return Err("event_payload_unknown_field".to_owned());
    }
    if kind == "run.rejected" {
        validate_run_rejected_context(object)?;
    }
    if kind == "run.completed" {
        validate_run_completed_metadata(object)?;
    }
    if kind == "run.model_turn" {
        if let Some(outcome) = object.get("outcome") {
            // serde_json 将动态 JSON 解码为领域类型 ModelOutcome，再调用该类型自己的不变量
            // 校验。这样事件契约检查的是业务对象约束，而不仅是“outcome 是一个 object”。
            let outcome: crate::ModelOutcome = serde_json::from_value(outcome.clone())
                .map_err(|_| "event_model_outcome_invalid".to_owned())?;
            outcome
                .validate()
                .map_err(|_| "event_model_outcome_invalid".to_owned())?;
        }
    }
    if kind == "model.prepared" && !object.contains_key("schema") {
        // 缺少 schema 的 model.prepared 属于旧的模型预算审计 envelope，而不是新模型调用事件；
        // 单独校验 permit、准备记录和 authority version 之间的绑定关系。
        validate_model_budget_prepared(object)?;
    }
    Ok(())
}

/// 校验 run.rejected 可选的评审上下文，使持久化诊断字段保持可读且结构稳定。
///
/// 【调用者】仅由 `validate_event_payload()` 在必需 ID 和字段白名单通过后调用。
/// 【输入】JSON object 的只读映射；字段可能缺失，因为拒绝原因可能正是输入资料不完整。
/// 【输出/失败】所有已提供字段形状正确时成功；非空文本字段为空/非字符串、author_role_id
/// 不是字符串，或 status 无法反序列化为 WorkPacketStatus 时返回带字段名的错误。
/// 【为什么 role 可以为空】缺失作者角色本身可以是评审被拒绝的原因；该值仅作诊断文本，
/// 不会因为它是字符串就授予任何角色或权限。
fn validate_run_rejected_context(object: &serde_json::Map<String, Value>) -> Result<(), String> {
    for field in [
        "command",
        "session_id",
        "packet_id",
        "author_session_id",
        "reviewer_session_id",
    ] {
        if let Some(value) = object.get(field) {
            if value.as_str().is_none_or(|text| text.trim().is_empty()) {
                return Err(format!("event_rejected_field_invalid:{field}"));
            }
        }
    }
    // 缺失作者角色本身就可能是拒绝评审的原因，因此诊断字符串允许为空；它不授予角色，
    // 但仍拒绝非字符串形状，保证读取方不必处理任意 JSON 类型。
    if object
        .get("author_role_id")
        .is_some_and(|role| !role.is_string())
    {
        return Err("event_rejected_field_invalid:author_role_id".to_owned());
    }
    if let Some(status) = object.get("status") {
        serde_json::from_value::<crate::WorkPacketStatus>(status.clone())
            .map_err(|_| "event_rejected_field_invalid:status".to_owned())?;
    }
    Ok(())
}

/// 校验 run.completed 终态事件可选附带的模型、token 用量和凭证引用。
///
/// 【调用者】由 `validate_event_payload()` 在完整 run 事件通过字段白名单后调用。
/// 【输入】完成事件载荷的 JSON object；三个元数据键都允许缺省，存在时必须符合其各自格式。
/// 【输出/失败】合法则成功；空模型 ID、非对象或多出字段/缺少 token 数字、无效凭证引用分别
/// 返回稳定错误码。
/// 【安全含义】secret_ref 只能是通过事件秘密扫描器检查的非空引用，不能在普通完成事件中
/// 偷渡凭证明文；token 用量仅接受无符号整数并且键只能是 input_tokens/output_tokens。
fn validate_run_completed_metadata(object: &serde_json::Map<String, Value>) -> Result<(), String> {
    if let Some(model_id) = object.get("model_id") {
        if model_id.as_str().is_none_or(|text| text.trim().is_empty()) {
            return Err("event_completed_model_id_invalid".to_owned());
        }
    }
    if let Some(usage) = object.get("usage") {
        let usage = usage
            .as_object()
            .ok_or_else(|| "event_completed_usage_invalid".to_owned())?;
        if usage
            .keys()
            .any(|field| !matches!(field.as_str(), "input_tokens" | "output_tokens"))
        {
            return Err("event_payload_unknown_field".to_owned());
        }
        if ["input_tokens", "output_tokens"]
            .iter()
            .any(|field| usage.get(*field).and_then(Value::as_u64).is_none())
        {
            return Err("event_completed_usage_invalid".to_owned());
        }
    }
    if let Some(reference) = object.get("secret_ref") {
        let reference = reference
            .as_str()
            .filter(|reference| !reference.trim().is_empty())
            .ok_or_else(|| "event_completed_secret_ref_invalid".to_owned())?;
        // SecretScanChannel::Event 表示扫描的是即将写进事件日志的文本；扫描失败时将其
        // 统一报告为无效引用，从而避免敏感值落入长期保存、可导出的事件事实。
        crate::scan_secret_sentinels(crate::SecretScanChannel::Event, reference)
            .map_err(|_| "event_completed_secret_ref_invalid".to_owned())?;
    }
    Ok(())
}

/// 根据审批决策 payload 的 schema 标记选择状态 journal 或直接决策契约。
///
/// 【调用者】仅由 `validate_event_payload()` 处理 `approval.approved`/`approval.denied` 时调用。
/// 【输入】payload 对象和该事件在登记表里的默认规格。
/// 【输出】返回该形状对应的必需 ID 与允许字段；无 schema 表示直接响应，有两个已知审批
/// schema 表示标准 journal transition。
/// 【失败】schema 存在但类型错误或名称未知时返回 `event_approval_payload_schema_invalid`。
/// 【设计原因】不同审批生产者写出的事实粒度不同，不能用字段并集放行混合信封。
fn approval_decision_payload_contract(
    object: &serde_json::Map<String, Value>,
    spec: &EventKindSpec,
) -> Result<(&'static [&'static str], &'static [&'static str]), String> {
    match object.get("schema") {
        Some(Value::String(schema))
            if schema == "kiana.approval-journal.v1" || schema == "kiana.approval.v1" =>
        {
            Ok((spec.required_ids, spec.allowed_fields))
        }
        None => Ok((
            APPROVAL_DIRECT_DECISION_IDS,
            APPROVAL_DIRECT_DECISION_FIELDS,
        )),
        _ => Err("event_approval_payload_schema_invalid".to_owned()),
    }
}

/// 为共用 model.* 名称的不同生产链路选择各自有限的载荷格式。
///
/// 【调用者】`validate_event_payload()` 在通常的事件规格之外调用它。
/// 【输入】事件类型、载荷对象和该类型的默认登记规格。
/// 【输出】预算 journal、具类型模型调用事件或 model attempt 生命周期各自的必需 ID 和字段白名单。
/// 【失败】schema 字段存在但类型/值不受支持时返回 `event_model_payload_schema_invalid`。
/// 【设计原因】不同生产者共用部分事件名，但事实内容不同；按 schema 选单一有界格式，可以避免
/// 把各格式字段取并集后接收一条混合来源、无法一致解释的事件。
fn model_payload_contract(
    kind: &str,
    object: &serde_json::Map<String, Value>,
    spec: &EventKindSpec,
) -> Result<(&'static [&'static str], &'static [&'static str]), String> {
    if !matches!(
        kind,
        "model.prepared" | "model.dispatching" | "model.settled"
    ) {
        return Ok((spec.required_ids, spec.allowed_fields));
    }
    match object.get("schema") {
        Some(Value::String(schema))
            if kind == "model.prepared" && schema == crate::MODEL_EVENT_SCHEMA =>
        {
            Ok((MODEL_EVENT_IDS, MODEL_EVENT_FIELDS))
        }
        Some(Value::String(schema)) if schema == crate::MODEL_ATTEMPT_EVENT_SCHEMA => {
            Ok((MODEL_ATTEMPT_LIFECYCLE_IDS, MODEL_ATTEMPT_LIFECYCLE_FIELDS))
        }
        None => match kind {
            "model.prepared" => Ok((MODEL_BUDGET_PREPARED_FIELDS, MODEL_BUDGET_PREPARED_FIELDS)),
            "model.dispatching" => Ok((MODEL_BUDGET_DISPATCH_FIELDS, MODEL_BUDGET_DISPATCH_FIELDS)),
            "model.settled" => Ok((&["run_id", "model_request_id"], MODEL_BUDGET_SETTLED_FIELDS)),
            _ => unreachable!("model kinds selected above"),
        },
        _ => Err("event_model_payload_schema_invalid".to_owned()),
    }
}

/// 检查旧模型预算链路的 model.prepared 载荷内部身份是否互相绑定。
///
/// 【调用者】当 model.prepared 没有显式 `schema` 字段时，由 payload 验证器调用。
/// 【输入】先前已通过顶层 required ID 检查的 JSON object，预期包含 permit、prepared 审计数据和
/// authority_versions。
/// 【输出/失败】Permit、审计字段、模型调用 ID、run/attempt/request hash 和 authority 版本全部
/// 一致时成功；JSON 结构无效、包含未知审计字段、身份不匹配或 authority 列表为空都会失败。
/// 【副作用】无；函数只验证，不会调用模型、预留预算或变更 authority。
///
/// 【为什么需要交叉比对】只检查每个字段“能否解码”不足以保证它们属于同一次调用；这里把
/// permit 与审计 event 的 run_id、model_request_id、request_hash 逐项绑定，避免拼接不同调用的
/// 记录后仍通过结构校验。
fn validate_model_budget_prepared(object: &serde_json::Map<String, Value>) -> Result<(), String> {
    // serde_json 将载荷中的 JSON 值反序列化为领域许可类型，后续可检查该类型的版本与 ID；
    // 解析失败意味着事件无法被当前代码安全理解，因此不应继续投影。
    let permit: crate::ModelCallPermit = serde_json::from_value(object["permit"].clone())
        .map_err(|_| "event_model_prepared_permit_invalid".to_owned())?;
    let audit = object["prepared"]
        .as_object()
        .ok_or_else(|| "event_model_prepared_audit_invalid".to_owned())?;
    let versions: Vec<crate::AggregateVersion> =
        serde_json::from_value(object["authority_versions"].clone())
            .map_err(|_| "event_model_prepared_authority_versions_invalid".to_owned())?;
    if audit
        .keys()
        .any(|key| !MODEL_BUDGET_AUDIT_FIELDS.contains(&key.as_str()))
    {
        return Err("event_payload_unknown_field".to_owned());
    }
    let call_id = audit
        .get("model_call_id")
        .and_then(Value::as_str)
        .and_then(crate::RequestId::parse_str);
    if call_id.is_none_or(|id| id.as_uuid().is_nil())
        || permit.schema != "kiana.model-call-permit.v1"
        || permit.permit_id.as_uuid().is_nil()
        || permit.run_id.as_uuid().is_nil()
        || permit.attempt_id.as_uuid().is_nil()
        || permit.request_hash.trim().is_empty()
        || permit.expires_at_unix_ms == 0
        || audit.get("schema") != Some(&Value::String(crate::MODEL_CALL_SCHEMA.to_owned()))
        || audit.get("run_id") != Some(&serde_json::json!(permit.run_id))
        || audit.get("model_request_id") != Some(&serde_json::json!(permit.attempt_id))
        || audit.get("request_hash") != Some(&Value::String(permit.request_hash))
    {
        return Err("event_model_prepared_identity_mismatch".to_owned());
    }
    if versions.is_empty()
        || versions.iter().any(|version| {
            version.aggregate_type.trim().is_empty() || version.aggregate_id.trim().is_empty()
        })
    {
        return Err("event_model_prepared_authority_versions_invalid".to_owned());
    }
    Ok(())
}

/// 验证完整运行事件信封是否属于本版本允许处理的范围。
///
/// 【调用者】主要由 `kiana-eventlog::event_store_core` 在追加现代事件前调用；Core 的数据撤销
/// 分页扫描、凭证恢复投影和 `kiana-quality::normalize` 也会在读取已提交事件时调用，用来拒绝
/// 不能安全归并的历史。领域契约测试会直接传入信封验证边界。
/// EventStore 对缺少聚合/版本元数据的 legacy frame 以及非对象旧载荷保留兼容路径，不会进入
/// 此 registry 验证；现代信封通过此函数后仍需通过 EventStore 的其他身份、审计、治理与秘密检查。
/// 【输入】由生产者构造的 `RuntimeEvent`，包含 kind、请求/关联信息和 JSON data。
/// 【输出】已登记事件通过 payload 契约时返回成功；关键未知事件和非法载荷返回错误。
/// 可选未知 kind 按 `PreserveOpaqueWithoutExecution` 留作不透明事实，不会在此解释其 payload。
/// 【副作用】本函数无副作用。失败不会触发重试、账号禁用或自动修复；EventStore 会记录
/// registry 校验失败诊断并拒绝追加。
///
/// 【核心流程】先确认 kind 对当前系统是已登记或允许不透明保留，再对已登记 kind 执行 payload
/// 验证。顶层事件信封字段的其他完整性约束由 RuntimeEvent/EventStore 的其他契约共同负责。
pub fn validate_runtime_event(event: &RuntimeEvent) -> Result<(), String> {
    unknown_event_policy(&event.kind)?;
    if event_kind_spec(&event.kind).is_some() {
        validate_event_payload(&event.kind, &event.data)?;
    }
    Ok(())
}
