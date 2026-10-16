//! Harness 能力调用在审批暂停前后保持同一身份的领域合同。
//!
//! 【系统位置与数据流】
//! ```text
//! Runner 形成工具批次、Turn/Step 和 pending queue 摘要
//!   -> Core 收到能力请求并判定需要审批
//!   -> 当前模块从请求、RequestContext、sandbox 和 queue 摘要生成绑定
//!   -> Core 把绑定与 approval.requested / run.awaiting_approval 一起记录
//!   -> 恢复时重载受保护请求材料并重新校验绑定、策略和授权
//!   -> 获批后沿原 CapabilityResult 回到 Runner 的 drive_run
//! ```
//!
//! 本模块只定义可序列化身份材料和纯数据校验，不保存可执行能力请求、不批准请求，也不
//! 驱动 Runner。执行请求仍由 Core 的受保护审批材料保存；有序待执行队列由 Runner 的
//! checkpoint 管理，所以队列摘要只在恢复 checkpoint 时由 Runner 对照，Domain 无法单独
//! 从此 DTO 重建它。
//!
//! SHA-256 摘要没有密钥，只用于发现材料不一致，不认证签发者。真正是否允许继续仍由
//! Core 每次恢复时重新检查审批证明、当前策略、authority、取消状态和 dispatch permit。

use crate::{
    canonical_action_input_digest, capability_action_catalog_digest, json_digest,
    CapabilityRequest, InvocationId, RequestContext, RequestId, RunId, SchemaVersion, SessionId,
    StepId, TurnId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 用于持久化事件中识别恢复绑定对象的稳定类型名。
pub const INVOCATION_RESUME_BINDING_SCHEMA: &str = "kiana.invocation-resume-binding.v1";
/// 当前绑定合同版本；校验采用兼容版本关系，而不是要求版本字符串手工相等。
pub const INVOCATION_RESUME_BINDING_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

// 所有摘要字段统一采用 `sha256:` 加 64 位十六进制的外形，实际绑定关系由下方 digest 重算。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

// 请求参数中的 turn_id/step_id 是 JSON 值；在提取前反序列化成强类型，避免把任意文本
// 当作合法执行身份。键缺失代表请求未显式提供该身份，格式错误则不能静默降级为 None。
fn parse_id_checked<T>(arguments: &Value, key: &str) -> Result<Option<T>, String>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|_| format!("invocation_resume_{key}_invalid"))
}

/// Core 恢复一条等待审批的原始 Invocation 时必须重新对照的身份快照。
///
/// 【职责与调用者】
/// `kiana-core::capabilities` 在 Gate 返回 AwaitingApproval 时构造它，并与审批请求事实一同
/// 持久化；`kiana-core::recovery` 在恢复/批准路径重载请求后调用 `validate_against()`。
/// 它不是新的权限，也不包含命令正文；校验通过后 Core 仍须重新 prepare、authorize 并取得
/// dispatch permit，随后把原结果交回 Runner。
///
/// 【核心不变量】
/// 同一绑定必须继续指向原 Run、Turn、Step、请求/Invocation、参数、catalog、authority epoch、
/// owner/session、project 和 sandbox。`event_request_id` 是事件流游标，可在显式恢复命令中
/// 重新绑定；`pending_batch_digest` 则由 Runner 对照恢复 checkpoint，因为只有 Runner 拥有
/// 有序 pending queue。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationResumeBinding {
    /// DTO 类型标签，避免不同事件载荷被误解为恢复绑定。
    pub schema: String,
    /// 可兼容读取的合同版本。
    pub version: SchemaVersion,
    /// 发生暂停的 Harness Run；将恢复限制在原运行实例。
    pub run_id: RunId,
    /// 当前绑定所在的事件请求流 ID；显式 resume 命令会有新的游标 ID。
    pub event_request_id: RequestId,
    /// 原始模型轮次；可从请求参数或 ExecutionScope 取得。
    pub turn_id: Option<TurnId>,
    /// 原始步骤身份；在请求显式携带时绑定，否则为 None。
    pub step_id: Option<StepId>,
    /// 从原 request_id 派生的 Invocation ID，保证恢复不是新的能力调用。
    pub invocation_id: InvocationId,
    /// 原能力请求 ID；参数摘要以该请求为输入计算。
    pub request_id: RequestId,
    /// 规范化能力动作及参数的摘要，用于发现审批等待期间请求内容漂移。
    pub parameter_digest: String,
    /// 请求所依据的 capability action catalog 摘要。
    pub catalog_digest: String,
    /// 请求准备时的 authority epoch；恢复还要与当前执行范围比较。
    pub authority_epoch: u64,
    /// 发起请求的 actor；在审批恢复时必须仍与调用上下文相同。
    pub owner_id: String,
    /// 发起请求的 session，防止其他会话消费该绑定。
    pub session_id: SessionId,
    /// project root/trust/role/department 上下文的内容摘要。
    pub project_digest: String,
    /// sandbox 文本的摘要；恢复时必须使用同一执行隔离配置。
    pub sandbox_digest: String,
    /// Runner 发出本 Invocation 时剩余有序工具批次的摘要；Domain 不拥有队列本身。
    pub pending_batch_digest: String,
    /// 对上面所有绑定字段计算的无密钥摘要；不构成来源认证或授权证明。
    pub binding_digest: String,
}

impl InvocationResumeBinding {
    /// 从服务端准备好的请求和执行上下文创建暂停绑定。
    ///
    /// 【调用者】Core 在请求被 Gate 判定为 AwaitingApproval 后调用，随后把返回值交给审批
    /// 持久化逻辑；Runner 提供待处理批次摘要，因为 Domain/Core 这里没有队列所有权。
    ///
    /// 【输入】`run_id` 和 `event_request_id` 标识暂停所属 Run 与事件流；`request` 是原始
    /// CapabilityRequest；`context` 提供 actor/session/project/role 等身份；`sandbox` 是原执行
    /// 隔离配置；`pending_batch_digest` 是 Runner 对剩余有序调用队列计算的摘要。
    ///
    /// 【输出/失败】成功返回带 `binding_digest` 的不可变身份快照。缺少 actor、turn/step JSON
    /// 身份无法解析、请求动作摘要无法生成或最终 DTO 校验失败时返回稳定错误；Core 会将其
    /// 转成能力动作错误并结束该请求的审批准备，不会因此直接派发能力。
    ///
    /// 【为什么拆分队列职责】Runner 是待处理调用顺序的唯一所有者；在这里复制队列会产生
    /// 第二份事实来源。Core 绑定它提供的摘要，恢复时再由 Runner checkpoint 校验摘要与队列。
    pub fn from_request(
        run_id: RunId,
        event_request_id: RequestId,
        request: &CapabilityRequest,
        context: &RequestContext,
        sandbox: &str,
        pending_batch_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let owner_id = context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "invocation_resume_owner_missing".to_owned())?;
        let turn_id = parse_id_checked(&request.arguments, "turn_id")?.or_else(|| {
            request
                .execution_scope
                .as_ref()
                .and_then(|scope| scope.turn_id)
        });
        let step_id = parse_id_checked(&request.arguments, "step_id")?;
        let catalog_digest = request
            .execution_scope
            .as_ref()
            .map(|scope| scope.catalog_digest.clone())
            .unwrap_or_else(capability_action_catalog_digest);
        let authority_epoch = request
            .execution_scope
            .as_ref()
            .map(|scope| scope.authority_epoch)
            .unwrap_or_default();
        let parameter_digest = canonical_action_input_digest(request)?;
        let project_digest = json_digest(&json!({
            "project_root": context.project_root,
            "project_trusted": context.project_trusted,
            "role_id": context.role_id,
            "department_id": context.department_id,
        }));
        let sandbox_digest = json_digest(&json!({ "sandbox": sandbox }));
        let pending_batch_digest = pending_batch_digest.into();
        let mut binding = Self {
            schema: INVOCATION_RESUME_BINDING_SCHEMA.to_owned(),
            version: INVOCATION_RESUME_BINDING_VERSION,
            run_id,
            event_request_id,
            turn_id,
            step_id,
            invocation_id: InvocationId::from_uuid(request.request_id.as_uuid()),
            request_id: request.request_id,
            parameter_digest,
            catalog_digest,
            authority_epoch,
            owner_id: owner_id.to_owned(),
            session_id: context.session_id.clone(),
            project_digest,
            sandbox_digest,
            pending_batch_digest,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    /// 校验 DTO 版本、必需 ID、身份字段格式、各摘要格式及整体绑定摘要。
    ///
    /// 【调用者】构造器在生成后调用；Core 从审批事件或快照反序列化后也应先调用，之后再
    /// 用 `validate_against()` 与当前重载请求和上下文对照。
    /// 【失败】字段/版本格式异常返回 `invocation_resume_binding_header_invalid` 或对应摘要
    /// 格式错误；字段被改动但摘要未同步时返回 `invocation_resume_binding_digest_mismatch`。
    /// 【边界】只验证 DTO 内部一致性，不查询审批 Store、Runner 队列、当前时钟或权限权威。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INVOCATION_RESUME_BINDING_SCHEMA
            || !self
                .version
                .is_compatible_with(&INVOCATION_RESUME_BINDING_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.event_request_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.request_id.as_uuid().is_nil()
            || self.invocation_id != InvocationId::from_uuid(self.request_id.as_uuid())
            || self.authority_epoch == 0
            || self.owner_id.trim().is_empty()
            || self.owner_id.len() > 256
            || self.owner_id.contains('\0')
            || self.session_id.is_empty()
        {
            return Err("invocation_resume_binding_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.parameter_digest, "invocation_resume_parameter_digest"),
            (&self.catalog_digest, "invocation_resume_catalog_digest"),
            (&self.project_digest, "invocation_resume_project_digest"),
            (&self.sandbox_digest, "invocation_resume_sandbox_digest"),
            (&self.pending_batch_digest, "invocation_resume_batch_digest"),
            (&self.binding_digest, "invocation_resume_binding_digest"),
        ] {
            valid_digest(value, field)?;
        }
        if self.binding_digest != self.digest() {
            return Err("invocation_resume_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将持久化绑定与审批恢复时重新载入的请求、上下文和 sandbox 逐项比较。
    ///
    /// 【调用者】Core 在恢复待审批 Invocation、重新准备/授权和 dispatch 之前调用。
    /// 【输入】原 Run 与事件流请求 ID、受保护材料解出的 CapabilityRequest、当前恢复上下文、
    /// 以及 sandbox。成功只表示这些可在 Core 侧重算的身份与暂停前一致，不代表授权通过。
    /// 【失败】绑定本身无效时返回 `validate()` 的错误；任何身份、参数、目录、权限版本、owner、
    /// session、project 或 sandbox 变化返回 `invocation_resume_binding_changed`，Core 会拒绝
    /// 这次恢复而不把变更后的请求当成原审批内容派发。
    ///
    /// pending batch 摘要在此处不重算：Runner 持有有序 checkpoint 队列，必须在恢复该队列时
    /// 独立比较。把队列校验挪到 Domain 会错误地让领域 DTO 冒充队列事实所有者。
    pub fn validate_against(
        &self,
        run_id: RunId,
        event_request_id: RequestId,
        request: &CapabilityRequest,
        context: &RequestContext,
        sandbox: &str,
    ) -> Result<(), String> {
        self.validate()?;
        let expected_turn = parse_id_checked(&request.arguments, "turn_id")?.or_else(|| {
            request
                .execution_scope
                .as_ref()
                .and_then(|scope| scope.turn_id)
        });
        let expected_step = parse_id_checked(&request.arguments, "step_id")?;
        let expected_catalog = request
            .execution_scope
            .as_ref()
            .map(|scope| scope.catalog_digest.clone())
            .unwrap_or_else(capability_action_catalog_digest);
        let expected_epoch = request
            .execution_scope
            .as_ref()
            .map(|scope| scope.authority_epoch)
            .unwrap_or_default();
        let owner_id = context.actor_id.as_deref().unwrap_or_default();
        let expected_project = json_digest(&json!({
            "project_root": context.project_root,
            "project_trusted": context.project_trusted,
            "role_id": context.role_id,
            "department_id": context.department_id,
        }));
        let expected_sandbox = json_digest(&json!({ "sandbox": sandbox }));
        if self.run_id != run_id
            || self.event_request_id != event_request_id
            || self.request_id != request.request_id
            || self.invocation_id != InvocationId::from_uuid(request.request_id.as_uuid())
            || self.turn_id != expected_turn
            || self.step_id != expected_step
            || self.parameter_digest != canonical_action_input_digest(request)?
            || self.catalog_digest != expected_catalog
            || self.authority_epoch != expected_epoch
            || self.owner_id != owner_id
            || self.session_id != context.session_id
            || self.project_digest != expected_project
            || self.sandbox_digest != expected_sandbox
        {
            return Err("invocation_resume_binding_changed".to_owned());
        }
        Ok(())
    }

    /// 将事件流游标改绑到已提交的显式恢复命令，同时保持原 Invocation 身份不变。
    ///
    /// 【调用者】ControlPlane recovery 在接续同一个 pending approval 时调用。
    /// 【输入/输出】新 `event_request_id` 必须非 nil；成功后只替换事件流请求 ID、重算绑定摘要，
    /// 返回校验结果。空 ID 返回 `invocation_resume_event_request_invalid`。
    /// 【容易误改】这里不能同步替换 `request_id`、Invocation、参数或 authority 字段；它们表示
    /// 原先获批的动作。若把 resume command 当作新能力请求，审批身份就会漂移。
    pub fn rebind_event_request(&mut self, event_request_id: RequestId) -> Result<(), String> {
        if event_request_id.as_uuid().is_nil() {
            return Err("invocation_resume_event_request_invalid".to_owned());
        }
        self.event_request_id = event_request_id;
        self.binding_digest = self.digest();
        self.validate()
    }

    /// 按固定字段集合生成整体绑定摘要，不把 `binding_digest` 自身纳入输入。
    ///
    /// 它用于在恢复时发现序列化材料被改动；由于摘要无密钥，不能认证事件写入者，也不能
    /// 单独代替 ApprovalProof、Core authority 或 DispatchPermit。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "event_request_id": self.event_request_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "invocation_id": self.invocation_id,
            "request_id": self.request_id,
            "parameter_digest": self.parameter_digest,
            "catalog_digest": self.catalog_digest,
            "authority_epoch": self.authority_epoch,
            "owner_id": self.owner_id,
            "session_id": self.session_id,
            "project_digest": self.project_digest,
            "sandbox_digest": self.sandbox_digest,
            "pending_batch_digest": self.pending_batch_digest,
        }))
    }
}
