//! Product-surface execution through the owned Kiana harness.
//!
//! Print mode and the SDK must start runs via `kiana-daemon` / `kiana-core`.
//! They must not call the legacy `runner.rs` model/tool loop.

//! 产品执行面：一次 run 从这里真正开始跑。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! 打印模式 / SDK
//!    ↓  【本文件】构造 RequestEnvelope + 身份
//! kiana_client::KianaClient
//!    ↓  LocalDaemonTransport（同进程直调，不开 socket）
//! kiana-daemon::DaemonHost
//!    ↓
//! kiana-core::ControlPlane  ← 授权在这里发生
//!    ↓  策略 → 关卡 → 审批 → 能力执行
//! ResponseEnvelope
//! ```
//!
//! 文件头那句话不是客套：**产品面必须经 daemon/core，不许走 legacy `runner.rs` 那个
//! 模型/工具循环。** 理由和 `AGENTS.md` 一样——只有一条执行脊柱，授权状态、事件流和
//! 审批才不会分裂成两套。
//!
//! # 与同目录 `mcp.rs` 的对照
//!
//! 两个文件都会「执行工具」，但只有本文件经过 `ControlPlane`：
//! - 本文件：run 先进控制面，授权是执行链上的**一环**；即便需要人工批准，
//!   被批准的也是控制面**已经作出的决定**，handler 只是把它呈现出来；
//! - `mcp.rs`：`execute_tool_call` 直接被调用，不存在控制面。
//! 那条路已作为安全发现记录在
//! [安全发现文档](../../docs/security/mcp-http-unauthenticated-tool-execution.md)。
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::{DaemonHost, LocalModelConfig};
use kiana_protocol::{
    normalize_role_path, ApprovalChallenge, ApprovalDecision, CapabilityRequest, EntryPointKind,
    ExecutionStatus, PermissionProfile, RequestEnvelope, RequestMetadata, ResponseEnvelope,
    RoleSpec, RunId, Symposium, WorkPacket, ROLE_BUILDER, ROLE_PM, ROLE_REVIEWER,
};
use kiana_tools::tool_execution::{
    PermissionPromptDecision, PermissionPromptHandler, PermissionPromptRequest,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// 协议请求中标识此产品 Harness 执行面的稳定名称。
pub const HARNESS_ID: &str = "kiana-harness";

/// 在指定 Host 上提交一个 CLI/界面命令，并返回服务端的协议响应。
///
/// Human command 的操作档位仍来自调用方选项；若 session 已绑定角色，则使用授权历史中的角色，
/// 避免后续命令通过改写 role 字段切换身份。命令解析和副作用由 daemon/Core 处理。
pub async fn command_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    name: impl Into<String> + Send,
    arguments: Value,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    // 将可能借用调用方数据的 session id 转为拥有值，以便跨越后续 await。
    let session_id = session_id.into();
    // 保留原选项中的 operator profile；这里只为恢复出的 session 角色创建独立副本。
    let mut command_options = options.clone();
    // 通过授权事件恢复 session 角色，同时执行已有项目/部门/sandbox 一致性检查。
    let session_options = session_options_on_host(&host, &session_id, None, options).await?;
    if let Some(role) = session_options.get("role") {
        // 只覆盖 role，避免恢复 session 身份时覆盖调用方明确选择的其它命令选项。
        command_options.insert("role".to_owned(), role.clone());
    }
    let (client, metadata) = client_on_host(host, session_id, &command_options)?;
    // transport 成功只表示 Host 返回了协议响应；命令的业务状态仍看 ResponseEnvelope.status。
    client
        .command(metadata, name, arguments)
        .await
        .map_err(anyhow::Error::msg)
}

/// 同进程直调 daemon 的 transport（与 `command_dispatch.rs` 里那个是同一种做法）。
///
/// 不开 socket，因此省掉了序列化往返与端口管理；代价是**没有网络层替你兜底**，
/// 所有身份与信任字段都必须由 [ `client_on_host` ] 认真构造。
pub struct LocalDaemonTransport {
    // Transport 与同一个 Host 共享所有权，避免异步请求期间 Host 被提前释放。
    host: Arc<DaemonHost>,
}

// ClientTransport 用 async_trait 声明异步方法；impl 同样使用宏以生成匹配的异步 trait 方法签名。
#[async_trait]
impl ClientTransport for LocalDaemonTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        // 同进程适配直接等待 Host.handle；运行失败通常编码在 ResponseEnvelope，而非传输 Err 中。
        Ok(self.host.handle(request).await)
    }
}

#[derive(Debug, Clone)]
/// 一次跑完之后的产出。
///
/// 之所以单独成型而不是直接返回 `ResponseEnvelope`：调用方真正需要的是
/// 「助手说了什么」和「对应哪个 run」，而不是整个信封。让每个调用方自己去信封里挖，
/// 迟早有人挖错字段。
pub struct HarnessRunResult {
    /// 从最终响应中提取的助手文本，供 CLI 或调用方直接展示。
    pub text: String,
    /// 响应携带的 harness step 数；字段缺失或格式不符时转换函数会回退为 1。
    pub steps: u64,
    /// 响应报告的沙箱档位；若响应未提供该字段，转换函数会回退为 `read-only`。
    pub sandbox: String,
    /// 保留协议响应中的结构化输出，避免扁平化时丢掉调用方仍需读取的字段。
    pub output: Value,
}

/// 根据入口选项启动一次 run。
///
/// 显式布尔值 `stream: true` 会交给流式展示适配器；其余情况走普通响应路径。
/// 两条路径最终都通过本地 `DaemonHost` 和 ControlPlane，不在入口侧执行工具。
pub async fn run_envelope(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    // `Value::as_bool` 只接受 JSON 布尔值；缺失、字符串或数字都按未启用流式处理。
    if options
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        // 流式适配器负责订阅并渲染增量事件；它仍会复用同一 Host 执行请求。
        return crate::stream_render::render_stream_envelope(
            session_id.into(),
            prompt.into(),
            options,
        )
        .await;
    }
    run_envelope_on_host(
        new_local_host_with_options(options)?,
        session_id,
        prompt,
        options,
    )
    .await
}

/// 在调用方已经装配的 Host 上发起 run，供工作台等复用 Host 的入口使用。
///
/// 先把松散的入口选项解析为沙箱策略，再将身份元数据和 prompt 交给 `KianaClient`。
/// 返回值是 daemon 的完整协议响应；客户端或 daemon 错误会转换为 `anyhow::Error` 向入口传播。
pub async fn run_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    // 这里只解析要传给运行时的沙箱档位，不代表入口自行完成了授权判断。
    let policy = sandbox_policy_from_options(options)?;
    // client 与 metadata 从同一 Host/选项构造，随后由协议客户端送入 DaemonHost。
    let (client, metadata) = client_on_host(host, session_id, options)?;
    // `.await` 等待 daemon 完成整个请求；客户端或 daemon 错误沿 ClientError 返回。
    client
        .run(metadata, prompt.into(), policy.sandbox)
        .await
        .map_err(anyhow::Error::msg)
}

/// 以空历史启动一个受控 Harness run，并把成功响应转换为简化结果。
pub async fn run_owned_harness(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<HarnessRunResult> {
    // 先消费 Into<String>，再委托给 history 版本；空 Vec 表示本次 prompt 没有附带旧消息。
    let prompt = prompt.into();
    run_owned_harness_with_history(session_id, prompt, Vec::new(), options).await
}

/// 带既有对话历史运行 Harness，不提供自动弹出的审批 UI 回调。
///
/// 如果服务端返回的状态不是 Completed，`completed_harness_result` 会将其转成错误。
pub async fn run_owned_harness_with_history(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    history: Vec<kiana_protocol::ConversationMessage>,
    options: &HashMap<String, Value>,
) -> Result<HarnessRunResult> {
    // 共用同一内部请求路径，None 表示 AwaitingApproval 原样返回给此包装层处理。
    let response = run_envelope_with_history_and_permission_handler(
        session_id, prompt, history, options, None,
    )
    .await?;
    completed_harness_result(response)
}

/// 带对话历史和 UI 审批回调运行 Harness。
///
/// handler 只呈现 ControlPlane 已创建的 challenge；最终批准/拒绝仍通过协议请求回到 Core。
pub async fn run_owned_harness_with_history_and_permission_handler(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    history: Vec<kiana_protocol::ConversationMessage>,
    options: &HashMap<String, Value>,
    permission_handler: &dyn PermissionPromptHandler,
) -> Result<HarnessRunResult> {
    // trait object 允许 CLI/SDK 注入不同 UI 实现；本函数只借用 handler，不取得其所有权。
    let response = run_envelope_with_history_and_permission_handler(
        session_id,
        prompt,
        history,
        options,
        Some(permission_handler),
    )
    .await?;
    completed_harness_result(response)
}

/// 带历史启动 run 并返回完整协议响应，不把 Completed/Failed/AwaitingApproval 折叠成单一结果类型。
pub async fn run_envelope_with_history(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    history: Vec<kiana_protocol::ConversationMessage>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    // 不带 UI 审批回调的重载仍使用同一内部流程；待批状态交给调用方自行展示或保留。
    run_envelope_with_history_and_permission_handler(session_id, prompt, history, options, None)
        .await
}

/// 真正发起一次 run 的地方。
///
/// 【流程】
/// 1. `new_local_host_with_options` —— 按选项装配 `DaemonHost`；
/// 2. `sandbox_policy_from_options` —— 把选项里的沙箱档位解析成策略；
/// 3. `client_on_host` —— 造 client 与 `RequestMetadata`；
/// 4. `client.run_with_history(...)` —— **run 从这里进入控制面**；
/// 5. 如果响应是 `AwaitingApproval`，且调用方给了 `permission_handler`，
///    才把这次待批**呈现**给 handler 决定。
///
/// 【第 5 步最容易被理解反】
/// 它不是「在这里请求授权」。授权已经在第 4 步里由 `ControlPlane` 作出了——
/// `AwaitingApproval` 意味着控制面判定「这件事需要人点头」，并且**已经把这次待批
/// 记录成事实**。handler 拿到的是那个决定的呈现，不是提问权。
/// 真正的批准仍然要通过 `decide_approval_envelope_on_host` 回到控制面去落。
///
/// 【所以 `permission_handler: None` 并不是「没人管」】
/// `run_envelope_with_history` 走的正是这条：没有 handler，于是把 `AwaitingApproval`
/// 原样返回给调用方，由界面去展示、再决定要不要批。**决定权在控制面，呈现权在调用方**，
/// 这两件事分开之后，缺一个 handler 只会让体验变差，不会让控制消失。
///
/// 【为什么 `PermissionPromptRequest` 里要塞那么多东西】
/// 因为 handler 往往长在另一个进程里（另一个 UI、另一个 agent）。要让人在远离现场
/// 的情况下做出决定，就得把「是谁、要做的是什么、为什么被拦」一次性带齐。
/// `decision_reason` 用 `type: other` 包一层，也是为了对上 `PermissionPromptDecision`
/// 期待的形状。
async fn run_envelope_with_history_and_permission_handler(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    history: Vec<kiana_protocol::ConversationMessage>,
    options: &HashMap<String, Value>,
    permission_handler: Option<&dyn PermissionPromptHandler>,
) -> Result<ResponseEnvelope> {
    // 每次独立调用都在这里装配本地 Host；Host 内部再统一组合 Core、Runner 和 Broker。
    let host = new_local_host_with_options(options)?;
    // 沙箱选项先解析成运行策略，避免把未经解释的 JSON 直接传给下游执行层。
    let policy = sandbox_policy_from_options(options)?;
    // 身份、会话和入口元数据与目标 Host 一起构造，作为本次协议请求的调用者上下文。
    let (client, metadata) = client_on_host(host, session_id, options)?;
    // Client 按值消费 RequestMetadata；这里克隆一份发起 run，保留原值供待批时回送审批决定。
    // await 期间异步任务可被取消；本函数不另建模型循环。
    let response = client
        .run_with_history(metadata.clone(), prompt.into(), history, policy.sandbox)
        .await
        .map_err(anyhow::Error::msg)?;
    if response.status != ExecutionStatus::AwaitingApproval {
        // 只有控制面明确返回待批状态才展示审批界面；完成、失败、取消等终态原样交还。
        return Ok(response);
    }
    let Some(handler) = permission_handler else {
        // 无交互 handler 的调用方仍拿到控制面已记录的 challenge，可自行显示或暂缓决定。
        return Ok(response);
    };
    // 协议输出使用 JSON 承载审批投影，这里还原为强类型以读取审批 ID、哈希和 nonce。
    // 字段缺失或形状不符时附加稳定上下文，避免把后续问题误报成无关的反序列化错误。
    let challenge: ApprovalChallenge = serde_json::from_value(response.output["approval"].clone())
        .context("approval_challenge_invalid")?;
    let capability: CapabilityRequest =
        serde_json::from_value(response.output["capability"].clone())
            .context("approval_capability_invalid")?;
    // UI 回调只接收展示所需的工具信息和拒绝原因；它不获得直接执行能力。
    let prompt_request = PermissionPromptRequest {
        request_id: challenge.approval_id.to_string(),
        tool_name: capability.operation.clone(),
        input: capability.arguments.clone(),
        tool_use_id: capability.arguments["call_id"]
            .as_str()
            .unwrap_or("tool")
            .to_owned(),
        permission_suggestions: Value::Array(Vec::new()),
        blocked_path: None,
        decision_reason: serde_json::json!({
            "type": "other",
            "reason": challenge.reason,
        }),
        agent_id: std::env::var("KIANA_AGENT_ID").ok(),
    };
    // 交互发生在异步 handler 内。handler 错误向调用方传播，不会被伪装成批准或拒绝。
    let decision = match handler
        .prompt(prompt_request)
        .await
        .map_err(anyhow::Error::msg)?
    {
        PermissionPromptDecision::Allow => ApprovalDecision::Approve,
        // 当前协议决定只保留批准/拒绝语义；handler 的拒绝文本没有随此决定发送。
        PermissionPromptDecision::Deny(_) => ApprovalDecision::Deny,
    };
    // 将 challenge 中的绑定材料随决定送回 Core，由控制面核验这次回答对应哪一项待批请求。
    client
        .approval_decision_with_proof(
            metadata,
            challenge.approval_id,
            decision,
            Some(challenge.request_hash),
            Some(challenge.nonce),
        )
        .await
        .map_err(anyhow::Error::msg)
}

/// 根据模型相关入口选项创建本地 DaemonHost，供 CLI/SDK 的独立请求包装使用。
///
/// Host 的其他 Core、Runner、Broker 和 EventStore 依赖仍由 DaemonHost 内部统一装配。
pub(crate) fn new_local_host_with_options(
    options: &HashMap<String, Value>,
) -> Result<Arc<DaemonHost>> {
    // JSON 选项只在值确实是字符串时覆盖默认配置；借用的 &str 在配置拥有值之前复制成 String。
    let string_option = |key| options.get(key).and_then(Value::as_str).map(str::to_owned);
    // Host 是本地组合根。返回 Arc 是为了让 transport 持有共享所有权，并在异步 send 期间保持 Host 存活。
    Ok(Arc::new(
        DaemonHost::local_with_model_config(LocalModelConfig {
            provider: string_option("provider"),
            api_key: string_option("api_key"),
            base_url: string_option("base_url"),
            model: string_option("model"),
        })
        .map_err(anyhow::Error::msg)?,
    ))
}

/// 造一个走默认本地 daemon 的 client。
fn local_client(
    session_id: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<(KianaClient<LocalDaemonTransport>, RequestMetadata)> {
    client_on_host(new_local_host_with_options(options)?, session_id, options)
}

/// 在指定的 host 上造 client，并把身份元数据一起造好。
///
/// 【为什么 metadata 和 client 要放在一起造】
/// 因为它们必须来自同一份配置。把它们拆开，调用方就可能给 host 配 A、
/// 给 metadata 填 B，于是「这台机器上」和「我是谁」对不上号。
///
/// 【⚠ 与 `command_dispatch.rs` 的对照】
/// 那个文件里 `session_id` 与 `actor_id` 缺失时会回落到常量
/// （`local-command` / `local-user`），并且 `permission_profile` 被写死为 `Safe`。
/// 本文件的对应逻辑在这里，语义应当保持一致；修改其中一处时记得另一处。
pub fn client_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<(KianaClient<LocalDaemonTransport>, RequestMetadata)> {
    // 先解析项目目录并读取其信任状态；这些值成为请求上下文，Core 仍负责后续权威检查。
    let project_root = project_root_from_options(options)?;
    let trusted = project_trusted(&project_root)?;
    // 一个解析器同时决定 sandbox 和 permission profile，避免两者从不同来源组合。
    let policy = sandbox_policy_from_options(options)?;
    // local() 建立本地请求的默认元数据，再补齐入口必须显式提供的 actor 和项目上下文。
    let mut metadata = RequestMetadata::local(session_id.into(), project_root);
    metadata.actor_id = Some("local-user".to_owned());
    metadata.project_trusted = trusted;
    metadata.permission_profile = policy.permission_profile;
    if let Some(role_id) = string_option(options, "role") {
        // 只接受注册表认识的 role，避免未知字符串进入后续权限与部门映射。
        let role = RoleSpec::lookup(&role_id).ok_or_else(|| anyhow!("role_unknown"))?;
        metadata.assign_role(&role);
    }
    // 返回配对的 transport 和 metadata，调用方不能不经此函数另拼一套调用者身份。
    Ok((KianaClient::new(LocalDaemonTransport { host }), metadata))
}

/// 从当前 daemon 的运行事件中恢复 continue 等操作所需的项目、角色和沙箱选项。
///
/// 该函数只重建请求上下文，不授予执行权限；调用这些选项的请求仍须由 ControlPlane
/// 检查所有权、当前信任状态和执行授权。找不到对应授权事件时保留调用方原选项。
pub(crate) async fn session_options_on_host(
    host: &DaemonHost,
    session_id: &str,
    requested_run: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<HashMap<String, Value>> {
    // 先读取 Host 暴露的事件视图；有些 Host 不支持此读取能力，此时不能假装恢复成功。
    let events = host
        .ui_events()
        .await
        .map_err(anyhow::Error::msg)?
        .ok_or_else(|| anyhow!("session_history_unsupported"))?;
    // 从 Host 返回的事件视图尾部向前筛选授权记录，并限定 session；有 requested_run 时再精确限定 run。
    let authorized = events.iter().rev().find(|event| {
        event.kind == "run.authorized"
            && event.data["session_id"].as_str() == Some(session_id)
            && requested_run.is_none_or(|run_id| event.data["run_id"] == run_id.to_string())
    });
    let Some(authorized) = authorized else {
        // 没有可作依据的历史记录时不合成历史身份，交回原选项让下游路径继续处理。
        return Ok(options.clone());
    };
    // 用规范化后的绝对路径比较项目边界，允许等价路径写法，同时拒绝切换到另一个项目。
    let project_root = project_root_from_options(options)?;
    let original_root = authorized.data["project_root"]
        .as_str()
        .ok_or_else(|| anyhow!("session_project_scope_missing"))?;
    if std::fs::canonicalize(&project_root).context("session_project_unavailable")?
        != std::fs::canonicalize(original_root).context("session_project_unavailable")?
    {
        return Err(anyhow!("session_project_scope_mismatch"));
    }
    // 角色和部门必须同时与授权事件一致，不能只靠调用方当前传入的 role 字符串恢复身份。
    let role = authorized.data["role_id"]
        .as_str()
        .and_then(RoleSpec::lookup)
        .ok_or_else(|| anyhow!("session_role_scope_missing"))?;
    if authorized.data["department_id"].as_str() != Some(role.department_id.as_str()) {
        return Err(anyhow!("session_department_scope_mismatch"));
    }
    // 沙箱值先通过已有解析器校验，再与角色/项目一起从历史事实覆盖入口选项。
    let sandbox = authorized.data["sandbox"]
        .as_str()
        .ok_or_else(|| anyhow!("session_sandbox_scope_missing"))?;
    sandbox_policy_from_name(sandbox)?;
    // 保留其它请求选项，但将会话绑定的三项上下文固定回最初授权事件中的值。
    let mut scoped = options.clone();
    scoped.insert("cwd".to_owned(), Value::String(original_root.to_owned()));
    scoped.insert("role".to_owned(), Value::String(role.role_id));
    scoped.insert("sandbox".to_owned(), Value::String(sandbox.to_owned()));
    Ok(scoped)
}

/// 从服务端审批视图中恢复该 challenge 对应的权限画像，并合并到入口选项副本。
fn approval_options_from_view(
    options: &HashMap<String, Value>,
    approval: &Value,
) -> Result<HashMap<String, Value>> {
    // 先保留调用方其它选项；只有审批视图确实包含 permission profile 时才覆盖权限档位。
    let mut scoped = options.clone();
    if !approval["permission_profile"].is_null() {
        // approval view 经 JSON 携带，先恢复强类型；形状错误带上字段专属上下文返回。
        let profile: PermissionProfile =
            serde_json::from_value(approval["permission_profile"].clone())
                .context("approval_permission_profile_invalid")?;
        // sandbox 在总解析器中优先于 permission_profile；移除旧 sandbox 才能让审批视图中的 profile 生效。
        scoped.remove("sandbox");
        scoped.insert(
            "permission_profile".to_owned(),
            serde_json::to_value(profile)?,
        );
        // 复用统一解析器做完整性检查，避免返回无法被后续 run 使用的选项组合。
        sandbox_policy_from_options(&scoped)?;
    }
    Ok(scoped)
}

/// 为独立 CLI 调用创建 Host，再转入同一个 Host 续跑实现。
pub async fn continue_envelope(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    // CLI 等独立入口先装配本地 Host，再复用与工作台相同的 Host 续跑实现。
    continue_envelope_on_host(
        new_local_host_with_options(options)?,
        session_id,
        prompt,
        run_id,
        options,
    )
    .await
}

/// 查询 Host 当前可见的待审批项，不在此处继续 run 或执行能力。
///
/// 若会话已有授权运行，会先从其事件恢复绑定的项目/角色/沙箱参数，再把查询交给 Core。
pub async fn pending_approvals_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    // 泛型 Into 在入口处消费一次，之后以同一 session 查询历史并构造请求身份。
    let session_id = session_id.into();
    let options = session_options_on_host(&host, &session_id, run_id, options).await?;
    let (client, metadata) = client_on_host(host, session_id, &options)?;
    // 列表由 daemon/Core 按其当前事实返回；本函数只传递查询，不自行筛选或决定审批。
    client
        .pending_approvals(metadata, run_id)
        .await
        .map_err(anyhow::Error::msg)
}

/// 请求 daemon/Core 恢复一个已有 run。
///
/// 本入口只重建该 session 的项目上下文并发送恢复命令；是否可恢复由服务端状态和权限检查决定。
pub async fn resume_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let session_id = session_id.into();
    // 从已授权 run 恢复固定上下文，避免恢复请求通过新的 cwd/role/sandbox 改写原运行边界。
    let options = session_options_on_host(&host, &session_id, run_id, options).await?;
    let (client, metadata) = client_on_host(host, session_id, &options)?;
    // 发送的是恢复命令而非重新发起模型请求；Core 根据 run_id 查其可恢复状态。
    client
        .resume_run(metadata, run_id)
        .await
        .map_err(anyhow::Error::msg)
}

/// 把一次审批决定送回控制面。
///
/// 【为什么批准必须回到这里，而不能由界面自己记账】
/// 因为「已批准」是一条**事实**，它必须和其余事实写进同一本账（EventLog），
/// 并且受同一套 epoch / fence / 一次性消费的约束。界面若自己认为「批过了」，
/// 就会出现一次执行有批准、事件流里却没有批准记录——而事后无法判断哪个是真的。
pub async fn decide_approval_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    challenge: ApprovalChallenge,
    decision: ApprovalDecision,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let session_id = session_id.into();
    // 从当前 Host 的授权事件恢复同一 session 的请求上下文，之后的审批查询也走相同 Host。
    let options = session_options_on_host(&host, &session_id, None, options).await?;
    // 先从服务端读取待审批投影；不能用浏览器提交的 profile 直接构造权限上下文。
    let listed =
        pending_approvals_envelope_on_host(host.clone(), &session_id, None, &options).await?;
    if listed.status != ExecutionStatus::Completed {
        // 未能成功读取列表时保留服务端的拒绝/错误状态，不继续提交审批决定。
        return Ok(listed);
    }
    // 在服务端返回的待批集合中按 challenge 的 approval_id 精确匹配，取回该项绑定的 profile。
    let options = match listed.output["approvals"].as_array().and_then(|approvals| {
        approvals.iter().find(|approval| {
            approval["challenge"]["approval_id"] == challenge.approval_id.to_string()
        })
    }) {
        // 若找到条目，用其中的 profile 调整选项；否则不伪造审批上下文，交由 Core 校验决定。
        Some(approval) => approval_options_from_view(&options, approval)?,
        None => options,
    };
    // 重新按最终选项生成调用身份，并连同 challenge 的 hash/nonce 交回服务端核验。
    let (client, metadata) = client_on_host(host, session_id, &options)?;
    client
        .approval_decision_with_proof(
            metadata,
            challenge.approval_id,
            decision,
            Some(challenge.request_hash),
            Some(challenge.nonce),
        )
        .await
        .map_err(anyhow::Error::msg)
}

/// 在既有 Host 上向 ControlPlane 请求继续运行一个 session/run。
///
/// 入口负责恢复原授权上下文和传递 prompt；是否存在可继续的运行、如何恢复状态由服务端判定。
pub async fn continue_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let session_id = session_id.into();
    // 续跑沿用授权运行绑定的项目、角色和沙箱；新 prompt 不会重写这些上下文。
    let options = session_options_on_host(&host, &session_id, run_id, options).await?;
    let policy = sandbox_policy_from_options(&options)?;
    let (client, metadata) = client_on_host(host, session_id, &options)?;
    // 请求回到 Core 的 continue 入口，由服务端决定 run 是否仍可续跑以及是否需要新的审批。
    client
        .continue_run(metadata, prompt.into(), policy.sandbox, run_id)
        .await
        .map_err(anyhow::Error::msg)
}

/// 为独立入口装配本地 Host，并请求取消指定 session/run。
///
/// 此包装函数不等待或自行模拟任务终止；取消状态由 daemon/Core 的运行生命周期处理。
pub async fn cancel_envelope(
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    reason: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    cancel_envelope_on_host(
        new_local_host_with_options(options)?,
        session_id,
        run_id,
        reason,
        options,
    )
    .await
}

/// 在现有 Host 上提交取消请求，复用该 session 原先授权的项目与角色上下文。
///
/// `run_id` 和原因文本按调用方给出的值交给协议客户端；服务端负责校验目标和记录状态变化。
pub async fn cancel_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    reason: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let session_id = session_id.into();
    // 若事件中存在该 session 的授权运行，恢复其绑定上下文，防止取消请求改用另一项目身份。
    let options = session_options_on_host(&host, &session_id, run_id, options).await?;
    let (client, metadata) = client_on_host(host, session_id, &options)?;
    // 将原因转为请求拥有的 String；实际取消/竞态处理发生在 daemon/Core，而不是入口适配器。
    client
        .cancel_run(metadata, run_id, reason.into())
        .await
        .map_err(anyhow::Error::msg)
}

/// 为独立入口装配本地 Host，再查询指定运行的 Receipt 投影。
pub async fn receipt_envelope(
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    receipt_envelope_on_host(
        new_local_host_with_options(options)?,
        session_id,
        run_id,
        options,
    )
    .await
}

/// 在现有 Host 上查询 Receipt，并保留服务端按请求身份实施的可见性检查。
///
/// 这是只读查询入口；本地包装层不从 transcript 推断运行结果，也不构造新的收据事实。
pub async fn receipt_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let session_id = session_id.into();
    // 使用运行授权时记录的项目/角色上下文，使 Receipt 查询针对同一身份范围。
    let options = session_options_on_host(&host, &session_id, run_id, options).await?;
    let (client, metadata) = client_on_host(host, session_id, &options)?;
    client
        .receipt(metadata, run_id)
        .await
        .map_err(anyhow::Error::msg)
}

/// 通过相同协议/daemon 主路径读取按调用者范围过滤的入口一致性投影。
pub async fn parity_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    entrypoint: EntryPointKind,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let session_id = session_id.into();
    // 如果目标运行属于已有授权会话，沿用其中的项目、角色和沙箱上下文。
    let options = session_options_on_host(&host, &session_id, run_id, options).await?;
    let (client, metadata) = client_on_host(host, session_id, &options)?;
    // entrypoint 用于选择要比较的入口面；投影与所有权判断仍由服务端执行。
    client
        .parity(metadata, entrypoint, run_id)
        .await
        .map_err(anyhow::Error::msg)
}

/// CLI 便捷包装：为本地调用装配 Host，再复用 Host 版本的 parity 查询。
pub async fn parity_envelope(
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    entrypoint: EntryPointKind,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    parity_envelope_on_host(
        new_local_host_with_options(options)?,
        session_id,
        run_id,
        entrypoint,
        options,
    )
    .await
}

/// 通过 daemon/ControlPlane 查询指定项目的治理链投影。
pub async fn company_governance_envelope_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    project_id: impl Into<String> + Send,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let session_id = session_id.into();
    // 复用 session 绑定的项目上下文；project_id 是查询目标，不能替代调用者身份。
    let options = session_options_on_host(&host, &session_id, None, options).await?;
    let (client, metadata) = client_on_host(host, session_id, &options)?;
    client
        .company_governance(metadata, project_id)
        .await
        .map_err(anyhow::Error::msg)
}

/// 从项目内的 WorkPacket 发起 Builder 工作请求。
///
/// 入口可接受省略的 role 选项，但最终总会把请求身份设为 Builder；packet 读取和验证完成后
/// 才把结构化任务交给 daemon/Core，入口本身不启动第二个模型循环。
pub async fn spawn_envelope(
    session_id: impl Into<String>,
    packet_path: impl AsRef<str>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    if let Some(role_id) = string_option(options, "role") {
        // 若调用方显式指定角色，必须与 packet 消费者 Builder 一致；未知值也会在此被拒绝。
        let builder = RoleSpec::lookup(&role_id).is_some_and(|role| role.role_id == ROLE_BUILDER);
        if !builder {
            return Err(anyhow!("packet_role_must_be_builder"));
        }
    }
    // 先解析执行档位，再创建 transport 和身份，确保请求元数据使用同一组入口选项。
    let policy = sandbox_policy_from_options(options)?;
    let (client, mut metadata) = local_client(session_id, options)?;
    // 即使 CLI 未显式传 role，也把 spawn 的固定职责写入请求身份。
    metadata.assign_role(&RoleSpec::builder());
    // WorkPacket 是结构化任务边界；读取 helper 会限制路径形式、反序列化并执行领域校验。
    let packet = load_work_packet(&metadata.project_root, packet_path.as_ref())?;
    // 只发送 spawn 命令；后续派工/执行及许可由 daemon/Core 路径决定。
    client
        .spawn(metadata, packet, policy.sandbox)
        .await
        .map_err(anyhow::Error::msg)
}

/// 以 PM 身份提交一次 Symposium（结构化规划会）请求。
///
/// `anti_meeting` 和固定最大轮数作为协议字段一并发送，用于约束服务端规划流程。
pub async fn symposium_envelope(
    session_id: impl Into<String>,
    goal: impl Into<String>,
    anti_meeting: bool,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    if let Some(role_id) = string_option(options, "role") {
        // 可选的 CLI role 若出现，只能指定 PM；省略时下方仍强制分配 PM 身份。
        let pm = RoleSpec::lookup(&role_id).is_some_and(|role| role.role_id == ROLE_PM);
        if !pm {
            return Err(anyhow!("symposium_chair_must_be_pm"));
        }
    }
    let policy = sandbox_policy_from_options(options)?;
    let (client, mut metadata) = local_client(session_id, options)?;
    // chair 身份由入口固定，避免调用者用普通 Builder 身份发起规划会命令。
    metadata.assign_role(&RoleSpec::pm());
    client
        .convene(
            metadata,
            goal.into(),
            anti_meeting,
            // 使用共享的有限轮数常量，避免入口创建无上限的讨论请求。
            Symposium::DEFAULT_MAX_ROUNDS,
            policy.sandbox,
        )
        .await
        .map_err(anyhow::Error::msg)
}

/// 以 Reviewer 身份请求评审另一会话的工作结果。
pub async fn review_envelope(
    session_id: impl Into<String>,
    author_session_id: impl AsRef<str>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    if let Some(role_id) = string_option(options, "role") {
        // 显式传入的 role 只用于校验；真正送入协议的身份会在下方统一设为 Reviewer。
        let reviewer = RoleSpec::lookup(&role_id).is_some_and(|role| role.role_id == ROLE_REVIEWER);
        if !reviewer {
            return Err(anyhow!("review_role_must_be_reviewer"));
        }
    }
    let (client, mut metadata) = local_client(session_id, options)?;
    metadata.assign_role(&RoleSpec::reviewer());
    // author session 转成拥有值以进入异步请求；可选的额外 review 参数当前固定为 None。
    client
        .review(metadata, author_session_id.as_ref().to_owned(), None)
        .await
        .map_err(anyhow::Error::msg)
}

/// 以 Closer 身份提交作者会话/运行的关闭请求。
pub async fn close_envelope(
    session_id: impl Into<String>,
    author_session_id: impl AsRef<str>,
    author_run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let (client, mut metadata) = local_client(session_id, options)?;
    // 入口固定关闭职责，避免把调用方自行填写的 role 当成关闭授权。
    metadata.assign_role(&RoleSpec::closer());
    client
        .close(
            metadata,
            author_session_id.as_ref().to_owned(),
            author_run_id,
        )
        .await
        .map_err(anyhow::Error::msg)
}

/// 从项目目录下读取并验证一个 WorkPacket 文件。
///
/// 该函数在本地同步读取文本：路径 helper 做词法规范化和绝对/非普通路径分量拒绝，
/// 它本身不负责执行期 sandbox，也不解析符号链接的最终目标。
fn load_work_packet(project_root: &str, packet_path: &str) -> Result<WorkPacket> {
    // 先规范化用户给出的相对路径；拒绝项无法 join 到项目根目录。
    let relative = normalize_role_path(packet_path).ok_or_else(|| anyhow!("packet_path_denied"))?;
    let path = Path::new(project_root).join(relative);
    // 文件错误统一映射为 packet_not_found，因此此处不会向调用方区分不存在、权限或底层 I/O 原因。
    let raw = std::fs::read_to_string(&path).map_err(|_| anyhow!("packet_not_found"))?;
    // JSON 语法错误和 WorkPacket 结构校验错误分开处理，方便定位是文件格式还是领域约束失败。
    let packet: WorkPacket = serde_json::from_str(&raw).map_err(|_| anyhow!("packet_invalid"))?;
    packet.validate().map_err(anyhow::Error::msg)?;
    Ok(packet)
}

/// 将协议响应压缩为 CLI/SDK 直接消费的成功结果。
///
/// 只接受 `Completed`；其它状态优先返回服务端的非空错误文本，没有错误文本时使用稳定兜底码。
pub fn completed_harness_result(response: ResponseEnvelope) -> Result<HarnessRunResult> {
    if response.status != ExecutionStatus::Completed {
        // as_deref 借用响应中的错误字符串；过滤空串后保留真实错误，否则返回通用 harness 失败码。
        // H05 分类只追加稳定失败码/恢复 disposition，不改变原有成功/失败判定与退出行为。
        return Err(anyhow!(
            "{}{}",
            response
                .error
                .as_deref()
                .filter(|error| !error.is_empty())
                .unwrap_or("kiana_harness_failed"),
            crate::model_failure_presenter::failure_suffix(&response)
        ));
    }
    // output 是兼容投影，字段可能缺失或类型不符；文本因此回退为空串，steps 在下方回退为 1。
    let text = response.output["output"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Ok(HarnessRunResult {
        text,
        steps: response.output["output"]["steps"].as_u64().unwrap_or(1),
        // 旧响应没有顶层 sandbox 时按 read-only 展示；原始结构化 output 仍完整保留在结果里。
        sandbox: response
            .output
            .get("sandbox")
            .and_then(Value::as_str)
            .unwrap_or("read-only")
            .to_owned(),
        output: response.output,
    })
}

/// 根据 CLI/SDK 选项确定本次请求的项目根目录。
///
/// 非空字符串 `cwd` 优先；未提供时读取进程当前目录。当前目录不可用会返回带上下文的错误。
pub fn project_root_from_options(options: &HashMap<String, Value>) -> Result<String> {
    if let Some(cwd) = string_option(options, "cwd") {
        // 显式 cwd 已由 string_option 去首尾空白；这里保留它的路径文本，不做 canonicalize。
        return Ok(cwd);
    }
    // current_dir 是操作系统 I/O，可能因进程目录不可访问而失败；lossy 转换用于产出 Rust String。
    std::env::current_dir()
        .context("failed to resolve current directory")
        .map(|path| path.to_string_lossy().into_owned())
}

/// 读取项目级信任记录；没有记录时按未信任处理，读取异常则显式报错。
///
/// 该布尔值会放入入口请求上下文供 Core 继续检查，不代表这个 helper 自己授予工具权限。
pub fn project_trusted(project_root: &str) -> Result<bool> {
    match kiana_types::read_project_trust(project_root) {
        // 文件中存在明确的 trust 值时转成布尔状态。
        Ok(Some(trust)) => Ok(trust.as_bool()),
        // 缺少 trust 记录必须 fail closed，不能把“没有数据”解释成已信任。
        Ok(None) => Ok(false),
        // 区分“明确未信任”和“无法读取信任状态”，让上层可以显示真实失败原因。
        Err(error) => Err(anyhow!("project_trust_unavailable:{error}")),
    }
}

#[derive(Debug, Clone)]
/// 入口选项解析后的成对权限提示：sandbox 字符串和传给请求元数据的 permission profile。
///
/// `sandbox` 为 `None` 时不向 client 传显式 sandbox 名；它仍会带有 `permission_profile`。
pub struct SandboxPolicy {
    /// 可选的运行档位名称；Workspace-write 等显式档位会通过该字段传给协议客户端。
    pub sandbox: Option<String>,
    /// 与该档位配套的请求权限画像，由 Core 用于后续策略计算。
    pub permission_profile: PermissionProfile,
}

/// 按优先级把松散入口参数折叠成一个 `SandboxPolicy`。
///
/// 顺序为显式 `sandbox`、选项/环境变量 `permission_profile`、选项/环境变量 `permission_mode`，
/// 最后默认 read-only；优先级更高但值不支持时直接报错，不静默降级到较低来源。
pub fn sandbox_policy_from_options(options: &HashMap<String, Value>) -> Result<SandboxPolicy> {
    // 显式 sandbox 最高优先级；解析失败会直接返回，不继续尝试 profile 或 mode。
    if let Some(sandbox) = string_option(options, "sandbox") {
        return sandbox_policy_from_name(&sandbox);
    }
    // option 覆盖环境变量；空值视为未提供，因而可以继续读取环境变量。
    if let Some(profile) = string_option(options, "permission_profile")
        .or_else(|| env_nonempty("KIANA_PERMISSION_PROFILE"))
    {
        return sandbox_policy_from_permission_profile(&profile);
    }
    // mode 只在没有 profile 来源时使用，避免同时合并两种不同层级的权限表达。
    if let Some(mode) =
        string_option(options, "permission_mode").or_else(|| env_nonempty("KIANA_PERMISSION_MODE"))
    {
        return sandbox_policy_from_permission_mode(&mode);
    }
    // 没有任何来源时选择最保守的默认映射。
    Ok(read_only_policy())
}

/// 把兼容格式的会话消息切成单个 prompt，供需要字符串输入的 CLI/SDK 适配路径使用。
///
/// 在 SDK 主路径中，调用方把最后一条消息作为 prompt，之前的消息另转成强类型 history；
/// 本函数也支持多条消息的文本展平。非文本 block 会被忽略，schema 作为文本说明追加在末尾。
pub fn prompt_from_session_messages(messages: &[Value], json_schema: Option<&Value>) -> String {
    // 单条消息直接取正文，避免无意义地加上 "user:"；零条消息则得到空 transcript。
    let transcript = if messages.len() <= 1 {
        messages.first().map(message_text).unwrap_or_default()
    } else {
        // 多条消息展平为 role: text 行；缺失 role 默认为 user，空内容不占用 prompt 空间。
        messages
            .iter()
            .filter_map(|message| {
                let role = message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user");
                let text = message_text(message);
                if text.is_empty() {
                    None
                } else {
                    Some(format!("{role}: {text}"))
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    // schema 被直接拼入文本提示；这里只构造说明，不负责验证模型最终返回内容。
    match json_schema {
        Some(schema) => {
            format!("{transcript}\n\nRespond with JSON matching this schema:\n{schema}")
        }
        None => transcript,
    }
}

/// 从兼容消息 JSON 的 `content` 字段提取文本，不解析图像等非文本 block。
///
/// 支持字符串、文本块数组和单个 `{ type: "text", text: ... }` 对象；其它形状返回空串。
/// 若 `content` 本身是字符串，即使消息 role 是 tool，也会照常返回该字符串。
pub fn message_text(message: &Value) -> String {
    match message.get("content") {
        // 最常见聊天消息的简化表示。
        Some(Value::String(text)) => text.clone(),
        // 多模态协议把内容拆成 block；这里只收字符串和显式 text block，不把其它块误转成文字。
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                if let Some(text) = block.as_str() {
                    return Some(text.to_string());
                }
                if block.get("type").and_then(Value::as_str) == Some("text") {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        // 某些协议把单个内容块直接放在 content 对象中。
        Some(Value::Object(block)) if block.get("type").and_then(Value::as_str) == Some("text") => {
            block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        }
        _ => String::new(),
    }
}

/// 把 sandbox 名称及其兼容别名映射为策略；完整主机访问档位始终拒绝。
fn sandbox_policy_from_name(value: &str) -> Result<SandboxPolicy> {
    // 先统一大小写、首尾空格和下划线写法，再匹配显式支持的名称集合。
    match normalize_token(value).as_str() {
        // 只读别名落到 Safe profile 且不传显式 sandbox 名。
        "read-only" | "readonly" | "read_only" => Ok(read_only_policy()),
        // 受限写入别名统一落到 workspace-write + Balanced。
        "workspace-write" | "workspacewrite" | "workspace_write" | "workspace" => {
            Ok(workspace_write_policy())
        }
        // 这些名称表示绕过受控执行面，入口在构造请求前就拒绝。
        "danger-full-access" | "dangerfullaccess" | "dangerously-skip-permissions" | "full" => {
            Err(anyhow!("danger_full_access_rejected"))
        }
        other => Err(anyhow!("sandbox_unsupported:{other}")),
    }
}

/// 将 PermissionProfile 的 CLI/环境变量别名映射为当前支持的两档策略。
fn sandbox_policy_from_permission_profile(value: &str) -> Result<SandboxPolicy> {
    match normalize_token(value).as_str() {
        // plan/safe 等 profile 映射到 Safe；Ask 保留受限写 sandbox，以便 ControlPlane 生成审批挑战。
        "read-only" | "readonly" | "read_only" | "plan" | "safe" => Ok(read_only_policy()),
        "ask" => Ok(ask_policy()),
        // default/balanced/workspace 别名映射到受限 workspace-write 档。
        "workspace" | "workspace-write" | "default" | "balanced" => Ok(workspace_write_policy()),
        // 不把 full/autonomous 静默降为受限模式，明确报告请求被拒。
        "full" | "autonomous" | "danger-full-access" => Err(anyhow!("danger_full_access_rejected")),
        other => Err(anyhow!("sandbox_unsupported:{other}")),
    }
}

/// 将权限 mode 别名映射为 Safe 或 Balanced；绕过权限的模式明确拒绝。
fn sandbox_policy_from_permission_mode(value: &str) -> Result<SandboxPolicy> {
    match normalize_token(value).as_str() {
        // 保守模式映射到 Safe；兼容 dont-ask 的常见分隔符写法。
        // Ask keeps the bounded workspace-write scope so Core can stage the capability for a
        // human approval; approval, rather than this mapper, remains the write gate.
        "default" | "plan" | "dontask" | "dont-ask" | "dont_ask" => Ok(read_only_policy()),
        // Ask 保留受限 workspace-write 边界，但把档位标成 Ask，让 ControlPlane 对每个 LocalWrite
        // 生成审批挑战；它不会被 Balanced 的自动放行规则误判为已授权。
        "ask" => Ok(ask_policy()),
        // accept-edits 只获得 workspace-write 档，不映射到 unrestricted access。
        "acceptedits" | "accept-edits" | "accept_edits" => Ok(workspace_write_policy()),
        // bypass/auto 等要求跳过权限判断的模式明确拒绝。
        "bypasspermissions" | "bypass-permissions" | "bypass_permissions" | "auto"
        | "danger-full-access" => Err(anyhow!("danger_full_access_rejected")),
        other => Err(anyhow!("sandbox_unsupported:{other}")),
    }
}

/// 只读入口映射：不设置显式 sandbox 名，同时将权限画像设为 Safe。
fn read_only_policy() -> SandboxPolicy {
    SandboxPolicy {
        sandbox: None,
        permission_profile: PermissionProfile::Safe,
    }
}

/// workspace-write 映射：显式设置 sandbox，并搭配 Balanced 权限画像。
fn workspace_write_policy() -> SandboxPolicy {
    SandboxPolicy {
        sandbox: Some("workspace-write".to_owned()),
        permission_profile: PermissionProfile::Balanced,
    }
}

fn ask_policy() -> SandboxPolicy {
    SandboxPolicy {
        sandbox: Some("workspace-write".to_owned()),
        permission_profile: PermissionProfile::Ask,
    }
}

/// 从 JSON 选项读取非空字符串：其它 JSON 类型和空白字符串都视为未提供。
fn string_option(options: &HashMap<String, Value>, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 读取并去空白的环境变量；未设置、非 Unicode 或最终为空时都返回 None。
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 统一权限别名的首尾空格、下划线和 ASCII 大小写，供显式 alias match 使用。
fn normalize_token(value: &str) -> String {
    value.trim().replace('_', "-").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 这些策略 helper 会读取进程级环境变量；ScopedEnv 在测试期间持锁并于 Drop 时恢复原值。
    fn isolated_policy_env() -> crate::test_support::ScopedEnv<'static> {
        let env =
            crate::test_support::scoped_env(&["KIANA_PERMISSION_PROFILE", "KIANA_PERMISSION_MODE"]);
        // 清空环境来源，让默认值断言不受开发机或 CI 配置影响；返回 guard 使隔离覆盖整个测试作用域。
        std::env::remove_var("KIANA_PERMISSION_PROFILE");
        std::env::remove_var("KIANA_PERMISSION_MODE");
        env
    }

    #[test]
    // 没有任何显式选项或环境变量时，默认策略同时不指定 sandbox 并使用 Safe profile。
    fn default_policy_is_read_only() {
        let _env = isolated_policy_env();
        let policy = sandbox_policy_from_options(&HashMap::new()).unwrap();
        assert_eq!(policy.sandbox, None);
        assert_eq!(policy.permission_profile, PermissionProfile::Safe);
    }

    #[test]
    // 显式 sandbox 优先级高于 permission profile/mode，并映射成一对 workspace-write + Balanced。
    fn workspace_write_requires_balanced_profile() {
        let options = HashMap::from([(
            "sandbox".to_string(),
            Value::String("workspace-write".to_string()),
        )]);
        let policy = sandbox_policy_from_options(&options).unwrap();
        assert_eq!(policy.sandbox.as_deref(), Some("workspace-write"));
        assert_eq!(policy.permission_profile, PermissionProfile::Balanced);
    }

    #[test]
    fn ask_permission_mode_keeps_workspace_scope_and_requires_approval_profile() {
        let options = HashMap::from([(
            "permission_mode".to_string(),
            Value::String("ask".to_string()),
        )]);
        let policy = sandbox_policy_from_options(&options).unwrap();
        assert_eq!(policy.sandbox.as_deref(), Some("workspace-write"));
        assert_eq!(policy.permission_profile, PermissionProfile::Ask);
    }

    #[test]
    fn ask_permission_profile_keeps_workspace_scope_and_requires_approval_profile() {
        let options = HashMap::from([(
            "permission_profile".to_string(),
            Value::String("ask".to_string()),
        )]);
        let policy = sandbox_policy_from_options(&options).unwrap();
        assert_eq!(policy.sandbox.as_deref(), Some("workspace-write"));
        assert_eq!(policy.permission_profile, PermissionProfile::Ask);
    }

    #[test]
    // 入口拒绝完整主机访问档位，避免该值被传入 daemon 请求。
    fn danger_full_access_is_rejected() {
        let options = HashMap::from([(
            "sandbox".to_string(),
            Value::String("danger-full-access".to_string()),
        )]);
        let error = sandbox_policy_from_options(&options)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "danger_full_access_rejected");
    }

    #[test]
    // 在未设置更高优先级 profile 的环境中，bypass mode 应拒绝，而不是降级为 Safe/Workspace。
    fn bypass_permissions_mode_is_rejected() {
        let options = HashMap::from([(
            "permission_mode".to_string(),
            Value::String("bypassPermissions".to_string()),
        )]);
        let error = sandbox_policy_from_options(&options)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "danger_full_access_rejected");
    }

    #[test]
    // 多条历史按 role 展平，数组 text block 被提取，schema 作为末尾的文本约束保留。
    fn transcript_includes_history_and_schema() {
        let prompt = prompt_from_session_messages(
            &[
                json!({"role":"assistant","content":[{"type":"text","text":"previous"}]}),
                json!({"role":"user","content":"follow up"}),
            ],
            Some(&json!({"type":"object"})),
        );
        assert!(prompt.contains("assistant: previous"));
        assert!(prompt.contains("user: follow up"));
        assert!(prompt.contains(r#""type": "object""#) || prompt.contains(r#""type":"object""#));
    }

    #[tokio::test]
    async fn run_envelope_on_host_preserves_event_identity_chain() {
        let env = crate::test_support::scoped_env(&[
            "KIANA_HOME",
            "HOME",
            "KIANA_HARNESS_SCRIPT",
            "KIANA_PROVIDER",
            "KIANA_FAKE_PROVIDER_SCRIPT",
            "KIANA_MODEL_MODE",
            "KIANA_STREAMING",
            "ANTHROPIC_API_KEY",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
        ]);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-entrypoint-er02-{stamp}"));
        let home = std::env::temp_dir().join(format!("kiana-entrypoint-er02-home-{stamp}"));
        let script = root.join("script.json");
        std::fs::create_dir_all(root.join(".git")).expect("project root");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(&script, r#"[{"text":"entrypoint identity"}]"#).expect("cassette");
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var("KIANA_HARNESS_SCRIPT", &script);
        env.set_var("KIANA_PROVIDER", "");
        env.set_var("KIANA_FAKE_PROVIDER_SCRIPT", "");
        env.set_var("KIANA_MODEL_MODE", "");
        env.set_var("KIANA_STREAMING", "off");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_OPENAI_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        kiana_types::write_project_trust(&root, kiana_types::ProjectTrust::Trusted)
            .expect("project trust");

        let options = HashMap::from([
            (
                "cwd".to_owned(),
                Value::String(root.to_string_lossy().into_owned()),
            ),
            ("sandbox".to_owned(), Value::String("read-only".to_owned())),
        ]);
        let host = new_local_host_with_options(&options).expect("local host");
        let response = run_envelope_on_host(
            Arc::clone(&host),
            "er02-entrypoint-session",
            "identity",
            &options,
        )
        .await
        .expect("entrypoint response");
        assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");

        let events = host
            .ui_events()
            .await
            .expect("event view")
            .expect("memory event view");
        let run_events: Vec<_> = events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind.as_str(),
                    "run.authorized" | "run.prompt" | "run.completed" | "run.receipt"
                )
            })
            .collect();
        assert!(run_events.len() >= 4, "{run_events:?}");
        let run_id = run_events[0].data["run_id"].as_str().expect("run id");
        for event in run_events {
            assert_eq!(event.data["run_id"].as_str(), Some(run_id));
            assert_eq!(event.request_id, response.request_id);
            assert_eq!(event.correlation_id, Some(response.request_id));
        }
        assert_eq!(response.output["run_id"].as_str(), Some(run_id));

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(home);
    }
}
