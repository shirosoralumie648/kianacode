//! # 文件职责
//!
//! 本文件把事件日志中的能力调用事实折叠成 `InvocationProjection`：每个 `request_id` 对应一份
//! 当前已知状态、关联事件 ID、请求摘要和结果。投影是读取模型（read model），不是授权来源，
//! 也不负责再次派发 capability。
//!
//! # 在系统中的位置与数据流
//!
//! ```text
//! KianaHarness / ControlPlane / CapabilityBroker 执行路径
//!   -> 写入 run.capability_requested、approval、invocation、execution、capability 事件
//!   -> EventStore 保存不可变事件事实
//!   -> 调用方读取与 run 关联的 RuntimeEvent 切片
//!   -> 【本文件 project_invocations()】校验身份、顺序、结果和状态转移
//!   -> InvocationProjection 列表
//!      -> receipts.rs：收据中的调用摘要
//!      -> projection.rs：ControlPlane 读取/缓存的投影
//!      -> persistence_read_model.rs：持久化只读模型
//!      -> replay_diagnostics.rs / versioning.rs：恢复诊断与版本恢复
//! ```
//!
//! `project_invocations()` 是纯计算：它不读取数据库、不调用执行器，也不能撤销已发生的副作用。
//! 它的防护作用是拒绝身份不一致、非法状态变化和冲突结果，避免错误投影被上层当作可信状态。
//! 上层收到错误后的处理方式不同：只读模型可整体失败，收据可以记录投影错误，回放诊断可以
//! 生成未知/分歧诊断，因此本函数只返回错误，不在这里决定 UI 文案或重试策略。
//!
//! # 顺序和恢复边界
//!
//! 同一事件聚合流只在完整 stream metadata 存在时用 `stream_version` 排序；缺少 metadata 的
//! legacy 或混合输入保留调用方提供的顺序，不把事件 `sequence` 当作全局版本。进程恢复时，最后停在 `Dispatching` 或 `Executing`
//! 的调用降为 `Unknown`：事件只能证明曾开始分派/执行，不能证明中断瞬间的外部效果。
//! 只有后续明确的终态事件（terminal event）才能把该不确定性变成已知结果。
//!
//! # 状态与术语
//!
//! `Requested` 表示已有请求事实；`Queued` 表示等待调度；`PolicyChecked` 表示策略/关卡已评估；
//! `AwaitingApproval` 表示仍需审批；`Authorized` 表示允许进入后续分派；`Dispatching` 和
//! `Executing` 表示事件记录的分派/执行进度。`Succeeded`、`Failed`、`Cancelled`、`Denied` 是
//! 已知终态，`Unknown` 表示当前证据不足以判断实际副作用，不能等同于失败或成功。
//!
//! 本文件中的载荷（payload）指 RuntimeEvent.data，信封（envelope）指事件类型/聚合/序列等
//! 外层字段；投影（projection）是从事实重建的只读视图；终态（terminal state）是一个调用生命
//! 周期的结束点；摘要（digest）用于比较内容，不是带密钥的数字签名。
//! 能力（capability）在此指一次可能触发工具或其他副作用的操作请求；其授权与执行由
//! ControlPlane 和 Broker 负责，本文件只解释已经记录下来的生命周期事实。

use kiana_domain::{
    ApprovalId, CapabilityErrorCode, CapabilityExecutionState, CapabilityKind, CapabilityRequest,
    CapabilityResult, CapabilityResultReceipt, ExecutionId, InvocationId, RequestId, RiskLevel,
    RunId, RuntimeEvent,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// 从一个 run 的事件切片重建出的单次能力调用只读视图。
///
/// 一个事件可能描述请求、审批、分派、执行或最终结果；本结构把同一 `request_id` 下的这些事实
/// 汇总起来，供收据、投影缓存、只读模型和恢复诊断使用。它不是 `CapabilityRequest` 本身：
/// 公开字段用于解释进度，`request` / `approval_id` 是仅供进程内恢复使用的辅助数据。
///
/// `Serialize` 是 serde 提供的序列化 trait，把 Rust 结构转换为 JSON 等外部表示。字段上的
/// `#[serde(skip_serializing)]` 明确阻止恢复数据进入 JSON 输出：完整请求可能含有调用参数，
/// 调用方不应通过一般收据意外暴露这个内部对象。
#[derive(Clone, Debug, Serialize)]
pub struct InvocationProjection {
    /// 能力请求的稳定关联 ID；本文件以它为键，把请求生命周期中的多条事件合并成同一投影。
    pub request_id: RequestId,

    /// Runner/模型工具调用给出的本地调用 ID；有些直接 capability 请求没有此值，因此允许缺省。
    pub call_id: Option<String>,

    /// 请求的操作名称，由事件载荷提取；同一 request_id 的后续事件若也携带该值，必须一致。
    pub operation: Option<String>,

    /// 按事件顺序折叠出的最新状态；它表示日志已证明到达的生命周期点，不表示动作当前仍在运行。
    pub state: CapabilityExecutionState,

    /// 请求动作的摘要：优先取 `args_fingerprint`，缺少时取 `action_digest`；多个事件给出不同值
    /// 会使整个投影失败，避免一个 request_id 被解释成两份不同调用。
    pub args_fingerprint: Option<String>,

    /// terminal event 对应的结果 JSON；没有终态时为 `None`。未知结果仍可能保留载荷，但其状态
    /// 必须通过 `state == Unknown` 判断，不能仅凭结果字段推断动作成功。
    pub result: Option<Value>,

    /// 投影过程中归属于此请求的事件 ID，供追溯从请求到最终结果经历了哪些事实。
    pub event_ids: Vec<String>,

    /// 从 `run.capability_requested` 重建出的进程内请求对象；不序列化，避免参数进入公开收据。
    /// 重复请求事件只有在重建出的完整结构相等时才会被合并。
    #[serde(skip_serializing)]
    pub request: Option<CapabilityRequest>,

    /// 与此请求关联的审批 ID；用于进程内恢复时连接审批事实，不属于序列化后的投影字段。
    #[serde(skip_serializing)]
    pub approval_id: Option<ApprovalId>,
}

/// 从事件信封和 payload 中确定事件所属的 run。
///
/// 【调用者】`event_matches_run()` 用它在投影前筛选目标 run。
/// 【输入】单个 `RuntimeEvent`；run 身份可能来自 `aggregate_type == "run"` 时的 `aggregate_id`，
/// 也可能来自 payload 的 `run_id`。
/// 【输出】两处均缺少时返回 `None`；只有一处存在时采用该值；两处都存在且一致时返回共同 ID。
/// 【失败】run ID 不能解析，`aggregate_type` 指向 run 但缺少 `aggregate_id`，或两处 ID 冲突时
/// 返回错误，调用方不能猜测归属。
/// 【设计原因】事件可同时携带聚合信封和业务 payload；交叉验证能发现错误关联，防止其他 run 的
/// 执行事实混入目标投影。
fn event_run_id(event: &RuntimeEvent) -> Result<Option<RunId>, String> {
    let aggregate_run_id = match (
        event.aggregate_type.as_deref(),
        event.aggregate_id.as_deref(),
    ) {
        (Some("run"), Some(value)) => Some(
            RunId::parse_str(value).ok_or_else(|| "invocation_event_run_id_invalid".to_owned())?,
        ),
        (Some("run"), None) => return Err("invocation_event_run_id_invalid".to_owned()),
        _ => None,
    };
    let payload_run_id = match event.data.get("run_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(
            RunId::parse_str(value).ok_or_else(|| "invocation_event_run_id_invalid".to_owned())?,
        ),
        Some(_) => return Err("invocation_event_run_id_invalid".to_owned()),
    };
    if let (Some(payload), Some(aggregate)) = (payload_run_id, aggregate_run_id) {
        if payload != aggregate {
            return Err("invocation_event_run_id_conflict".to_owned());
        }
    }
    Ok(payload_run_id.or(aggregate_run_id))
}

/// 判断事件是否属于指定 run；无法可靠解析其归属时返回错误，而不是当作“不匹配”跳过。
///
/// 【调用者】`project_invocations()` 的第一阶段逐条调用。
/// 【输入】事件和本次投影目标 `run_id`。
/// 【输出】有可解析的 run ID 且相等时为 `true`；无 run ID 或属于其他 run 时为 `false`。
/// 【副作用】无；身份格式/冲突错误会向上传播并使整个投影失败。
fn event_matches_run(event: &RuntimeEvent, run_id: RunId) -> Result<bool, String> {
    Ok(event_run_id(event)?.is_some_and(|event_run_id| event_run_id == run_id))
}

/// 从多个兼容事件形状中提取同一个能力请求 ID。
///
/// 【调用者】投影主循环和 `request_from_event()` 都使用它。
/// 【输入】单个事件；候选位置包括通用 `capability_request_id`、旧的
/// `run.capability_requested.data.request_id`、审批事件的 `subject_request_id`、permit 内的
/// `request_id`，以及两类旧事件信封自身的 `request_id`。
/// 【输出】至少有一个候选且所有候选解码为同一 RequestId 时返回该 ID。
/// 【失败】候选无法反序列化或彼此冲突时返回 `None`。非 invocation 事件即使没有 request ID，
/// 也由主循环按“是否属于 invocation 事件族”决定是跳过还是报错。
/// 【设计原因】历史事件的 ID 放置方式并不完全一致；只允许明确的兼容位置，并要求交叉一致，
/// 不能按 operation 名称猜测，因为并发调用可能执行同一种 operation。
fn request_id_for(event: &RuntimeEvent) -> Option<RequestId> {
    let mut candidate = None;
    for value in [
        event.data.get("capability_request_id"),
        (event.kind == "run.capability_requested")
            .then(|| event.data.get("request_id"))
            .flatten(),
        matches!(event.kind.as_str(), "approval.approved" | "approval.denied")
            .then(|| event.data.get("subject_request_id"))
            .flatten(),
        event
            .data
            .get("permit")
            .and_then(|permit| permit.get("request_id")),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.is_null())
    {
        let parsed = serde_json::from_value::<RequestId>(value.clone()).ok()?;
        if candidate.is_some_and(|previous| previous != parsed) {
            return None;
        }
        candidate = Some(parsed);
    }
    candidate.or_else(|| {
        matches!(
            event.kind.as_str(),
            "run.tool_call" | "run.capability_requested"
        )
        .then_some(event.request_id)
    })
}

/// 读取 payload 中指定的字符串字段并拥有一份副本。
///
/// 缺键、null 或非字符串值均返回 `None`；调用方据此决定字段是否可选或是否冲突。此 helper
/// 不验证字段是否在 schema 白名单内，也不修改事件。
fn string_field(event: &RuntimeEvent, name: &str) -> Option<String> {
    event
        .data
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// 将明确代表调用最终结果的事件类型映射为 terminal 状态。
///
/// 【调用者】`project_invocations()` 先调用本函数，再回退到 `intermediate_state()`。
/// 【输入】事件类型、结果载荷及 effect 标志。
/// 【输出】terminal event 返回对应状态；非 terminal 事件返回 `None`。
/// 【失败】执行结果结构损坏或无法关联 request ID 时返回错误，整个调用投影失败。
/// 【设计原因】`run.tool_result` 可能只记录“工具没有执行”，原因既可能是取消，也可能是审批/关卡
/// 拒绝；不能把每个 not-executed 都视为取消。只有显式取消或 ResultUnknown 才从该事件生成终态，
/// 审批拒绝和策略阻止分别由自己的 authoritative event 说明。
fn terminal_state(event: &RuntimeEvent) -> Result<Option<CapabilityExecutionState>, String> {
    let state = match event.kind.as_str() {
        "execution.result_committed" => {
            let effect_known = event
                .data
                .get("effect_known")
                .and_then(Value::as_bool)
                .ok_or_else(|| "invocation_result_malformed".to_owned())?;
            let result = event
                .data
                .get("result")
                .ok_or_else(|| "invocation_result_malformed".to_owned())?;
            if !result.is_object() || result.get("success").and_then(Value::as_bool).is_none() {
                return Err("invocation_result_malformed".to_owned());
            }
            let request_id =
                request_id_for(event).ok_or_else(|| "invocation_request_id_missing".to_owned())?;
            let result = result_from_value(request_id, result)?;
            if !effect_known {
                CapabilityExecutionState::Unknown
            } else {
                result.execution_state()
            }
        }
        "capability.completed" => CapabilityExecutionState::Succeeded,
        "capability.failed" => CapabilityExecutionState::Failed,
        "capability.cancelled" => CapabilityExecutionState::Cancelled,
        "capability.result_unknown" => CapabilityExecutionState::Unknown,
        // `run.tool_result` 是 Runner 的工具账务/结果事实。审批拒绝和关卡阻止也可能留下
        // not-executed 结果，但权威拒绝事件分别是 approval.denied 和 run.capability_blocked；
        // 把所有 not-executed 都解释成取消，会在恢复时制造 Denied → Cancelled 的假冲突。
        "run.tool_result" => {
            let request_id =
                request_id_for(event).ok_or_else(|| "invocation_request_id_missing".to_owned())?;
            let result =
                result_from_value(request_id, event.data.get("result").unwrap_or(&Value::Null))?;
            if event.data.get("effect_known") == Some(&Value::Bool(false))
                || result.failure_code() == Some(CapabilityErrorCode::ResultUnknown)
            {
                CapabilityExecutionState::Unknown
            } else if event.data.get("cancelled") == Some(&Value::Bool(true))
                || result.failure_code() == Some(CapabilityErrorCode::Cancelled)
            {
                CapabilityExecutionState::Cancelled
            } else {
                return Ok(None);
            }
        }
        "approval.denied" | "run.capability_blocked" => CapabilityExecutionState::Denied,
        _ => return Ok(None),
    };
    Ok(Some(state))
}

/// 将非结果类生命周期事件映射为状态检查点。
///
/// 【调用者】`project_invocations()` 在 `terminal_state()` 没有给出结果时调用。
/// 【输入/输出】按事件 kind 和策略/关卡 decision 字段映射到 Requested、Queued、PolicyChecked、
/// AwaitingApproval、Authorized、Denied、Dispatching 或 Executing；无法产生状态点时返回 `None`。
/// 注意 `Denied` 本身是 terminal 状态，尽管它由这个“非直接结果映射”helper 返回，主循环仍会
/// 根据 `CapabilityExecutionState::is_terminal()` 做终态签名检查。
/// 【失败情况】JSON 下标读取依赖上游事件契约提供对应 gate 结构；整体事件格式的校验由 EventStore
/// 契约和投影调用方共同承担。
/// 【设计原因】事件可能压缩若干状态边，例如一次策略决策直接从 Requested 到 Authorized；合法性
/// 不是在这里重写，而由 `transition_allowed()` 委托给领域层的共享状态图。
fn intermediate_state(event: &RuntimeEvent) -> Option<CapabilityExecutionState> {
    match event.kind.as_str() {
        "run.capability_requested" => Some(CapabilityExecutionState::Requested),
        "run.queued" => Some(CapabilityExecutionState::Queued),
        "capability.decision" => match event.data["gate"]["decision"].as_str() {
            Some("awaiting_approval") => Some(CapabilityExecutionState::AwaitingApproval),
            Some("allowed") => Some(CapabilityExecutionState::Authorized),
            Some("denied") => Some(CapabilityExecutionState::Denied),
            _ => Some(CapabilityExecutionState::PolicyChecked),
        },
        "approval.requested" | "run.awaiting_approval" => {
            Some(CapabilityExecutionState::AwaitingApproval)
        }
        "approval.approved" => Some(CapabilityExecutionState::Authorized),
        "execution.prepared" | "invocation.dispatching" => {
            Some(CapabilityExecutionState::Dispatching)
        }
        "invocation.executing" => Some(CapabilityExecutionState::Executing),
        _ => None,
    }
}

/// 生成 terminal event 的规范化结果摘要，用于比较重复终态是否是同一事实。
///
/// 【调用者】`project_invocations()` 首次记录终态以及比较后续重复终态时调用。
/// 【输入】terminal event 和它关联的 RequestId。
/// 【输出】返回 JSON 内容摘要（digest）；它不是带密钥的数字签名，也不单独证明事件来自可信生产者。
/// 【失败】receipt 格式/digest 不合法、未提交、ID/attempt/effect/state 与事件不一致，或结果载荷
/// 无法读取时返回错误；调用方会拒绝整个投影。
/// 【核心流程】有 `result_receipt` 时先校验 receipt 自身，再根据事件类型交叉检查可用的身份和
/// 生命周期字段；对于 `execution.result_committed` 还从结果重建 receipt 比较。旧事件没有 receipt
/// 时按事件类型提取结果、移除保留的关联元数据键并对规范化 JSON 求 digest。
/// 【为什么这样设计】不同阶段会把相同业务输出放在不同 envelope 中；先规范化才能比较同一个
/// request_id 是否出现了互相矛盾的终态，同时维持旧日志可读。
fn terminal_signature(event: &RuntimeEvent, request_id: RequestId) -> Result<String, String> {
    if let Some(value) = event.data.get("result_receipt") {
        // from_json 会先解码结构，再验证 receipt schema、不变量及 receipt_digest；之后才检查
        // 它与当前 event 的 request、result 和 effect 元数据是否一致。
        let receipt = CapabilityResultReceipt::from_json(value)
            .map_err(|_| "invocation_result_receipt_invalid".to_owned())?;
        if receipt.request_id != request_id {
            return Err("invocation_result_receipt_request_id_conflict".to_owned());
        }
        if !receipt.committed {
            return Err("invocation_result_receipt_uncommitted".to_owned());
        }

        match event.kind.as_str() {
            "execution.result_committed" => {
                let execution_id = event
                    .data
                    .get("execution_id")
                    .and_then(|value| serde_json::from_value::<ExecutionId>(value.clone()).ok())
                    .ok_or_else(|| "invocation_result_receipt_execution_id_conflict".to_owned())?;
                let invocation_id = event
                    .data
                    .get("invocation_id")
                    .and_then(|value| serde_json::from_value::<InvocationId>(value.clone()).ok())
                    .ok_or_else(|| "invocation_result_receipt_invocation_id_conflict".to_owned())?;
                let attempt = event.data.get("attempt").and_then(Value::as_u64);
                if receipt.execution_id != Some(execution_id)
                    || receipt.invocation_id != Some(invocation_id)
                    || attempt != Some(u64::from(receipt.attempt))
                {
                    return Err("invocation_result_receipt_identity_conflict".to_owned());
                }
                let value = event
                    .data
                    .get("result")
                    .ok_or_else(|| "invocation_result_malformed".to_owned())?;
                let result: CapabilityResult = serde_json::from_value(value.clone())
                    .map_err(|_| "invocation_result_malformed".to_owned())?;
                if result.request_id != request_id {
                    return Err("invocation_result_request_id_conflict".to_owned());
                }
                let mut expected = CapabilityResultReceipt::from_result(
                    &result,
                    receipt.execution_id,
                    receipt.invocation_id,
                    receipt.attempt,
                    receipt.committed,
                )
                .map_err(|_| "invocation_result_receipt_invalid".to_owned())?;
                if event.data.get("stop_requested").and_then(Value::as_bool) != Some(true)
                    && expected.stop_confirmed.is_some()
                {
                    expected.stop_confirmed = None;
                    expected.receipt_digest = expected.digest();
                }
                if expected != receipt {
                    return Err("invocation_result_receipt_result_conflict".to_owned());
                }
                let effect_known = event
                    .data
                    .get("effect_known")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| "invocation_result_malformed".to_owned())?;
                if effect_known != receipt.effect_known {
                    return Err("invocation_result_receipt_effect_known_conflict".to_owned());
                }
                if let Some(value) = event.data.get("outcome_state") {
                    let outcome_state: CapabilityExecutionState =
                        serde_json::from_value(value.clone()).map_err(|_| {
                            "invocation_result_receipt_outcome_state_invalid".to_owned()
                        })?;
                    if outcome_state != receipt.dimensions.execution_state() {
                        return Err("invocation_result_receipt_outcome_state_conflict".to_owned());
                    }
                }
            }
            "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown" => {
                // 此分支校验 receipt 自身摘要以及状态、attempt 和事件中出现的生命周期标志；
                // 但它没有把扁平事件载荷重新构造成原始 result.output 再与 receipt.result_digest 比较。
                // ⚠ 因此这里的终态 digest 绑定的是 receipt 声明，不应被描述成已证明载荷内容相同。
                let expected_state = match event.kind.as_str() {
                    "capability.completed" => {
                        receipt.success
                            && receipt.dimensions.execution_state()
                                == CapabilityExecutionState::Succeeded
                    }
                    "capability.failed" => {
                        !receipt.success
                            && receipt.dimensions.execution_state()
                                == CapabilityExecutionState::Failed
                    }
                    "capability.cancelled" => {
                        !receipt.success
                            && receipt.dimensions.execution_state()
                                == CapabilityExecutionState::Cancelled
                    }
                    "capability.result_unknown" => {
                        !receipt.success
                            && receipt.dimensions.execution_state()
                                == CapabilityExecutionState::Unknown
                    }
                    _ => unreachable!(),
                };
                if !expected_state {
                    return Err("invocation_result_receipt_state_conflict".to_owned());
                }
                if event
                    .data
                    .get("attempt")
                    .is_some_and(|value| value.as_u64() != Some(u64::from(receipt.attempt)))
                {
                    return Err("invocation_result_receipt_attempt_conflict".to_owned());
                }
                for (field, expected) in [
                    ("effect_started", Some(receipt.effect_started)),
                    ("effect_known", Some(receipt.effect_known)),
                    ("zero_effect", Some(receipt.zero_effect)),
                    ("fenced", Some(receipt.fenced)),
                    ("stop_confirmed", receipt.stop_confirmed),
                ] {
                    if let Some(value) = event.data.get(field) {
                        let actual = match value {
                            Value::Null if expected.is_none() => None,
                            Value::Bool(value) => Some(*value),
                            _ => {
                                return Err(
                                    "invocation_result_receipt_lifecycle_conflict".to_owned()
                                );
                            }
                        };
                        if actual != expected {
                            return Err("invocation_result_receipt_lifecycle_conflict".to_owned());
                        }
                    }
                }
            }
            _ => return Err("invocation_result_receipt_kind_invalid".to_owned()),
        }

        return Ok(kiana_domain::json_digest(&serde_json::json!({
            "request_id": receipt.request_id,
            "attempt": receipt.attempt,
            "success": receipt.success,
            "dimensions": receipt.dimensions,
            "result_digest": receipt.result_digest,
            "effect_started": receipt.effect_started,
            "effect_known": receipt.effect_known,
            "zero_effect": receipt.zero_effect,
            "fenced": receipt.fenced,
            "stop_confirmed": receipt.stop_confirmed,
            "committed": receipt.committed,
        })));
    }

    // execution 结果、Runner 工具结果和 capability 终态使用不同 envelope。旧格式兼容路径先
    // 取出各自代表的业务结果，并移除保留的关联元数据，再比较规范摘要；日志中尚未携带
    // result_receipt 的历史事件仍可通过这条较弱的旧格式路径读取。
    let mut result = match event.kind.as_str() {
        "execution.result_committed" => event
            .data
            .get("result")
            .and_then(|result| result.get("output"))
            .or_else(|| event.data.get("result"))
            .cloned()
            .unwrap_or(Value::Null),
        "run.tool_result" => event.data.get("result").cloned().unwrap_or(Value::Null),
        // capability.* 事件使用扁平载荷：handler 输出与关联 metadata 写在同一个 object。
        // 不要把用户/MCP 输出中名为 result 的业务字段当作 envelope 并剥掉；它属于持久结果本身。
        _ => event.data.clone(),
    };
    if let Value::Object(object) = &mut result {
        for key in [
            "run_id",
            "session_id",
            "capability",
            "operation",
            "cell_id",
            "capability_grant_id",
            "budget_lease_id",
            "capability_request_id",
            "approval_id",
            "request_hash",
            "decision",
            "scope",
            "expires_at_unix_ms",
        ] {
            object.remove(key);
        }
    }
    Ok(kiana_domain::json_digest(&result))
}

/// 判断事件类型是否属于“缺少 request_id 时必须报错”的 invocation 事件族。
///
/// 该函数不是主过滤器：主循环会先按 run 筛选事件；其他事件若能通过允许的兼容位置关联到
/// request_id，仍可能贡献事件 ID 或合并字段。它只区分无 request_id 的事件应当失败，还是可跳过。
fn is_invocation_event(kind: &str) -> bool {
    matches!(
        kind,
        "run.tool_call"
            | "run.capability_requested"
            | "capability.decision"
            | "approval.requested"
            | "run.awaiting_approval"
            | "approval.approved"
            | "approval.denied"
            | "run.capability_blocked"
            | "execution.prepared"
            | "invocation.dispatching"
            | "invocation.executing"
            | "execution.result_committed"
            | "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "run.tool_result"
    )
}

/// 把事件中的结果 JSON 转为统一的 `CapabilityResult` 领域对象。
///
/// 【调用者】`terminal_state()` 和 `terminal_signature()` 用它处理 execution/tool result。
/// 【输入】预期的 request ID 与结果 JSON object。
/// 【输出】完整结构可反序列化时返回原对象（并验证 request_id）；旧格式不完整时构造兼容结果，
/// `success` 缺失/类型错误默认按 `false` 处理，`output` 优先取嵌套字段，否则保留整个对象。
/// 【失败】非 object 返回 `invocation_result_malformed`；完整对象中的 request_id 不一致会拒绝。
/// 其他无法完全解码的旧形状会走兼容路径，因此调用者仍要依赖事件类型和结果 receipt 做后续检查。
/// 【设计原因】历史事件的 result 形状不统一；集中归一化，避免每个投影分支各自解释旧字段。
fn result_from_value(request_id: RequestId, value: &Value) -> Result<CapabilityResult, String> {
    if !value.is_object() {
        return Err("invocation_result_malformed".to_owned());
    }
    if let Ok(result) = serde_json::from_value::<CapabilityResult>(value.clone()) {
        if result.request_id != request_id {
            return Err("invocation_result_request_id_conflict".to_owned());
        }
        return Ok(result);
    }
    let success = value
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let output = value
        .get("output")
        .cloned()
        .unwrap_or_else(|| value.clone());
    Ok(CapabilityResult {
        request_id,
        success,
        output,
        evidence_refs: Vec::new(),
    })
}

/// 合并同一 request_id 在不同事件中重复提供的可选字符串字段。
///
/// `current` 尚无值时接纳 `next`；只有一侧缺值时保留已知值；两侧都有值但内容不同则返回
/// 调用方提供的冲突错误码。它不判断字符串业务格式，只建立“同一请求的重复描述必须一致”约束。
fn merge_optional_field(
    current: &mut Option<String>,
    next: Option<String>,
    conflict: &'static str,
) -> Result<(), String> {
    let Some(next) = next else {
        return Ok(());
    };
    if let Some(previous) = current.as_ref() {
        if previous != &next {
            return Err(conflict.to_owned());
        }
    } else {
        *current = Some(next);
    }
    Ok(())
}

/// 将状态转移检查委托给领域层定义的共享状态图。
///
/// 【调用者】`project_invocations()` 在更新投影状态之前调用。
/// 【输入】上一个状态、事件映射出的下一个状态及产生该状态的事件 kind。
/// 【输出】事件边界允许此转移时为 `true`。
/// 【规则来源】具体允许边由 `kiana-domain::CapabilityExecutionState::can_transition_via()` 定义：
/// 它在纯状态图外只补充少量与事件 kind 绑定的压缩边，例如 capability.decision 可直接从
/// Requested 到 Authorized/AwaitingApproval/Denied。这里不复制整张表，避免状态规则在两处漂移。
/// terminal 状态的重复事实还要经过 `project_invocations()` 的额外检查：状态相同且规范 digest
/// 相同时才按幂等重复接受；单靠领域 helper 的“同状态可转移”不足以验证终态结果。
fn transition_allowed(
    previous: CapabilityExecutionState,
    next: CapabilityExecutionState,
    event: &RuntimeEvent,
) -> bool {
    previous.can_transition_via(next, event.kind.as_str())
}

/// 从 `run.capability_requested` 事件恢复结构化 `CapabilityRequest`。
///
/// 【调用者】`project_invocations()` 用它初始化并交叉比较某个请求的完整定义。
/// 【输入】任意事件；仅匹配目标 kind 后才会读取 payload 字段。
/// 【输出】kind 不匹配或字段缺少/解码失败时返回 `None`；完整时构造 request_id、capability、
/// operation、arguments、risk，并恢复可选 scope / cell / grant / budget lease。
/// 【副作用】无；JSON 到领域枚举的 serde_json 反序列化只做形状解码，不执行 capability。
/// 【失败处理】主循环会把 `run.capability_requested` 的 `None` 转为
/// `invocation_request_malformed`，不能静默创建一个缺少授权上下文的请求。
fn request_from_event(event: &RuntimeEvent) -> Option<CapabilityRequest> {
    if event.kind != "run.capability_requested" {
        return None;
    }
    Some(CapabilityRequest {
        request_id: request_id_for(event)?,
        capability: serde_json::from_value::<CapabilityKind>(event.data.get("capability")?.clone())
            .ok()?,
        operation: event.data.get("operation")?.as_str()?.to_owned(),
        arguments: event.data.get("arguments")?.clone(),
        risk: serde_json::from_value::<RiskLevel>(event.data.get("risk")?.clone()).ok()?,
        execution_scope: event
            .data
            .get("execution_scope")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        cell_id: event
            .data
            .get("cell_id")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .flatten(),
        capability_grant_id: event
            .data
            .get("capability_grant_id")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .flatten(),
        budget_lease_id: event
            .data
            .get("budget_lease_id")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .flatten(),
    })
}

/// 从一段事件事实中重建目标 run 下每个 capability request 的当前投影视图。
///
/// 【调用者】由 receipts、ControlPlane 投影缓存、持久化只读模型、回放诊断和版本恢复路径调用；
/// 它们分别决定把错误写入收据、返回读取失败或生成诊断。此函数不决定最终用户看到的文案。
///
/// 【输入】目标 `run_id` 与调用方读取到的 `RuntimeEvent` 切片。切片可以混有其他 run 的事件，
/// 但其跨聚合输入顺序必须由调用方提供；本函数只在确认所有目标事件属于同一聚合流时重排。
///
/// 【输出】按 request_id 字典序排列的投影列表。每个元素的 `event_ids` 保留本次折叠事件顺序；
/// 列表顺序本身不是事件发生时间顺序。
///
/// 【副作用】无：不访问 EventStore、不调用 Runner/Broker、不执行请求，也不改变事件或系统状态。
///
/// 【异常/失败情况】run 身份无法解析/冲突、事件身份链接不一致、事件 ID 重复、关键调用缺少
/// request ID、请求/审批字段冲突、无效结果 receipt、终态签名冲突或状态边不允许时返回错误。
/// 错误直接返回调用方；不会在本函数重试、自动修复、禁用账号或把失败吞成空投影。
///
/// 【核心流程】按 run 过滤并验证归属 → 同流时按版本排序 → 按 request ID 累积事件和请求字段
/// → 识别状态点并验证状态边 → 校验重复终态的状态及 digest → 把中断在分派/执行阶段的调用标为
/// Unknown → 返回确定顺序的只读结果。
///
/// 【为什么这么设计】事件日志是调用的持久事实源，投影不能用“当前内存状态”补造执行结果。
/// 对跨流顺序不作推测、对冲突记录失败关闭，并把恢复时无法确认的副作用标成 Unknown，能防止
/// 上层把缺少证据误当成成功或安全重试许可。
pub fn project_invocations(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<InvocationProjection>, String> {
    // 步骤 1：筛选目标 run。保留输入下标，是因为同流版本号可能相同，后面用原始位置稳定打破平手。
    let mut ordered_events = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event_matches_run(event, run_id) {
            Ok(true) => Some(Ok((index, event))),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, _>>()?;

    // 步骤 2：只有聚合类型和聚合 ID 都相同时，事件才共享可比较的 stream version。
    // 不同聚合流没有全局版本序列，因此不能把各自的版本号直接混排。
    let same_stream = ordered_events.first().is_some_and(|(_, first)| {
        first.aggregate_type.is_some()
            && first.aggregate_id.is_some()
            && first.stream_version.is_some()
            && ordered_events.iter().all(|(_, event)| {
                event.aggregate_type == first.aggregate_type
                    && event.aggregate_id == first.aggregate_id
                    && event.stream_version.is_some()
            })
    });
    if same_stream {
        // 只按已验证的同流版本排序；原始下标保证版本相同的输入保持稳定。
        ordered_events
            .sort_by_key(|(index, event)| (event.stream_version.unwrap_or(event.sequence), *index));
    }

    // projections 按 request_id 排序，保证返回投影列表的顺序确定；它不是按事件时间排序。
    let mut projections: BTreeMap<String, InvocationProjection> = BTreeMap::new();
    // terminals 记录每个请求首次到达的终态及 digest；后续终态必须是同状态、同结果才算重放。
    let mut terminals: HashMap<String, (CapabilityExecutionState, String, String)> = HashMap::new();
    // 仅对目标 run 中处理到的事件去重；event_id 重复表示输入切片重复包含同一事实。
    let mut seen_event_ids = HashSet::new();

    // 步骤 4：依事件顺序把原始事实归并成每个 request_id 的状态和上下文。
    for (_, event) in ordered_events {
        // 在合并前验证 RuntimeEvent 内部的身份交叉引用，避免先污染投影再发现身份不一致。
        event
            .validate_identity_links()
            .map_err(|error| format!("invocation_identity_links_invalid:{error}"))?;

        // 防止同一事件被两个读取/拼接来源重复交给投影器。
        if !seen_event_ids.insert(event.event_id.to_string()) {
            return Err("invocation_duplicate_event_id".to_owned());
        }

        // 已知 invocation 事件没有关联 ID 时不能猜测归属；非 invocation 事件若没有候选 ID，则
        // 与本投影无关，可以跳过。若其他事件能明确提供 request_id，它仍可能贡献追溯/上下文信息。
        let Some(id) = request_id_for(event) else {
            if is_invocation_event(&event.kind) {
                return Err("invocation_request_id_missing".to_owned());
            }
            continue;
        };

        let key = id.to_string();

        // 首次遇到某 ID 时以 Requested 作为折叠起点；后续事件会通过状态边逐步更新它。
        let entry = projections
            .entry(key.clone())
            .or_insert_with(|| InvocationProjection {
                request_id: id,
                call_id: None,
                operation: None,
                state: CapabilityExecutionState::Requested,
                args_fingerprint: None,
                result: None,
                event_ids: Vec::new(),
                request: None,
                approval_id: None,
            });

        // 保留所有能关联此 request_id 的事件，而不仅是会改变状态的事件，方便审计追踪。
        entry.event_ids.push(event.event_id.to_string());

        // 初始请求事件缺少 capability/operation/risk 等定义字段时，不能建立可恢复请求。
        if event.kind == "run.capability_requested" && request_from_event(event).is_none() {
            return Err("invocation_request_malformed".to_owned());
        }

        // request_from_event 仅识别 run.capability_requested；重复定义必须逐字段完全相同。
        if let Some(request) = request_from_event(event) {
            if let Some(previous) = entry.request.as_ref() {
                // 相同 ID 却出现不同参数时立即拒绝，避免后续审批/结果被归到错误的请求定义。
                if previous != &request {
                    return Err("invocation_request_conflict".to_owned());
                }
            }
            entry.request = Some(request);
        }

        // 审批事件可补充关联 ID；同一请求不能被拼接到不同审批记录上。
        if let Some(value) = event.data.get("approval_id") {
            let approval_id = serde_json::from_value::<ApprovalId>(value.clone())
                .map_err(|_| "invocation_approval_id_invalid".to_owned())?;
            if entry
                .approval_id
                .is_some_and(|previous| previous != approval_id)
            {
                return Err("invocation_approval_conflict".to_owned());
            }
            entry.approval_id = Some(approval_id);
        }

        // 将不同事件上重复出现的调用描述合并；首次出现时填充，之后只接受相同值。
        merge_optional_field(
            &mut entry.call_id,
            string_field(event, "call_id"),
            "invocation_call_id_conflict",
        )?;
        merge_optional_field(
            &mut entry.operation,
            string_field(event, "operation"),
            "invocation_operation_conflict",
        )?;
        if let Some(args_fingerprint) =
            string_field(event, "args_fingerprint").or_else(|| string_field(event, "action_digest"))
        {
            merge_optional_field(
                &mut entry.args_fingerprint,
                Some(args_fingerprint),
                "invocation_action_digest_conflict",
            )?;
        }

        // 明确的结果事件优先映射为 terminal；其余再看请求、决策、审批或执行准备等状态检查点。
        let next = terminal_state(event)?.or_else(|| intermediate_state(event));
        let Some(next) = next else {
            // 即使不改变状态，关联事件 ID 和可选上下文已记录；此事件不是状态机边，继续处理下一条。
            continue;
        };

        // 到达终态后只允许重复同一终态事实；新的中间状态或不同结果都不能覆盖它。
        if let Some((previous, previous_kind, previous_signature)) = terminals.get(&key) {
            // 先确认新状态仍是终态，再用结果 digest 区分“同一终态重放”和“冲突终态”。
            if !next.is_terminal() {
                return Err("invocation_state_transition_invalid".to_owned());
            }
            if *previous != next || previous_signature != &terminal_signature(event, id)? {
                return Err("invocation_terminal_conflict".to_owned());
            }
            let _ = previous_kind;
            continue; // 同状态、同 digest 的重复终态已验证；投影状态和结果无需再次覆盖。
        }

        // 保存首次终态供后续事件比较，并从该事件载荷提取面向调用方的结果。
        if next.is_terminal() {
            terminals.insert(
                key,
                (next, event.kind.clone(), terminal_signature(event, id)?),
            );
            // execution/tool result 使用嵌套 result；旧的扁平 capability 事件以整个 data 作为结果。
            entry.result = Some(event.data.get("result").unwrap_or(&event.data).clone());
        }

        // 除 Runner 的旧 tool_call 兼容形状外，状态变化前必须已经重建出原始 capability 请求。
        if entry.request.is_none() && event.kind != "run.tool_call" {
            return Err("invocation_request_missing".to_owned());
        }

        // 状态规则集中在 kiana-domain，拒绝无效边后不返回部分投影。
        if !transition_allowed(entry.state, next, event) {
            return Err("invocation_state_transition_invalid".to_owned());
        }

        // 仅在身份、请求、结果和状态边都通过后推进当前状态。
        entry.state = next;
    }

    // 步骤 5：把事件流结束在进行中状态的调用标为 Unknown。恢复只能看到最后一条日志，无法
    // 判断进程停止前外部执行器是否已完成副作用，因此不能伪造成功，也不能假设可安全重试。
    for entry in projections.values_mut() {
        if matches!(
            entry.state,
            CapabilityExecutionState::Dispatching | CapabilityExecutionState::Executing
        ) {
            entry.state = CapabilityExecutionState::Unknown;
        }
    }

    // BTreeMap::into_values 保持 request_id 字典序；该稳定顺序便于比较/序列化，但不是时间顺序。
    Ok(projections.into_values().collect())
}
