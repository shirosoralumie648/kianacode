//! 派发层：Core 的最后一步，把「已授权的动作」变成一次真实的执行许可（permit）。
//!
//! # 分发的职责与流程
//!
//! 派发层位于授权层之下，Core 与 Runner/Broker 之间的边界。它完成以下工作：
//! 1. 接收已授权的能力请求（AuthorizedCapabilityRequest）
//! 2. 验证权限、约束、授权版本的一致性
//! 3. 发行一张执行许可（DispatchPermit）——这是一个可核查、有时限的凭证
//! 4. 将许可交给 Broker 执行，并追踪结果返回
//! 5. 把结果投递给 Runner（harness），维持与运行的同步
//!
//! # 为什么 permit 是必须的
//!
//! 「已授权」是一句对过去的结论。「允许这次执行开始」是一张对**现在**的凭证。
//! 两者之间隔着排队、等待、重试、重连——这段时间里，授权可能被撤销、
//! 租约可能过期、epoch 可能推进、公司可能关闭。
//!
//! permit 就是「授权在这一刻仍然有效」这个事实的可验证记录。Broker 收到它时，
//! 可以检查权限当前版本、Cell 预留、预算约束，而不用假设「Core 说过 OK」。
//!
//! 没有 permit，就只能靠「我记得我批过」——而撤销之后，那句记忆就是安全漏洞。
//!
//! # fail-closed 原则
//!
//! 派发在不确定时拒绝。若版本冲突、若收据损坏、若身份不匹配，一律返回错误。
//! 允许执行的条件必须**完整可追踪**：从 request_id 到 execution_id 到 event digest，
//! 每次状态转换都带着预期版本提交，防止并发更新被静默覆盖。这里的原子性只针对
//! **单个 EventStore transition batch**：准备许可、Broker 执行副作用、提交执行结果和
//! 回灌 Runner 是分开的阶段，不能合成一个跨数据库与外部 handler 的事务。尤其在
//! `execution.prepared` 已提交后发生错误时，准备记录可能仍存在，结果也可能只能标为
//! `result_unknown`；不得据此声称整个派发“要么全做、要么完全没发生”。
//!
//! # 派发的三阶段
//!
//! 1. **准备阶段**（dispatch_authorized）
//!    - 验证权限、拒绝策略、授权版本
//!    - 吸收 Approval 消费版本变化
//!    - 构造 DispatchPermit（带时限、带权限快照）
//!    - 原子化提交 execution.prepared 事件
//!
//! 2. **执行阶段**（Broker 侧，不在本模块）
//!    - Broker 验证 permit 内容与权限、时限、Cell 预留
//!    - Broker 执行能力操作
//!    - Broker 收集执行结果
//!
//! 3. **结果投递阶段**（deliver_capability_result）
//!    - 验证结果来自已授权的 permit 的执行
//!    - 验证结果的收据（CapabilityResultReceipt）完整且未被篡改
//!    - 原子化提交 execution.result_committed 事件
//!    - 向 Runner 报告执行完成
//!
//! # 与授权系统的耦合
//!
//! 派发紧密依赖授权系统的状态：
//! - **Authority**（权限授权）：决定这个 session 对这个 project 的基础权限
//! - **Approval**（审批）：某些高风险操作需要额外的时限审批
//! - **Cell/Grant/Budget/Lease**（资源配额）：限制并发、限制配额、限制租约期
//! - **Company**（企业配置）：企业停用会导致派发拒绝
//!
//! 派发在 permit 中快照了这些版本号，使得后续 Broker 的验证可以离线进行。
//! 若在提交时版本已经不匹配，就以 CAS 冲突拒绝。
use super::*;
use kiana_domain::{
    derived_request_id, json_digest, AggregateVersion, CapabilityResultReceipt, CommandReceipt,
    CommitOutcome, DispatchPermit, ExecutionId, InvocationId, TransitionBatch, TurnId,
    DISPATCH_PERMIT_SCHEMA, DISPATCH_PERMIT_VERSION,
};

/// 结果投递的来源数据结构。
///
/// 当投递一个能力执行结果时，需要验证这个结果的来源：
/// - 它来自哪个 permit 的执行（event）
/// - 收据是否完整且未被篡改（receipt）
/// - 这个结果是否已经被 CAS 确认过（commit）
/// - 权限版本号是否一致（version）
struct CommittedDeliverySource {
    /// 执行结果提交的事件（execution.result_committed）
    event: RuntimeEvent,
    /// 结果的收据，包含执行状态、效果标记、安全边界等信息
    receipt: CapabilityResultReceipt,
    /// CAS 操作的确认收据，证明这次提交成功了
    commit: CommandReceipt,
    /// 权限流的当前版本，用于检测并发修改
    version: AggregateVersion,
}

/// 从已读回的 `run.prompt` 事件中解析出当前 turn 身份。
///
/// `deliver_capability_result` 会从 run 事件流找到当前活动 turn，再把该事件传入这里。函数
/// 逐项核对 `turn_id`、`TurnIdentity`、传入 `run_id` 和事件载荷中的 run/session 字段，确保
/// 结果不会因 JSON 某个字段被改写而投递到另一个 run/turn。
/// 成功返回 turn id；任一字段缺失、无法反序列化或相互矛盾时返回
/// `turn_identity_invalid`。这里只验证 EventStore 中的身份一致性，不发起执行。
fn committed_prompt_turn(prompt: &RuntimeEvent, run_id: RunId) -> Result<TurnId, PortError> {
    let turn_id: TurnId = serde_json::from_value(prompt.data["turn_id"].clone())
        .map_err(|_| dispatch_error("turn_identity_invalid"))?;
    let identity: kiana_domain::TurnIdentity = serde_json::from_value(prompt.data["turn"].clone())
        .map_err(|_| dispatch_error("turn_identity_invalid"))?;
    if identity.validate().is_err()
        || turn_id.as_uuid().is_nil()
        || identity.run_id != run_id
        || identity.turn_id != turn_id
        || prompt.data["run_id"] != json!(run_id)
        || prompt.data["session_id"] != json!(identity.session_id)
    {
        return Err(dispatch_error("turn_identity_invalid"));
    }
    Ok(turn_id)
}

/// 构造写入 `execution.result_committed` 事件的结果负载。
///
/// 该事件将以下内容与一次 execution permit 绑定：
/// - 执行身份（run/turn/invocation/execution）
/// - 结果本身与执行状态
/// - 结果收据（effect_started、effect_known、zero_effect、fenced）
/// - 取消状态（stop_requested、stop_confirmed）
/// - 调用身份与待处理项
///
/// `effect_known`、`zero_effect`、`fenced` 与取消字段让后续读取者区分“明确无副作用”“已知结果”
/// 和“效果未知”。事件记录的是本次提交被确认的事实；它本身不证明 handler 对现实效果的声明正确。
/// 当前 dispatch 流把 attempt 固定记为 1：这是本次 execution 的尝试编号，不表示整个 run 只调用过一次工具。
fn committed_result_payload(
    permit: &DispatchPermit,
    result: &CapabilityResult,
    receipt: &CapabilityResultReceipt,
    invocation: Option<Value>,
    stop_requested: bool,
    confirmed: bool,
) -> Value {
    json!({
        "run_id":permit.run_id,"turn_id":permit.turn_id,
        "invocation_id":permit.invocation_id,"execution_id":permit.execution_id,
        "capability_request_id":permit.request_id,"result":result,"attempt":1,
        "outcome_state":result.execution_state(),"outcome_ready":true,
        "effect_started":receipt.effect_started,"effect_known":receipt.effect_known,
        "zero_effect":receipt.zero_effect,"fenced":receipt.fenced,
        "stop_state":if stop_requested { if confirmed {"confirmed"} else {"unconfirmed"} } else {"not_requested"},
        "stop_requested":stop_requested,"stop_confirmed":stop_requested.then_some(confirmed),
        "result_receipt":receipt,"invocation":invocation,
    })
}

/// 检查 `execution.result_committed` 顶层取消字段是否与嵌套结果收据表达同一状态。
///
/// `CapabilityResultReceipt.stop_confirmed` 是可选值：`None` 表示没有请求取消，`Some(true)` 表示停止已
/// 确认，`Some(false)` 表示请求过停止但 handler 未确认。事件同时保留顶层 `stop_requested`、`stop_state`、
/// `stop_confirmed` 供投影读取；如果两处值不一致，读模型就会得到两个彼此竞争的生命周期事实，因此这里
/// 要求顶层严格表示相同的三态组合。
///
/// 本函数只比较已构造的 JSON 与 receipt，不读取取消系统、不发送 stop 请求、不更改事件，也不推断现实中
/// 的副作用是否已撤销；不匹配时返回 false，由调用方将结果按无效/Unknown 路径拒绝。
fn committed_stop_metadata_matches(source: &Value, receipt: &CapabilityResultReceipt) -> bool {
    let stop_requested = source.get("stop_requested").and_then(Value::as_bool);
    let stop_state = source.get("stop_state").and_then(Value::as_str);
    let stop_confirmed = match source.get("stop_confirmed") {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::Null) => None,
        _ => return false,
    };

    match receipt.stop_confirmed {
        None => {
            stop_requested == Some(false)
                && stop_state == Some("not_requested")
                && stop_confirmed.is_none()
        }
        Some(confirmed) => {
            stop_requested == Some(true)
                && stop_state
                    == Some(if confirmed {
                        "confirmed"
                    } else {
                        "unconfirmed"
                    })
                && stop_confirmed == Some(confirmed)
        }
    }
}

/// 获取项目根目录在本次检查时观察到的文件系统身份信息，而不仅仅记录路径字符串。
///
/// 【为什么不能只用路径字符串】
/// 路径是不稳定的引用：目录可以被改名、被移动、甚至被一个全新的目录替换。
/// 一个只依赖路径的守卫无法回答「这还是当初那个物理目录吗」这个问题。
/// 将规范化路径和可用的文件系统元数据一同放进许可，能让消费许可的一方检查观察结果是否变化。
///
/// 【Unix 上采用 device + inode 组合】
/// `device` 标识所在文件系统，`inode` 是该文件系统当前对象的元数据编号；同一文件系统内
/// 改名通常不会改变 inode。它比路径字符串更具体，但不是密码学身份：对象删除后 inode 可能
/// 被重用，跨文件系统移动也会得到新身份。这里读取的是某一时刻的 metadata，并没有持有
/// 目录文件句柄或锁住路径；后续消费许可时还要重新读取并比较，不能把它解释成永久防替换保证。
///
/// 【非 Unix 系统上降级并明说降级了】
/// 拿不到 dev/ino 时，返回的对象包含 `identity_strength: "path_only"` 标记。
/// 这个标记被序列化进许可载荷，使用方能看到「这次的身份验证是弱的」。
/// 这比沉默地降级然后假装身份很强要诚实得多。
///
/// # 返回
///
/// 成功时返回 JSON，包含：
/// - `canonical_root`：规范化后的绝对路径
/// - `device`、`inode`：Unix 上的物理身份（如果可用）
/// - 或 `identity_strength`：非 Unix 上的降级标记
///
/// 【副作用与失败】只解析路径并读取 metadata，不写文件。路径不存在或不可访问时返回
/// `project_root_unavailable`，最终目标不是目录时返回 `project_root_not_directory`。
pub fn project_root_identity(root: &str) -> Result<Value, PortError> {
    let canonical = Path::new(root)
        .canonicalize()
        .map_err(|_| dispatch_error("project_root_unavailable"))?;
    let metadata =
        fs::metadata(&canonical).map_err(|_| dispatch_error("project_root_unavailable"))?;
    if !metadata.is_dir() {
        return Err(dispatch_error("project_root_not_directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Unix：返回设备号和 inode，这是内核层的身份
        Ok(json!({"canonical_root":canonical,"device":metadata.dev(),"inode":metadata.ino()}))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        // 非 Unix：降级到基于路径的身份，但明确标记为弱身份
        Ok(json!({"canonical_root":canonical,"identity_strength":"path_only"}))
    }
}

/// 把派发层稳定错误码包装成 Core 端口错误。
/// 集中构造可避免不同拒绝分支意外采用不同错误变体；调用方仍会保留这里的错误字符串。
fn dispatch_error(code: &str) -> PortError {
    PortError::Failed(code.to_owned())
}

/// 获取当前系统时间对应的毫秒级 Unix 时间戳，用于计算并验证 permit 截止时间。
/// 系统时钟早于 Unix epoch 或时间超出 `u64` 范围时返回错误；不能默认为 0，
/// 否则一个无效时钟可能使过期许可被错误接受。
fn now_ms() -> Result<u64, PortError> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| dispatch_error("clock_untrusted"))?
        .as_millis();
    u64::try_from(value).map_err(|_| dispatch_error("clock_overflow"))
}

/// 从能力请求上下文构造可序列化的调用身份；直接命令不伪造 Harness run/turn 身份。
///
/// Harness 请求必须同时有 `run_id` 和 `turn_id`，并把 Core 创建的 invocation/execution id、
/// 请求中的 assistant `call_id`（如有）及 attempt 编入 `InvocationIdentity`。该对象随准备与
/// 结果事件保存，供结果校验和恢复路径证明“这是同一次工具调用”。`None` 的 run id 表示直接
/// 命令，它有自己的 Command scope，因此正常返回 `None` 而不是合成 run/turn。
/// 身份缺失、领域校验失败或 JSON 编码失败返回端口错误；不访问 Runner 或 Broker。
fn typed_invocation_identity(
    run_id: Option<RunId>,
    turn_id: Option<TurnId>,
    invocation_id: InvocationId,
    execution_id: ExecutionId,
    request: &CapabilityRequest,
    attempt: u32,
) -> Result<Option<Value>, PortError> {
    // 【为什么需要这个结构】
    // 一个能力调用不仅仅是「执行一个操作」，还要记录它在运行中的位置：
    // - 这是哪个 Run 的哪个 Turn 中的哪个 Invocation
    // - 这是第几次尝试（attempt）
    // - 这次调用的源代码呼叫点是什么（call_id）
    // 这些信息一起形成了「调用身份」，用于结果追踪、重试管理、日志相关性。

    let Some(run_id) = run_id else {
        // 直接命令有显式的 Command 作用域，不应该构造虚假的 Harness Run/Turn 身份
        return Ok(None);
    };
    let turn_id = turn_id.ok_or_else(|| dispatch_error("turn_identity_required"))?;
    let call_id = request.arguments["call_id"].as_str().map(str::to_owned);
    let identity = kiana_domain::InvocationIdentity::new(
        run_id,
        turn_id,
        invocation_id,
        execution_id,
        call_id,
        attempt,
    )
    .map_err(|error| dispatch_error(&format!("invocation_identity_invalid:{error}")))?;
    serde_json::to_value(identity)
        .map(Some)
        .map_err(|error| dispatch_error(&format!("invocation_identity_encode:{error}")))
}
/// 以 EventStore 的原子版本比较提交一个状态转换批次。
///
/// 【调用者】许可准备/消费、结果提交和 result-delivery claim 等路径共用此 helper。
///
/// 【输入与输出】`batch` 带有 `command_id`、摘要、预期聚合版本和完整事件列表。成功首次提交
/// 返回 `Ok(false)`；相同 command 被确认重放则返回 `Ok(true)`，让调用者按幂等语义处理。
/// 这里会把 domain digest 的 `sha256:` 前缀去掉，以符合 EventStore journal 字段格式。
///
/// 【失败情况】不支持原子转换的 EventStore 返回 unavailable；版本变化返回 conflict；存储无法
/// 确认提交结果时返回 `result_unknown`。`Unknown` 不能解释成“肯定没写入”，调用方须按该操作
/// 的领域规则读回 command receipt 或保留未知状态。该 helper 只保证一个 batch，不包住外部执行。
pub(crate) async fn commit_confirmed(
    events: &dyn EventStorePort,
    mut batch: TransitionBatch,
) -> Result<bool, PortError> {
    // 【本次 transition batch 的 CAS 语义】
    // expected_versions 与事件批次一起交给 EventStore；版本完全匹配时，这一批状态转换
    // 原子化提交，冲突时不会只写入其中一部分。此保证只覆盖本次日志提交，不能跨越之后的
    // Broker handler 副作用或 Runner 回调；那些阶段分别写入事实并处理结果未知。

    // 域层的摘要包含算法名（如 "sha256:"），但日志的写入字段是裸十六进制
    if let Some(hex) = batch.command_digest.strip_prefix("sha256:") {
        batch.command_digest = hex.to_owned();
    }
    if !events.supports_atomic_transitions() {
        return Err(PortError::Unavailable(
            "control_journal_required".to_owned(),
        ));
    }
    match events.commit_transition(batch).await? {
        // 首次提交成功，新数据已记录
        CommitOutcome::Committed { .. } => Ok(false),
        // 幂等重放：这个 command_id 已经被提交过了，数据已存在
        CommitOutcome::Replayed { .. } => Ok(true),
        // 版本冲突：期望的版本与当前不匹配，转换被拒绝
        CommitOutcome::Conflict { .. } => Err(PortError::Conflict(
            "control_transition_conflict".to_owned(),
        )),
        // 状态异常：日志无法确认提交是否成功，返回 Unknown
        CommitOutcome::Unknown { .. } => {
            Err(dispatch_error("result_unknown:control_commit_unconfirmed"))
        }
    }
}

/// 从 EventStore 独立核验并一次性消费执行许可的 adapter。
///
/// 这个组件的职责是验证一个执行许可（permit）是否真实存在、尚未过期、
/// 且权限版本没有被更新过（从而导致许可失效）。
///
/// 【为什么要单独验证】
/// 派发层是 Core 的最后一步，Broker 是 Core 外的一个独立组件。
/// Broker 不能盲目信任「Core 说这个许可有效」，必须自己验证。
/// `DaemonHost` 将它提供给 Capability Broker，Broker 可在真正调用 handler 前自行检查许可，
/// 不依赖“Core 之前已经说过允许”。检查仍需读取 EventStore 当前版本，因此“独立验证”不等于
/// 完全离线或脱离事实存储。
///
/// 【验证的内容】
/// 1. permit 的摘要、字段完整性（是否被篡改过）
/// 2. permit 的时限（是否已过期）
/// 3. permit 中快照的权限版本是否仍然是当前版本（是否被撤销过）
/// 4. permit 对应的 execution_permit 流是否未被消费过（幂等性）
///
/// 本类型只持有 EventStore 引用，不拥有模型循环或 handler 执行权。
pub struct JournalPermitVerifier {
    events: Arc<dyn EventStorePort>,
}
impl JournalPermitVerifier {
    /// 创建 Broker 在执行 handler 前使用的 EventStore permit verifier。
    ///
    /// 【调用者/输入】DaemonHost 在装配 Capability Broker 时传入共享 EventStore port；该 port 用于
    /// 读取 `execution_permit` 流，并在后续 `verify_and_consume()` 中原子提交消费事件。
    ///
    /// 【输出/副作用】只保存 Arc 引用，不立即读取或写入 EventStore，也不验证当前状态；实际异步 I/O
    /// 只会在 Broker 请求消费 permit 时发生。
    ///
    /// 【为什么注入 EventStore】permit 的 authority 版本和一次性消费事实必须可由 Broker 独立读回检查，
    /// 不能只依赖 Core 之前给出的内存布尔结论。此构造函数本身不授予 handler 执行权限。
    pub fn new(events: Arc<dyn EventStorePort>) -> Self {
        Self { events }
    }
}

#[async_trait::async_trait]
impl kiana_ports::ExecutionPermitVerifierPort for JournalPermitVerifier {
    /// 验证并消费一个执行许可。
    ///
    /// 【调用者与输入】由 Broker 在 `execute_cancellable` 进入具体 handler 之前调用；输入
    /// `AuthorizedCapabilityRequest.authorization_id` 必须为 `permit:<execution_id>`，请求内容
    /// 还必须与 permit 中冻结的 operation、arguments 和 scope 一致。
    ///
    /// 【流程】
    /// 1. 从 authorization_id 中提取 permit ID（格式：`permit:<execution_id>`）
    /// 2. 从 execution_permit 流中读取许可记录
    /// 3. 验证许可是否存在、未被消费、内容完整
    /// 4. 验证许可的时限（是否已过期）
    /// 5. 验证许可中快照的所有权限版本是否仍然是当前版本
    /// 6. 构造 invocation_identity，验证与许可中的声称一致
    /// 7. 原子化地提交 dispatching → executing 事件，标记许可已消费
    ///
    /// 【副作用与失败】成功时向 `execution_permit` 流原子提交 dispatching/executing 事件，
    /// 从而把许可消费状态与 permit 及授权版本一起 CAS 锁定。EventStore 冲突、许可过期、
    /// 请求内容或身份不匹配时返回拒绝错误；Broker 不应进入 handler。
    ///
    /// 【消费的含义】
    /// 一旦消费成功，这个 permit 就被标记为「已使用」。
    /// 后续的消费尝试会失败（幂等性）。
    /// 同一 permit 不能再次通过本验证器消费；这用于阻止重放，而不是证明 handler 的现实效果。
    ///
    /// # 错误代码
    /// - `execution_permit_required`：authorization_id 格式不对或为空
    /// - `execution_permit_already_consumed`：这个 permit 已经被消费过了
    /// - `execution_permit_unavailable`：流中没有相应的 execution.prepared 事件
    /// - `execution_permit_invalid`：许可内容被篡改或不匹配
    /// - `execution_permit_scope_or_expiry_mismatch`：许可已过期或作用域不匹配
    /// - `old_epoch_permit_rejected`：许可中的权限版本已经被更新过，许可失效
    /// - `execution_permit_invocation_mismatch`：invocation_identity 与许可不符
    async fn verify_and_consume(
        &self,
        request: &AuthorizedCapabilityRequest,
    ) -> Result<(), PortError> {
        let id = request
            .authorization_id
            .strip_prefix("permit:")
            .ok_or_else(|| dispatch_error("execution_permit_required"))?;
        if id.trim().is_empty() {
            return Err(dispatch_error("execution_permit_required"));
        }
        let records = self.events.read_stream("execution_permit", id).await?;
        if records.len() > 1 {
            return Err(dispatch_error("execution_permit_already_consumed"));
        }
        if records.len() != 1 || records[0].kind != "execution.prepared" {
            return Err(dispatch_error("execution_permit_unavailable"));
        }
        let permit = DispatchPermit::from_json(&records[0].data["permit"])
            .map_err(|_| dispatch_error("execution_permit_invalid"))?;
        if permit.execution_id.to_string() != id
            || records[0].request_id
                != derived_request_id("execution.prepare", &permit.request_id.to_string())
            || records[0].aggregate_type.as_deref() != Some("execution_permit")
            || records[0].aggregate_id.as_deref() != Some(id)
            || records[0].stream_version != Some(1)
            || records[0].data.get("execution_id") != Some(&json!(permit.execution_id))
            || records[0].data.get("invocation_id") != Some(&json!(permit.invocation_id))
            || records[0].data.get("capability_request_id") != Some(&json!(permit.request_id))
            || records[0].data.get("run_id") != Some(&json!(permit.run_id))
            || records[0].data.get("turn_id") != Some(&json!(permit.turn_id))
            || records[0].data.get("action_digest") != Some(&json!(permit.action_digest))
            || records[0].data.get("attempt") != Some(&json!(1))
            || permit.to_json().map_or(true, |stored| {
                records[0].data.get("permit") != Some(&stored)
            })
        {
            return Err(dispatch_error("execution_permit_invalid"));
        }
        // 预算/资源预留必须与本条 prepared 事实绑定在同一 CAS 记录：若记录声明了预留快照
        // （非 null），它必须带有能由记录本身与 permit read-set 重算出来的绑定摘要。缺失摘要
        // 意味着“已准备但未能证明预留随同一事务提交”，漂移则意味着快照被改写/错配；两种都必须在
        // 消费许可（写 dispatching/executing）之前拒绝，不能带病进入 handler。
        let reservation = &records[0].data["cell_reservation"];
        if !reservation.is_null() {
            let expected = json!(kiana_domain::dispatch_reservation_digest(
                reservation,
                &permit.authority_versions,
            ));
            if records[0].data.get("reservation_digest") != Some(&expected) {
                return Err(dispatch_error("execution_permit_reservation_unbound"));
            }
        }
        let now = now_ms()?;
        let project_identity = project_root_identity(&permit.context.project_root)?;
        if permit.version != DISPATCH_PERMIT_VERSION
            || permit
                .validate_for_request(&request.request, &project_identity, now)
                .is_err()
        {
            return Err(dispatch_error("execution_permit_scope_or_expiry_mismatch"));
        }
        for dependency in &permit.authority_versions {
            let current = self
                .events
                .read_stream(&dependency.aggregate_type, &dependency.aggregate_id)
                .await?
                .iter()
                // Request-local sequence is not an aggregate version. Legacy readers that
                // cannot expose an explicit stream version must re-authorize before dispatch.
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0);
            if current != dependency.version {
                return Err(dispatch_error("old_epoch_permit_rejected"));
            }
        }
        let invocation = typed_invocation_identity(
            permit.run_id,
            permit.turn_id,
            permit.invocation_id,
            permit.execution_id,
            &request.request,
            1,
        )?;
        if records[0].data.get("invocation") != Some(&json!(invocation)) {
            return Err(dispatch_error("execution_permit_invocation_mismatch"));
        }
        let prepared_invocation = records[0].data["invocation"].clone();
        let mut expected = permit.authority_versions.clone();
        expected.push(AggregateVersion {
            aggregate_type: "execution_permit".to_owned(),
            aggregate_id: id.to_owned(),
            version: 1,
        });
        let command_id = derived_request_id("permit.consume", id);
        let dispatching = RuntimeEvent::new(command_id,1,"invocation.dispatching",json!({
            "run_id":permit.run_id,"turn_id":permit.turn_id,"invocation_id":permit.invocation_id,
            "execution_id":permit.execution_id,"capability_request_id":permit.request_id,
            "call_id":request.request.arguments["call_id"],"operation":request.request.operation,
            "capability":request.request.capability,"permit_digest":permit.permit_digest,
            "args_fingerprint":permit.action_digest,"action_digest":permit.action_digest,
            "decision_id":permit.decision_id,
            "attempt":1,"started":false,"effect_started":false,"effect_known":true,
            "zero_effect":true,"stop_state":"not_requested","fenced":true,
            "boundary":"broker_admission","invocation":prepared_invocation,
        })).map_err(|e|dispatch_error(&e.to_string()))?.with_stream_metadata("execution_permit",id,2)
            .with_identity_links(Some(command_id), Some(permit.request_id), Some(records[0].event_id), None);
        let dispatching_event_id = dispatching.event_id;
        let executing = RuntimeEvent::new(command_id,2,"invocation.executing",json!({
            "run_id":permit.run_id,"turn_id":permit.turn_id,"invocation_id":permit.invocation_id,
            "execution_id":permit.execution_id,"capability_request_id":permit.request_id,
            "call_id":request.request.arguments["call_id"],"operation":request.request.operation,
            "capability":request.request.capability,"action_digest":permit.action_digest,
            "args_fingerprint":permit.action_digest,"decision_id":permit.decision_id,
            "permit_digest":permit.permit_digest,
            "attempt":1,"started":true,"effect_started":true,"effect_known":true,
            "zero_effect":false,"stop_state":"not_requested","fenced":true,
            "boundary":"handler_execution","invocation":prepared_invocation,
        })).map_err(|e|dispatch_error(&e.to_string()))?.with_stream_metadata("execution_permit",id,3)
            .with_identity_links(Some(command_id), Some(permit.request_id), Some(dispatching_event_id), Some(records[0].event_id));
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(&json!({
                "permit":&permit,
                "request":&request.request,
                "dispatching":&dispatching.data,
                "executing":&executing.data,
            })),
            expected_versions: expected,
            events: vec![dispatching, executing],
        };
        let committed = commit_confirmed(self.events.as_ref(), batch).await;
        if committed? {
            return Err(dispatch_error("execution_permit_already_consumed"));
        }
        Ok(())
    }
}

impl ControlPlane {
    /// 从 EventStore 中重建并核验一次能力结果所引用的 permit、执行事件、收据和 commit receipt。
    ///
    /// 【调用者】只由 `deliver_capability_result` 在准备向 Runner 回灌结果前调用。
    ///
    /// 【输入与输出】输入是 run/turn 身份和 Broker 返回的 `CapabilityResult`。成功返回的
    /// `CommittedDeliverySource` 包含已经核实的 `execution.result_committed` 事件、对应
    /// `CapabilityResultReceipt`、EventStore commit 回执和执行 permit 的当前流版本。
    ///
    /// 【验证内容】函数沿 request id 找到 `execution.prepared`，检查 run/turn/execution/invocation
    /// 链接、结果正文、终态字段、effect 状态、receipt 摘要和 CAS commit receipt。缺失或矛盾的
    /// 事实一律返回 `result_unknown:*`，因为无法确认传入结果是否来自该次真实执行。
    /// 本方法只读 EventStore，不调用 Broker 或 Runner。
    async fn committed_delivery_source(
        &self,
        run_id: RunId,
        turn_id: TurnId,
        result: &CapabilityResult,
    ) -> Result<CommittedDeliverySource, PortError> {
        let prepare_id = derived_request_id("execution.prepare", &result.request_id.to_string());
        let preparation = self
            .events
            .read_command(&prepare_id)
            .await?
            .ok_or_else(|| dispatch_error("result_unknown:result_delivery_prepare_unconfirmed"))?;
        let mut sources = preparation
            .versions
            .iter()
            .filter(|version| version.aggregate_type == "execution_permit");
        let prepared_version = sources
            .next()
            .filter(|version| version.version == 1 && sources.next().is_none())
            .ok_or_else(|| dispatch_error("result_unknown:result_delivery_prepare_invalid"))?;
        let execution_id: ExecutionId =
            serde_json::from_value(json!(prepared_version.aggregate_id))
                .map_err(|_| dispatch_error("result_unknown:result_delivery_prepare_invalid"))?;
        let stream = self
            .events
            .read_stream("execution_permit", &execution_id.to_string())
            .await?;
        let prepared = stream
            .first()
            .filter(|event| event.kind == "execution.prepared")
            .ok_or_else(|| dispatch_error("result_unknown:result_delivery_prepare_invalid"))?;
        let permit = DispatchPermit::from_json(&prepared.data["permit"])
            .map_err(|_| dispatch_error("result_unknown:result_delivery_prepare_invalid"))?;
        if preparation.command_id != prepare_id
            || prepared.request_id != prepare_id
            || !preparation.event_ids.contains(&prepared.event_id)
            || permit.execution_id != execution_id
            || permit.request_id != result.request_id
            || permit.run_id != Some(run_id)
            || permit.turn_id != Some(turn_id)
            || prepared.data["action_digest"] != json!(permit.action_digest)
            || stream.iter().any(|event| event.stream_version.is_none())
        {
            return Err(dispatch_error(
                "result_unknown:result_delivery_source_version_missing",
            ));
        }
        if stream.iter().enumerate().any(|(index, event)| {
            event.aggregate_type.as_deref() != Some("execution_permit")
                || event.aggregate_id.as_deref() != Some(prepared_version.aggregate_id.as_str())
                || event.stream_version != Some(index as u64 + 1)
                || event.data["run_id"] != json!(run_id)
                || event.data["turn_id"] != json!(turn_id)
                || event.data["execution_id"] != json!(execution_id)
                || event.data["invocation_id"] != json!(permit.invocation_id)
                || event.data["capability_request_id"] != json!(result.request_id)
                || event.data["attempt"] != json!(1)
        }) {
            return Err(dispatch_error(
                "result_unknown:result_delivery_source_mismatch",
            ));
        }
        let mut committed = stream
            .iter()
            .filter(|event| event.kind == "execution.result_committed");
        let source = committed
            .next()
            .ok_or_else(|| dispatch_error("result_unknown:result_delivery_source_missing"))?;
        if committed.next().is_some() {
            return Err(dispatch_error(
                "result_unknown:result_delivery_source_ambiguous",
            ));
        }
        let final_id = derived_request_id("execution.result", &execution_id.to_string());
        let source_version = source.stream_version.ok_or_else(|| {
            dispatch_error("result_unknown:result_delivery_source_version_missing")
        })?;
        if source.request_id != final_id
            || source.command_id.is_some_and(|id| id != final_id)
            || source.sequence != 1
            || stream.last().map(|event| event.event_id) != Some(source.event_id)
            || source.data.get("result") != Some(&json!(result))
            || source.data["outcome_ready"] != true
            || source.data["outcome_state"] != json!(result.execution_state())
        {
            return Err(dispatch_error(
                "result_unknown:result_delivery_source_mismatch",
            ));
        }
        let identity: kiana_domain::InvocationIdentity =
            serde_json::from_value(source.data["invocation"].clone())
                .map_err(|_| dispatch_error("result_unknown:result_delivery_source_mismatch"))?;
        if identity.validate().is_err()
            || identity.run_id != run_id
            || identity.turn_id != turn_id
            || identity.execution_id != execution_id
            || identity.invocation_id != permit.invocation_id
            || identity.attempt != 1
            || source.data["invocation"] != prepared.data["invocation"]
        {
            return Err(dispatch_error(
                "result_unknown:result_delivery_source_mismatch",
            ));
        }
        let receipt = CapabilityResultReceipt::from_json(&source.data["result_receipt"])
            .map_err(|_| dispatch_error("result_unknown:result_delivery_receipt_invalid"))?;
        let mut expected = CapabilityResultReceipt::from_result(
            result,
            Some(execution_id),
            Some(permit.invocation_id),
            1,
            true,
        )
        .map_err(|_| dispatch_error("result_unknown:result_delivery_receipt_invalid"))?;
        if source.data.get("stop_requested").and_then(Value::as_bool) == Some(false)
            && expected.stop_confirmed.is_some()
        {
            expected.stop_confirmed = None;
            expected.receipt_digest = expected.digest();
        }
        if receipt != expected
            || source.data["effect_started"] != json!(receipt.effect_started)
            || source.data["effect_known"] != json!(receipt.effect_known)
            || source.data["zero_effect"] != json!(receipt.zero_effect)
            || source.data["fenced"] != json!(receipt.fenced)
        {
            return Err(dispatch_error(
                "result_unknown:result_delivery_receipt_mismatch",
            ));
        }
        if !committed_stop_metadata_matches(&source.data, &receipt) {
            return Err(dispatch_error(
                "result_unknown:result_delivery_stop_mismatch",
            ));
        }
        let final_commit =
            self.events.read_command(&final_id).await?.ok_or_else(|| {
                dispatch_error("result_unknown:result_delivery_source_unconfirmed")
            })?;
        let version =
            AggregateVersion::new("execution_permit", execution_id.to_string(), source_version);
        let result_digest = json_digest(&json!(result));
        let batch = TransitionBatch {
            command_id: final_id,
            command_digest: result_digest
                .strip_prefix("sha256:")
                .unwrap_or(&result_digest)
                .to_owned(),
            expected_versions: vec![AggregateVersion::new(
                "execution_permit",
                execution_id.to_string(),
                source_version - 1,
            )],
            events: vec![source.clone()],
        };
        if final_commit.versions != vec![version.clone()]
            || final_commit.validate_against(&batch).is_err()
        {
            return Err(dispatch_error(
                "result_unknown:result_delivery_source_unconfirmed",
            ));
        }
        Ok(CommittedDeliverySource {
            event: source.clone(),
            receipt,
            commit: final_commit,
            version,
        })
    }

    /// Close a prepared permit when a pre-Broker phase fails.
    ///
    /// `execution.prepared` is already durable before data revocation, checkpoint preparation,
    /// and the final cancellation check run. Returning directly from any of those phases leaves
    /// an orphan permit that cannot be retried or reconciled from the execution stream. This
    /// helper appends the only terminal fact allowed from that state: an opaque
    /// `execution.result_committed` with an Unknown effect and a fencing marker.
    async fn commit_prepared_result_unknown(
        &self,
        permit: &DispatchPermit,
        request: &CapabilityRequest,
        reason: &str,
        stop_requested: bool,
    ) -> Result<(), PortError> {
        let final_id = derived_request_id("execution.result", &permit.execution_id.to_string());
        let stream = self
            .events
            .read_stream("execution_permit", &permit.execution_id.to_string())
            .await?;
        if stream.len() == 2
            && stream[0].kind == "execution.prepared"
            && stream[0].stream_version == Some(1)
            && stream[1].kind == "execution.result_committed"
            && stream[1].stream_version == Some(2)
        {
            return self
                .events
                .read_command(&final_id)
                .await?
                .map(|_| ())
                .ok_or_else(|| dispatch_error("result_unknown:prepared_terminal_unconfirmed"));
        }
        if stream.len() != 1
            || stream[0].kind != "execution.prepared"
            || stream[0].stream_version != Some(1)
        {
            return Err(dispatch_error(
                "result_unknown:prepared_terminal_source_advanced",
            ));
        }
        if permit.request_id != request.request_id {
            return Err(dispatch_error(
                "result_unknown:prepared_terminal_request_mismatch",
            ));
        }

        let mut result = CapabilityResult::failure(permit.request_id, reason);
        if !result.output.is_object() {
            result.output = json!({"detail": result.output});
        }
        result.output["effect_known"] = json!(false);
        result.output["fenced"] = json!(true);
        if stop_requested {
            result.output["cancelled"] = json!(true);
            result.output["stop_confirmed"] = json!(false);
        }
        let result = kiana_domain::normalize_capability_result(permit.request_id, result);
        let receipt = kiana_domain::CapabilityResultReceipt::from_result(
            &result,
            Some(permit.execution_id),
            Some(permit.invocation_id),
            1,
            true,
        )
        .map_err(|error| {
            dispatch_error(&format!(
                "result_unknown:prepared_terminal_receipt_invalid:{error}"
            ))
        })?;
        let invocation = typed_invocation_identity(
            permit.run_id,
            permit.turn_id,
            permit.invocation_id,
            permit.execution_id,
            request,
            1,
        )?;
        let event = RuntimeEvent::new(
            final_id,
            1,
            "execution.result_committed",
            committed_result_payload(permit, &result, &receipt, invocation, stop_requested, false),
        )
        .map_err(|error| dispatch_error(&error.to_string()))?
        .with_stream_metadata("execution_permit", permit.execution_id.to_string(), 2);
        let batch = TransitionBatch {
            command_id: final_id,
            command_digest: json_digest(&json!(result)),
            expected_versions: vec![AggregateVersion::new(
                "execution_permit",
                permit.execution_id.to_string(),
                1,
            )],
            events: vec![event],
        };
        commit_confirmed(self.events.as_ref(), batch)
            .await
            .map(|_| ())
            .map_err(|error| {
                dispatch_error(&format!(
                    "result_unknown:prepared_terminal_commit_failed:{error}"
                ))
            })
    }

    /// 将已经提交、且来源已核验的能力结果一次性回灌给同一个 Runner run。
    ///
    /// 【调用者】`capabilities::broker_harness_capability` 在 `dispatch_capability_action` 和
    /// finalizer 确认结果可继续后调用；拒绝、取消或 `ResultUnknown` 不应走普通回灌路径。
    ///
    /// 【数据流与核心流程】先读 run 事件流定位仍活动的 `run.prompt` turn，拒绝已取消或终止的
    /// run；通过 `committed_delivery_source` 验证 Broker 结果与已记录 permit/receipt 一致；再以
    /// result_delivery、run 和 execution_permit 三路 expected version 原子提交
    /// `result.delivery_claimed`。CAS claim 获胜后，才调用 `RunnerPort::send` 发送
    /// `RunnerCommand::CapabilityResult`。
    ///
    /// 【为什么只能投递一次】Runner 回调在 EventStore claim 之外，无法与日志事务组成跨组件原子操作。
    /// claim 成功后若进程崩溃或 Runner 发送失败，调用返回 `result_unknown`，不能再次回灌，否则同一
    /// 工具结果可能推进模型两次。成功返回 Runner 事件；身份、来源、终态、CAS 或发送失败会返回
    /// 端口错误交给上层处理，不代表 handler 副作用可以安全重试。
    pub(crate) async fn deliver_capability_result(
        &self,
        run_id: RunId,
        result: CapabilityResult,
    ) -> Result<Vec<RunnerEvent>, PortError> {
        let command_id = derived_request_id("result.deliver", &result.request_id.to_string());
        let run = self.events.read_stream("run", &run_id.to_string()).await?;
        let version = run
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let turn = run
            .iter()
            .rposition(|event| event.kind == "run.prompt")
            .ok_or_else(|| dispatch_error("result_unknown:result_delivery_turn_unavailable"))?;
        if run[turn..].iter().any(|event| {
            matches!(
                event.kind.as_str(),
                "run.cancelling"
                    | "run.cancelled"
                    | "run.result_unknown"
                    | "run.failed"
                    | "run.completed"
            )
        }) {
            return Err(dispatch_error("cancelled:result_delivery_run_inactive"));
        }
        let turn_id = committed_prompt_turn(&run[turn], run_id)
            .map_err(|_| dispatch_error("result_unknown:result_delivery_turn_invalid"))?;
        let source = self
            .committed_delivery_source(run_id, turn_id, &result)
            .await?;
        let outcome_state = result.execution_state();
        let receipt_digest = &source.receipt.receipt_digest;
        let event = RuntimeEvent::new(
            command_id,
            1,
            "result.delivery_claimed",
            json!({"run_id":run_id,"turn_id":turn_id,
            "execution_id":source.receipt.execution_id,"invocation_id":source.receipt.invocation_id,
            "attempt":source.receipt.attempt,
            "capability_request_id":result.request_id,"result_digest":json_digest(&json!(result)),
            "receipt_digest":receipt_digest,"outcome_state":outcome_state,"outcome_ready":true,
            "delivery_policy":"single_advance"}),
        )
        .map_err(|e| dispatch_error(&e.to_string()))?
        .with_stream_metadata("result_delivery", result.request_id.to_string(), 1)
        .with_identity_links(
            Some(command_id),
            Some(result.request_id),
            Some(source.event.event_id),
            None,
        );
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(&json!({"run_id":run_id,"turn_id":turn_id,"result":result,
                "source_event_digest":json_digest(&json!(source.event)),"source_commit":source.commit})),
            expected_versions: vec![
                AggregateVersion::new("result_delivery", result.request_id.to_string(), 0),
                AggregateVersion::new("run", run_id.to_string(), version),
                source.version,
            ],
            events: vec![event],
        };
        // 若取消或来源版本先发生变化，CAS 会阻止 claim。claim 获胜就预留唯一一次 Runner 回调；
        // 回调确认位于 journal 事务之外，因此 claim 后发送失败或进程崩溃只能保留为 Unknown，不能重试。
        if commit_confirmed(self.events.as_ref(), batch).await? {
            return Err(dispatch_error(
                "result_unknown:result_delivery_already_claimed",
            ));
        }
        self.runner
            .send(RunnerCommand::CapabilityResult { run_id, result })
            .await
            .map_err(|e| dispatch_error(&format!("result_unknown:result_delivery_unconfirmed:{e}")))
    }

    /// 为一个已净化的能力请求固定当时的项目 authority 版本与动作摘要。
    ///
    /// 【调用者】`capabilities::authorize_capability_action_cancellable` 在 policy/gate 判断后、
    /// 派发或审批前调用；后续 `dispatch_authorized` 必须看到同一请求、同一项目上下文和同一
    /// authority 版本才能继续。
    ///
    /// 【数据流】根据规范项目根计算 authority stream key，读取当前 `project_trusted` 和版本，
    /// 把 RequestContext 与 `capability_action_digest` 合成动作摘要，并向 `action` 聚合 CAS 写入
    /// `action.authority_pinned`。相同摘要/版本的重复绑定视为幂等成功；若同 request id 对应内容
    /// 或 authority 版本已变化，返回 `action_authority_changed`。
    ///
    /// 【失败与副作用】缺少 authority、信任状态变化、EventStore 冲突或存储错误都会拒绝；
    /// 成功只固定后续比较基线，不执行 capability，也不等同于最终授权。
    pub(crate) async fn pin_action_authority(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<(), CoreError> {
        let root = Self::canonical_project_root(&context.project_root);
        let key = json_digest(&json!({"project_root":root}));
        let authority = self.events.read_stream("authority", &key).await?;
        let current = authority
            .last()
            .ok_or_else(|| dispatch_error("action_authority_missing"))?;
        if current.data["project_trusted"] != json!(context.project_trusted) {
            return Err(dispatch_error("action_trust_changed").into());
        }
        let mut scope = context.clone();
        scope.request_id = request.request_id;
        let digest = json_digest(
            &json!({"scope":scope,"action_digest":kiana_domain::capability_action_digest(request)}),
        );
        let existing = self
            .events
            .read_stream("action", &request.request_id.to_string())
            .await?;
        let version = current.stream_version.unwrap_or(0);
        if let Some(pin) = existing.first() {
            if pin.data["action_digest"] != digest || pin.data["authority_version"] != version {
                return Err(dispatch_error("action_authority_changed").into());
            }
            return Ok(());
        }
        let command_id = derived_request_id("action.pin", &request.request_id.to_string());
        let event = RuntimeEvent::new(
            command_id,
            1,
            "action.authority_pinned",
            json!({"action_digest":digest,"authority_version":version,
            "request_id":request.request_id,"authority_key":key,"actor_id":context.actor_id}),
        )?
        .with_stream_metadata("action", request.request_id.to_string(), 1);
        let batch = TransitionBatch {
            command_id,
            command_digest: digest,
            expected_versions: vec![
                AggregateVersion::new("action", request.request_id.to_string(), 0),
                AggregateVersion::new("authority", key, version),
            ],
            events: vec![event],
        };
        commit_confirmed(self.events.as_ref(), batch).await?;
        Ok(())
    }

    /// 读取派发许可提交时必须仍然匹配的授权与运行状态版本集合。
    ///
    /// 该集合覆盖 session/project assignment、project authority、资源 quarantine、company
    /// aggregate，以及存在时的 run stream。返回值写入后续 `execution.prepared` 的 CAS 预期版本；
    /// 并发撤销、企业关闭、资源隔离或 run 终止只要先提交，就会使准备许可失败。
    ///
    /// 读取期间还会验证 project trust/revision、未释放的 quarantine 和 run 当前 turn 非终态。
    /// `run_id == None` 代表直接命令，没有 Harness run 聚合；缺失聚合按版本 0 参与 CAS。
    /// 任一依赖不能可信读取或发现拒绝状态时返回错误，而不是生成不完整 permit。
    async fn dispatch_authority_versions(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
    ) -> Result<Vec<AggregateVersion>, PortError> {
        let root = Self::canonical_project_root(&context.project_root);
        let assignment = json_digest(&json!({"session_id":context.session_id,"project_root":root}));
        let mut scopes = vec![
            ("session_assignment".to_owned(), assignment),
            (
                "authority".to_owned(),
                json_digest(&json!({"project_root":root})),
            ),
            (
                "resource_quarantine".to_owned(),
                json_digest(&json!({"project_root":root})),
            ),
        ];
        if let Some(id) = run_id {
            scopes.push(("run".to_owned(), id.to_string()));
        }
        // Domain changes and revocations are also read dependencies, even for root commands.
        scopes.push((
            "company".to_owned(),
            super::company::company_aggregate_id(context),
        ));
        let mut versions = Vec::new();
        let authority_key = json_digest(&json!({"project_root":root}));
        let authority = self.events.read_stream("authority", &authority_key).await?;
        let current_authority = authority
            .last()
            .ok_or_else(|| dispatch_error("dispatch_authority_missing"))?;
        if current_authority.data["project_trusted"] != json!(context.project_trusted) {
            return Err(dispatch_error("dispatch_trust_changed"));
        }
        let authority_revision = format!(
            "{}:{}",
            current_authority.stream_version.unwrap_or(0),
            current_authority.data["revision_digest"]
                .as_str()
                .unwrap_or_default()
        );
        for (aggregate_type, aggregate_id) in scopes {
            let events = self
                .events
                .read_stream(&aggregate_type, &aggregate_id)
                .await?;
            if aggregate_type == "resource_quarantine" {
                let mut quarantined = std::collections::HashSet::new();
                for event in &events {
                    if let Some(id) = event.data["run_id"].as_str() {
                        match event.kind.as_str() {
                            "resource.quarantined" => {
                                quarantined.insert(id.to_owned());
                            }
                            "resource.released" => {
                                quarantined.remove(id);
                            }
                            _ => return Err(dispatch_error("resource_quarantine_record_invalid")),
                        }
                    }
                }
                if !quarantined.is_empty() {
                    return Err(dispatch_error("project_resources_quarantined"));
                }
            }
            if aggregate_type == "run" {
                if events
                    .iter()
                    .find(|event| event.kind == "run.authorized")
                    .is_none_or(|event| event.data["authority_revision"] != authority_revision)
                {
                    return Err(dispatch_error("run_dispatch_authority_changed"));
                }
                let turn = events
                    .iter()
                    .rposition(|event| event.kind == "run.prompt")
                    .unwrap_or(0);
                if events[turn..].iter().any(|event| {
                    matches!(
                        event.kind.as_str(),
                        "run.cancelling"
                            | "run.cancelled"
                            | "run.result_unknown"
                            | "run.failed"
                            | "run.completed"
                    )
                }) {
                    return Err(dispatch_error("run_dispatch_after_terminal_or_cancel"));
                }
            }
            versions.push(AggregateVersion {
                aggregate_type,
                aggregate_id,
                version: events
                    .iter()
                    .filter_map(|e| e.stream_version)
                    .max()
                    .unwrap_or(0),
            });
        }
        Ok(versions)
    }

    /// 重新校验已授权动作、准备短期 permit、调用 Broker 并提交结果事实。
    ///
    /// 【调用者】`capabilities::dispatch_capability_action` 在取消预检、Admission scheduler 与
    /// Cell lease 准备后调用。入参 `authorized` 已带 Core 得出的 authorization id，但本函数仍
    /// 会重新检查 policy、company scope、action pin 和所有 CAS 版本；它不信任调用方的一次性判断。
    ///
    /// 【输入与输出】`context` 是服务端 RequestContext，`run_id` 对 Harness 调用有值、直接命令
    /// 可为空，`authorized.request` 是已经净化的 CapabilityRequest；`cancellation` 是异步取消信号。
    /// 成功返回结构化 CapabilityResult。具体 handler 由 Broker 执行，结果之后由上层 finalizer
    /// 分类结算；Harness 模式还会在适当路径调用 `deliver_capability_result` 回灌 Runner。
    ///
    /// 【核心流程】
    /// 1. 拒绝已取消或已准备过的 request，并读取 session、project、run、company 等授权版本；
    /// 2. 再验业务范围、policy 和先前固定的 action digest；若带审批，则将审批消费事件与
    ///    `execution.prepared` 放进同一个 EventStore transition batch；
    /// 3. 构造包含 operation digest、project identity、authority versions、期限和可选 Cell
    ///    reservation 的 `DispatchPermit`，通过 CAS 提交准备事实；
    /// 4. 执行数据/恢复前置处理，再检查取消，给 Broker 设置 permit authorization id 并调用
    ///    `execute_cancellable`；
    /// 5. 脱敏、规范化 Broker 结果，处理 stop proof，构造 `CapabilityResultReceipt`，单独 CAS
    ///    写入 `execution.result_committed`，返回给调用方。
    ///
    /// 【重要的阶段边界】`execution.prepared` 在 Broker handler 之前已提交；handler 执行与
    /// result commit 是后续阶段，不属于同一个事务。prepared 之后的取消、Broker 错误、时钟/存储
    /// 不确定性都可能留下准备记录或产生 `result_unknown`。不能将这些路径重试为新执行，除非上层
    /// 已证明旧 attempt 未产生效果。
    ///
    /// 【失败与副作用】CAS 冲突、scope/permit 错误会在 Broker 前拒绝；进入 Broker 后返回错误不
    /// 一定表示无副作用。结果无法确认时保留 Unknown，交给 finalizer/生命周期记录并阻止普通回灌。
    /// 此函数不驱动模型循环，也不直接向 UI 输出。
    pub(crate) async fn dispatch_authorized(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        mut authorized: AuthorizedCapabilityRequest,
        cancellation: watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        if *cancellation.borrow() {
            return Err(dispatch_error("cancelled:before_dispatch"));
        }
        let request = &authorized.request;
        let invocation_id = InvocationId::from_uuid(request.request_id.as_uuid());
        let dispatch_command_id =
            derived_request_id("execution.prepare", &request.request_id.to_string());
        if self
            .events
            .read_command(&dispatch_command_id)
            .await?
            .is_some()
        {
            return Err(PortError::Conflict(
                "result_unknown:invocation_already_prepared".to_owned(),
            ));
        }
        let execution_id = ExecutionId::new();
        let mut authority_versions = self.dispatch_authority_versions(context, run_id).await?;
        // Revalidate business scope after pinning the read set; concurrent changes fail the commit.
        self.guard_company_capability(context, request)
            .await
            .map_err(|e| dispatch_error(&e.to_string()))?;
        if let PolicyDecision::Deny { reason } = self.evaluate_policy(context, request) {
            return Err(dispatch_error(&reason));
        }
        let pin = self
            .events
            .read_stream("action", &request.request_id.to_string())
            .await?;
        let pinned = pin
            .first()
            .ok_or_else(|| dispatch_error("action_authority_not_pinned"))?;
        let mut scope = context.clone();
        scope.request_id = request.request_id;
        if pinned.data["action_digest"]
            != json_digest(
                &json!({"scope":scope,"action_digest":kiana_domain::capability_action_digest(request)}),
            )
            || !authority_versions.iter().any(|version| {
                version.aggregate_type == "authority"
                    && pinned.data["authority_version"] == version.version
            })
        {
            return Err(dispatch_error("action_authority_changed"));
        }
        authority_versions.push(AggregateVersion::new(
            "action",
            request.request_id.to_string(),
            pin.iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0),
        ));
        let mut events = Vec::new();
        // 60_000 毫秒即 60 秒：permit 默认只在短窗口内可消费，缩小“授权时”与“Broker 真正开始”
        // 之间权限可能变化的时间；若审批 challenge 更早过期，下面会取二者较早时刻。
        // saturating_add 避免极端 Unix 时间值加时长时整数溢出。
        let mut expiry = now_ms()?.saturating_add(60_000);
        let approval_id = authorized
            .authorization_id
            .strip_prefix("approval:")
            .map(|id| {
                serde_json::from_value::<ApprovalId>(json!(id))
                    .map_err(|_| dispatch_error("approval_id_invalid"))
            })
            .transpose()?;
        if let Some(approval_id) = approval_id {
            let prepared = self
                .approvals
                .prepare_consumption(context, approval_id, dispatch_command_id)
                .await?;
            if prepared.pending.request != *request {
                return Err(dispatch_error("approval_action_changed"));
            }
            expiry = expiry.min(prepared.pending.challenge.expires_at_unix_ms);
            for value in prepared.authority_versions {
                if let Some(observed) = authority_versions.iter().find(|existing| {
                    existing.aggregate_type == value.aggregate_type
                        && existing.aggregate_id == value.aggregate_id
                }) {
                    if observed.version != value.version {
                        return Err(dispatch_error("approval_authority_changed"));
                    }
                } else {
                    authority_versions.push(value);
                }
            }
            authority_versions.push(prepared.expected_version);
            events.push(prepared.event);
        }
        let turn_id = if let Some(run_id) = run_id {
            self.events
                .read_stream("run", &run_id.to_string())
                .await?
                .iter()
                .rev()
                .find(|event| event.kind == "run.prompt")
                .map(|event| committed_prompt_turn(event, run_id))
                .transpose()?
        } else {
            None
        };
        let mut permit_authority = authority_versions.clone();
        // Consumption itself advances approval; the Broker must observe that committed version.
        for value in &mut permit_authority {
            if events.iter().any(|event| {
                event.aggregate_type.as_deref() == Some(value.aggregate_type.as_str())
                    && event.aggregate_id.as_deref() == Some(value.aggregate_id.as_str())
            }) {
                value.version += 1;
            }
        }
        let issued_at_unix_ms = now_ms()?;
        let mut permit = DispatchPermit {
            schema: DISPATCH_PERMIT_SCHEMA.to_owned(),
            version: DISPATCH_PERMIT_VERSION,
            execution_id,
            invocation_id,
            request_id: request.request_id,
            run_id,
            turn_id,
            decision_id: authorized.authorization_id.clone(),
            approval_id,
            context: context.clone(),
            action_digest: kiana_domain::capability_action_digest(request),
            project_identity: project_root_identity(&context.project_root)?,
            authority_versions: permit_authority,
            issued_at_unix_ms,
            expires_at_unix_ms: expiry,
            permit_digest: String::new(),
        };
        permit.permit_digest = permit.digest();
        permit.validate().map_err(|error| dispatch_error(&error))?;
        authority_versions.push(AggregateVersion {
            aggregate_type: "execution_permit".to_owned(),
            aggregate_id: execution_id.to_string(),
            version: 0,
        });
        // 当前 dispatch 按一个 invocation attempt 记录，attempt 固定为 1；permit verifier、
        // dispatching/executing 与结果收据都用同一编号。若未来引入 attempt 重试，必须一起扩展
        // 身份派生、CAS 流和收据验证，不能只改其中一个 `1`。
        let invocation =
            typed_invocation_identity(run_id, turn_id, invocation_id, execution_id, request, 1)?;
        let cell_reservation = if let Some(cell_id) = request.cell_id {
            self.cell_registry
                .reservation_for_cell(cell_id)
                .await?
                .map(|reservation| {
                    json!({
                        "budget":reservation.budget,
                        "grant":reservation.grant,
                        "cell":reservation.cell,
                    })
                })
        } else {
            None
        };
        let mut event = RuntimeEvent::new(
            dispatch_command_id,
            1,
            "execution.prepared",
            json!({
                "run_id":run_id,
                "turn_id":turn_id,
                "invocation_id":invocation_id,
                "execution_id":execution_id,
                "capability_request_id":request.request_id,
                "action_digest":permit.action_digest,
                "attempt":1,
                "permit":permit,
                "cell_reservation":cell_reservation,
                "invocation":invocation,
            }),
        )
        .map_err(|error| dispatch_error(&error.to_string()))?
        .with_stream_metadata("execution_permit", execution_id.to_string(), 1);
        event.data = super::redaction::redact_event_value(&event.data);
        // 预算与资源预留必须与本条 prepared 事实同属一个 CAS 记录：在脱敏**之后**对最终写入的
        // `cell_reservation` 快照与 permit 的 authority read-set 计算绑定摘要。这样许可消费方
        // （Broker verifier）可以从记录本身重算并发现“prepared 事实存在但预留缺失/漂移”，
        // 不必信任 Core 内存中的一次性判断；尚未满足预算/锁时也不会写到这里。
        let reservation_digest = kiana_domain::dispatch_reservation_digest(
            &event.data["cell_reservation"],
            &permit.authority_versions,
        );
        event.data["reservation_digest"] = json!(reservation_digest);
        events.push(event);
        let batch = TransitionBatch {
            command_id: dispatch_command_id,
            command_digest: json_digest(
                &json!({"context":context,"run_id":run_id,"request":request}),
            ),
            expected_versions: authority_versions,
            events,
        };
        let committed = commit_confirmed(self.events.as_ref(), batch).await;
        if let Some(run_id) = run_id {
            self.invalidate_invocation_projection(run_id);
        }
        if committed? {
            return Err(dispatch_error("result_unknown:invocation_already_prepared"));
        }
        if let Err(error) = self.prepare_data_revocation(&authorized.request).await {
            let reason = format!(
                "result_unknown:prepared_data_revocation_failed:{}",
                super::redaction::redact_event_text(&error.to_string())
            );
            self.commit_prepared_result_unknown(&permit, &authorized.request, &reason, false)
                .await?;
            return Err(dispatch_error(&reason));
        }
        if let Err(error) = self.prepare_checkpoint_restore(&authorized.request).await {
            let reason = format!(
                "result_unknown:prepared_checkpoint_failed:{}",
                super::redaction::redact_event_text(&error.to_string())
            );
            self.commit_prepared_result_unknown(&permit, &authorized.request, &reason, false)
                .await?;
            return Err(dispatch_error(&reason));
        }
        if *cancellation.borrow() {
            let reason = "result_unknown:prepared_cancelled_before_broker";
            self.commit_prepared_result_unknown(&permit, &authorized.request, reason, true)
                .await?;
            return Err(dispatch_error("cancelled:before_broker"));
        }
        authorized.authorization_id = format!("permit:{execution_id}");
        let executed_request = authorized.request.clone();
        let (stop_tx, _rx) = watch::channel(None);
        if let Some(id) = run_id {
            self.capability_stops
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id, stop_tx.clone());
        }
        let result = self
            .capabilities
            .execute_cancellable(authorized, cancellation.clone())
            .await;
        let result = match result {
            Ok(result) if result.request_id == permit.request_id => {
                super::redaction::redact_capability_result(result)
            }
            Ok(_) => CapabilityResult::failure(
                permit.request_id,
                "result_unknown:capability_result_mismatch",
            ),
            Err(error) => CapabilityResult::failure(
                permit.request_id,
                format!(
                    "result_unknown:{}",
                    super::redaction::redact_event_text(&error.to_string())
                ),
            ),
        };
        let result = kiana_domain::normalize_capability_result(permit.request_id, result);
        let result = match self
            .finish_checkpoint_restore(&executed_request, &result)
            .await
        {
            Ok(()) => result,
            Err(error) => {
                CapabilityResult::failure(permit.request_id, format!("result_unknown:{error}"))
            }
        };
        let mut result = kiana_domain::normalize_capability_result(permit.request_id, result);
        if *cancellation.borrow()
            && result.failure_code() != Some(kiana_domain::CapabilityErrorCode::ResultUnknown)
        {
            if !result.output.is_object() {
                result.output = json!({"detail":result.output});
            }
            result.output["completed_before_cancel"] = json!(result.success);
            result.output["cancelled"] = json!(true);
            result.output["stop_confirmed"] = json!(true);
            result = kiana_domain::normalize_capability_result(permit.request_id, result);
        }
        let unknown =
            result.failure_code() == Some(kiana_domain::CapabilityErrorCode::ResultUnknown);
        let confirmed = !unknown
            && (result.output["cancelled"] != true || result.output["stop_confirmed"] == true);
        let stop_requested = *cancellation.borrow();
        stop_tx.send_replace(Some(confirmed));
        let invocation = typed_invocation_identity(
            permit.run_id,
            permit.turn_id,
            permit.invocation_id,
            permit.execution_id,
            &executed_request,
            1,
        )?;
        let mut receipt = match kiana_domain::CapabilityResultReceipt::from_result(
            &result,
            Some(permit.execution_id),
            Some(permit.invocation_id),
            1,
            true,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                result = kiana_domain::normalize_capability_result(
                    permit.request_id,
                    CapabilityResult::failure(
                        permit.request_id,
                        format!("result_unknown:capability_result_receipt_invalid:{error}"),
                    ),
                );
                kiana_domain::CapabilityResultReceipt::from_result(
                    &result,
                    Some(permit.execution_id),
                    Some(permit.invocation_id),
                    1,
                    true,
                )
                .map_err(|error| dispatch_error(&error))?
            }
        };
        // A handler may expose `stop_confirmed` as a normal command-completion detail (the
        // shell adapter does this), but only the ControlPlane cancellation watch can establish
        // that a stop was requested. Keep the model-facing result intact while ensuring the
        // durable receipt's optional stop field follows the committed three-state envelope.
        if !stop_requested && receipt.stop_confirmed.is_some() {
            receipt.stop_confirmed = None;
            receipt.receipt_digest = receipt.digest();
        }
        let stream = self
            .events
            .read_stream("execution_permit", &execution_id.to_string())
            .await?;
        let version = stream
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let final_id = derived_request_id("execution.result", &execution_id.to_string());
        let event = RuntimeEvent::new(
            final_id,
            1,
            "execution.result_committed",
            committed_result_payload(
                &permit,
                &result,
                &receipt,
                invocation,
                stop_requested,
                confirmed,
            ),
        )
        .map_err(|e| dispatch_error(&e.to_string()))?
        .with_stream_metadata("execution_permit", execution_id.to_string(), version + 1);
        let batch = TransitionBatch {
            command_id: final_id,
            command_digest: json_digest(&json!(result)),
            expected_versions: vec![AggregateVersion {
                aggregate_type: "execution_permit".to_owned(),
                aggregate_id: execution_id.to_string(),
                version,
            }],
            events: vec![event],
        };
        if let Some(run_id) = run_id {
            self.invalidate_invocation_projection(run_id);
        }
        commit_confirmed(self.events.as_ref(), batch)
            .await
            .map_err(|e| dispatch_error(&format!("result_unknown:result_commit_failed:{e}")))?;
        Ok(result)
    }

    /// 从已提交事实对一次 execution 做派发崩溃分类。
    ///
    /// 【调用者】恢复/对账路径在重启后调用；用来判断某次 execution 是否可能已进入 handler，
    /// 从而决定是补写 Unknown 终结事实、按既有终态收敛，还是（尚未准备）直接视为从未派发。
    /// 【输入/输出】execution id；返回 [`kiana_domain::DispatchCrashClass`]，只由 `execution_permit`
    /// 流的已提交记录决定。
    /// 【副作用与边界】只读 EventStore，不消费 permit、不调用 Broker、不写事件。分类表达的是提交
    /// 边界，不是外部副作用的现实状态；`EffectPossiblyStarted`/`ResultCommitted` 一律按效果未知处理。
    pub async fn classify_dispatch_execution(
        &self,
        execution_id: &ExecutionId,
    ) -> Result<kiana_domain::DispatchCrashClass, PortError> {
        let records = self
            .events
            .read_stream("execution_permit", &execution_id.to_string())
            .await?;
        Ok(kiana_domain::classify_dispatch_stream(&records))
    }
}

#[cfg(test)]
#[path = "dispatch_delivery_tests.rs"]
mod delivery_tests;
