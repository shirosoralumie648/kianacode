//! # 文件职责
//!
//! 本文件实现 Core 提供给 Runner 的模型调用预算端口：它从已授权 Run 和项目授权事件读取
//! 当前限额，在 EventStore 中记录每次模型尝试的预留、许可准备、派发和结算事实。它不负责
//! 选择 Provider、不构造 HTTP 请求，也不直接发送网络请求；真正的请求仍由现有 Harness 和
//! Provider Client 完成。
//!
//! # 在系统中的位置与数据流
//!
//! ```text
//! DaemonHost 组合根
//!   -> 把 JournalModelBudget 安装到 Runner / KianaHarness
//!   -> Harness::invoke_model 为一次 attempt 调用 ModelClient::prepare_call
//!   -> Harness 先在本地 BudgetLedger 预留，再调用 reserve_prepared()
//!   -> 本文件校验 Run / authority / budget，并原子追加 model.reserved + model.prepared
//!   -> Provider 的 complete_admitted() 调用 consume_prepared()
//!   -> 本文件核对同一份 PreparedModelCall 和许可，再追加 model.dispatching
//!   -> Provider transport 才可发送已封存的请求
//!   -> Harness 收到响应或判断结果未知后调用 settle()
//!   -> 本文件追加 model.settled，供后续预算读取和审计使用
//! ```
//!
//! 每次模型尝试（attempt）都单独预留，因此重试也会占用一次模型调用额度。预留、派发和结算是不同事实：
//! `model.reserved` 记录最坏情况下可消耗的预算，`model.dispatching` 说明许可被一次性消费，
//! `model.settled` 记录可知的实际用量；如果 Provider 用量未知，结算按预留上限计费，避免把
//! 无法证明没有发生的消耗退还给后续请求。比较并交换（Compare-And-Swap，CAS）会把“读取到的
//! 聚合版本”和“写入事件”放在同一提交中核对；EventStore 多聚合 CAS 只保护本文件写入的状态，
//! 不包含外部 Provider 副作用。
use super::*;
use kiana_domain::{
    derived_request_id, json_digest, AggregateVersion, BudgetLeaseId, BudgetReservationFact,
    BudgetScope, BudgetSettlementFact, ExecutionId, RuntimeBudget, TransitionBatch,
};

/// 以 EventStore 中的 Run、授权和预算事件作为事实来源的模型预算端口实现。
///
/// DaemonHost 为 Harness 安装本对象；`events` 是共享 EventStore 端口的 `Arc`（原子引用计数
/// 指针），允许多个组件共享同一个存储端口，而不会复制一份可独立变化的预算状态。实际预算
/// 累计在调用时从事件流重新折叠，不保存在本结构的内存字段中。
pub struct JournalModelBudget {
    events: Arc<dyn EventStorePort>,
}
impl JournalModelBudget {
    /// 创建一个只依赖 EventStore 端口的预算 adapter。
    ///
    /// 【调用者】由 `kiana-daemon::DaemonHost::with_runner_events_and_authority()` 在装配
    /// Runner 时创建，并通过 `RunnerPort::install_model_budget()` 安装。
    /// 【输入】`events` 是 Daemon 已包裹必要 stream 行为的共享事件存储端口。
    /// 【副作用】只保存 Arc 引用；不会立即读写日志或预留预算。
    pub fn new(events: Arc<dyn EventStorePort>) -> Self {
        Self { events }
    }
}

/// 把领域校验原因包装为端口层的稳定失败类别。
///
/// 上层端口调用者收到 `PortError::Failed` 后负责转成当前操作的错误；此 helper 不重试、
/// 不写日志，也不判断某个失败是否应当由外层 Run 终止。
fn failed(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}

/// 读取可写入持久事件的 Unix 毫秒时间戳。
///
/// 这里使用墙上时钟而不是进程内单调时钟，因为 `issued_at_unix_ms`、截止时间和结算时间需要
/// 序列化进事件，在进程重启后仍能比较。系统时间早于 Unix epoch 或无法转换为 `u64` 时返回
/// `clock_untrusted` 并拒绝预算操作；这个检查不能消除主机时钟被错误配置或回拨的风险。
fn time_ms() -> Result<u64, PortError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or_else(|| failed("clock_untrusted"))
}

#[async_trait::async_trait]
impl kiana_ports::ModelBudgetPort for JournalModelBudget {
    /// 端口契约中的简化预留入口；它没有 `PreparedModelCall`，因此不会签发可供 Provider 网络
    /// 边界校验的 `ModelCallPermit`，只会写预算预留事实。当前 Harness 的生产模型路径使用
    /// `reserve_prepared()`；目前未找到生产代码调用此简化方法，不应拿它替代准备请求与许可绑定流程。
    async fn reserve(
        &self,
        run_id: RunId,
        request_id: RequestId,
        tokens: u64,
    ) -> Result<(), PortError> {
        self.reserve_call(run_id, request_id, tokens, None).await
    }

    /// 为一份已经封存的模型请求检查预算并签发短期、attempt 绑定的 Provider 调用许可。
    ///
    /// 【调用者】`KianaHarness::invoke_model()` 在构造 `PreparedModelCall` 和本地预算预留之后、
    /// 调用 `ModelClient::complete_admitted()` 之前调用。它受 Harness 剩余 deadline 和取消信号约束。
    /// 【输入】PreparedModelCall 包含 Run assignment、attempt ID、deadline、请求 hash、路由、预算、
    /// credential revision 与 provider account 绑定；本函数不接收 Provider credential 明文。
    /// PreparedModelCall 在内存中仍有 request/wire body，但下面持久化的只是 `audit()` 安全视图，
    /// 不把完整 prompt、工具参数或编译后的请求体写入事件。
    /// 【输出】成功返回 `ModelCallPermit`；其中 request hash、路由/配置 digest、authority revision、
    /// credential revision 和 provider account 必须与同一份准备请求一致。
    /// 【失败】准备请求/assignment 无效、截止时间已过、Run 或授权已变化、预算耗尽、EventStore
    /// CAS 冲突或提交结果未知时返回端口错误。Harness 会在预留失败时释放自己的本地 attempt
    /// reservation，并且不会进入 Provider 调用。
    /// 【副作用】经 `reserve_call()` 原子写入 `model.reserved` 与 `model.prepared` 两条相关流事实；
    /// 返回的许可是依赖事件日志与一次性 CAS 消费的应用层许可，不是独立数字签名；它也不是
    /// Provider 请求已发送的证明。
    async fn reserve_prepared(
        &self,
        prepared: &kiana_domain::PreparedModelCall,
    ) -> Result<kiana_domain::ModelCallPermit, PortError> {
        prepared.validate().map_err(|e| failed(&e.to_string()))?;
        let run_id = prepared
            .spec
            .assignment
            .as_ref()
            .ok_or_else(|| failed("model_assignment_required"))?
            .run_id;
        let now = time_ms()?;
        // 许可不能在准备请求已经超时后签发。下面的 60_000 毫秒（60 秒）只是许可自身的最长
        // 存活时间；若模型调用 deadline 更近，就使用更早的那个时点，避免 admission 延长 Run 权限。
        if prepared.spec.deadline_unix_ms <= now {
            return Err(failed("model_deadline_expired"));
        }
        let permit = kiana_domain::ModelCallPermit {
            schema: "kiana.model-call-permit.v1".to_owned(),
            permit_id: derived_request_id("model.permit", &prepared.spec.attempt_id.to_string()),
            run_id,
            attempt_id: prepared.spec.attempt_id,
            request_hash: prepared.request_hash.clone(),
            expires_at_unix_ms: prepared
                .spec
                .deadline_unix_ms
                .min(now.saturating_add(60_000)),
            route_digest: Some(prepared.route.digest()),
            configuration_revision: Some(prepared.route.configuration_revision.clone()),
            authority_revision: prepared
                .spec
                .assignment
                .as_ref()
                .and_then(|assignment| assignment.authority_revision.clone()),
            credential_revision: prepared.credential_revision.clone(),
            provider_account: prepared.provider_account.clone(),
        };
        self.reserve_call(
            run_id,
            prepared.spec.attempt_id,
            prepared.budget.total,
            Some((prepared, &permit)),
        )
        .await?;
        Ok(permit)
    }

    /// 在进入 Provider transport 前原子消费预留许可，确保一份 attempt 许可不能重复派发。
    ///
    /// 【调用者】Provider gateway 的 `complete_admitted()` 在真正调用 `transport::send()` 前调用；
    /// 对不支持 admission 的兼容 client，端口默认拒绝而不是默默绕过此检查。
    /// 【输入】同一份准备请求以及 `reserve_prepared()` 返回的许可。
    /// 【核心流程】重算请求封存值并校验许可；读回 attempt stream 中唯一的 `model.prepared` 记录；
    /// 比较许可审计视图；最后通过 expected versions 原子追加 `model.dispatching`。
    /// 【失败】过期/作用域不符、准备记录缺失或被篡改、attempt 已有派发记录、CAS 冲突或结果未知
    /// 都会拒绝继续发送。失败由 Provider 转成模型调用错误；本方法不会自行重试或重新签发许可。
    /// 【设计原因】仅在内存中检查一份许可不足以阻止重放；持久 attempt stream 的版本 CAS 让并发
    /// 消费同一许可时至多一个调用能成功跨过派发边界。
    async fn consume_prepared(
        &self,
        prepared: &kiana_domain::PreparedModelCall,
        permit: &kiana_domain::ModelCallPermit,
    ) -> Result<(), PortError> {
        prepared.validate().map_err(|e| failed(&e.to_string()))?;
        let now = time_ms()?;
        prepared
            .spec
            .assignment
            .as_ref()
            .ok_or_else(|| failed("model_assignment_required"))?;
        permit
            .validate_for_prepared(prepared, now)
            .map_err(|error| failed(&error.code))?;
        // 再核对关键身份与有效期，避免只依赖单个校验器的宽松兼容逻辑；此处采用 `>=`，所以
        // 到达 `expires_at_unix_ms` 的同一毫秒即视为过期。
        if permit.schema != "kiana.model-call-permit.v1"
            || permit.request_hash != prepared.request_hash
            || permit.attempt_id != prepared.spec.attempt_id
            || prepared
                .spec
                .assignment
                .as_ref()
                .is_none_or(|assignment| assignment.run_id != permit.run_id)
            || now >= permit.expires_at_unix_ms
        {
            return Err(failed("model_permit_scope_or_expiry_mismatch"));
        }
        let records = self
            .events
            .read_stream("model_attempt", &permit.attempt_id.to_string())
            .await?;
        // 预留阶段为这个 attempt 原子写入一次 prepared 记录。不存在、重复或 permit 审计值不同，
        // 都说明当前内存请求与持久 admission 事实不能一一对应，因此不能继续走网络副作用。
        if records.len() != 1
            || records[0].kind != "model.prepared"
            || records[0].data["permit"] != kiana_domain::redact_value(&json!(permit))
        {
            return Err(failed("model_permit_unavailable"));
        }
        let mut expected: Vec<AggregateVersion> =
            serde_json::from_value(records[0].data["authority_versions"].clone())
                .map_err(|_| failed("model_permit_authority_invalid"))?;
        // 除了 attempt stream 的版本 1，本次派发还依赖预留时读取的 Run 与 authority 版本。
        // 把这些版本放进同一个原子转换，可以拒绝读取许可后授权或 Run 状态又发生变化的竞态。
        expected.push(AggregateVersion::new(
            "model_attempt",
            permit.attempt_id.to_string(),
            1,
        ));
        let command_id = derived_request_id("model.dispatch", &permit.attempt_id.to_string());
        let event=RuntimeEvent::new(command_id,1,"model.dispatching",json!({"run_id":permit.run_id,"model_request_id":permit.attempt_id,"request_hash":permit.request_hash,"permit_id":permit.permit_id}))
            .map_err(|e|failed(&e.to_string()))?.with_stream_metadata("model_attempt",permit.attempt_id.to_string(),2);
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(&json!({"permit":permit})),
            expected_versions: expected,
            events: vec![event],
        };
        // commit_confirmed: false 表示本次首次消费成功；true 表示同一 command 已被重放。
        // 对许可来说重放必须失败，否则同一模型 attempt 可能重复产生上游费用或响应。
        if super::dispatch::commit_confirmed(self.events.as_ref(), batch).await? {
            return Err(failed("model_permit_already_consumed"));
        }
        Ok(())
    }

    /// 把 attempt 的模型用量折叠成一条持久结算事实。
    ///
    /// 【调用者】`KianaHarness::invoke_model()` 在一次已派发 attempt 的模型结果（包含超时/取消等
    /// 结果未知情况）归并后调用；`tokens` 是 Provider 报告的输入/输出 token 总量，`None` 表示
    /// 当前不能证明实际用量。
    /// 【数据流】按 Run 读取 `model.reserved` → 还原其中的预算 reservation fact → 构造结算 fact →
    /// 原子提交 `model.settled` → 后续 reservation fold 用收费值替换原预留上限。
    /// 【失败】找不到 reservation、记录字段损坏、事实身份不匹配、重复/冲突结算或无法确认提交时
    /// 返回错误；调用方会把它作为 `model_usage_settlement_failed` 向 Run 上层传播。
    /// 【重要副作用】`tokens == None` 时按 reservation 的完整上限收费，不能退款未知消耗。
    /// 若报告的用量超过预留，本函数仍先持久化超额结算，再返回
    /// `model_reported_usage_exceeds_reservation`；不能把错误检查移到提交之前，否则超额使用事实会丢失。
    async fn settle(
        &self,
        run_id: RunId,
        request_id: RequestId,
        tokens: Option<u64>,
    ) -> Result<(), PortError> {
        let records = self
            .events
            .read_stream("model_budget", &run_id.to_string())
            .await?;
        let reservation = records
            .iter()
            .find(|event| {
                event.kind == "model.reserved"
                    && event.data["model_request_id"] == json!(request_id)
            })
            .ok_or_else(|| failed("model_budget_reservation_missing"))?;
        let upper = reservation.data["tokens"]
            .as_u64()
            .ok_or_else(|| failed("model_budget_record_invalid"))?;
        let charged = tokens.unwrap_or(upper);
        let command_id = derived_request_id("model.settle", &request_id.to_string());
        let version = records
            .iter()
            .filter_map(|e| e.stream_version)
            .max()
            .unwrap_or(0);
        let settled_at = time_ms()?;
        let reservation: BudgetReservationFact =
            serde_json::from_value(reservation.data["reservation_fact"].clone())
                .map_err(|_| failed("model_budget_reservation_fact_invalid"))?;
        if reservation.reservation_id != request_id
            || reservation.run_id != run_id
            || reservation.budget_lease_id != BudgetLeaseId::from_uuid(run_id.as_uuid())
        {
            return Err(failed("model_budget_reservation_fact_invalid"));
        }
        // BudgetSettlementFact 同时保存 `usage_known` 和 `charged_tokens`。tokens 为 None 时，
        // settlement fact 自身保留“实际用量未知”，而外层预算账目仍按 upper 收取预留值。
        let settlement = BudgetSettlementFact::new(&reservation, tokens, settled_at)
            .map_err(|error| failed(&error))?;
        let payload = json!({"run_id":run_id,"model_request_id":request_id,"charged_tokens":charged,
            "reported_tokens":tokens,"usage_known":tokens.is_some(),"reservation_exceeded":charged>upper,"at_unix_ms":settled_at,
            "settlement_fact":settlement});
        let event = RuntimeEvent::new(command_id, 1, "model.settled", payload)
            .map_err(|e| failed(&e.to_string()))?
            .with_stream_metadata("model_budget", run_id.to_string(), version + 1);
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(
                &json!({"run_id":run_id,"request_id":request_id,"tokens":tokens}),
            ),
            expected_versions: vec![AggregateVersion {
                aggregate_type: "model_budget".to_owned(),
                aggregate_id: run_id.to_string(),
                version,
            }],
            events: vec![event],
        };
        // 先提交不可变结算事实，再报告用量超额。事件是后续审计和预算折叠的权威依据，返回错误
        // 不能撤销已经提交的事实；调用方不应把这个错误解释为“没有结算”。
        super::dispatch::commit_confirmed(self.events.as_ref(), batch).await?;
        if charged > upper {
            return Err(failed("model_reported_usage_exceeds_reservation"));
        }
        Ok(())
    }
}

impl JournalModelBudget {
    /// 读取授权状态、重算 Run 预算，再为一次模型尝试准备可原子提交的事件批次。
    ///
    /// 【调用者】`reserve()` 与 `reserve_prepared()` 共用此函数；后者额外传入已经封存的请求和
    /// 待写入 attempt stream 的许可记录。
    /// 【输入】`run_id` 选择 Run 与预算聚合；`request_id` 是本次模型 attempt 的稳定身份；`tokens`
    /// 是需要预留的最大 token 数；`prepared` 为 `Some` 时同时绑定 Provider 路由和请求 hash。
    /// 【输出/副作用】所有检查通过后，通过 `dispatch::commit_confirmed()` 原子写入
    /// `model.reserved`，并在 prepared 路径额外写入 `model.prepared`；没有任何 EventStore 写入时
    /// 才可把失败理解为未预留。若提交返回 `Unknown`，调用方不能假设事件肯定没有落盘。
    /// 【失败】Run/授权不存在或过期、assignment 不符、Run 已进入终态、预算记录损坏/重复、额度或
    /// wall time 用尽、版本冲突、EventStore 不支持原子转换以及无法确认提交都会返回错误；本函数
    /// 不自行重试，Harness 会把 admission 失败返回给上层，并且不会调用 Provider。
    ///
    /// 【核心流程】先用 `run.authorized` 找项目，再用规范化项目根目录定位当前 authority 流；然后
    /// 校验模型 assignment 与 Run 仍绑定于该授权，折叠已有 `model.reserved`/`model.settled` 事实，
    /// 检查单次调用、累计 token 与总时长限制；最后把 authority、Run、预算流（及可选 attempt 流）
    /// 的期望版本一并交给 EventStore，避免多个并发请求各自按同一个旧余额同时通过。
    async fn reserve_call(
        &self,
        run_id: RunId,
        request_id: RequestId,
        tokens: u64,
        prepared: Option<(
            &kiana_domain::PreparedModelCall,
            &kiana_domain::ModelCallPermit,
        )>,
    ) -> Result<(), PortError> {
        // Run 的授权事件保存了启动时使用的 project_root 和 authority_revision；先从它得到
        // 项目身份，再计算与 ControlPlane 相同的规范化根目录摘要，才能定位当前 authority 聚合。
        let run = self.events.read_stream("run", &run_id.to_string()).await?;
        let authority = run
            .iter()
            .find(|event| event.kind == "run.authorized")
            .ok_or_else(|| failed("model_budget_authority_missing"))?;
        let root = authority.data["project_root"]
            .as_str()
            .ok_or_else(|| failed("model_budget_project_missing"))?;
        let authority_key =
            json_digest(&json!({"project_root":ControlPlane::canonical_project_root(root)}));
        let current_authority = self.events.read_stream("authority", &authority_key).await?;
        let current = current_authority
            .last()
            .ok_or_else(|| failed("model_budget_authority_missing"))?;
        let authority_version = current.stream_version.unwrap_or(0);
        let authority_revision = format!(
            "{}:{}",
            authority_version,
            current.data["revision_digest"].as_str().unwrap_or_default()
        );
        // Run.authority_revision 是“EventStore 流版本 + 配置摘要”的快照绑定字符串；它不等同于
        // AuthorityLedger 重建出的语义 authority_epoch。下方发现的构造参数混用与这里的 revision
        // 比较是两条不同校验，不能认为此比较已经证明 epoch 字段也正确。
        if authority.data["authority_revision"].as_str() != Some(authority_revision.as_str()) {
            return Err(failed("model_budget_authority_changed"));
        }
        // PreparedModelCall 的 assignment 是服务端在 Run 建立时选定的角色/模型配置；这里要求
        // Run、role、profile 和 revision 与授权事实一致，防止请求在预算边界偷换身份或模型配置。
        if let Some((call, _)) = prepared {
            let assignment = call
                .spec
                .assignment
                .as_ref()
                .ok_or_else(|| failed("model_assignment_required"))?;
            if assignment.run_id != run_id
                || authority.data["role_id"] != assignment.role_id
                || authority.data["model_profile"] != assignment.profile
                || authority.data["authority_revision"]
                    != serde_json::json!(assignment.authority_revision)
            {
                return Err(failed("model_assignment_authority_mismatch"));
            }
        }
        let limit: RuntimeBudget = serde_json::from_value(authority.data["runtime_budget"].clone())
            .map_err(|_| failed("model_budget_authority_missing"))?;
        limit.validate().map_err(failed)?;
        // `turn` 指向最近一次 run.prompt 在 Run stream 中的位置。只检查该 prompt 之后的事件，
        // 因为旧 turn 的 completed/failed 不应阻止后续已开始的新 turn；当前 turn 若已取消、结束
        // 或结果未知，则模型调用必须在接触 Provider 前拒绝。
        let turn = run
            .iter()
            .rposition(|e| e.kind == "run.prompt")
            .unwrap_or(0);
        if run[turn..].iter().any(|event| {
            matches!(
                event.kind.as_str(),
                "run.cancelling"
                    | "run.cancelled"
                    | "run.result_unknown"
                    | "run.completed"
                    | "run.failed"
            )
        }) {
            return Err(failed("model_budget_run_not_active"));
        }
        let records = self
            .events
            .read_stream("model_budget", &run_id.to_string())
            .await?;
        // `reserved` 同时保存尚未结算 attempt 的预留上限和已结算 attempt 的实际收费额；遇到
        // model.settled 时把对应值替换为 charged_tokens。`reservation_facts` 用来验证结算是否
        // 确实对应先前预留，`settled` 只用来拒绝同一 attempt 的重复结算，`calls` 统计所有尝试数，
        // 所以失败重试也消耗 max_model_calls。
        let mut reserved = HashMap::new();
        let mut reservation_facts = HashMap::new();
        let mut settled = std::collections::HashSet::new();
        let mut calls = 0u64;
        for event in &records {
            let key = event.data["model_request_id"]
                .as_str()
                .ok_or_else(|| failed("model_budget_record_invalid"))?;
            match event.kind.as_str() {
                "model.reserved" => {
                    calls = calls.saturating_add(1);
                    let reservation: BudgetReservationFact =
                        serde_json::from_value(event.data["reservation_fact"].clone())
                            .map_err(|_| failed("model_budget_reservation_fact_invalid"))?;
                    if reservation.validate().is_err()
                        || reservation.reservation_id
                            != RequestId::parse_str(key)
                                .ok_or_else(|| failed("model_budget_reservation_fact_invalid"))?
                        || reservation.execution_id
                            != ExecutionId::from_uuid(reservation.reservation_id.as_uuid())
                        || reservation.run_id != run_id
                        || reservation.budget_lease_id != BudgetLeaseId::from_uuid(run_id.as_uuid())
                        || reservation.model_calls != 1
                        || reservation.tool_calls != 0
                        || reservation.tokens
                            != event.data["tokens"]
                                .as_u64()
                                .ok_or_else(|| failed("model_budget_record_invalid"))?
                    {
                        return Err(failed("model_budget_reservation_fact_invalid"));
                    }
                    // 一个 attempt ID 只能对应一份 reservation。重复 ID 会使后续 settlement 无法
                    // 唯一指出应结算哪次调用，因此即使两条记录的数字相同也拒绝整个历史。
                    if reserved
                        .insert(
                            key.to_owned(),
                            event.data["tokens"]
                                .as_u64()
                                .ok_or_else(|| failed("model_budget_record_invalid"))?,
                        )
                        .is_some()
                    {
                        return Err(failed("model_budget_duplicate_reservation"));
                    }
                    reservation_facts.insert(key.to_owned(), reservation);
                }
                "model.settled" => {
                    let settlement: BudgetSettlementFact =
                        serde_json::from_value(event.data["settlement_fact"].clone())
                            .map_err(|_| failed("model_budget_settlement_fact_invalid"))?;
                    let reservation = reservation_facts
                        .get(key)
                        .ok_or_else(|| failed("model_budget_settlement_orphan"))?;
                    settlement
                        .validate_against(reservation)
                        .map_err(|_| failed("model_budget_settlement_fact_invalid"))?;
                    if !settled.insert(key.to_owned()) {
                        return Err(failed("model_budget_duplicate_settlement"));
                    }
                    let charge = event.data["charged_tokens"]
                        .as_u64()
                        .ok_or_else(|| failed("model_budget_record_invalid"))?;
                    let old = reserved
                        .get_mut(key)
                        .ok_or_else(|| failed("model_budget_settlement_orphan"))?;
                    *old = charge;
                }
                // 预算流只允许本实现认识的预留/结算事实；不认识的类型可能改变余额含义，故失败关闭。
                _ => return Err(failed("model_budget_unknown_required_event")),
            }
        }
        let used = reserved
            .values()
            .try_fold(0u64, |sum, n| sum.checked_add(*n))
            .ok_or_else(|| failed("model_budget_overflow"))?;
        let now = time_ms()?;
        // 首条预留事实的时间代表该 Run 的模型预算窗口起点。时间倒退或达到 max_wall_time_ms
        // 都拒绝新请求；空流没有已记录的起点，因此此限制只能从第一份 reservation 开始计时。
        if let Some(first) = records.first() {
            let started = first.data["at_unix_ms"]
                .as_u64()
                .ok_or_else(|| failed("model_budget_clock_missing"))?;
            if now < started || now - started >= limit.max_wall_time_ms {
                return Err(failed("model_budget_wall_time_exhausted"));
            }
        }
        if tokens == 0
            || calls >= limit.max_model_calls
            || used
                .checked_add(tokens)
                .is_none_or(|n| n > limit.max_tokens)
        {
            return Err(failed("model_budget_exhausted_before_provider"));
        }
        let version = records
            .iter()
            .filter_map(|e| e.stream_version)
            .max()
            .unwrap_or(0);
        let command_id = derived_request_id("model.reserve", &request_id.to_string());
        // 预留事实自身也记录 60 秒后的 expires_at，但本文件的预算折叠不会因该时刻到达而删除
        // 未结算记录或自动退款；若进程在结算前中断，这笔上限会继续占用本 Run 的预算。
        // Provider 许可的截止时间是在 consume_prepared() 中单独检查的，不能把事实字段到期
        // 理解成预算预留自动释放。
        // 这是一份 Run 级模型预算预留：execution_id 由 attempt ID 确定以保持重放身份稳定，
        // budget_lease_id 由 Run ID 确定；它只登记 1 次模型调用，不把模型调用算作 tool call
        // 或额外 effect，也没有父级 reservation。输出 token 上限与总 token 预留分开保存，
        // 供后续核验 Provider 响应是否越界。
        let reservation_fact = BudgetReservationFact::new(
            request_id,
            ExecutionId::from_uuid(request_id.as_uuid()),
            run_id,
            BudgetLeaseId::from_uuid(run_id.as_uuid()),
            BudgetScope::Run,
            None,
            1,
            0,
            tokens,
            0,
            // prepared 路径把响应输出上限写入 fact；输入预算已经包含在 `tokens` 中。
            // 简化路径没有细分输入/输出，只能把整个预留视为最大输出预算。
            prepared
                .map(|(call, _)| call.budget.reserved_output.max(1))
                .unwrap_or(tokens.max(1)),
            // 注意：此处读取到的 `authority_version` 是 authority EventStore 流的版本号。
            // `BudgetReservationFact` 参数名为 `authority_epoch`，而 Authority 模块把语义授权代次
            // 与 EventStore stream version 明确区分；当前实现将流版本传入该字段，存在两种概念
            // 被混用的疑点。此注释记录实现现状，不在本次注释任务中改变数据。
            authority_version,
            version,
            now,
            now.saturating_add(60_000),
        )
        .map_err(|error| failed(&error))?;
        // 审计 payload 把“当前 attempt 序号”和“已结算费用 + 未结算预留”分开记录。
        // `used + tokens` 在前面的 checked_add 预算判断中已证明没有溢出；basis 说明当前预留
        // 的估算口径来自文本字节数与输出 token 上限，并非 Provider 的最终计量。
        let payload = json!({"run_id":run_id,"model_request_id":request_id,"tokens":tokens,"at_unix_ms":now,
            "reservation_fact":reservation_fact,
            "request_count":calls+1,"charged_and_reserved_tokens":used+tokens,"limit":limit,"basis":"text_bytes_plus_output_limit"});
        let event = RuntimeEvent::new(command_id, 1, "model.reserved", payload.clone())
            .map_err(|e| failed(&e.to_string()))?
            .with_stream_metadata("model_budget", run_id.to_string(), version + 1);
        let mut batch = TransitionBatch {
            command_id,
            command_digest: json_digest(
                &json!({"run_id":run_id,"request_id":request_id,"tokens":tokens}),
            ),
            expected_versions: vec![
                AggregateVersion {
                    aggregate_type: "authority".to_owned(),
                    aggregate_id: authority_key,
                    version: authority_version,
                },
                AggregateVersion {
                    aggregate_type: "model_budget".to_owned(),
                    aggregate_id: run_id.to_string(),
                    version,
                },
                AggregateVersion {
                    aggregate_type: "run".to_owned(),
                    aggregate_id: run_id.to_string(),
                    version: run
                        .iter()
                        .filter_map(|e| e.stream_version)
                        .max()
                        .unwrap_or(0),
                },
            ],
            events: vec![event],
        };
        if let Some((call, permit)) = prepared {
            // Reservation 需要锁住授权、Run 与预算三个快照；prepared 路径还要保证 attempt ID 尚无
            // 先前准备记录。`authority_versions` 一并保存在审计 payload，消费许可时会重新作为 CAS
            // 条件使用，防止读到许可后授权/Run 又变化。
            let authority_versions = batch
                .expected_versions
                .iter()
                .filter(|v| v.aggregate_type != "model_budget")
                .cloned()
                .collect::<Vec<_>>();
            batch.expected_versions.push(AggregateVersion::new(
                "model_attempt",
                request_id.to_string(),
                0,
            ));
            // 事件日志只保留经脱敏的 permit 与 PreparedModelCall audit view，不保存编译后的 wire body、
            // 完整消息/工具内容或 credential 明文；请求 hash 和之后的 validate_for_prepared 仍把
            // 当前内存中的原始准备请求绑定到这条审计记录。
            batch.events.push(RuntimeEvent::new(command_id,2,"model.prepared",json!({"permit":kiana_domain::redact_value(&json!(permit)),"prepared":kiana_domain::redact_value(&call.audit()),"authority_versions":authority_versions}))
                .map_err(|e|failed(&e.to_string()))?.with_stream_metadata("model_attempt",request_id.to_string(),1));
            batch.command_digest =
                json_digest(&json!({"request_hash":call.request_hash,"permit":permit}));
        }
        // 命令 ID 由 attempt request_id 确定。确认重放代表同一 reservation 已存在；本方法不会
        // 把它当成一次新的额度，也不会重新发出相同许可，以免同一 attempt 被调用两次。
        if super::dispatch::commit_confirmed(self.events.as_ref(), batch).await? {
            return Err(failed("model_request_already_reserved"));
        }
        Ok(())
    }
}
