//! 控制面事件事实的构造、校验和提交：把一次执行中的决定与结果变成可审计的 EventLog 记录。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! 入口 / 生命周期 / 审批 / Broker 路径
//!    ↓  形成事件种类与业务载荷
//! 【本文件】脱敏、载荷边界检查、关联身份和聚合版本、terminal 冲突保护
//!    ↓  EventStorePort：幂等追加或 CAS 事务提交
//! EventLog（可恢复的事件事实）
//!    ↓
//! run / invocation 投影、Receipt、恢复与审计
//! ```
//!
//! **上游**：`lifecycle.rs`、`approvals.rs`、`data_governance.rs` 等 ControlPlane 执行路径调用
//! `record_event()`、`append_event()` 或 `record_terminal_event()`；它们先决定业务事件是什么，
//! 本文件负责把事件安全地转成可追加事实。
//! **下游**：普通事件经 `EventStorePort` 的幂等 CAS 追加，终态事件经确认过的事务批次提交；
//! `receipts.rs`、recovery 和各投影模块随后从 EventLog 重建响应视图。本文件不直接选择 adapter，
//! 也不调用模型或工具执行器。
//!
//! 本文件分成两类逻辑：载荷 helper 负责脱敏/关联字段/响应元数据；`ControlPlane` 方法负责
//! EventStore 读写、版本比较、并发冲突重试和终态互斥。调用者仍是业务语义的权威来源，
//! 不要在这里把事件名或调用方决定替换成另一套业务状态机。
//!
//! # 为什么事件载荷需要这么多检查
//!
//! 因为事件是**不可撤销的**。一条已经提交的事件会被重放、被投影、被审计、被当作事实引用；
//! 写进去的东西如果有问题，事后无法修正，只能在它之上再叠一层解释。所以本文件的职责是
//! **在写之前把几类不可逆的错误拦下来**：
//!
//! 1. **太深或含 NUL 的载荷** —— 会让下游的解析器、投影器和日志系统出现不一致行为；
//! 2. **含 secret 的载荷** —— 事件一旦落盘，secret 就有第二份副本；
//! 3. **关联身份不符合事件契约** —— 事件类型要求的 actor/project/run 等关联字段缺失或损坏时，
//!    下游无法可靠判断它属于谁、哪个项目或哪次运行；具体必填字段由对应事件契约决定；
//! 4. **把 `result_unknown` 当成失败或成功** —— 这是最严重的一类，下面单独说。
//!
//! # `Unknown` 在这里是默认立场，不是例外
//!
//! `result_unknown_value` 把「结果未知」识别出来，而调用方据此**保留 Unknown** 而不是
//! 折叠成成功或失败。外部 effect 可能已经发生也可能没有发生，把它记成任何一种确定结论，
//! 都是编造。这也是为什么它同时识别 `compensation_required`——需要补偿，意味着
//! 事情多半发生了，但具体后果未知。
use super::redaction::*;
use super::*;
use kiana_domain::CapabilityErrorCode;
use serde::de::DeserializeOwned;

/// 从事件载荷里取一个「可选的关联对象」，取不到就是 `None`，取到了但形状不对就拒绝。
///
/// 【为什么可选，而不是必填】
/// 因为事件形状在演进：早期事件没有这个字段。如果必填，旧事件会在重放时被整条拒掉，
/// 而已提交的事实不允许重写。所以这里容忍缺失。
///
/// 【但缺失和损坏是两件事】
/// 字段**不存在**或**显式为 null** → `Ok(None)`；
/// 字段存在但反序列化失败 → 拒绝，并给出 `event_{field}_invalid`。
/// 静默丢弃一个存在但损坏的关联对象，会让一条本该指向某个 run/approval 的事件变成孤儿。
fn optional_event_link<T: DeserializeOwned>(
    data: &Value,
    field: &str,
) -> Result<Option<T>, CoreError> {
    match data.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| PortError::Failed(format!("event_{field}_invalid")).into()),
    }
}

/// 判断一段结果里是否在说「我不知道」。
///
/// 【作用】
/// 同时看 `error_code` 和 `error` 两个字段，因为不同来源的 adapter 写不同的键；
/// 统一在这里处理，下游就不用为每种键名各写一遍。
///
/// 【判定范围】
/// 命中 `ResultUnknown` 或 `CompensationRequired` 就算「不知道」。
/// 第二个之所以算，是因为需要补偿恰恰意味着**很可能已经发生、但后果未确定**——
/// 把它记成「成功」是错的，记成「失败」同样是错的。
///
/// 【为什么重要】
/// 这是整条事件链上最容易出错的一步。调用方拿到 `false` 就可以写下确定结论；
/// 拿到 `true` 则必须保留 Unknown。所以这个函数的保守方向是**宁可多认成 Unknown**：
/// 误判为 Unknown 只是让人去查，误判为确定则会直接进入账本。
fn result_unknown_value(value: &Value) -> bool {
    [Some(value), value.get("result")]
        .into_iter()
        .flatten()
        .filter_map(|candidate| {
            candidate
                .get("error_code")
                .and_then(Value::as_str)
                .map(CapabilityErrorCode::from_reason)
                .or_else(|| {
                    candidate
                        .get("error")
                        .and_then(Value::as_str)
                        .map(CapabilityErrorCode::from_reason)
                })
        })
        .any(|code| {
            matches!(
                code,
                CapabilityErrorCode::ResultUnknown | CapabilityErrorCode::CompensationRequired
            )
        })
}

/// 如果这次取消携带了完整事实，就把它构造出来；没有就返回 `None`。
///
/// 【作用】
/// 取消不是一个布尔值。仓库的约定是：排空已启动的工作、合成未启动的结果、
/// 并且有独立证据确认副作用停止，才可以把 `stop_confirmed` 记为 true。取消事实被打包成
/// `RunCancellationFact`，写进终态事件供恢复和审计核验；本 helper 只根据调用方提交的字段构造
/// 并检查事实，不会自行探测 Runner、进程组或外部副作用是否真的停止。
///
/// 【为什么可以是 `None`】
/// 载荷里没有 `cancellation_state` 时就没有事实。这不是为了方便，
/// 而是因为**不能凭空合成一个「已确认停止」**——那会让一条没核实的取消看起来像核过的。
/// 缺事实就是缺事实。
fn cancellation_fact_for(
    kind: &str,
    data: &Value,
    run_id: RunId,
    command_id: kiana_domain::RequestId,
    expected_version: u64,
) -> Result<Option<kiana_domain::RunCancellationFact>, CoreError> {
    // 只有载荷显式声明取消状态时才构造；普通 completed/failed 事件不能凭空带出取消证明。
    if data.get("cancellation_state").is_none() {
        return Ok(None);
    }
    let state = serde_json::from_value(
        data.get("cancellation_state")
            .cloned()
            .unwrap_or(Value::Null),
    )
    .map_err(|_| PortError::Failed("run_cancellation_fact_state_invalid".to_owned()))?;
    let targets = serde_json::from_value(
        data.get("cancellation_targets")
            .cloned()
            .unwrap_or_else(|| json!([])),
    )
    .map_err(|_| PortError::Failed("run_cancellation_fact_targets_invalid".to_owned()))?;
    let reason = data
        .get("cancellation_reason")
        .and_then(Value::as_str)
        .or_else(|| data.get("error").and_then(Value::as_str))
        .unwrap_or("user");
    let actor_id = data
        .get("cancel_actor_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let stop_confirmed = data.get("stop_confirmed").and_then(Value::as_bool);
    let at_unix_ms = data
        .get("cancellation_at_unix_ms")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
                .unwrap_or(0)
        });
    let fact = kiana_domain::RunCancellationFact::new(
        run_id,
        command_id,
        state,
        &redact_event_text(reason),
        actor_id,
        targets,
        expected_version,
        true,
        stop_confirmed,
        at_unix_ms,
    )
    .map_err(PortError::Failed)?;
    if (kind == "run.cancelled") != (fact.state == kiana_domain::RunCancellationState::Cancelled)
        || (kind == "run.result_unknown")
            != (fact.state == kiana_domain::RunCancellationState::ResultUnknown)
    {
        return Err(PortError::Failed("run_cancellation_fact_state_mismatch".to_owned()).into());
    }
    Ok(Some(fact))
}

/// 从载荷读取可选的事件身份关联，并补上当前请求的默认 correlation ID。
///
/// `command_id`、`causation_event_id`、`parent_event_id` 缺失或为 null 时保留为空；字段存在但类型/格式不符时
/// 通过 `optional_event_link()` 返回错误，避免静默丢掉一个已经声明的因果关系。
/// `correlation_id` 缺省时回落到当前 request ID。返回事件只增加追踪关联，不建立权限或改变事件载荷。
fn stamp_event_links(
    event: RuntimeEvent,
    request_id: RequestId,
    data: &Value,
) -> Result<RuntimeEvent, CoreError> {
    // 载荷可携带显式 command/correlation/causation/parent 关联；缺省 correlation 统一回落到
    // 当前 request_id。这里记录的是追踪关系，不代表给事件授予执行权限。
    Ok(event.with_identity_links(
        optional_event_link(data, "command_id")?,
        optional_event_link(data, "correlation_id")?.or(Some(request_id)),
        optional_event_link(data, "causation_event_id")?,
        optional_event_link(data, "parent_event_id")?,
    ))
}

/// 判断一个 JSON 值是否「足够浅」且「不含 NUL」。
///
/// 【作用】
/// 两个约束，都不是风格问题：
///
/// - **深度**上界复用 `kiana_domain::MAX_REDACTION_DEPTH`。上限的意义是：同一段文本要能被
///   脱敏器、投影器、日志系统和 digest 计算走完，任何一方的递归上限不一致，
///   就会出现「脱敏能处理但投影递归爆栈」这种只在特定载荷下才出现的差异；
/// - **NUL 字节**在键和值里都拒绝。NUL 会截断 C 字符串，是典型的「在一种语言里安全、
///   在另一种语言里截断」的载荷，也是日志注入的常见载体。
///
/// 【深度超限返回 `false` 而不是报错】
/// 它是**谓词**不是校验器：返回 `false` 让调用方决定怎么拒绝。这样「超深」和「含 NUL」
/// 共用一个出口，调用方只需要写一次拒绝逻辑，不会漏掉其中一种。
fn payload_depth(value: &Value, depth: usize) -> bool {
    if depth > kiana_domain::MAX_REDACTION_DEPTH {
        return false;
    }
    match value {
        Value::Array(items) => items.iter().all(|item| payload_depth(item, depth + 1)),
        Value::Object(fields) => fields
            .iter()
            .all(|(key, value)| !key.contains('\0') && payload_depth(value, depth + 1)),
        Value::String(text) => !text.contains('\0'),
        _ => true,
    }
}

/// 收集一条事件引用的 artifact，并排好序、去重。
///
/// 【为什么排序去重】
/// digest 是对内容算的。如果同一个 artifact 引用在两条事件里以不同顺序出现，
/// 它们会被当成两条不同的事实去重放。排序让顺序不再是语义的一部分。
///
/// 【两个上限】
/// - **去重后最多 256 个唯一引用**：一条事件如果能引用无限多的 artifact，它就会变成一张事实清单，
///   重放时把整个存储拖在后面；
/// - **每个引用最长 4096 字节**：这里用 Rust `String::len()` 计 UTF-8 字节，不是 Unicode 字符数。
///   Artifact 引用应该是短的稳定标识，不是路径列表或内容片段；超长时通常意味着有人把 payload 塞进来了。
///
/// 两个数字都是**上限**而不是建议：超了就是 `event_artifact_refs_invalid`，不做截断。
/// 截断会让调用方以为引用完整，而它其实不完整。
fn event_artifact_refs(data: &Value) -> Result<Vec<String>, CoreError> {
    let mut refs = Vec::new();
    if let Some(reference) = data.get("artifact_ref") {
        let reference = reference
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| PortError::Failed("event_artifact_ref_invalid".to_owned()))?;
        refs.push(reference.to_owned());
    }
    if let Some(values) = data.get("artifact_refs") {
        let values = values
            .as_array()
            .ok_or_else(|| PortError::Failed("event_artifact_refs_invalid".to_owned()))?;
        for value in values {
            let reference = value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| PortError::Failed("event_artifact_refs_invalid".to_owned()))?;
            refs.push(reference.to_owned());
        }
    }
    refs.sort_unstable();
    refs.dedup();
    if refs.len() > 256 || refs.iter().any(|reference| reference.len() > 4_096) {
        return Err(PortError::Failed("event_artifact_refs_invalid".to_owned()).into());
    }
    Ok(refs)
}

/// 将业务载荷转换为可写入 EventLog 的脱敏版本，并提取随事件保存的治理元数据。
///
/// 【调用者】普通 `append_event()` 与 `record_terminal_event()` 都在创建 `RuntimeEvent` 前调用，
/// 所以非法载荷不会进入 CAS 追加或终态事务。
/// 【输入/输出】接收任意 JSON 值；成功返回脱敏值、Audit profile digest、可选根级 `data_epoch` 和排序去重后的
/// artifact 引用。`data_epoch` 缺失/null 时为 None，存在但不是非负整数时拒绝。
/// 【核心流程】先以 `redact_event_value()` 递归脱敏 → 检查脱敏结果的深度/NUL → JSON 编码大小上限
/// → 再脱敏必须保持不变 → 结构化凭证扫描 → 提取 epoch 和 artifact refs → 生成 profile digest。
/// 【失败/副作用】深度、大小、序列化、脱敏稳定性、scanner、epoch 或 artifact 引用任一检查失败都返回
/// `CoreError`，调用方不得继续写入；本函数本身不写 EventLog、投影缓存或外部存储。
///
/// ⚠ `redact_event_value()` 本身是递归函数且没有深度上限；本函数在它返回后才调用 `payload_depth()`。
/// 所以 32 层限制能挡住后续处理，却不能保护首次脱敏遍历免于极深输入导致的栈耗尽风险。
/// 另外，Audit profile 当前只用于生成并记录 digest；此处没有调用 `encode_bounded_value()`，
/// 本函数对脱敏后 payload 按 `MAX_JOURNAL_EVENT_BYTES`（1 MiB）限长，不是 Audit 默认 profile 的 16 KiB。
/// EventStore 后续还会用同一常量检查完整 RuntimeEvent 的编码大小。
/// 不能仅凭事件保存了 profile digest，就推断该 profile 的所有边界都已执行。
fn prepare_event_payload(
    data: &Value,
) -> Result<(Value, String, Option<u64>, Vec<String>), CoreError> {
    // EventLog 是不可变事实边界：所有后续投影都只能看到这个版本，因此先脱敏，再检查深度、
    // 字节大小、脱敏幂等性、secret、data_epoch 和 artifact 引用；任一校验失败都不写入。
    // 返回的 profile/epoch/artifact_refs 会随事件元数据一同持久化，供审计和治理读取。
    let redacted = redact_event_value(data);
    if !payload_depth(&redacted, 0) {
        return Err(PortError::Failed("event_payload_depth_limit".to_owned()).into());
    }
    let bytes = serde_json::to_vec(&redacted)
        .map_err(|_| PortError::Failed("event_payload_encode_failed".to_owned()))?;
    if bytes.len() > kiana_domain::MAX_JOURNAL_EVENT_BYTES {
        return Err(PortError::Failed("event_payload_size_limit".to_owned()).into());
    }
    if redact_event_value(&redacted) != redacted {
        return Err(PortError::Failed("event_redaction_not_stable".to_owned()).into());
    }
    kiana_domain::scan_secret_value_with_limit(
        kiana_domain::SecretScanChannel::Event,
        &redacted,
        kiana_domain::MAX_JOURNAL_EVENT_BYTES,
    )
    .map_err(|finding| PortError::Failed(finding.to_string()))?;
    let data_epoch = match redacted.get("data_epoch") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or_else(|| PortError::Failed("event_data_epoch_invalid".to_owned()))?,
        ),
    };
    let artifact_refs = event_artifact_refs(&redacted)?;
    let profile = kiana_domain::RedactionProfile::for_signal(kiana_domain::RedactionSignal::Audit);
    Ok((redacted, profile.profile_digest, data_epoch, artifact_refs))
}

impl ControlPlane {
    /// 追加一条请求内事件，并在成功后推进该请求的局部序号。
    ///
    /// 【调用者】ControlPlane 的生命周期、审批、治理、消息和恢复流程在需要连续写多条事件时调用。
    /// 【输入】request_id 将事件关联到命令；sequence 是命令内顺序；kind 与 data 由业务调用者决定。
    /// 【输出】成功返回 `()`；载荷校验、EventStore 或 CAS 竞争失败返回 `CoreError`。
    /// 【副作用】委托 `append_event()` 写入 EventStore；只有成功后才递增 sequence，失败时调用方可
    /// 按原序号进入自己的错误处理路径。
    /// 【注意】这个局部 sequence 与聚合的 `stream_version` 是不同字段；后者由 append CAS 读取并递增。
    pub(crate) async fn record_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: &mut u64,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        self.append_event(request_id, *sequence, kind, data).await?;
        *sequence += 1;
        Ok(())
    }

    /// 对事件载荷进行加固，并以幂等的聚合版本 CAS 追加到 EventStore。
    ///
    /// 【调用者】`record_event()` 以及需要显式固定 request-local sequence 的少数 ControlPlane 路径。
    /// 【输入】request_id、命令内 sequence、事件种类和业务载荷；如果载荷属于通知事件，还必须
    /// 带可识别的 source 并满足对应通知契约。
    /// 【数据流】载荷脱敏/边界校验 → 根据 message_id、packet_id、run_id 或 request_id 选择聚合 →
    /// 读取该聚合当前 stream_version → 构造并盖章 RuntimeEvent → `append_idempotent_expected()`。
    /// 【输出/失败】成功表示 EventStore 接受了追加或幂等重放；无法安全写入的载荷返回结构化错误，
    /// `run.prompt` 遇到已持久化的 ResultUnknown 返回 `run_unknown_requires_reconciliation`；
    /// 4 次版本/顺序冲突重试仍未成功时返回 `event_append_contention`。
    /// 【副作用】写入 EventStore，并在写入尝试前后使受影响 Run 的 invocation 投影缓存失效；
    /// 这样即使 adapter 在“实际已写入、但返回错误”的窗口报告歧义，旧缓存也不能冒充当前事实。
    /// 【为什么要 CAS】多个 ControlPlane 操作可并发修改同一个聚合；期望版本必须来自用于检查
    /// Unknown fence 的同一次读取，否则一次新 prompt 可能越过并发提交的终态。
    pub(crate) async fn append_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: u64,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        if data.get("source").is_some() {
            let source = data
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| PortError::Failed("notification_event_source_unknown".to_owned()))?;
            let source =
                kiana_domain::notification_event_source(source).map_err(PortError::Failed)?;
            kiana_domain::validate_notification_event(kind, source, &data)
                .map_err(PortError::Failed)?;
        }
        // EventLog 是事实边界：Runner 等上游的通用输出必须先脱敏，才能成为持久记录或进入 Receipt 投影。
        let (data, redaction_profile, data_epoch, artifact_refs) = prepare_event_payload(&data)?;
        let (aggregate_type, aggregate_id) = aggregate_for_event(request_id, &data);
        // CAS 尝试前后都使投影缓存失效：adapter 可能在事件已落盘后才报告不确定错误，
        // 若只在明确成功后失效，旧 fold 就可能在状态不确定时继续被读取。
        let affected_run = data
            .get("run_id")
            .and_then(Value::as_str)
            .and_then(RunId::parse_str)
            .or_else(|| {
                (aggregate_type == "run")
                    .then(|| RunId::parse_str(&aggregate_id))
                    .flatten()
            });
        if let Some(run_id) = affected_run {
            self.invalidate_invocation_projection(run_id);
        }
        let base_idempotency_key =
            format!("{request_id}:{aggregate_type}:{aggregate_id}:{sequence}:{kind}");
        let idempotency_key = base_idempotency_key;
        // 有界重试避免持续竞争时无限占用 ControlPlane；每次 CAS 冲突都会从头重读并重做 Unknown 检查。
        for _ in 0..4 {
            let prior = self
                .events
                .read_stream(&aggregate_type, &aggregate_id)
                .await?;
            if kind == "run.prompt" {
                if let Some(run_id) = affected_run {
                    let state = project_run_state(run_id, &prior)
                        .map_err(|error| PortError::Failed(error.to_string()))?;
                    if state.outcome == Some(RunOutcome::ResultUnknown) {
                        return Err(PortError::Conflict(
                            "run_unknown_requires_reconciliation".to_owned(),
                        )
                        .into());
                    }
                }
            }
            // 当前版本与 run.prompt 的 Unknown fence 都来自本次读取；CAS 失败后必须重读并重新检查两者，
            // 不能只采用新版本却沿用旧状态判断。
            let current_version = prior
                .iter()
                .map(|event| event.stream_version.unwrap_or(event.sequence))
                .max()
                .unwrap_or(0);
            let event = stamp_event_links(
                RuntimeEvent::new(request_id, sequence, kind, data.clone())?
                    .with_stream_metadata(
                        aggregate_type.clone(),
                        aggregate_id.clone(),
                        current_version.saturating_add(1),
                    )
                    .with_idempotency_key(idempotency_key.clone()),
                request_id,
                &data,
            )?
            .with_redaction_metadata(
                redaction_profile.clone(),
                false,
                data_epoch,
                artifact_refs.clone(),
            );
            match self
                .events
                .append_idempotent_expected(event, Some(current_version))
                .await
            {
                Ok(_) => {
                    if let Some(run_id) = affected_run {
                        self.invalidate_invocation_projection(run_id);
                    }
                    return Ok(());
                }
                Err(PortError::Conflict(reason))
                    if reason == "event_stream_version_mismatch"
                        || reason == "event_sequence_not_monotonic" => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(CoreError::Port(PortError::Conflict(
            "event_append_contention".to_owned(),
        )))
    }

    /// 幂等地提交当前 Run/turn 的唯一终态，并在结果未知时同步隔离项目资源。
    ///
    /// 【调用者】run 生命周期、审批结果、workspace checkpoint 等路径在要结束当前 turn 时调用。
    /// 【输入】请求内 sequence、Run ID、四种合法 terminal kind 之一以及终态数据。
    /// 【输出】`true` 表示本次事务新提交；`false` 表示同一 turn 已有相同 kind 的终态；若同一 turn
    /// 已有不同终态，返回 `run_terminal_conflict`。CAS 连续冲突 8 次后返回未确认错误。
    /// 【核心流程】按 Run 的 EventLog 读取最新 prompt 所属 turn → 拒绝第二个互相矛盾的 terminal →
    /// 构造 cancellation fact（若适用）→ 用 command ID/digest 和期望版本提交事务批次。
    /// `run.result_unknown` 若关联到项目，还会把 `resource.quarantined` 与 terminal 放进同一批事务，
    /// 避免只落下“结果未知”却没有隔离可能已受影响资源。
    /// 【副作用】提交 EventStore transition batch、失效 invocation 投影缓存，并尝试排队后续蒸馏。
    /// 【为什么需要单独路径】普通事件可以逐条 CAS 追加；terminal 必须保证同一 turn 只能有一个结论，
    /// 并且未知结果时的资源隔离要与该结论原子绑定。
    pub(crate) async fn record_terminal_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: &mut u64,
        run_id: RunId,
        kind: &str,
        data: Value,
    ) -> Result<bool, CoreError> {
        if !matches!(
            kind,
            "run.completed" | "run.failed" | "run.cancelled" | "run.result_unknown"
        ) {
            return Err(PortError::Failed("terminal_kind_invalid".to_owned()).into());
        }
        let scope = self
            .active_terminal_scopes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&run_id)
            .cloned();
        let _scope_lock = match scope.as_ref() {
            Some(scope) => Some(scope.recorded.lock().await),
            None => None,
        };
        // 终态批次的冲突同样有界；耗尽 8 次后不能宣称终态已提交，因此返回 unconfirmed 错误。
        for _ in 0..8 {
            let prior = self.events.read_stream("run", &run_id.to_string()).await?;
            let turn = prior
                .iter()
                .rposition(|event| event.kind == "run.prompt")
                .unwrap_or(0);
            if let Some(previous) = prior[turn..].iter().find(|event| {
                matches!(
                    event.kind.as_str(),
                    "run.completed" | "run.failed" | "run.cancelled" | "run.result_unknown"
                )
            }) {
                if previous.kind == kind {
                    return Ok(false);
                }
                return Err(PortError::Conflict("run_terminal_conflict".to_owned()).into());
            }
            let version = prior
                .iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0);
            let command_id =
                kiana_domain::derived_request_id("run.terminal", &format!("{run_id}:{turn}"));
            let (redacted_data, redaction_profile, data_epoch, artifact_refs) =
                prepare_event_payload(&data)?;
            let mut redacted_data = redacted_data;
            if let Some(fact) = cancellation_fact_for(kind, &data, run_id, command_id, version)? {
                redacted_data["cancellation_fact"] = serde_json::to_value(fact).map_err(|_| {
                    PortError::Failed("run_cancellation_fact_encode_failed".to_owned())
                })?;
            }
            let event = stamp_event_links(
                RuntimeEvent::new(request_id, *sequence, kind, redacted_data.clone())?
                    .with_stream_metadata("run", run_id.to_string(), version + 1)
                    .with_idempotency_key(format!("run:{run_id}:turn:{turn}:terminal")),
                request_id,
                &redacted_data,
            )?
            .with_redaction_metadata(
                redaction_profile,
                false,
                data_epoch,
                artifact_refs,
            );
            let mut expected_versions = vec![kiana_domain::AggregateVersion {
                aggregate_type: "run".to_owned(),
                aggregate_id: run_id.to_string(),
                version,
            }];
            let mut terminal_events = vec![event];
            if kind == "run.result_unknown" {
                if let Some(root) = prior
                    .iter()
                    .find(|event| event.kind == "run.authorized")
                    .and_then(|event| event.data["project_root"].as_str())
                {
                    let key = kiana_domain::json_digest(
                        &json!({"project_root":Self::canonical_project_root(root)}),
                    );
                    let records = self.events.read_stream("resource_quarantine", &key).await?;
                    let version = records
                        .iter()
                        .filter_map(|event| event.stream_version)
                        .max()
                        .unwrap_or(0);
                    expected_versions.push(kiana_domain::AggregateVersion {
                        aggregate_type: "resource_quarantine".to_owned(),
                        aggregate_id: key.clone(),
                        version,
                    });
                    terminal_events.push(RuntimeEvent::new(request_id,*sequence+1,"resource.quarantined",json!({"run_id":run_id,"project_root":root,"reason":data["error"],"release_requires_stop_evidence":true}))?
                        .with_stream_metadata("resource_quarantine",key,version+1));
                }
            }
            let batch = kiana_domain::TransitionBatch {
                command_id,
                command_digest: kiana_domain::json_digest(
                    &json!({"kind":kind,"data":redacted_data}),
                ),
                expected_versions,
                events: terminal_events,
            };
            match super::dispatch::commit_confirmed(self.events.as_ref(), batch).await {
                Ok(_) => {
                    *sequence += 1;
                    self.invalidate_invocation_projection(run_id);
                    self.queue_terminal_distillation(run_id, kind).await;
                    return Ok(true);
                }
                Err(PortError::Conflict(_)) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(PortError::Failed("result_unknown:terminal_append_unconfirmed".to_owned()).into())
    }

    /// 为本进程内某个 Run 建立 terminal 写入互斥作用域，并返回离开作用域时自动清理的 guard。
    ///
    /// 调用者通常在启动/继续 Runner 前保存 guard；并发 terminal 写入在 scope 内由 async mutex 串行。
    /// 该索引只是本进程的收尾协调，不是持久状态或“副作用已停止”的证明，权威终态仍在 EventLog。
    pub(crate) fn begin_terminal_scope(&self, run_id: RunId) -> TerminalScopeGuard<'_> {
        self.active_terminal_scopes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                run_id,
                Arc::new(RunTerminalScope {
                    recorded: AsyncMutex::new(false),
                }),
            );
        TerminalScopeGuard {
            control_plane: self,
            run_id,
        }
    }

    /// 移除本进程的 terminal 协调索引；通常由 `TerminalScopeGuard::drop()` 自动调用。
    pub(crate) fn end_terminal_scope(&self, run_id: RunId) {
        self.active_terminal_scopes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&run_id);
    }
}

/// 根据载荷确定事件应追加到哪个聚合流。
///
/// 【调用者】`append_event()` 在读取 stream_version 之前调用。
/// 【输入/输出】请求 ID 与载荷；按固定优先级选择 communication/message_id、work_packet/packet_id、
/// run/run_id，否则选择 request/request_id，返回 `(aggregate_type, aggregate_id)`。
/// 【副作用/失败】纯函数，不读取或写入 EventStore；此处只选流，不推导 stream_version。
/// stream_version 必须由 `append_event()` 根据同一次 stream read 取得并用于 CAS，避免并发丢失更新。
pub(crate) fn aggregate_for_event(
    request_id: kiana_domain::RequestId,
    data: &Value,
) -> (String, String) {
    if let Some(message_id) = data
        .get("message_id")
        .and_then(Value::as_str)
        .filter(|message_id| !message_id.trim().is_empty())
    {
        return ("communication".to_owned(), message_id.to_owned());
    }
    if let Some(packet_id) = data
        .get("packet_id")
        .and_then(Value::as_str)
        .filter(|packet_id| !packet_id.trim().is_empty())
    {
        return ("work_packet".to_owned(), packet_id.to_owned());
    }
    if let Some(run_id) = data
        .get("run_id")
        .and_then(Value::as_str)
        .filter(|run_id| !run_id.trim().is_empty())
    {
        return ("run".to_owned(), run_id.to_owned());
    }
    ("request".to_owned(), request_id.to_string())
}
/// 构造直接 Run 响应中的稳定身份元数据块。
///
/// 【调用者】`lifecycle.rs`、`approvals.rs`、`collaboration.rs` 在构造 Run 的终态或等待态响应时调用。
/// 【输入/输出】请求上下文、Run ID 和已经授权的 sandbox 名称；返回携带角色目录版本、模型 profile、
/// actor/session/project 等标识的 JSON 对象。它是响应说明，不取代 EventLog 事件或 Receipt。
/// 【副作用/失败】不访问存储或运行副作用；若角色目录找不到 role，响应会保留上下文给出的 role/department
/// 并以 `role_resolution=false` 表示未解析，调用者仍按原有路径处理该响应。
///
/// 【sandbox 为什么进身份】
/// 因为 sandbox 档位是**授权结论的一部分**。同一条命令在 `read-only` 与可写 sandbox 下
/// 是两种不同的授权结果，事后审计必须能看出当时是哪一种。
pub(crate) fn run_identity(context: &RequestContext, run_id: RunId, sandbox: &str) -> Value {
    let worker = RoleSpec::lookup(&context.role_id);
    let role_id = worker
        .as_ref()
        .map(|role| role.role_id.clone())
        .unwrap_or_else(|| context.role_id.clone());
    let department_id = worker
        .as_ref()
        .map(|role| role.department_id.clone())
        .unwrap_or_else(|| context.department_id.clone());
    with_work_packet(
        json!({
            "schema": RUN_RESULT_SCHEMA,
            "run_id": run_id,
            "session_id": context.session_id,
            "harness": HARNESS_ID,
            "sandbox": sandbox,
            "actor_id": context.actor_id,
            "role_id": role_id,
            "department_id": department_id,
            "prompt_hash": worker.as_ref().map(|role| role.prompt_hash.clone()),
            "role_spec_schema": worker.as_ref().map(|role| role.schema.clone()),
            "role_version": worker.as_ref().map(|role| role.version),
            "role_catalog_schema": kiana_domain::ROLE_CATALOG_SCHEMA,
            "role_catalog_version": kiana_domain::SchemaVersion::new(1, 0),
            "input_schema": worker.as_ref().map(|role| role.input_schema.clone()),
            "output_schema": worker.as_ref().map(|role| role.output_schema.clone()),
            "model_profile": worker.as_ref().map(|role| role.model_profile.clone()),
            "role_resolution": worker.is_some(),
        }),
        context,
    )
}

/// 如果响应/Receipt 有所属 WorkPacket，就附加它的标识和输入来源标签。
///
/// 【调用者】`run_identity()` 与 Receipt 投影组装路径调用。
/// 【输入/输出】已有 JSON 对象和请求上下文；只在 `work_packet_id` 非空白时写入
/// `work_packet_id`，并把 `input` 标为 `work_packet`。
/// 【为什么是可选】普通直接用户 prompt 没有 WorkPacket；不能为缺少的 packet 伪造 ID。
/// 该函数仅补充来源元数据，不创建 packet，也不改变执行输入。
pub(crate) fn with_work_packet(mut receipt: Value, context: &RequestContext) -> Value {
    if let Some(work_packet_id) = context
        .work_packet_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        receipt["work_packet_id"] = json!(work_packet_id);
        receipt["input"] = json!("work_packet");
    }
    receipt
}

/// 为 Run 内执行的能力结果补齐事件所需的执行、效果和身份关联字段。
///
/// 【调用者】`capabilities.rs` 在能力结果结束后调用，再把返回载荷写入 `run.tool_result` 等事实。
/// 【输入】Broker/handler 的 output、已由 ControlPlane 标记身份的 CapabilityRequest、RequestContext 和 Run ID。
/// 【数据流】先把 output 放入 `result` 并脱敏 → 根据 result_unknown/not_executed 推导保守的效果标志 →
/// 补 attempt、stop_state、run/session、capability/operation/grant/lease/request ID。
/// 【为什么保守】Unknown 表示 effect 是否发生无法确定，因此标记 effect_known=false 与 fenced=true；
/// 明确 not_executed 才可记录 zero_effect=true。调用方已显式提供的默认字段通过 `entry().or_insert()` 保留。
/// 【副作用/失败】只构造 JSON，不写 EventLog；后续 append 仍会再次执行统一的载荷校验和脱敏。
pub(crate) fn capability_event_payload(
    output: &Value,
    request: &CapabilityRequest,
    context: &RequestContext,
    run_id: RunId,
) -> Value {
    let mut payload = capability_result_payload(output);
    let result_unknown = result_unknown_value(&payload);
    let result = capability_result_value(&payload);
    let not_executed = result.get("not_executed") == Some(&json!(true));
    let stop_confirmed = result.get("stop_confirmed").and_then(Value::as_bool);
    let object = payload
        .as_object_mut()
        .expect("capability event payload is normalized to an object");
    object.entry("attempt".to_owned()).or_insert(json!(1));
    object
        .entry("effect_started".to_owned())
        .or_insert(json!(!not_executed));
    object
        .entry("effect_known".to_owned())
        .or_insert(json!(!result_unknown));
    object
        .entry("zero_effect".to_owned())
        .or_insert(json!(not_executed));
    object
        .entry("fenced".to_owned())
        .or_insert(json!(result_unknown));
    if let Some(stop_confirmed) = stop_confirmed {
        object
            .entry("stop_state".to_owned())
            .or_insert(json!(if stop_confirmed {
                "confirmed"
            } else {
                "unconfirmed"
            }));
    } else {
        object
            .entry("stop_state".to_owned())
            .or_insert(json!("not_requested"));
    }
    object.insert("run_id".to_owned(), json!(run_id));
    object.insert("session_id".to_owned(), json!(context.session_id));
    object.insert("capability".to_owned(), json!(request.capability));
    object.insert("operation".to_owned(), json!(request.operation));
    object.insert("cell_id".to_owned(), json!(request.cell_id));
    object.insert(
        "capability_grant_id".to_owned(),
        json!(request.capability_grant_id),
    );
    object.insert("budget_lease_id".to_owned(), json!(request.budget_lease_id));
    object.insert(
        "capability_request_id".to_owned(),
        json!(request.request_id),
    );
    payload
}

/// 为不属于 Harness Run 的直接能力结果创建相同的基础效果字段，但不添加 Run/Session 身份。
///
/// 【调用者】与 `capability_event_payload()` 对应的直接能力路径使用；请求 ID、授权 grant 和 lease
/// 仍绑定到结果，避免直接调用的审计字段比 Run 内调用少一层身份关联。
/// 【区别】没有 Run ID、Session ID，也无法从此处声明取消停止证明，所以缺省 `stop_state` 为
/// `not_requested`；结果仍可能是 Unknown，效果标志仍按 Unknown 保守填写。
pub(crate) fn direct_capability_event_payload(
    output: &Value,
    request: &CapabilityRequest,
) -> Value {
    let mut payload = capability_result_payload(output);
    let result_unknown = result_unknown_value(&payload);
    let result = capability_result_value(&payload);
    let not_executed = result.get("not_executed") == Some(&json!(true));
    let object = payload
        .as_object_mut()
        .expect("direct capability payload is normalized to an object");
    object.entry("attempt".to_owned()).or_insert(json!(1));
    object
        .entry("effect_started".to_owned())
        .or_insert(json!(!not_executed));
    object
        .entry("effect_known".to_owned())
        .or_insert(json!(!result_unknown));
    object
        .entry("zero_effect".to_owned())
        .or_insert(json!(not_executed));
    object
        .entry("fenced".to_owned())
        .or_insert(json!(result_unknown));
    object
        .entry("stop_state".to_owned())
        .or_insert(json!("not_requested"));
    object.insert("capability".to_owned(), json!(request.capability));
    object.insert("operation".to_owned(), json!(request.operation));
    object.insert("cell_id".to_owned(), json!(request.cell_id));
    object.insert(
        "capability_grant_id".to_owned(),
        json!(request.capability_grant_id),
    );
    object.insert("budget_lease_id".to_owned(), json!(request.budget_lease_id));
    object.insert(
        "capability_request_id".to_owned(),
        json!(request.request_id),
    );
    payload
}

/// 将任意 Broker 输出包进 `result` 字段，并在此处先做一次事件值脱敏。
///
/// 标量、数组和对象都被保留为 `result` 的 JSON 值；若结果对象含 `source`，额外复制到根字段
/// `result_source`，便于旧的审计/通知消费者读取。后续事件追加仍会经过 `prepare_event_payload()`，
/// 这次局部脱敏不能替代最终 EventLog 边界校验。
fn capability_result_payload(output: &Value) -> Value {
    // 统一包装器接受标量、数组或对象输出，确保上层总能在 `result` 下读取执行器原始结果。
    let result = redact_event_value(output);
    let mut payload = json!({"result": result});
    if let Some(source) = payload["result"].get("source").cloned() {
        payload["result_source"] = source;
    }
    payload
}

/// 取得包装后的执行器结果；若输入是旧的根级布局，则把整个对象作为兼容结果读取。
///
/// 这个 accessor 让 `result_unknown` / `not_executed` 判断同时兼容新旧事件形状，不会改写传入值。
fn capability_result_value(value: &Value) -> &Value {
    // 新载荷用 result 包装；旧/中间形状可能直接把效果字段放在根对象，保留兼容读取。
    value.get("result").unwrap_or(value)
}
/// 把服务端解析出的身份盖到一个能力请求上。
///
/// 【⚠ 这是防冒充的关键位置】
/// 请求里的 actor / project / role 字段**一律以服务端解析出来的为准**。
/// 客户端自报的身份在这里被覆盖，而不是被采纳。
///
/// 为什么值得单独一个函数：这类「用服务端值覆盖客户端值」的操作，最容易在后续重构里
/// 被写成「如果客户端没传就用它自己的」——那就是一个完整的身份伪造漏洞。
/// 集中在这里，是为了让这个决定只有一个地方可以发生。
///
/// 【作用域收窄】
/// 盖上身份的同时也盖上**作用域**：命令能到达的范围由服务端决定，
/// 客户端声明的范围只会被取交集。
///
/// 【调用者】`capabilities.rs` 在进入授权/Broker 路径前调用。
/// 【输入/副作用】可变能力请求与已验证的 RequestContext；当 arguments 是 JSON object 时覆盖服务端身份，
/// 并把路径范围收窄到角色与上下文允许范围的交集。
/// 【特殊返回】arguments 不是 object 时不修改请求；后续 capability/schema 校验负责拒绝无效形状。
/// 不能改成只在客户端字段为空时填充，否则调用方可以伪造 actor、project 或越权扩大 path_allow。
pub(crate) fn stamp_request_identity(request: &mut CapabilityRequest, context: &RequestContext) {
    let Some(arguments) = request.arguments.as_object_mut() else {
        return;
    };
    arguments.insert("role_id".to_owned(), json!(context.role_id));
    arguments.insert("department_id".to_owned(), json!(context.department_id));
    arguments.insert("session_id".to_owned(), json!(context.session_id.as_str()));
    arguments.insert("project_root".to_owned(), json!(context.project_root));
    arguments.insert("actor_id".to_owned(), json!(context.actor_id));
    arguments.insert("project_trusted".to_owned(), json!(context.project_trusted));
    let role = RoleSpec::lookup(&context.role_id);
    let role_paths = role.map(|role| role.path_allow).unwrap_or_default();
    let paths = if context.path_allow.is_empty() {
        role_paths
    } else {
        let mut intersection = Vec::new();
        for allowed in &context.path_allow {
            if kiana_domain::enforce_path_containment(&role_paths, allowed).is_ok() {
                intersection.push(allowed.clone());
            }
        }
        for allowed in &role_paths {
            if kiana_domain::enforce_path_containment(&context.path_allow, allowed).is_ok() {
                intersection.push(allowed.clone());
            }
        }
        intersection.sort();
        intersection.dedup();
        intersection
    };
    arguments.insert("path_allow".to_owned(), json!(paths));
}
