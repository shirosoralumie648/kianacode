//! Core 对 trace 与模型版本治理查询命令的处理入口。
//!
//! `commands.rs` 只把 `trace.capture`、`trace.replay`、`version.drift` 这三个名字路由到本文件。
//! 本文件负责在读取历史前检查项目/操作者范围，从 EventStore 读取事实，再交给 Domain 版本合同、
//! receipt helper 或 history fold：
//!
//! ```text
//! command intent
//!   -> commands.rs 精确匹配命令名
//!   -> 本文件检查 project_trusted 与 actor
//!   -> EventStore 已提交事件 + actor/project 的 run.authorized 归属
//!      -> version.drift: run.model_turn -> Domain DriftReport -> 只读响应
//!      -> trace.replay: 已保存 trace -> digest/revocation 检查 -> history/invocations 响应
//!      -> trace.capture: 指定 run + 安全读取源文件 -> 哈希清单/事件/receipt -> 追加捕获事件
//! ```
//!
//! drift 和 replay 只读；capture 会追加 `golden_trace.captured`。本模块不调用模型接口或能力执行适配器，
//! 也不会因 drift 自动切换模型。它用 `read_all()` 取得全量事件快照，因此存储
//! 必须支持全量读取，查询耗时与临时内存会随日志规模增长；不能把读取失败误当成“没有事件”。
use super::*;
use std::collections::BTreeMap;

impl ControlPlane {
    /// 处理版本 drift 投影或 golden trace 捕获/回放命令。
    ///
    /// 【调用者】`commands.rs` 在 intent 名精确匹配 `trace.capture`、`trace.replay` 或
    /// `version.drift` 后调用。
    /// 【输入】上游构造的 RequestContext、命令名和 JSON 参数。`project_trusted` 与 actor 是查询前提；
    /// 项目根参与 run ownership 过滤。
    /// 【输出】权限/归属不符合时返回 blocked CoreResponse；成功时返回 drift 报告、纯读取 replay
    /// 结果或刚捕获的 trace JSON。
    /// 【副作用】drift/replay 不追加事件；capture 读取源文件并追加 `golden_trace.captured`，写入仍经
    /// Core `append_event()` 的脱敏、幂等和 EventStore CAS 边界。
    /// 【失败情况】全量读取错误、参数缺失/格式错误、Artifact 不可读、摘要/事件形状无效或 Domain
    /// drift 合同失败会返回 Core/Port 错误；项目不可信、capture run owner 不匹配、数据撤销或 replay
    /// digest 不匹配会返回 blocked；replay 未找到同时匹配 trace/owner/project root 的事件则返回
    /// `trace_not_found` 错误。
    /// 这里没有 Provider 重试、账号禁用或自动路由切换。
    /// 【核心流程】拒绝不可信项目或空 actor → 读取全量事件 → 从同一 actor 且规范化项目根相同的
    /// `run.authorized` 事件收集 owned run IDs → 按命令名执行三个分支。
    /// 【为什么先做归属过滤】EventStore 是跨 run 的事实集合；不能把用户参数中的 run_id/trace_id
    /// 直接当成所有权证明。capture/replay 还会独立核对数据撤销状态。
    /// 【维护注意】路由器当前只传三个名字，函数最后的默认分支因此表示 capture。若以后增加调用名，
    /// 必须在这里增加显式分支，否则未知名字会被解释成 trace.capture 参数并进入捕获路径。
    pub(crate) async fn handle_version_command(
        &self,
        context: RequestContext,
        name: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if !context.project_trusted || context.actor_id.as_deref().is_none_or(str::is_empty) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "project_untrusted",
            ));
        }
        // 全量读取失败时直接向上传播；EventStore 默认端口实现可以明确报告“不支持全量扫描”，
        // 不能把该失败伪装成空报告或“找不到 trace”。三个分支复用这一次完整快照。
        let all = self.events.read_all().await?;
        // 只把曾由同一 actor 在同一规范化项目根授权的 run 纳入可见集合。后续 drift/capture
        // 依赖此集合，不能仅凭参数里的 run_id 扩大查询范围。
        let owned = all
            .iter()
            .filter(|event| {
                event.kind == "run.authorized"
                    && event.data["actor_id"] == json!(context.actor_id)
                    && event.data["project_root"].as_str().is_some_and(|root| {
                        Self::canonical_project_root(root)
                            == Self::canonical_project_root(&context.project_root)
                    })
            })
            .filter_map(|event| event.data["run_id"].as_str())
            .collect::<HashSet<_>>();
        if name == "version.drift" {
            // 这是 committed EventLog 的只读投影：扫描 owned run 的每条模型调用事件，不询问模型
            // 当前状态，也不重新执行调用。全量扫描和报告里的 event_ids 都会随历史规模增长。
            let mut report = kiana_domain::DriftReport::default();
            for event in all.iter().filter(|event| {
                event.kind == "run.model_turn"
                    && event.data["run_id"]
                        .as_str()
                        .is_some_and(|id| owned.contains(id))
            }) {
                let metadata = &event.data;
                // 使用事件保存的审计标签，不重新解析当时的真实 Provider 请求。runtime_version
                // 传入当前二进制的 Cargo 包版本，而不是从历史事件恢复生产该事件时的版本；同一批历史
                // 事件换一个版本的程序查询，可能因此进入不同版本桶。字段形状校验也不认证其来源。
                let version = kiana_domain::RouteDecision::from_model_turn(
                    metadata,
                    env!("CARGO_PKG_VERSION"),
                );
                // Runner 成功时把 error 写为 null，失败时写入错误对象；缺字段或非 null 都按错误计数。
                // elapsed_ms 缺失、负数或类型不匹配时按 0 记入，不会单独报数据质量错误。
                let error = metadata.get("error").is_some_and(|value| !value.is_null());
                report
                    .record(
                        version,
                        event.event_id,
                        metadata["elapsed_ms"].as_u64().unwrap_or(0),
                        error,
                    )
                    .map_err(|reason| PortError::Failed(reason.to_owned()))?;
            }
            report
                .validate()
                .map_err(|reason| PortError::Failed(reason.to_owned()))?;
            // 返回本次查询刚计算出的 JSON；这里没有 append_event，所以不是持久化 EvalStore、告警记录
            // 或模型路由变更。
            return Ok(CoreResponse::completed(
                context.request_id,
                serde_json::to_value(report)
                    .map_err(|_| PortError::Failed("drift_report_encode_failed".to_owned()))?,
            ));
        }
        if name == "trace.replay" {
            // Replay 只从已捕获 trace 重建展示用 history/invocations，不会再次运行模型或能力。
            let id = arguments["trace_id"]
                .as_str()
                .ok_or_else(|| PortError::Failed("trace_id_required".to_owned()))?;
            // 此处项目根是字符串精确比较；上面的 owned-run 检查使用 canonical_project_root。
            // 同一路径的别名/符号链接写法可能通过 ownership 检查，却无法命中保存的 trace。
            let event = all
                .iter()
                .find(|event| {
                    event.kind == "golden_trace.captured"
                        && event.data["trace_id"] == id
                        && event.data["owner_id"] == json!(context.actor_id)
                        && event.data["project_root"] == context.project_root
                })
                .ok_or_else(|| PortError::Failed("trace_not_found".to_owned()))?;
            let trace = &event.data;
            let events: Vec<RuntimeEvent> = serde_json::from_value(trace["events"].clone())
                .map_err(|_| PortError::Failed("trace_events_invalid".to_owned()))?;
            if kiana_domain::json_digest(&json!(events)) != trace["events_hash"] {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "trace_digest_mismatch",
                ));
            }
            let run_id = serde_json::from_value::<RunId>(trace["run_id"].clone())
                .map_err(|_| PortError::Failed("trace_run_invalid".to_owned()))?;
            if self.run_data_revoked(run_id).await? {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "trace_data_revoked",
                ));
            }
            // 从保存事件重新折叠模型可见历史，并从相同事件派生 invocation 视图。receipt 是捕获时
            // 保存的回执；响应显式标记 side_effects=false、provider_calls=0。
            let history = crate::history::fold_model_visible_history(
                &events,
                trace["session_id"].as_str().unwrap_or_default(),
                Some(run_id),
            );
            let invocations =
                crate::project_invocations(run_id, &events).map_err(PortError::Failed)?;
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"schema":"kiana.golden-replay.v1","trace_id":id,
                "history":history,"invocations":invocations,"receipt":trace["receipt"],"side_effects":false,"provider_calls":0}),
            ));
        }
        // 命令路由只允许三个固定名字；正常情况下到达此处就是 trace.capture。run_id 必须解析成
        // Domain RunId，并通过 actor/project 归属和数据撤销检查。
        let run_id = arguments["run_id"]
            .as_str()
            .and_then(RunId::parse_str)
            .ok_or_else(|| PortError::Failed("trace_run_required".to_owned()))?;
        if !owned.contains(run_id.to_string().as_str()) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_owner_mismatch",
            ));
        }
        if self.run_data_revoked(run_id).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "trace_data_revoked",
            ));
        }
        // 排序 map 固定 manifest 的 JSON 摘要顺序。source_files 限制为 1 到 128 项；每个路径仍交给
        // read_project_artifact() 做项目目录边界内的安全读取，不能把列表当任意文件读取器。
        let mut manifest = BTreeMap::new();
        let files = arguments["source_files"]
            .as_array()
            .filter(|files| !files.is_empty() && files.len() <= 128)
            .ok_or_else(|| PortError::Failed("trace_source_manifest_required".to_owned()))?;
        for file in files {
            let path = file
                .as_str()
                .ok_or_else(|| PortError::Failed("trace_source_path_invalid".to_owned()))?;
            let contents = crate::artifacts::read_project_artifact(
                Path::new(&context.project_root),
                Path::new(path),
                "trace_source_unavailable",
            )
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
            manifest.insert(path, kiana_domain::prompt_hash(&contents));
        }
        // 只捕获该 run 的事实事件，并排除可重建的 run.snapshot，避免把投影快照当独立事实重复保存。
        let events = crate::receipts::filter_run_events(&all, run_id)
            .into_iter()
            .filter(|event| event.kind != "run.snapshot")
            .collect::<Vec<_>>();
        let author = events
            .iter()
            .find(|event| event.kind == "run.authorized")
            .ok_or_else(|| PortError::Failed("trace_author_missing".to_owned()))?;
        let session = author.data["session_id"].clone();
        let source_hash = kiana_domain::json_digest(&json!(manifest));
        let events_hash = kiana_domain::json_digest(&json!(events));
        let trace_id =
            kiana_domain::json_digest(&json!({"source":source_hash,"events":events_hash}));
        let input = events
            .iter()
            .filter(|event| event.kind == "run.prompt")
            .map(|event| &event.data)
            .collect::<Vec<_>>();
        // 摘要由 source manifest 与捕获事件派生；receipt 也由这批 run 事件构造。无密钥摘要用于
        // 发现内容变化，不是对文件或事件来源的签名认证。
        let trace = json!({"schema":"kiana.golden-trace.v1","trace_id":trace_id,"owner_id":context.actor_id,
            "project_root":context.project_root,"run_id":run_id,"session_id":session,
            "source_snapshot":source_hash,"source_manifest":manifest,"input_hash":kiana_domain::json_digest(&json!(input)),
            "runtime_version":env!("CARGO_PKG_VERSION"),"events_hash":events_hash,
            "receipt":crate::receipts::receipt_from_events(&context,run_id,"read-only",Value::Null,&events),"events":events});
        self.append_event(
            context.request_id,
            // 本命令路径只追加这一条捕获事件；1 是 request-local event sequence，参与幂等键，
            // 不是 run 序号、trace 序号或重试次数。append_event 内部仍会按聚合版本执行 CAS。
            1,
            "golden_trace.captured",
            trace.clone(),
        )
        .await?;
        Ok(CoreResponse::completed(context.request_id, trace))
    }
}
