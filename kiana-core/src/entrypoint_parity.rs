//! 入口中立的命令规范化与一致性（parity）矩阵。
//!
//! # 文件职责与数据流
//!
//! CLI、TTY、Web、Workbench、Desktop、scheduler、swarm 和 connector 都可以标记请求来自哪个
//! 界面，但不能选择不同的授权路径：
//!
//! ```text
//! 任一入口的 RequestContext + CommandIntent
//!        -> EntrypointCommand（ID/参数/context digest + 固定 route）
//!        -> 同一个 DaemonHost -> ControlPlane
//!        -> 产生 Denied / AwaitingApproval / Allowed / Unknown decision
//!        -> 各入口回传同一 command digest
//!        -> EntrypointParityMatrix 比较四至八个入口的 parity
//! ```
//!
//! 本文件只规范化和校验证据，不执行 command、不调用 handler/provider、不决定 approval。
//! `Denied` 时 `handler_calls` 必须是 0，防止入口在显示拒绝的同时偷偷执行；`Unknown` 和
//! `AwaitingApproval` 也只是结构化状态，不是成功。digest 是未加密钥的一致性指纹，不能
//! 作为签名或授权证明。

use kiana_domain::{
    json_digest, CommandIntent, EntryPointKind, RequestContext, RequestId, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const ENTRYPOINT_COMMAND_SCHEMA: &str = "kiana.entrypoint-command.v1";
pub const ENTRYPOINT_PARITY_MATRIX_SCHEMA: &str = "kiana.entrypoint-parity-matrix.v1";
pub const ENTRYPOINT_ROUTE: &str = "daemonhost.controlplane";
pub const ENTRYPOINT_PARITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_PARITY_COMMANDS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 一个入口观察到的统一决策状态。
pub enum EntrypointDecision {
    /// ControlPlane 拒绝，且 deny-first 要求没有 handler effect。
    Denied,
    /// 需要 approval，尚未执行受保护动作。
    AwaitingApproval,
    /// 已允许进入后续统一执行路径；不表示本函数执行了 handler。
    Allowed,
    /// 当前结果不能安全确定，不能渲染成成功。
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个入口规范化后的 command envelope。
///
/// 它只保存稳定身份和 digest：原始 arguments/context 不在这里执行或持久化。固定
/// `ENTRYPOINT_ROUTE` 把所有入口锁到 `daemonhost.controlplane`，避免 adapter 自行加入本地
/// authority、模型循环或 provider 路由。
pub struct EntrypointCommand {
    /// command envelope schema。
    pub schema: String,
    /// 可兼容的 wire 版本。
    pub version: SchemaVersion,
    /// 产生 envelope 的入口标签。
    pub entrypoint: EntryPointKind,
    /// 来自 RequestContext 的请求 ID。
    pub request_id: RequestId,
    /// 已规范化的命令名称。
    pub command_name: String,
    /// `CommandIntent.arguments` 的内容摘要。
    pub arguments_digest: String,
    /// 经过挑选的 RequestContext 字段摘要。
    pub context_digest: String,
    /// 绑定 request/name/arguments/context/route 的总摘要。
    pub command_digest: String,
    /// 唯一执行路径，必须等于 `ENTRYPOINT_ROUTE`。
    pub route: String,
    /// 当前来自统一控制面的决策状态。
    pub decision: EntrypointDecision,
}

impl EntrypointCommand {
    /// 把入口上下文和命令意图规范化成统一 route 的 envelope。
    ///
    /// 【输入】
    /// `entrypoint` 只是来源标签；`context` 提供 request ID 和项目/角色/packet/path 等边界；
    /// `intent` 提供命令名和 JSON 参数。参数内容只计算 digest，不在这里执行。
    ///
    /// 【输出/失败】
    /// 成功返回初始 `Unknown` decision 的 command，后续由统一 ControlPlane 结果覆盖；空/过长/
    /// 含 NUL 的 command name 或 header/digest 不合法时返回错误。构造不会写事件、调用 handler
    /// 或自动把 Unknown 改成 Allowed。
    pub fn new(
        entrypoint: EntryPointKind,
        context: &RequestContext,
        intent: &CommandIntent,
    ) -> Result<Self, String> {
        if intent.name.trim().is_empty() || intent.name.len() > 256 || intent.name.contains('\0') {
            return Err("entrypoint_command_name_invalid".to_owned());
        }
        let context_digest = context_digest(context);
        let arguments_digest = json_digest(&intent.arguments);
        let command_digest = json_digest(&json!({
            "request_id": context.request_id,
            "command_name": intent.name,
            "arguments_digest": arguments_digest,
            "context_digest": context_digest,
            "route": ENTRYPOINT_ROUTE,
        }));
        let command = Self {
            schema: ENTRYPOINT_COMMAND_SCHEMA.to_owned(),
            version: ENTRYPOINT_PARITY_VERSION,
            entrypoint,
            request_id: context.request_id,
            command_name: intent.name.clone(),
            arguments_digest,
            context_digest,
            command_digest,
            route: ENTRYPOINT_ROUTE.to_owned(),
            decision: EntrypointDecision::Unknown,
        };
        command.validate()?;
        Ok(command)
    }

    /// 为同一个规范化 command 写入控制面决策并重新校验。
    ///
    /// 该方法只改变内存中的 `decision`；它不验证 approval 本身，也不执行 command。调用方
    /// 必须把决策来源保持在同一个 ControlPlane 结果上，不能按入口各自猜测。
    pub fn with_decision(mut self, decision: EntrypointDecision) -> Result<Self, String> {
        self.decision = decision;
        self.validate()?;
        Ok(self)
    }

    /// 从 JSON 解码并验证 command envelope。
    ///
    /// `deny_unknown_fields` 和 `validate` 共同防止客户端添加未被合同识别的权限/route 字段。
    /// 解码只产生值，不会触发执行。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let command: Self = serde_json::from_value(value.clone())
            .map_err(|_| "entrypoint_command_decode_failed".to_owned())?;
        command.validate()?;
        Ok(command)
    }

    /// 将已验证的 command envelope 编码为 JSON；只做序列化，不写网络或 EventLog。
    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "entrypoint_command_encode_failed".to_owned())
    }

    /// 校验 command 的 schema、request ID、命令名、固定 route 和三个摘要字段。
    ///
    /// 这里不重新读取 context/arguments，因此只能确认 envelope 自洽，不能证明摘要对应的
    /// 原始输入仍存在或请求已经获得权限；真实授权仍由 ControlPlane 完成。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ENTRYPOINT_COMMAND_SCHEMA
            || !self.version.is_compatible_with(&ENTRYPOINT_PARITY_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.command_name.trim().is_empty()
            || self.command_name.len() > 256
            || self.route != ENTRYPOINT_ROUTE
        {
            return Err("entrypoint_command_header_invalid".to_owned());
        }
        for (digest, field) in [
            (
                &self.arguments_digest,
                "entrypoint_command_arguments_digest",
            ),
            (&self.context_digest, "entrypoint_command_context_digest"),
            (&self.command_digest, "entrypoint_command_digest"),
        ] {
            validate_digest(digest, field)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 多入口共享同一 command 的 parity 矩阵。
///
/// 矩阵把同一 request/command digest 的多个入口观察放在一起，拒绝入口重复、身份/摘要漂移，
/// 并强制 denied decision 对应 `handler_calls == 0`。它是跨界面证据投影，不创建执行循环。
pub struct EntrypointParityMatrix {
    /// 固定 parity matrix schema。
    pub schema: String,
    /// 可兼容的 wire 版本。
    pub version: SchemaVersion,
    /// 所有 command 必须共享的 request ID。
    pub request_id: RequestId,
    /// 所有 command 必须共享的 command digest。
    pub command_digest: String,
    /// 各入口各一条 command，数量上限为 8。
    pub commands: Vec<EntrypointCommand>,
    /// 统一 handler 被调用次数；只要存在 Denied 就必须为零。
    pub handler_calls: u32,
    /// 对 matrix 内容重算的 canonical digest。
    pub matrix_digest: String,
}

impl EntrypointParityMatrix {
    /// 排序并构造 parity matrix，随后立即执行完整验证。
    ///
    /// 空列表、超过 8 条、重复入口、command/request digest 不一致和 denied 后有 handler
    /// 都会失败；排序只为了稳定输出，不代表执行顺序，也不会触发任何 handler。
    pub fn new(mut commands: Vec<EntrypointCommand>, handler_calls: u32) -> Result<Self, String> {
        commands.sort_by_key(|command| format!("{:?}", command.entrypoint));
        let first = commands
            .first()
            .ok_or_else(|| "entrypoint_parity_commands_required".to_owned())?;
        let mut matrix = Self {
            schema: ENTRYPOINT_PARITY_MATRIX_SCHEMA.to_owned(),
            version: ENTRYPOINT_PARITY_VERSION,
            request_id: first.request_id,
            command_digest: first.command_digest.clone(),
            commands,
            handler_calls,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    /// 从 JSON 解码并验证矩阵；未知字段或任何 parity 规则失败都会拒绝。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let matrix: Self = serde_json::from_value(value.clone())
            .map_err(|_| "entrypoint_parity_matrix_decode_failed".to_owned())?;
        matrix.validate()?;
        Ok(matrix)
    }

    /// 将矩阵编码为 JSON；只产生展示/传输值，不改变控制面事实。
    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "entrypoint_parity_matrix_encode_failed".to_owned())
    }

    /// 检查矩阵头部、入口唯一性、跨入口绑定、deny-first handler 计数和 digest。
    ///
    /// 通过只表示输入观察值一致，不表示四个入口都真的跑过 provider 或已产生 receipt。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ENTRYPOINT_PARITY_MATRIX_SCHEMA
            || !self.version.is_compatible_with(&ENTRYPOINT_PARITY_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.commands.is_empty()
            || self.commands.len() > MAX_PARITY_COMMANDS
        {
            return Err("entrypoint_parity_matrix_header_invalid".to_owned());
        }
        let mut entrypoints = BTreeSet::new();
        for command in &self.commands {
            command.validate()?;
            if command.request_id != self.request_id
                || command.command_digest != self.command_digest
            {
                return Err("entrypoint_parity_command_mismatch".to_owned());
            }
            if !entrypoints.insert(format!("{:?}", command.entrypoint)) {
                return Err("entrypoint_parity_duplicate_entrypoint".to_owned());
            }
        }
        if self
            .commands
            .iter()
            .any(|command| matches!(command.decision, EntrypointDecision::Denied))
            && self.handler_calls != 0
        {
            return Err("entrypoint_parity_denied_handler_effect".to_owned());
        }
        validate_digest(&self.command_digest, "entrypoint_parity_command_digest")?;
        validate_digest(&self.matrix_digest, "entrypoint_parity_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("entrypoint_parity_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算不包含自身 `matrix_digest` 的矩阵 canonical digest。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "command_digest": self.command_digest,
            "commands": self.commands,
            "handler_calls": self.handler_calls,
        }))
    }
}

/// 对 RequestContext 中会影响命令边界的字段计算摘要。
///
/// 这不是把完整 context 变成授权票据；它只让不同 project/role/path/trust 上下文的 command
/// envelope 不能悄悄共享同一个 context digest。真正的 context 认证和权限判断仍在上层。
fn context_digest(context: &RequestContext) -> String {
    json_digest(&json!({
        "request_id": context.request_id,
        "session_id": context.session_id,
        "actor_id": context.actor_id,
        "project_root": context.project_root,
        "project_trusted": context.project_trusted,
        "permission_profile": context.permission_profile,
        "role_id": context.role_id,
        "department_id": context.department_id,
        "work_packet_id": context.work_packet_id,
        "cell_id": context.cell_id,
        "path_allow": context.path_allow,
    }))
}

/// 检查固定 `sha256:` 摘要的文本形状；不提供签名或来源认证。
fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
