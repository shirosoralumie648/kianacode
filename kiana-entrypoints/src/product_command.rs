//! 显式的、可脚本化的人类命令，经由 **versioned daemon 协议**送出。
//!
//! # 这个文件提供四个子命令
//!
//! ```text
//! kiana command <name> --arguments '<JSON>'   送一条具名命令
//! kiana approvals                             列出待批准
//! kiana resume --session-id ID                恢复一次中断的 run
//! kiana approval <ID> --decision …            批准或拒绝，并带上凭证
//! ```
//!
//! # 它和 REPL 的分工
//!
//! REPL 是「人说一句，模型答一句」的交互面；本文件是「脚本一次做完一件事」的批处理面。
//! 两者都把请求送进 `DaemonHost` → `ControlPlane`，**都在受控路径上**。
//! 对照本目录下的 `mcp.rs`——那个不经过控制面，是已记录的安全发现。
//!
//! # 为什么这个文件很短
//!
//! 因为它只做两件事：把 argv 解析成一个 `RequestEnvelope`，然后交给 daemon。
//! 授权、审批、事件记录全在下游，这里一行副作用都没有。
//! **权限判断不在这���**——这是本文件最需要记住的一点：它解析出来的
//! `--role` / `--permission-profile` 只是调用方的**声明**，最终由控制面裁定。
//! Explicit human commands sent through the versioned daemon protocol.
use anyhow::{anyhow, Context, Result};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ApprovalDecision, ApprovalId, PermissionProfile, RequestEnvelope, RequestMetadata, RoleSpec,
};
use std::path::PathBuf;

const USAGE: &str = "kiana command <name> --arguments '<JSON>' [--session-id ID] [--project-root DIR] [--role ROLE] [--permission-profile safe|balanced]\nkiana approvals|resume --session-id ID [--project-root DIR] [--role ROLE] [--permission-profile safe|balanced]\nkiana approval <ID> --decision approve|deny --request-hash HASH --nonce NONCE --session-id ID [--project-root DIR] [--role ROLE] [--permission-profile safe|balanced]";

/// 解析参数、构造 `RequestEnvelope`、交给 daemon。
///
/// 【流程】
/// 1. `--help` 打印用法并返回；
/// 2. 第一个非 flag 参数决定模式：`command` / `approval` 是「具名」模式
///    （需要再读一个名字），`approvals` / `resume` 是直接模式；
/// 3. 逐个解析 flag。其中 `--arguments` 收的是一整段 JSON，
///    它被原样放进请求的 `arguments` 字段，由下游按命令自己的 schema 校验；
/// 4. 按模式构造四种不同的 `RequestEnvelope` 并发出。
///
/// 【`--permission-profile` 的缺省值是 `safe`】
/// 缺省 `PermissionProfile::Safe` 而不是 `balanced`：脚本不该因为「没写参数」
/// 就自动拿到更大的权限。要放宽必须显式写出来——这样「这条脚本到底要多大权限」
/// 一眼可读。
///
/// 【⚠ 批准为什么必须带 `--request-hash` 和 `--nonce`】
/// 这两个字段是**防重放**的一对，缺一不可：
/// - `request_hash` 把「你批准的到底是哪一个请求」钉死。少了它，
///   一次批准可能被拿去套用到另一个内容不同的请求上；
/// - `nonce` 让这一次批准只能用一次。少了它，同一条批准可以被反复提交。
///
/// 也就是说：**批准不是一句「我同意」，而是对某个具体请求的一次性签章。**
/// 这正是 [ `harness_run.rs` ] 里说「决定权在控制面、呈现权在调用方」的那件事
/// 在脚本面上的样子——这里提供凭证，落章仍然在控制面。
///
/// 【它不验证 hash 是否对得上】
/// 这里只是把值传下去。核对是下游的事：控制面会比对它手上那个请求算出来的 hash。
/// 本地校验等于把「我以为的请求」当成事实。
pub async fn main_from_args(args: &[String]) -> Result<()> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("{USAGE}");
        return Ok(());
    }
    let mode = args.first().map(String::as_str).unwrap_or_default();
    let named = matches!(mode, "command" | "approval");
    let name = if named {
        Some(args.get(1).ok_or_else(|| anyhow!("{USAGE}"))?.clone())
    } else {
        None
    };
    let mut arguments = serde_json::json!({});
    let mut session = None;
    let mut root = std::env::current_dir().context("project_root_unavailable")?;
    let mut role = RoleSpec::builder();
    let mut profile = PermissionProfile::Safe;
    let (mut decision, mut request_hash, mut nonce) = (None, None, None);
    let mut index = if named { 2 } else { 1 };
    while index < args.len() {
        let (flag, value, consumed) = match args[index].split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, value, 1),
            _ => (
                args[index].as_str(),
                args.get(index + 1)
                    .ok_or_else(|| anyhow!("command_option_value_required:{}", args[index]))?
                    .as_str(),
                2,
            ),
        };
        match flag {
            "--arguments" => {
                arguments = serde_json::from_str(value).context("command_arguments_invalid")?
            }
            "--session-id" => session = Some(value.to_owned()),
            "--project-root" | "--workdir" => root = PathBuf::from(value),
            "--role" => role = RoleSpec::lookup(value).ok_or_else(|| anyhow!("role_unknown"))?,
            "--permission-profile" => {
                profile = match value {
                    "safe" => PermissionProfile::Safe,
                    "balanced" => PermissionProfile::Balanced,
                    _ => return Err(anyhow!("permission_profile_unsupported")),
                }
            }
            "--decision" => {
                decision = Some(match value {
                    "approve" => ApprovalDecision::Approve,
                    "deny" => ApprovalDecision::Deny,
                    _ => return Err(anyhow!("approval_decision_invalid")),
                })
            }
            "--request-hash" => request_hash = Some(value.to_owned()),
            "--nonce" => nonce = Some(value.to_owned()),
            _ => return Err(anyhow!("command_option_unknown:{flag}")),
        }
        index += consumed;
    }
    if !arguments.is_object() {
        return Err(anyhow!("command_arguments_object_required"));
    }
    if mode != "command" && session.as_deref().is_none_or(|id| id.trim().is_empty()) {
        return Err(anyhow!("session_id_required"));
    }
    let root = root.canonicalize().context("project_root_unavailable")?;
    let mut metadata = RequestMetadata::local(
        session.unwrap_or_else(|| kiana_protocol::RunId::new().to_string()),
        root.to_string_lossy().into_owned(),
    );
    metadata.assign_role(&role);
    metadata.permission_profile = profile;
    let request = match mode {
        "command" => RequestEnvelope::command(metadata, name.unwrap(), arguments),
        "approvals" => RequestEnvelope::pending_approvals(metadata, None),
        "resume" => RequestEnvelope::resume_run(metadata, None),
        "approval" => RequestEnvelope::approval_decision_with_proof(
            metadata,
            serde_json::from_value::<ApprovalId>(serde_json::Value::String(name.unwrap()))
                .context("approval_id_invalid")?,
            decision.ok_or_else(|| anyhow!("approval_decision_required"))?,
            request_hash,
            nonce,
        ),
        _ => return Err(anyhow!("{USAGE}")),
    };
    let host = DaemonHost::local().map_err(anyhow::Error::msg)?;
    let response = host.handle(request).await;
    println!("{}", serde_json::to_string_pretty(&response)?);
    if matches!(
        response.status,
        kiana_protocol::ExecutionStatus::Denied
            | kiana_protocol::ExecutionStatus::Blocked
            | kiana_protocol::ExecutionStatus::Failed
            | kiana_protocol::ExecutionStatus::ResultUnknown
    ) {
        return Err(anyhow!(
            "{}:{}",
            response.status.as_str(),
            response.error.unwrap_or_default()
        ));
    }
    Ok(())
}
