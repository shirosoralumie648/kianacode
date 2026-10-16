//! 后台长进程的服务端身份合同，供进程启动方、后续操作和恢复路径共同核对。
//!
//! 【系统位置与调用链】
//! ```text
//! Core 授权 process.start
//!   -> Daemon ExecutionHandler 准备并启动 OS 子进程
//!   -> 当前文件构造 JobHandle，并随 process.started 事实记录
//!   -> 后续独立的 poll/stdin/resize/stop 请求携带句柄
//!   -> Daemon 校验身份、范围、authority epoch 和到期时间
//! ```
//!
//! 本模块只定义和校验可序列化的身份材料，不启动、查找或控制 OS 进程，也不授予
//! capability。`job_id` 用于查找生命周期事实；PID/进程组编号只是启动时的进程证据，
//! 不能单独证明当前同一主体仍有权操作进程。Daemon 的适配层位于
//! `kiana-daemon/src/execution_control.rs`，Core 授权入口位于
//! `kiana-core/src/capabilities.rs`。
//!
//! 句柄摘要是无密钥内容摘要，只能发现字段与摘要不一致，不能认证签发者。实际授权、
//! 当前 authority 和到期检查仍由调用方按每个独立操作重新执行。

use crate::{json_digest, InvocationId, RequestId, RunId, SchemaVersion, SessionId, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// 持久化/传输时用于识别本合同类型的稳定 schema 名称。
pub const JOB_HANDLE_SCHEMA: &str = "kiana.job-handle.v1";
/// 当前 JobHandle 合同版本；反序列化和校验端必须使用同一版本约定。
pub const JOB_HANDLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

// 非空文本和 SHA-256 外形检查集中在这里，避免各字段校验对空白、NUL 或长度采用不同规则。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 一次进程启动对应的一份不可变续用身份材料。
///
/// 【职责】
/// Daemon 在 `process.start` 成功 spawn 后创建它，并把它写入 `process.started` 事实；
/// 后续每次 poll/stdin/resize/stop 都要重新携带它，经 Core 的独立授权后再由 Daemon
/// 与原始启动身份比对。它关联“哪次启动、哪个主体、哪个项目和授权版本”，不是进程
/// 控制票据，也不能替代 ControlPlane 的 DispatchPermit。
///
/// 【生命周期】
/// 进程启动前先记录 `process.prepared`；OS spawn 后才有进程组证据并创建句柄；
/// `process.started` 记录成功后，调用方才把 start 响应作为已启动返回。若句柄或
/// started 记录阶段失败，Daemon 的进程监督保护负责尽力清理已经启动的子进程。
/// 重启后找不到内存中的 live process entry 时，持久化句柄只允许 poll 已记录结果，
/// 不能按其中的 PID 重新挂接 stdin 或 stop。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobHandle {
    /// 合同类型标签；防止把其他 JSON 对象误读为 JobHandle。
    pub schema: String,
    /// 合同版本，用于拒绝不兼容的持久化数据。
    pub version: SchemaVersion,
    /// 这一个后台任务的稳定查找 ID；它不等于 OS PID。
    pub job_id: RequestId,
    /// 创建进程的原始 CapabilityRequest ID，后续 operation 不得用自己的请求 ID 替换。
    pub start_request_id: RequestId,
    /// 从 `start_request_id` 确定性派生的启动 Invocation ID，用于把句柄关联到原执行。
    pub start_invocation_id: InvocationId,
    /// 可选 Run 关联；没有 Harness Run 的进程仍可有 job，但有值时续用请求必须匹配。
    pub run_id: Option<RunId>,
    /// 可选 Turn 关联；用于把后台任务限制在发起它的对话轮次。
    pub turn_id: Option<TurnId>,
    /// 发起者身份标签；Daemon 用它拒绝其他主体操作该任务。
    pub owner_id: String,
    /// 发起任务的会话身份，防止同一 owner 的其他会话误用句柄。
    pub session_id: SessionId,
    /// 由项目根目录和会话等启动身份材料生成的摘要，绑定句柄所处项目范围。
    pub project_digest: String,
    /// 启动时使用的授权账本 epoch；续用端还要和当前 authority 比较。
    pub authority_epoch: u64,
    /// OS 进程组 ID 是进程身份的辅助证据，不是授权凭证；缺失或为 0 时拒绝续用。
    pub process_group_id: Option<u32>,
    /// Unix 毫秒绝对到期时刻；此结构只检查非零，Daemon 还要与当前时间比较。
    pub expires_at_unix_ms: u64,
    /// 对除本字段外所有句柄身份字段计算的无密钥摘要，用于检测意外篡改。
    pub handle_digest: String,
}

impl JobHandle {
    /// 从已准备好的进程启动身份构造句柄，并在返回前执行结构与摘要校验。
    ///
    /// 【调用者】
    /// `kiana-daemon::execution_control::ExecutionHandler` 在 OS 子进程 spawn 成功后调用；
    /// 此时进程监督 guard 已接管清理责任，但 `process.started` 事实尚待写入。
    ///
    /// 【输入】
    /// - `job_id`：Daemon 为这次长任务分配的稳定 ID。
    /// - `start_request_id`：原始 start 请求 ID；Invocation ID 会由它确定性派生。
    /// - `run_id` / `turn_id`：若任务来自 Harness，则携带对应执行范围。
    /// - `owner_id` / `session_id`：请求主体和会话。
    /// - `project_digest` / `authority_epoch`：启动时的项目与授权快照。
    /// - `process_group_id`：OS spawn 返回的进程标识证据。
    /// - `expires_at_unix_ms`：Daemon 根据启动 timeout 算出的绝对过期时刻。
    ///
    /// 【输出与失败】
    /// 成功时返回带自摘要的句柄。输入身份缺失、摘要格式错误或句柄校验失败时返回
    /// 稳定字符串错误；Daemon 将其映射为 `job_handle_invalid`。此函数本身不写 EventStore，
    /// 也不检查现在是否已经晚于到期时间。
    ///
    /// 【为什么这样设计】
    /// Invocation ID 与 start request ID 使用相同 UUID，使开始身份可重复推导，避免调用者
    /// 另造一个可能漂移的关联值。到期时钟和 Core authority 由运行时适配层持有，因此
    /// Domain 构造器只校验可从传入值确定的合同，不假装自己掌握当前权威状态。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: RequestId,
        start_request_id: RequestId,
        run_id: Option<RunId>,
        turn_id: Option<TurnId>,
        owner_id: impl Into<String>,
        session_id: SessionId,
        project_digest: impl Into<String>,
        authority_epoch: u64,
        process_group_id: Option<u32>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut handle = Self {
            schema: JOB_HANDLE_SCHEMA.to_owned(),
            version: JOB_HANDLE_VERSION,
            job_id,
            start_request_id,
            start_invocation_id: InvocationId::from_uuid(start_request_id.as_uuid()),
            run_id,
            turn_id,
            owner_id: owner_id.into(),
            session_id,
            project_digest: project_digest.into(),
            authority_epoch,
            process_group_id,
            expires_at_unix_ms,
            handle_digest: String::new(),
        };
        handle.handle_digest = handle.digest();
        handle.validate()?;
        Ok(handle)
    }

    /// 检查 schema、必需身份、字段格式以及句柄摘要是否互相一致。
    ///
    /// 【调用者】构造器在创建时调用；Daemon 在读取持久化句柄、继续控制 live 进程或
    /// 读取重启后的结果时也会调用。
    ///
    /// 【输出/失败】完全一致返回 `Ok(())`；头部字段、owner/project/digest 格式异常分别
    /// 返回稳定错误码，摘要与字段不匹配返回 `job_handle_digest_mismatch`。
    ///
    /// 【重要边界】这里只要求 `expires_at_unix_ms != 0`，不会读取系统时钟，也不会确认
    /// actor、session、project 或 authority 当前仍然有效。Daemon 的
    /// `validate_job_handle()` 会另外做这些运行时比较；不能把本函数单独当作授权判断。
    /// 校验也只证明无密钥摘要与内容相符，不能证明句柄由可信服务端签发。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != JOB_HANDLE_SCHEMA
            || self.version != JOB_HANDLE_VERSION
            || self.job_id.as_uuid().is_nil()
            || self.start_request_id.as_uuid().is_nil()
            || self.start_invocation_id != InvocationId::from_uuid(self.start_request_id.as_uuid())
            || self.authority_epoch == 0
            || self.process_group_id.is_none_or(|pid| pid == 0)
            || self.expires_at_unix_ms == 0
            || self.session_id.is_empty()
        {
            return Err("job_handle_header_invalid".to_owned());
        }
        required(&self.owner_id, "job_handle_owner", 256)?;
        digest(&self.project_digest, "job_handle_project_digest")?;
        digest(&self.handle_digest, "job_handle_digest")?;
        if self.handle_digest != self.digest() {
            return Err("job_handle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 按字段的规范 JSON 形状计算句柄内容摘要。
    ///
    /// 摘要输入故意不包含 `handle_digest` 自身，避免递归；调用 `validate()` 时会再将
    /// 计算值与字段保存值比较。它可发现字段改动后没有同步更新摘要的情况，但 SHA-256
    /// 未使用秘密密钥，因此不能防止能同时改写内容和摘要的人伪造整份句柄，也不提供授权。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "job_id": self.job_id,
            "start_request_id": self.start_request_id,
            "start_invocation_id": self.start_invocation_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "owner_id": self.owner_id,
            "session_id": self.session_id,
            "project_digest": self.project_digest,
            "authority_epoch": self.authority_epoch,
            "process_group_id": self.process_group_id,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}
