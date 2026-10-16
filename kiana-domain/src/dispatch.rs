//! 派发许可（DispatchPermit）的领域数据结构、JSON 边界和静态校验规则。
//!
//! # 在执行主路径中的位置
//!
//! ```text
//! RequestEnvelope / CapabilityRequest
//!     -> ControlPlane 完成当前策略、关卡、审批和权限检查
//!     -> kiana-core::dispatch::dispatch_authorized 构造 DispatchPermit
//!     -> EventStore 原子提交 execution.prepared（许可事实成为 journal 记录）
//!     -> Broker 的 JournalPermitVerifier 读回并校验许可、请求、项目身份、时限和授权版本
//!     -> CAS 消费 permit 后才允许进入 capability handler
//!     -> Core 再单独提交执行结果并交回 Runner
//! ```
//!
//! 本文件只定义可序列化 DTO 和纯校验/摘要函数，不自行授权、不访问 EventStore，也不调用 Broker。
//! `DispatchPermit` 是 Core 与 Broker 之间携带本次授权快照的凭证；真正的执行权来自 journal 中可被
//! 重新读取并一次性消费的 `execution.prepared` 事实，而不是仅凭内存中的 DTO 或它的 digest。准备、
//! 外部 handler 执行、结果提交是分阶段操作，不构成跨 EventStore 和外部副作用的单个事务。
//!
//! Serde（Rust 序列化/反序列化库）负责把 DTO 转成 JSON 或从 JSON 恢复；`deny_unknown_fields` 会让
//! 当前结构未声明的额外字段导致解码失败。digest 是无密钥的规范 JSON SHA-256 指纹，用于检查字段是否
//! 与快照一致，不是签名、来源认证或权限授予。
use crate::{
    AggregateVersion, ApprovalId, CapabilityRequest, ExecutionId, InvocationId, RequestContext,
    RequestId, RunId, SchemaVersion, TurnId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// DispatchPermit 在线路 JSON 中使用的 schema 标识。
pub const DISPATCH_PERMIT_SCHEMA: &str = "kiana.dispatch-permit.v1";
/// 当前 Core 生成 permit 时使用的 schema 版本；同 major 的 minor 版本按 `SchemaVersion` 规则兼容。
pub const DISPATCH_PERMIT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 一次已经通过 ControlPlane 授权检查、准备交给 Broker 验证消费的不可变许可快照。
///
/// 这不是长期授权：许可有时限，并绑定 request/action/project/authority 版本。反序列化时拒绝未知字段，
/// 以免新旧契约中的未理解字段被静默忽略。它包含的是授权时观察到的事实；是否仍可使用必须在 Broker
/// 侧读取当前 journal 和状态后重新核验。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchPermit {
    /// 必须是本模块固定的 permit schema 名称。
    pub schema: String,
    /// 许可 DTO 的 major/minor schema 版本。
    pub version: SchemaVersion,
    /// 这次将要执行的 handler 调用身份；不是 run ID，也不直接标识用户会话。
    pub execution_id: ExecutionId,
    /// 本次能力调用在 Core 中的 invocation 身份；validate 要求其 UUID 与 request_id 的 UUID 相同。
    pub invocation_id: InvocationId,
    /// 原始 CapabilityRequest 的唯一身份；用于派生准备命令并绑定执行动作。
    pub request_id: RequestId,
    /// 请求属于 Harness Run 时的 Run ID；直接命令可为空。
    pub run_id: Option<RunId>,
    /// Harness Run 中当前活动轮次的 Turn ID；必须与 run_id 同时存在或同时为空。
    pub turn_id: Option<TurnId>,
    /// ControlPlane 对请求作出的决策身份；这里只保存 ID，不重做 Policy/Gate 判定。
    pub decision_id: String,
    /// 如果执行依据额外审批则保存 Approval ID；为空不代表本 DTO 自己证明了不需审批。
    pub approval_id: Option<ApprovalId>,
    /// 当时请求的会话、项目、主体、权限档位和 scope 上下文快照；消费方仍需核验其与请求和当前状态一致。
    pub context: RequestContext,
    /// 规范化后 capability action 的 SHA-256 摘要；用于阻止消费时替换操作或参数。
    pub action_digest: String,
    /// Core 构造许可时观察到的项目根身份 JSON；格式检查只要求它是 object，具体比较由消费方完成。
    pub project_identity: serde_json::Value,
    /// 准备许可依赖的 authority 聚合版本快照；消费前必须与 EventStore 当前版本比较。
    pub authority_versions: Vec<AggregateVersion>,
    /// Core 签发许可时的 Unix epoch 毫秒时间；0 被视为无效。
    pub issued_at_unix_ms: u64,
    /// 许可失效的 Unix epoch 毫秒边界；now 等于此值时已经过期。
    pub expires_at_unix_ms: u64,
    /// 对其他许可字段计算的 SHA-256 内容摘要；不含自身，也不是密钥签名。
    pub permit_digest: String,
}

impl DispatchPermit {
    /// 将 JSON 值反序列化为许可，并立即执行领域字段及 digest 校验。
    ///
    /// 【调用者】Core 的 `JournalPermitVerifier` 从 `execution.prepared` 事件读回 permit 时调用；
    /// 结果投递验证也会读回同一条准备事实。
    ///
    /// 【输入/输出】接收 JSON `Value` 的借用，克隆后交给 Serde；解码成功且 `validate()` 通过才返回结构体。
    /// 未知字段、类型不匹配或许可字段无效分别变成 decode/validate 错误字符串。
    ///
    /// 【副作用】只在内存中解码和重算摘要，不访问当前授权、时间、EventStore 或文件系统；通过仅说明
    /// 载荷内部有效，Broker 后续仍须执行 `validate_for_request()` 及 journal 版本/一次性消费检查。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let permit: Self = serde_json::from_value(value.clone())
            .map_err(|_| "dispatch_permit_decode_failed".to_owned())?;
        permit.validate()?;
        Ok(permit)
    }

    /// 把当前结构序列化成 JSON `Value`，供 Core 写入 `execution.prepared` 等事件载荷。
    ///
    /// 本方法只处理编码，不先调用 `validate()`；构造者应在写 journal 前显式验证许可。编码失败返回
    /// `dispatch_permit_encode_failed`，不会写 EventStore 或修正对象。
    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "dispatch_permit_encode_failed".to_owned())
    }

    /// 检查许可内部的 schema、身份、时间形状、版本列表和自带 digest。
    ///
    /// 【调用者/输入】Core 构造许可后以及 Broker 从 journal 解码许可时调用。此纯函数只看当前对象。
    ///
    /// 【校验内容】检查 schema/兼容 major、非空 UUID 身份和决策 ID、有效期限、Run/Turn 配对、版本列表
    /// 非空、上下文必要字段与项目身份 JSON 形状；要求 invocation UUID 从 request UUID 派生；检查
    /// action/permit digest 格式、authority 聚合键唯一，并拒绝 version=0 的 authority 项；最后重算摘要。
    /// 其他聚合版本是否为零由对应聚合语义决定，本函数只额外限制 authority。
    ///
    /// 【输出/失败】全部规则通过才返回 `Ok(())`；否则返回具体 `dispatch_permit_*` 错误。这里不把错误
    /// 转为用户消息、不重试、不记录事件、不检查 permit 是否已消费，也不查询实时 authority。
    ///
    /// ⚠ 自摘要和对象形状不能证明 permit 是 Core 签发的；来源/版本/一次性消费证明来自后续 journal
    /// 读取和 CAS 流程。不要单独用本方法结果允许 handler 执行。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DISPATCH_PERMIT_SCHEMA
            || !self.version.is_compatible_with(&DISPATCH_PERMIT_VERSION)
            || self.execution_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.request_id.as_uuid().is_nil()
            || self.decision_id.trim().is_empty()
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.run_id.is_some() != self.turn_id.is_some()
            || self.authority_versions.is_empty()
        {
            return Err("dispatch_permit_header_invalid".to_owned());
        }
        if self.context.request_id.as_uuid().is_nil()
            || self.context.session_id.is_empty()
            || self.context.project_root.trim().is_empty()
            || !self.project_identity.is_object()
        {
            return Err("dispatch_permit_context_invalid".to_owned());
        }
        if self.invocation_id != InvocationId::from_uuid(self.request_id.as_uuid()) {
            return Err("dispatch_permit_invocation_request_mismatch".to_owned());
        }
        validate_digest(&self.action_digest, "dispatch_permit_action_digest")?;
        let mut seen = std::collections::BTreeSet::new();
        for version in &self.authority_versions {
            version.validate().map_err(str::to_owned)?;
            if !seen.insert((
                version.aggregate_type.as_str(),
                version.aggregate_id.as_str(),
            )) || (version.aggregate_type == "authority" && version.version == 0)
            {
                return Err("dispatch_permit_authority_versions_invalid".to_owned());
            }
        }
        validate_digest(&self.permit_digest, "dispatch_permit_digest")?;
        if self.permit_digest != self.digest() {
            return Err("dispatch_permit_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 把许可快照绑定到当前请求、项目身份和消费时钟。
    ///
    /// 【调用者】Broker 的 `JournalPermitVerifier::verify_and_consume()` 在实际 handler 前调用；Core
    /// 负责先从 permit 的项目根路径重新读取 canonical path/device/inode 等身份，再把该观察值传入。
    ///
    /// 【输入】`request` 是即将进入 Broker 的能力请求；`project_identity` 是本次重新观察的项目根身份；
    /// `now_unix_ms` 是 Broker 当前信任的 Unix 毫秒时间。
    ///
    /// 【核心流程】先复用 `validate()`，再要求 permit request_id、action digest 和 project identity 与
    /// 当前值一致；最后要求当前时刻处于半开区间 `[issued_at_unix_ms, expires_at_unix_ms)`。
    ///
    /// 【失败/副作用】任何作用域或时限不匹配统一返回
    /// `dispatch_permit_scope_or_expiry_mismatch`；不修改许可、不消费 journal 记录、不重试。Broker
    /// 外层随后还需逐项比较 `authority_versions`，并用 CAS 标记 permit 已消费；因此通过这里仍不是最终放行。
    pub fn validate_for_request(
        &self,
        request: &CapabilityRequest,
        project_identity: &Value,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.request_id != request.request_id
            || self.action_digest != crate::capability_action_digest(request)
            || &self.project_identity != project_identity
            || now_unix_ms < self.issued_at_unix_ms
            || now_unix_ms >= self.expires_at_unix_ms
        {
            return Err("dispatch_permit_scope_or_expiry_mismatch".to_owned());
        }
        Ok(())
    }

    /// 规范序列化全部决定许可含义的字段（不含 permit_digest）并计算 SHA-256。
    ///
    /// `json_digest()` 对 JSON object key 使用规范顺序，避免仅因 map 插入顺序不同而改变摘要。列表字段
    /// 如 `authority_versions` 的顺序仍是内容的一部分；该 digest 用于完整性比对，不提供签名认证。
    pub fn digest(&self) -> String {
        crate::json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "execution_id": self.execution_id,
            "invocation_id": self.invocation_id,
            "request_id": self.request_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "decision_id": self.decision_id,
            "approval_id": self.approval_id,
            "context": self.context,
            "action_digest": self.action_digest,
            "project_identity": self.project_identity,
            "authority_versions": self.authority_versions,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// 校验内部字段名对应的 SHA-256 文本格式，并将失败映射为带字段前缀的稳定错误码。
///
/// 算法前缀必须精确为小写 `sha256:`，十六进制部分接受大小写；这里不计算摘要对应的数据。
fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 以用途和主体共同派生确定性 request ID，供 journal 命令重放和恢复时重新定位同一命令。
///
/// 用途字符串与主体字符串之间插入一个 NUL 字节，避免简单拼接时边界不明；SHA-256 的前 16 字节
/// 被调整成合法 UUID 的 version/variant 位后封装为 `RequestId`。同一组 purpose/subject 每次得到相同
/// ID，不依赖随机数或墙钟；用途不同则输入不同。这个 ID 派生函数不写 journal，也不授权命令。
pub fn derived_request_id(purpose: &str, subject: &str) -> RequestId {
    let mut hash = Sha256::new();
    hash.update(purpose.as_bytes());
    hash.update([0]);
    hash.update(subject.as_bytes());
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    RequestId::from_uuid(uuid::Uuid::from_bytes(bytes))
}
