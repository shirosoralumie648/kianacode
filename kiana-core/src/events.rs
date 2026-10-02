//! 事件载荷的构造与加固：把一次执行的结果，变成一条**可以永久保存**的事实。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! ControlPlane 决定发生了什么
//!    ↓  【本文件】加固、盖章、绑定身份
//! EventLog（append-only 的事实）
//!    ↓
//! 投影 / Receipt / 审计
//! ```
//!
//! **上游**：`ControlPlane` 的各个执行路径在 append 之前调用本文件。
//! **下游**：本文件不写盘、不发网络、不调用任何 adapter，它只**返回**一个 `Value`。
//!
//! # 为什么事件载荷需要这么多检查
//!
//! 因为事件是**不可撤销的**。一条已经提交的事件会被重放、被投影、被审计、被当作事实引用；
//! 写进去的东西如果有问题，事后无法修正，只能在它之上再叠一层解释。所以本文件的职责是
//! **在写之前把几类不可逆的错误拦下来**：
//!
//! 1. **太深或含 NUL 的载荷** —— 会让下游的解析器、投影器和日志系统出现不一致行为；
//! 2. **含 secret 的载荷** —— 事件一旦落盘，secret 就有第二份副本；
//! 3. **不认识的身份** —— 一条不绑定 actor/project/run 的事件，事后无法追责；
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
    value
        .get("error_code")
        .and_then(Value::as_str)
        .map(CapabilityErrorCode::from_reason)
        .or_else(|| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(CapabilityErrorCode::from_reason)
        })
        .is_some_and(|code| {
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
/// 并且由进程组确认「真的停了」，才叫 `stop_confirmed`。这些信息被打包成
/// `RunCancellationFact`，写进事件，事后可核。
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

fn stamp_event_links(
    event: RuntimeEvent,
    request_id: RequestId,
    data: &Value,
) -> Result<RuntimeEvent, CoreError> {
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
/// - **最多 256 个引用**：一条事件如果能引用无限多的 artifact，它就会变成一张事实清单，
///   重放时把整个存储拖在后面；
/// - **每个引用最长 4096 字符**：artifact 引用应该是短的稳定标识，不是路径列表或内容片段。
///   超过这个长度通常意味着有人把 payload 塞进来了。
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

fn prepare_event_payload(
    data: &Value,
) -> Result<(Value, String, Option<u64>, Vec<String>), CoreError> {
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
    kiana_domain::scan_secret_value(kiana_domain::SecretScanChannel::Event, &redacted)
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
        // EventLog is the canonical boundary: generic runner output must be redacted before it
        // can become durable fact or feed a receipt projection.
        let (data, redaction_profile, data_epoch, artifact_refs) = prepare_event_payload(&data)?;
        let (aggregate_type, aggregate_id) = aggregate_for_event(request_id, &data);
        // Invalidate before attempting the CAS append as well as after success: an adapter may
        // report an ambiguous error after durably writing the event, and a stale fold must never
        // survive that uncertainty.
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
        for _ in 0..4 {
            let current_version = self
                .events
                .read_stream(&aggregate_type, &aggregate_id)
                .await?
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

    pub(crate) fn end_terminal_scope(&self, run_id: RunId) {
        self.active_terminal_scopes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&run_id);
    }
}

/// 取出事件要写入的聚合标识与版本。
///
/// 【作用】
/// EventLog 的 CAS 是**按聚合**做的，所以每条事件都必须说清「我改的是哪个聚合的第几版」。
/// 这个函数把上下文里已经解析好的东西整理成事件载荷需要的形状，不做任何查询。
///
/// 【为什么版本号必须由调用方给出而不是自己推断】
/// 因为推断版本正是并发丢失更新的方式。读到的版本和写入的版本必须来自同一次读取，
/// 中间任何一次重新读取都会让 CAS 失去意义。
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
/// 构造一条事件里的 run 身份块：run id、sandbox、以及它是谁发起的。
///
/// 【为什么身份要打进每条事件，而不是只存一份】
/// 因为事件会被独立重放、独立投影、独立审计。一条只有 run id 没有 actor 的事件，
/// 在三个月后被单独取出来看时，无法回答「当时是谁让它跑的」。
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
pub(crate) fn capability_event_payload(
    output: &Value,
    request: &CapabilityRequest,
    context: &RequestContext,
    run_id: RunId,
) -> Value {
    let mut payload = capability_result_payload(output);
    let result_unknown = result_unknown_value(&payload);
    let object = payload
        .as_object_mut()
        .expect("capability event payload is normalized to an object");
    let not_executed = object.get("not_executed") == Some(&json!(true));
    let stop_confirmed = object.get("stop_confirmed").and_then(Value::as_bool);
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

pub(crate) fn direct_capability_event_payload(
    output: &Value,
    request: &CapabilityRequest,
) -> Value {
    let mut payload = capability_result_payload(output);
    let result_unknown = result_unknown_value(&payload);
    let object = payload
        .as_object_mut()
        .expect("direct capability payload is normalized to an object");
    let not_executed = object.get("not_executed") == Some(&json!(true));
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

fn capability_result_payload(output: &Value) -> Value {
    let mut payload = redact_event_value(output);
    if !payload.is_object() {
        payload = json!({ "output": payload });
    }
    // Capability result data cannot populate the notification authority envelope.
    if let Some(object) = payload.as_object_mut() {
        if let Some(source) = object.remove("source") {
            object.insert("result_source".to_owned(), source);
        }
    }
    payload
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
