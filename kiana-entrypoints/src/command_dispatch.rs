//! 入口命令的**唯一出口**：把界面/CLI 里的一个命令，翻译成 versioned 协议请求，
//! 交给 `DaemonHost`，再把响应翻译回命令结果。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! CLI / Web / TUI / SDK / MCP  （各自解析参数、渲染界面）
//!        ↓  Command + CommandContext
//! 【本文件】构造 RequestMetadata → RequestEnvelope
//!        ↓  KianaClient（本地进程内 transport）
//! kiana-daemon::DaemonHost
//!        ↓  ControlPlane：策略 → 关卡 → 审批 → 执行
//! ResponseEnvelope
//!        ↓  【本文件】分类响应
//! CommandDispatchOutcome::{Completed, AwaitingApproval}
//! ```
//!
//! **上游**：任何界面模块。**下游**：`DaemonHost`；本文件不直接触碰模型、文件系统或网络。
//!
//! # 为什么值得单独一个文件
//!
//! 因为「命令从界面到控制面要走哪条路」是初学者最需要先确定的一件事，而它必须**只有一条**。
//! 如果 CLI 自己拼一套请求、Web 再拼一套，两条路径迟早在某个细节上分叉——一处补了身份校验，
//! 另一处没补，就是一个只在某个界面里能触发的提权漏洞。这个文件的存在就是为了让
//! 「拼请求」这件事只发生在一个地方。
//!
//! # 「本地执行」是什么意思
//!
//! 这里的 transport 不开 socket。`LocalDaemonTransport` 直接把 envelope 交给同进程内的
//! `DaemonHost`。这是本机单用户形态的特权：**没有网络边界，所以也没有网络层可以替你兜底**，
//! 因此身份与信任字段必须在下面 `request_metadata` 里被认真对待。
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_commands::{Command, CommandContext, CommandResult, CommandRoute};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ApprovalChallenge, ApprovalDecision, ApprovalId, ExecutionStatus, PermissionProfile,
    RequestEnvelope, RequestMetadata, ResponseEnvelope, RiskLevel,
};
use serde_json::Value;
use std::sync::{Arc, OnceLock};

/// app_state 里那个「自动批准本地写」的开关键名。
///
/// 它的存在本身就是一个需要解释的决定，所以先说清楚**它不做什么**：
/// 它**不会**批准任何 `Critical` 风险的动作，也不批准 `LocalWrite` 以外的风险等级。
/// 唯一的用途是让本机交互式使用不必为每一次本地写操作点一次确认。
///
/// ⚠ 注意它的名字里有 `local`——这个开关是为本机形态准备的。把它带到一个
/// 多人或远程形态里，就等于给了每个人一个「自动批准写操作」的按钮。
pub const APPROVE_LOCAL_WRITE_APP_STATE_KEY: &str = "approve_local_write";

/// 进程内唯一的 `DaemonHost`。
///
/// 【为什么是单例】
/// 因为 `DaemonHost` 是**组合根**——它持有 EventLog、审批存储、run stream 这些有状态的东西。
/// 如果每个命令各自 new 一个，就会出现多套授权状态和多套事件流：同一个审批在 A 实例里有效、
/// 在 B 实例里查不到，事件也会被写进两个地方。所以整个进程只能有一个。
///
/// 【`OnceLock` 而不是 `LazyLock`】
/// 因为构造可能失败（`DaemonHost::local()` 会做真实装配），而 `OnceLock` 允许把失败留给
/// 第一次调用时显式处理，而不是在初始化路径上 panic。
static LOCAL_DAEMON: OnceLock<Arc<DaemonHost>> = OnceLock::new();

/// 把「发送请求」实现成**同进程直接调用**。
///
/// 【它省掉了什么】
/// 没有 socket、没有序列化往返、没有端口冲突。`RequestEnvelope` 原样交给 `DaemonHost.handle`。
///
/// 【它同时意味着什么】
/// 意味着**没有网络层可以替你兜底**。一个远程 transport 至少还有一层「请求来自网络」的
/// 事实，而这个 transport 的所有输入都来自本进程内——所以调用方给的任何身份字段都必须
/// 在 [ `request_metadata` ] 里被重新推导，而不能被信任。
struct LocalDaemonTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for LocalDaemonTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        Ok(self.host.handle(request).await)
    }
}

pub async fn execute_command(
    command: &dyn Command,
    context: CommandContext,
) -> anyhow::Result<CommandResult> {
    let approve_local_write = context
        .app_state
        .get(APPROVE_LOCAL_WRITE_APP_STATE_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let approval_context = context.clone();
    match dispatch_command(command, context).await? {
        CommandDispatchOutcome::Completed(result) => Ok(result),
        CommandDispatchOutcome::AwaitingApproval(challenge)
            if should_auto_approve_local_write(approve_local_write, &challenge) =>
        {
            resolve_command_approval_with_challenge(
                &approval_context,
                &challenge,
                ApprovalDecision::Approve,
            )
            .await
        }
        CommandDispatchOutcome::AwaitingApproval(challenge) => Err(anyhow!(
            "control_plane_command_awaiting_approval:{}",
            serde_json::to_string(&challenge)?
        )),
    }
}

/// 判断这次「待审批」是否可以就地自动批准。
///
/// 【三个条件同时成立才自动批准】
/// 1. 调用方**显式**打开了那个开关（不是默认值，必须是主动设置）；
/// 2. 风险等级恰好是 `LocalWrite`；
/// 3. ——隐含的第三件事——它只在 `dispatch_command` 处理 `AwaitingApproval` 时被调用，
///    也就是说控制面**已经决定需要审批**了，这里只是决定「要不要问」。
///
/// 【⚠ 为什么条件 2 不能放宽】
/// 这是整个文件里最容易被「顺手改一下」的地方。把 `risk == RiskLevel::LocalWrite`
/// 放宽成「任何非 Critical」，看起来只差一档，但 `Critical` 与 `LocalWrite` 之间的
/// 那一档往往正是「写工作区之外的东西」。放宽它等于让一个交互式便利选项变成了
/// 无人值守的提权开关。
///
/// 【⚠ 还有一层：这是个函数，不是策略】
/// 它不做任何策略判断，只把「调用方要不要自动批准」翻译成布尔值。真正的风险分级在
/// ControlPlane 那边已经做完了。这里再分一次，就等于有两个地方能决定一件事——
/// 而两处迟早不一致。
fn should_auto_approve_local_write(
    approve_local_write: bool,
    challenge: &ApprovalChallenge,
) -> bool {
    approve_local_write && challenge.risk == RiskLevel::LocalWrite
}

/// 一条命令跑完之后的两种结局。
///
/// 【为什么只有两种，而不是一个「结果」】
/// 因为「完成了」和「在等一个人点头」是**性质完全不同**的两件事，合并它们会让调用方
/// 不得不用一个布尔字段去表达「到底算不算做完」。分成两个变体之后，
/// 「没跑完」在类型上就是看得见的，调用方不可能忽略它。
#[derive(Clone, Debug)]
pub enum CommandDispatchOutcome {
    Completed(CommandResult),
    AwaitingApproval(ApprovalChallenge),
}

pub async fn dispatch_command(
    command: &dyn Command,
    context: CommandContext,
) -> anyhow::Result<CommandDispatchOutcome> {
    match command.route(&context)? {
        CommandRoute::Local => command
            .execute(context)
            .await
            .map(CommandDispatchOutcome::Completed),
        CommandRoute::ControlPlane { name, arguments } => {
            execute_control_plane_command(&context, name, arguments).await
        }
    }
}

async fn execute_control_plane_command(
    context: &CommandContext,
    name: String,
    arguments: Value,
) -> anyhow::Result<CommandDispatchOutcome> {
    let metadata = request_metadata(context)?;
    let client = KianaClient::new(LocalDaemonTransport {
        host: local_daemon()?,
    });
    let response = client
        .command(metadata, name, arguments)
        .await
        .map_err(anyhow::Error::msg)?;
    response_outcome(response)
}

pub async fn resolve_command_approval(
    context: &CommandContext,
    approval_id: ApprovalId,
    decision: ApprovalDecision,
) -> anyhow::Result<CommandResult> {
    let response = resolve_command_approval_response(context, approval_id, decision).await?;
    match response_outcome(response)? {
        CommandDispatchOutcome::Completed(result) => Ok(result),
        CommandDispatchOutcome::AwaitingApproval(_) => {
            Err(anyhow!("approval_decision_returned_new_challenge"))
        }
    }
}

pub async fn resolve_command_approval_response(
    context: &CommandContext,
    approval_id: ApprovalId,
    decision: ApprovalDecision,
) -> anyhow::Result<ResponseEnvelope> {
    let metadata = request_metadata(context)?;
    let client = KianaClient::new(LocalDaemonTransport {
        host: local_daemon()?,
    });
    client
        .approval_decision(metadata, approval_id, decision)
        .await
        .map_err(anyhow::Error::msg)
}

pub async fn resolve_command_approval_with_challenge(
    context: &CommandContext,
    challenge: &ApprovalChallenge,
    decision: ApprovalDecision,
) -> anyhow::Result<CommandResult> {
    let response = resolve_command_approval_response_with_proof(
        context,
        challenge.approval_id,
        decision,
        Some(challenge.request_hash.clone()),
        Some(challenge.nonce.clone()),
    )
    .await?;
    match response_outcome(response)? {
        CommandDispatchOutcome::Completed(result) => Ok(result),
        CommandDispatchOutcome::AwaitingApproval(_) => {
            Err(anyhow!("approval_decision_returned_new_challenge"))
        }
    }
}

pub async fn resolve_command_approval_response_with_proof(
    context: &CommandContext,
    approval_id: ApprovalId,
    decision: ApprovalDecision,
    request_hash: Option<String>,
    nonce: Option<String>,
) -> anyhow::Result<ResponseEnvelope> {
    let metadata = request_metadata(context)?;
    let client = KianaClient::new(LocalDaemonTransport {
        host: local_daemon()?,
    });
    client
        .approval_decision_with_proof(metadata, approval_id, decision, request_hash, nonce)
        .await
        .map_err(anyhow::Error::msg)
}

/// 从命令上下文里**重新推导**出服务端要用的身份与信任字段。
///
/// 【⚠ 这是整个入口层最需要认真读的一个函数】
/// `CommandContext.app_state` 里的东西是**界面给的**。这个函数把其中三项重新推导：
///
/// - `project_root`：从 `cwd` 取，**缺失即报错**（`control_plane_project_root_required`）。
///   这是唯一一个「没有默认值」的字段，因为没有工作区根目录就无法做任何 containment 判断——
///   而没有 containment，一切路径相关的授权都无从谈起。
/// - `session_id`：缺失时回落为常量 `"local-command"`。
/// - `actor_id`：缺失时回落为常量 `"local-user"`。
///
/// 【那两个默认值值得警惕】
/// 它们让「身份缺失」不至于变成一次报错，于是身份缺失的请求会以 `local-user` 的身份继续往下走。
/// 在本机单用户形态下这是合理的；在任何多人形态下，**这两个常量就是「所有人都叫同一个名字」**。
/// 如果要把 Kiana 变成多用户，第一个要拆的就是它们。
///
/// - `project_trusted` 不在这里被假设，而是从 app_state 读取真实判定；
/// - `permission_profile` 被**写死**为 `PermissionProfile::Safe`。
///   写死是有意的：入口层不负责提权，只负责把最保守的档位交上去，让控制面按需放宽。
///   反过来做——让界面自己声明权限档——就等于让被授权者给自己发权限。
fn request_metadata(context: &CommandContext) -> anyhow::Result<RequestMetadata> {
    let project_root = context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("control_plane_project_root_required"))?;
    let session_id = context
        .app_state
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local-command");
    let mut metadata = RequestMetadata::local(session_id, project_root);
    metadata.actor_id = Some(
        context
            .app_state
            .get("actor_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .unwrap_or("local-user")
            .to_owned(),
    );
    metadata.project_trusted =
        kiana_types::project_trust_from_app_state(&context.app_state).as_bool();
    metadata.permission_profile = PermissionProfile::Safe;
    Ok(metadata)
}

/// 把 `ResponseEnvelope` 分类成两种结局之一，其余一切都变成错误。
///
/// 【分类的顺序不能换】
/// 先看 `AwaitingApproval`，再看 `Completed`。反过来的话，一个正在等审批的响应会因为
/// 落进「既不是等审批也不是完成」而被报成失败——而它其实是**正常进展**，不是失败。
///
/// 【为什么要把非完成态变成错误】
/// 因为调用方需要知道「这次到底成没成」。`accepted` / `queued` / `running` 对一个
/// 同步的入口命令来说都不是结论——它们意味着「还没完」。把它们如实变成错误，
/// 比返回一个含义模糊的「成功」要诚实得多。
fn response_outcome(response: ResponseEnvelope) -> anyhow::Result<CommandDispatchOutcome> {
    if response.status == ExecutionStatus::AwaitingApproval {
        let challenge = response
            .output
            .get("approval")
            .cloned()
            .ok_or_else(|| anyhow!("control_plane_approval_challenge_missing"))?;
        let challenge = serde_json::from_value(challenge)
            .context("control_plane_approval_challenge_invalid")?;
        return Ok(CommandDispatchOutcome::AwaitingApproval(challenge));
    }
    if response.status != ExecutionStatus::Completed {
        return Err(anyhow!(
            "control_plane_command_{}:{}",
            status_name(response.status),
            response.error.as_deref().unwrap_or("unknown")
        ));
    }
    let result = response
        .output
        .get("command_result")
        .cloned()
        .ok_or_else(|| anyhow!("control_plane_command_result_missing"))?;
    serde_json::from_value(result)
        .context("control_plane_command_result_invalid")
        .map(CommandDispatchOutcome::Completed)
}

/// 拿到（或第一次构造）进程内的 `DaemonHost`。
///
/// 【为什么 `set` 之后还要再 `get` 一次】
/// `OnceLock::set` 在**已经有值时返回 Err**。两个线程同时第一次进来时，会有一个 set 成功、
/// 另一个失败。如果失败的那个直接用自己的 candidate 返回，两个入口就会拿到**两个不同的
/// 组合根**——于是出现两套授权状态。所以失败的一方必须回头去读那个已经赢家的值。
/// 这个「先 set 再 get」的写法就是为了处理这一次良性竞争。
fn local_daemon() -> anyhow::Result<Arc<DaemonHost>> {
    if let Some(host) = LOCAL_DAEMON.get() {
        return Ok(host.clone());
    }
    let candidate = Arc::new(DaemonHost::local()?);
    let _ = LOCAL_DAEMON.set(candidate);
    LOCAL_DAEMON
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("local_daemon_initialization_failed"))
}

/// 把执行状态翻译成错误码里用的稳定短名。
///
/// 【为什么要显式列举，而不是靠 `Debug`】
/// 因为这些短名会出现在错误串里，而错误串会被断言、被展示、被写进审计。
/// 靠 `{:?}` 意味着上游改一次枚举的 `Debug` 实现，线上错误码就跟着变了。
/// 显式列举让「错误码长什么样」成为这份代码的一部分，而不是别人的实现细节。
///
/// ⚠ 新增一个 `ExecutionStatus` 变体时，这里会编译不过——那是故意的。
/// 忘记决定它该叫什么，比编译失败难查得多。
fn status_name(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Accepted => "accepted",
        ExecutionStatus::Queued => "queued",
        ExecutionStatus::Cancelling => "cancelling",
        ExecutionStatus::Denied => "denied",
        ExecutionStatus::AwaitingApproval => "awaiting_approval",
        ExecutionStatus::Running => "running",
        ExecutionStatus::Completed => "completed",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
        ExecutionStatus::ResultUnknown => "result_unknown",
        ExecutionStatus::Blocked => "blocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_commands::{context::ContextCommand, CommandType};
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct LocalCommand;

    #[async_trait]
    impl Command for LocalCommand {
        fn name(&self) -> &str {
            "local"
        }

        fn description(&self) -> &str {
            "local test command"
        }

        fn command_type(&self) -> CommandType {
            CommandType::Local
        }

        async fn execute(&self, _context: CommandContext) -> anyhow::Result<CommandResult> {
            Ok(CommandResult::text("local-result"))
        }
    }

    struct RoutedCommand;

    #[async_trait]
    impl Command for RoutedCommand {
        fn name(&self) -> &str {
            "routed"
        }

        fn description(&self) -> &str {
            "routed test command"
        }

        fn command_type(&self) -> CommandType {
            CommandType::Local
        }

        fn route(&self, _context: &CommandContext) -> anyhow::Result<CommandRoute> {
            Ok(CommandRoute::ControlPlane {
                name: "unknown.command".to_owned(),
                arguments: Value::Null,
            })
        }

        async fn execute(&self, _context: CommandContext) -> anyhow::Result<CommandResult> {
            panic!("routed command must not execute locally")
        }
    }

    #[tokio::test]
    async fn local_commands_keep_the_existing_execution_path() {
        let result = execute_command(
            &LocalCommand,
            CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.value, "local-result");
    }

    #[tokio::test]
    async fn routed_commands_require_an_explicit_project_root() {
        let error = execute_command(
            &RoutedCommand,
            CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "control_plane_project_root_required");
    }

    #[test]
    fn request_metadata_defaults_to_the_authenticated_local_actor() {
        let root = fixture_root("metadata");
        let metadata = request_metadata(&CommandContext {
            args: String::new(),
            app_state: HashMap::from([(
                "cwd".to_owned(),
                Value::String(root.display().to_string()),
            )]),
        })
        .unwrap();

        assert_eq!(metadata.actor_id.as_deref(), Some("local-user"));
    }

    #[test]
    fn approve_local_write_auto_approval_is_limited_to_local_write_challenges() {
        assert!(should_auto_approve_local_write(
            true,
            &approval_challenge(RiskLevel::LocalWrite),
        ));
        assert!(!should_auto_approve_local_write(
            false,
            &approval_challenge(RiskLevel::LocalWrite),
        ));
        for risk in [
            RiskLevel::ReadOnly,
            RiskLevel::ExternalSideEffect,
            RiskLevel::Critical,
        ] {
            assert!(!should_auto_approve_local_write(
                true,
                &approval_challenge(risk),
            ));
        }
    }

    #[tokio::test]
    async fn context_repo_map_uses_client_daemon_core_and_query_handler() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let root = fixture_root("repo-map");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub struct RoutedContext;\n").unwrap();
        kiana_types::write_project_trust(&root, kiana_types::ProjectTrust::Trusted).unwrap();
        let result = execute_command(
            &ContextCommand,
            CommandContext {
                args: "repo-map --json --max-tokens 1000".to_owned(),
                app_state: HashMap::from([
                    ("cwd".to_owned(), Value::String(root.display().to_string())),
                    ("project_trusted".to_owned(), Value::Bool(true)),
                ]),
            },
        )
        .await
        .unwrap();
        let map: Value = serde_json::from_str(&result.value).unwrap();
        assert_eq!(map["token_budget"], 1000);
        assert_eq!(map["files"][0]["path"], "src/lib.rs");
        let _ = kiana_types::remove_project_trust(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn context_repo_map_does_not_promote_unknown_project_trust() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let root = fixture_root("untrusted");
        fs::create_dir_all(&root).unwrap();
        let error = execute_command(
            &ContextCommand,
            CommandContext {
                args: "repo-map --json".to_owned(),
                app_state: HashMap::from([(
                    "cwd".to_owned(),
                    Value::String(root.display().to_string()),
                )]),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "control_plane_command_denied:project_untrusted"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn context_read_queries_use_the_same_dispatcher() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let root = fixture_root("read-queries");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout_flow() {}\n// checkout workflow\n",
        )
        .unwrap();
        kiana_types::write_project_trust(&root, kiana_types::ProjectTrust::Trusted).unwrap();
        let app_state = HashMap::from([
            ("cwd".to_owned(), Value::String(root.display().to_string())),
            ("project_trusted".to_owned(), Value::Bool(true)),
        ]);

        let search = execute_command(
            &ContextCommand,
            CommandContext {
                args: "search checkout --json --limit 1".to_owned(),
                app_state: app_state.clone(),
            },
        )
        .await
        .unwrap();
        let search: Value = serde_json::from_str(&search.value).unwrap();
        assert_eq!(search["schema"], "kiana.context-search.v1");
        assert_eq!(search["hits"][0]["path"], "src/lib.rs");

        let pack = execute_command(
            &ContextCommand,
            CommandContext {
                args: "pack checkout --json --limit 1 --max-snippet-lines 1".to_owned(),
                app_state,
            },
        )
        .await
        .unwrap();
        let pack: Value = serde_json::from_str(&pack.value).unwrap();
        assert_eq!(pack["schema"], "kiana.context-pack.v1");
        assert_eq!(pack["snippets"][0]["path"], "src/lib.rs");

        let _ = kiana_types::remove_project_trust(&root);
        let _ = fs::remove_dir_all(root);
    }

    fn fixture_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-command-dispatch-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn approval_challenge(risk: RiskLevel) -> ApprovalChallenge {
        ApprovalChallenge {
            schema: "kiana.approval-challenge.v1".to_owned(),
            approval_id: ApprovalId::new(),
            request_id: kiana_protocol::RequestId::new(),
            request_hash: "sha256:challenge".to_owned(),
            risk,
            expires_at_unix_ms: u64::MAX,
            reason: "approval_required".to_owned(),
            nonce: "nonce".to_owned(),
            policy_version: "kiana.policy.v1".to_owned(),
        }
    }
}
