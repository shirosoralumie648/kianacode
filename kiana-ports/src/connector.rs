//! Narrow connector transport ports.
//!
//! These traits are deliberately below the ControlPlane.  They receive only server-owned
//! admission material and return bounded observations.  They do not expose an EventStore,
//! approval store, raw credential bytes, or a second authorization path.
//!
//! # 这个文件在系统里的位置
//!
//! 连接器（connector）是 Kiana 与外部世界通信的适配层：HTTP 调用、Webhook 接收、
//! MCP 握手、凭据探测、效果观测。本文件定义这些能力的**端口契约**。
//!
//! ```text
//! kiana-core::ControlPlane
//!        ↓ 签发 ConnectorPreparedPermit（连接器许可）+ CredentialLease（凭据租约）
//! 【本文件：ConnectorAdapter 等四个 trait】   ← 只声明接口
//!        ↓
//!    具体适配器实现（HTTP / MCP / Webhook …）
//!        ↓
//!    外部服务
//! ```
//!
//! # 最重要的一条：适配器在结构上无法扩大自己的权限
//!
//! 文件头说这些 trait "receive only server-owned admission material and return
//! bounded observations"（只接收服务端持有的准入材料，返回有界的观测结果）。
//!
//! 展开说，一个连接器适配器**能做的**只有两件事：
//! 1. 拿着服务端给的许可去做服务端已经批准的事；
//! 2. 把结果以**有界**的形式报回去。
//!
//! 它**不能**做的（文件头列了四条禁令）：
//! - 不接触事件账本（`EventStore`）—— 不能自己记录事实；
//! - 不接触审批存储（approval store）—— 不能自己批准自己；
//! - 拿不到凭据原文（raw credential bytes）—— 只有租约，不是密钥；
//! - **没有第二条授权路径** —— 唯一的授权来自服务端签发的许可。
//!
//! 最后一条是根本性的。即使适配器想绕过授权，接口上也没有对应的方法可调。
//! **安全性来自接口的形状，而不是来自实现者的自觉。**
//!
//! # 为什么"有界的观测结果"很重要
//!
//! 适配器返回的不是原始响应，而是 [`EffectObservation`] 这类结构化、有大小上限、
//! 带摘要的结构。这意味着：
//! - 适配器不能通过"返回一个巨大的错误页面"来耗尽服务端内存；
//! - 返回的内容带摘要，服务端可以验证它没被中途改写；
//! - 观测结果不含凭据原文，泄漏面被压到最小。
//!
//! # 术语
//!
//! - **Port（端口）**：架构抽象边界，由核心声明、由外层实现。
//! - **ConnectorPreparedPermit（连接器许可）**：服务端签发的执行凭证。
//!   一次调用一张，用完即废。
//! - **CredentialLease（凭据租约）**：一份**限时、限范围**的凭据使用许可，
//!   本身不含密钥原文。
//! - **bounding（限界）**：对大小、数量、时间等设定硬上限。
//! - **EffectObservation（效果观测）**：一次外部调用的结构化结果记录。
//! - **fail-closed**：证据不足时拒绝。
//!
//! # 上游契约
//!
//! These traits are deliberately below the ControlPlane.  They receive only server-owned
//! admission material and return bounded observations.  They do not expose an EventStore,
//! approval store, raw credential bytes, or a second authorization path.

use crate::PortError;
use async_trait::async_trait;
use kiana_domain::{
    json_digest, provider_payload_hash_valid, valid_extension_identifier, ConnectorBindingSnapshot,
    CredentialLease, EffectObservation, InvocationId, McpCapabilityHandshake, ProviderReceipt,
    StopReport, WorkflowEventIngress, WorkflowEventOccurrence, WorkflowEventSourcePolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub use kiana_domain::ConnectorHealthStatus;

/// 连接器端口契约的 schema 标识符。
///
pub const CONNECTOR_PORT_CONTRACT_SCHEMA: &str = "kiana.connector-port-contract.v1";
pub const CONNECTOR_PREPARED_PERMIT_SCHEMA: &str = "kiana.connector-prepared-permit.v1";
pub const CONNECTOR_PAYLOAD_SCHEMA: &str = "kiana.connector-canonical-payload.v1";
pub const CONNECTOR_HEALTH_SCHEMA: &str = "kiana.connector-health.v1";
pub const CONNECTOR_PROBE_RESULT_SCHEMA: &str = "kiana.connector-probe-result.v1";
pub const CONNECTOR_OBSERVATION_REQUEST_SCHEMA: &str = "kiana.connector-observation-request.v1";
pub const CONNECTOR_CANCEL_REQUEST_SCHEMA: &str = "kiana.connector-cancel-request.v1";
pub const CONNECTOR_WEBHOOK_REQUEST_SCHEMA: &str = "kiana.connector-webhook-request.v1";
pub const CONNECTOR_VERIFIED_WEBHOOK_SCHEMA: &str = "kiana.connector-verified-webhook.v1";
pub const CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA: &str = "kiana.connector-mcp-handshake-request.v1";
/// 连接器载荷（payload）的最大字节数：64 KiB。
///
/// 【为什么是 64 KiB】
/// 连接器传的是结构化的 API 请求体，不是文件。
/// 64 KiB 足够表达一个复杂的 API 请求，同时防止有人把超大 JSON
/// 塞进连接器通道耗尽服务端内存。
///
/// ⚠ 注意：文件传输不走这个通道。连接器只传"引用"和"参数"。
///
pub const CONNECTOR_PAYLOAD_MAX_BYTES: usize = 64 * 1024;
/// 一个连接器适配器最多能声明多少项"限制"。
///
/// 【为什么需要这个上限】
/// 适配器可以声明自己的已知限制（不支持并发、不支持流式等）。
/// 16 项足够表达一个复杂的适配器，同时防止声明列表被撑爆。
///
pub const CONNECTOR_LIMITATIONS_MAX: usize = 16;

/// Capabilities are descriptive only.  A capability never grants an account scope or an
/// approval; the checked methods below reject calls when the implementation has not registered
/// the corresponding operation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 连接器适配器的能力项。
///
/// 【作用】
/// 一个封闭的能力枚举。适配器通过 [`ConnectorAdapterCapabilities`]
/// 声明自己支持哪些。
///
/// 【⚠ 为什么用封闭枚举而不是字符串】
/// 如果能力名是任意字符串，那么拼错一个名字（比如 `"streming"`
/// 而不是 `"streaming"`）就会静默失效 —— 适配器以为自己声明了流式支持，
/// 系统却认为它不支持。
///
/// 封闭枚举让拼错在编译期就变成错误。
///
pub enum ConnectorAdapterCapability {
    Discover,
    ReadOnlyProbe,
    Invoke,
    ObserveReceipt,
    Cancel,
    WebhookVerify,
    CapabilityHandshake,
}

impl ConnectorAdapterCapability {
    /// 能力项的稳定字符串形式。
    ///
    /// 【⚠ 这些字符串是对外契约】
    /// 会进事件日志和适配器注册表。下游可能按它们分类，改写会导致静默失效。
    ///
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::ReadOnlyProbe => "read_only_probe",
            Self::Invoke => "invoke",
            Self::ObserveReceipt => "observe_receipt",
            Self::Cancel => "cancel",
            Self::WebhookVerify => "webhook_verify",
            Self::CapabilityHandshake => "capability_handshake",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorAdapterCapabilities {
    #[serde(default)]
    pub capabilities: BTreeSet<ConnectorAdapterCapability>,
}

impl ConnectorAdapterCapabilities {
    /// 从一个能力列表构造能力集合。
    ///
    /// 【作用】
    /// 便捷构造函数，把迭代器转成 [`ConnectorAdapterCapabilities`]。
    ///
    pub fn from_iter(capabilities: impl IntoIterator<Item = ConnectorAdapterCapability>) -> Self {
        Self {
            capabilities: capabilities.into_iter().collect(),
        }
    }

    /// 这个适配器是否支持某项能力。
    ///
    /// 【作用】
    /// 纯查询，不报错。用于"要不要走这条路径"的分支判断。
    ///
    pub fn supports(&self, capability: ConnectorAdapterCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// 要求适配器必须支持某项能力，否则报错。
    ///
    /// 【⚠ 与 `supports` 的区别】
    /// `supports` 返回布尔值，让调用方自己决定；
    /// `require` 直接报错，适合"缺了这项就没法继续"的场景。
    ///
    /// 用 `require` 而不是 `if !supports { return Err }` 手动写，
    /// 是因为错误码是稳定的，直接在端口层给出比让每个调用方自己编更可靠。
    ///
    pub fn require(&self, capability: ConnectorAdapterCapability) -> Result<(), PortError> {
        if self.supports(capability) {
            Ok(())
        } else {
            Err(PortError::Unavailable(format!(
                "connector_capability_missing:{}",
                capability.as_str()
            )))
        }
    }

    /// 校验这组能力声明自身是否合法。
    ///
    /// 【核心检查】
    /// - 能力列表不能超过 16 项；
    /// - 不能有重复项（重复声明会让语义变得有歧义）。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.capabilities.len() > 16 {
            return Err(PortError::Failed(
                "connector_capabilities_too_many".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Immutable adapter metadata.  It is a discovery projection, not an authority grant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 连接器适配器的描述符 —— 适配器向系统"自我介绍"的结构。
///
/// 【作用】
/// 一个适配器实现需要先注册成 [`ConnectorAdapterDescriptor`]，
/// 系统才知道"存在这样一个适配器，它能做这些事"。
///
/// 【⚠ 描述符里没有实现代码，只有元数据】
/// 它描述的是"是什么"（哪个连接器、什么版本、支持什么能力），
/// 不是"怎么做"。具体怎么做由实现 [`ConnectorAdapter`] 的类型负责。
///
/// 这种分离让系统可以在不加载实现的情况下，
/// 先检查"这个适配器存不存在、够不够用"。
///
pub struct ConnectorAdapterDescriptor {
    pub schema: String,
    pub connector_id: String,
    pub version: String,
    pub transport: String,
    pub capabilities: ConnectorAdapterCapabilities,
    pub descriptor_digest: String,
}

impl ConnectorAdapterDescriptor {
    /// 构造一个适配器描述符。
    ///
    /// 【作用】
    /// 组装并校验一个描述符。
    ///
    pub fn new(
        connector_id: impl Into<String>,
        version: impl Into<String>,
        transport: impl Into<String>,
        capabilities: ConnectorAdapterCapabilities,
    ) -> Result<Self, PortError> {
        let mut descriptor = Self {
            schema: CONNECTOR_PORT_CONTRACT_SCHEMA.to_owned(),
            connector_id: connector_id.into(),
            version: version.into(),
            transport: transport.into(),
            capabilities,
            descriptor_digest: String::new(),
        };
        descriptor.descriptor_digest = descriptor.digest();
        descriptor.validate()?;
        Ok(descriptor)
    }

    /// 校验描述符自身是否合法。
    ///
    /// 【核心检查】
    /// - 各标识字段符合 `valid_extension_identifier` 的要求；
    /// - 能力集合自身合法（见 `ConnectorAdapterCapabilities::validate`）；
    /// - 描述符合法、不超长、不含控制字符。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_PORT_CONTRACT_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.version)
            || self.transport.trim().is_empty()
            || self.transport.len() > 128
            || self.transport.contains(['\0', '\r', '\n'])
        {
            return Err(PortError::Failed(
                "connector_adapter_descriptor_invalid".to_owned(),
            ));
        }
        self.capabilities.validate()?;
        if self.descriptor_digest != self.digest() {
            return Err(PortError::Failed(
                "connector_adapter_descriptor_digest_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// 计算描述符的摘要。
    ///
    /// 【作用】
    /// 让描述符可以被比对和记录。摘要相同意味着是同一个描述符。
    ///
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "version": self.version,
            "transport": self.transport,
            "capabilities": self.capabilities,
        }))
    }
}

/// Canonical payload supplied after schema, data-class and policy validation.  The adapter may
/// need the value to perform an operation, but the value is never returned by a port as
/// credential material and is not accepted as an authority input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 规范化后的连接器载荷。
///
/// 【作用】
/// 封装一份"已经规范化"的请求体。
///
/// 【为什么要"规范化"】
/// 不同来源的 JSON 可能有不同的键顺序、无意义的空白、
/// 数字与字符串的表示差异。这些差异会让摘要不同，
/// 从而让幂等判断失效。
///
/// 规范化把同一份逻辑内容压成唯一的字节表示，
/// 保证"同样的请求 → 同样的摘要"。
///
pub struct CanonicalConnectorPayload {
    pub schema: String,
    pub value: Value,
    pub payload_digest: String,
}

impl CanonicalConnectorPayload {
    /// 规范化并封装一份载荷。
    ///
    /// 【失败情况】
    /// 超过 `CONNECTOR_PAYLOAD_MAX_BYTES`（64 KiB）即报错。
    ///
    pub fn new(value: Value) -> Result<Self, PortError> {
        let mut payload = Self {
            schema: CONNECTOR_PAYLOAD_SCHEMA.to_owned(),
            value,
            payload_digest: String::new(),
        };
        payload.payload_digest = json_digest(&payload.value);
        payload.validate()?;
        Ok(payload)
    }

    /// 校验载荷本身是否合法。
    ///
    /// 【核心检查】
    /// - 大小不超上限；
    /// - 必须是合法的 JSON 值（不含 NaN、无穷大这类非 JSON 数值）；
    /// - 深度不超限（防止深层嵌套的 JSON 造成栈溢出）。
    ///
    /// ⚠ 深度检查容易被忽略：一个形如 `{"a":{"a":{"a":...}}}` 的载荷
    /// 可以嵌套上万层，序列化时可能耗尽调用栈。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        let bytes = serde_json::to_vec(&self.value)
            .map_err(|_| PortError::Failed("connector_payload_invalid".to_owned()))?;
        if self.schema != CONNECTOR_PAYLOAD_SCHEMA
            || bytes.len() > CONNECTOR_PAYLOAD_MAX_BYTES
            || self.payload_digest != json_digest(&self.value)
        {
            return Err(PortError::Failed(
                "connector_canonical_payload_invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Effect-time permit produced by the ControlPlane.  The adapter cannot mint or widen this
/// object; it only receives the already bound connector/version/account/operation and digests.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 连接器许可 —— 本文件最核心的类型。
///
/// 【作用】
/// 服务端签发给适配器的一次性执行凭证。
/// 适配器拿到它，才能执行一次具体的连接器调用。
///
/// 【为什么需要"许可"这种间接层】
/// 因为它把"被允许做什么"和"实际做什么"分开。
/// 适配器不能自己决定调用什么 ——
/// 它只能执行许可里明确写明的那一次调用。
///
/// 【关键字段】
/// - `connector_id` / `binding_id` / `account_id` —— 调用的是谁、走哪个绑定、用哪个账号；
/// - `operation` —— 执行哪个操作；
/// - `invocation_id` —— **本次调用的唯一 ID**，用于把结果对回这次调用；
/// - `attempt` —— 第几次尝试（重试时递增）；
/// - `authority_epoch` —— 授权世代；
/// - `issued_at_unix_ms` / `expires_at_unix_ms` —— **有效期**；
/// - `action_digest` / `payload_digest` / `idempotency_key_digest` —— 三个内容摘要；
/// - `permit_digest` —— 许可自身的摘要。
///
/// 【⚠ 三个摘要各防什么】
/// - `action_digest` —— 这次调用"要做什么"没被改；
/// - `payload_digest` —— 请求内容没被改；
/// - `idempotency_key_digest` —— 幂等键没被换。
///   ⚠ 存的是幂等键的**摘要**而不是原文，
///   因为幂等键可能包含可推测的信息，直接存储有泄漏风险。
///
/// 【⚠ 为什么有效期是硬性的】
/// `expires_at_unix_ms` 让许可**自动失效**。
/// 即使一张许可被泄漏了，攻击者也只能在有效期内使用它。
///
/// 没有有效期的话，一张泄漏的许可就是永久凭证。
///
pub struct ConnectorPreparedPermit {
    pub schema: String,
    pub connector_id: String,
    pub connector_version: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub action_digest: String,
    pub payload_digest: String,
    pub idempotency_key_digest: String,
    pub authority_epoch: u64,
    pub policy_revision: String,
    pub binding_revision: u64,
    pub credential_generation: u64,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub permit_digest: String,
}

impl ConnectorPreparedPermit {
    /// 校验许可是否合法、是否过期。
    ///
    /// 【核心检查 —— 分三组】
    ///
    /// ① 标识字段
    /// 所有标识必须符合 `valid_extension_identifier`；
    /// `invocation_id` 不能是全零 UUID；
    /// `attempt` 必须大于 0（第 0 次尝试不存在）；
    /// `authority_epoch` 非 0（0 不代表任何真实世代）；
    /// `binding_revision` 非 0。
    ///
    /// ② 有效期
    /// - `issued_at_unix_ms` 非 0；
    /// - `expires_at_unix_ms` **必须大于** `issued_at_unix_ms`
    ///   （⚠ 用 `<=` 拒绝，意味着有效期为零的许可不被接受 ——
    ///   那张许可一签发就过期，没有意义）。
    ///
    /// ③ 摘要
    /// 四个摘要都必须格式合法，
    /// 且 `permit_digest` 必须与重算值一致。
    ///
    /// 【⚠ 关于可选的 `now_unix_ms` 参数】
    /// 传 `Some(now)` 才会检查是否处于有效期内；
    /// 传 `None` 则**跳过时效检查**，只做格式校验。
    ///
    /// 这个设计让"先校验格式、稍后再校验时效"成为可能 ——
    /// 比如适配器在准备阶段先验一次格式，真正发送前再验一次时效。
    ///
    /// 【⚠ 时效检查用的是 `now < issued || now >= expires`】
    /// 注意用的是 `>=` 排除到期时刻：**到期那一毫秒就已经失效了**。
    /// 如果写成 `now > expires`，许可会多活 1 毫秒。
    ///
    /// 【失败情况】
    /// 全部不合法或已过期，返回同一个原因码
    /// `connector_prepared_permit_invalid_or_expired`。
    ///
    /// ⚠ 注意格式错误和过期用的是**同一个码**。
    /// 这是有意的：不区分"这张许可坏了"和"这张许可用完了"，
    /// 避免通过错误码探测许可状态。
    ///
    pub fn validate(&self, now_unix_ms: Option<u64>) -> Result<(), PortError> {
        if self.schema != CONNECTOR_PREPARED_PERMIT_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.connector_version)
            || !valid_extension_identifier(&self.binding_id)
            || !valid_extension_identifier(&self.account_id)
            || !valid_extension_identifier(&self.operation)
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || self.authority_epoch == 0
            || self.policy_revision.trim().is_empty()
            || self.policy_revision.len() > 256
            || self.binding_revision == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.idempotency_key_digest)
            || !valid_digest(&self.permit_digest)
            || self.permit_digest != self.digest()
            || now_unix_ms
                .is_some_and(|now| now < self.issued_at_unix_ms || now >= self.expires_at_unix_ms)
        {
            return Err(PortError::Conflict(
                "connector_prepared_permit_invalid_or_expired".to_owned(),
            ));
        }
        Ok(())
    }

    /// 计算许可的摘要。
    ///
    /// 【⚠ 摘要字段自身不参与计算】
    /// 它不出现在 `json!` 里，否则要算自己，无解。
    ///
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "connector_version": self.connector_version,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "action_digest": self.action_digest,
            "payload_digest": self.payload_digest,
            "idempotency_key_digest": self.idempotency_key_digest,
            "authority_epoch": self.authority_epoch,
            "policy_revision": self.policy_revision,
            "binding_revision": self.binding_revision,
            "credential_generation": self.credential_generation,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }

    pub(crate) fn validate_for_binding(
        &self,
        binding: &ConnectorBindingSnapshot,
        payload: &CanonicalConnectorPayload,
        lease: &CredentialLease,
    ) -> Result<(), PortError> {
        self.validate(None)?;
        binding
            .validate()
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        binding
            .operation(&self.operation)
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        payload.validate()?;
        if self.connector_id != binding.definition.connector_id
            || self.connector_version != binding.definition.version
            || self.binding_id != binding.binding.binding_id
            || self.account_id != binding.binding.account_id
            || self.binding_revision != binding.revision
            || self.payload_digest != payload.payload_digest
            || binding.binding.credential_ref.as_ref() != Some(&lease.secret_ref)
            || lease.secret_ref.generation != self.credential_generation
        {
            return Err(PortError::Conflict(
                "connector_prepared_permit_binding_mismatch".to_owned(),
            ));
        }
        lease
            .validate_at(self.issued_at_unix_ms)
            .map_err(|error| PortError::Conflict(format!("connector_credential_lease:{error}")))
    }
}

/// Health is a redacted status projection.  Provider response bodies and credential material do
/// not cross this port.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 连接器的健康状态。
///
/// 【作用】
/// 描述"这个连接器现在能不能用"，供运维和 UI 展示。
///
/// 【⚠ 它是观测数据，不是授权依据】
/// 健康状态只用于展示和告警。
/// **绝不能因为"健康"就跳过授权检查。**
///
pub struct ConnectorHealth {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub status: ConnectorHealthStatus,
    pub checked_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ConnectorHealth {
    /// 构造一个健康状态记录。
    ///
    pub fn new(
        connector_id: impl Into<String>,
        binding_id: impl Into<String>,
        status: ConnectorHealthStatus,
        checked_at_unix_ms: u64,
        evidence_digest: Option<String>,
        limitations: Vec<String>,
    ) -> Result<Self, PortError> {
        let health = Self {
            schema: CONNECTOR_HEALTH_SCHEMA.to_owned(),
            connector_id: connector_id.into(),
            binding_id: binding_id.into(),
            status,
            checked_at_unix_ms,
            evidence_digest,
            limitations,
        };
        health.validate()?;
        Ok(health)
    }

    /// 校验健康状态记录是否合法。
    ///
    /// 【核心检查】
    /// - 各标识字段合法；
    /// - 时间戳非零；
    /// - 状态字符串在允许的取值范围内；
    /// - 原因说明有长度上限（防止把整个错误堆栈塞进来）。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_HEALTH_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.binding_id)
            || self.checked_at_unix_ms == 0
            || self.limitations.len() > CONNECTOR_LIMITATIONS_MAX
            || self.limitations.iter().any(|value| {
                value.trim().is_empty() || value.len() > 512 || value.contains(['\0', '\r', '\n'])
            })
            || self
                .evidence_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return Err(PortError::Failed("connector_health_invalid".to_owned()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 凭据探测请求。
///
/// 【作用】
/// "检查一下这个账号的凭据还能不能用？"—— 只读操作，不产生副作用。
///
/// 【典型用途】
/// 配置完一个服务商账号后，验证凭据是否有效、是否过期、范围够不够。
///
pub struct CredentialProbeRequest {
    pub schema: String,
    pub binding: ConnectorBindingSnapshot,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<CredentialLease>,
    pub now_unix_ms: u64,
}

impl CredentialProbeRequest {
    /// 校验探测请求是否合法。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_PORT_CONTRACT_SCHEMA || self.now_unix_ms == 0 {
            return Err(PortError::Failed(
                "connector_probe_request_invalid".to_owned(),
            ));
        }
        self.binding
            .validate()
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        if self
            .binding
            .operation(&self.operation)
            .map_err(|error| PortError::Conflict(format!("connector_probe_operation:{error}")))?
            .risk()
            != kiana_domain::RiskLevel::ReadOnly
        {
            return Err(PortError::Failed(
                "connector_probe_must_be_read_only".to_owned(),
            ));
        }
        if let Some(lease) = &self.lease {
            lease.validate_at(self.now_unix_ms).map_err(|error| {
                PortError::Conflict(format!("connector_credential_lease:{error}"))
            })?;
            if self.binding.binding.credential_ref.as_ref() != Some(&lease.secret_ref) {
                return Err(PortError::Conflict(
                    "connector_probe_credential_binding_mismatch".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 凭据探测结果。
///
/// 【作用】
/// 探测的答案。**只包含状态，不包含凭据原文** ——
/// 这是本文件最重要的设计约束之一。
///
pub struct CredentialProbeResult {
    pub schema: String,
    pub status: ConnectorHealthStatus,
    pub health: ConnectorHealth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
}

/// Server-owned input for a stdio MCP capability handshake.  It contains no command, URL,
/// headers or scope grant; the trusted registry resolves those after the ControlPlane permit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// MCP 能力握手请求。
///
/// 【作用】
/// 与 MCP 服务建立连接时，交换双方支持哪些能力。
///
pub struct McpCapabilityHandshakeRequest {
    pub schema: String,
    pub binding: ConnectorBindingSnapshot,
    pub server: String,
    pub session_ref: String,
}

impl McpCapabilityHandshakeRequest {
    /// 校验握手请求是否合法。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA
            || !valid_extension_identifier(&self.server)
            || self.session_ref.trim().is_empty()
            || self.session_ref.len() > 256
            || self.session_ref.chars().any(char::is_control)
        {
            return Err(PortError::Failed(
                "connector_mcp_handshake_request_invalid".to_owned(),
            ));
        }
        self.binding
            .validate()
            .map_err(|error| PortError::Conflict(error.to_owned()))
    }
}

impl CredentialProbeResult {
    /// 校验探测结果是否合法。
    ///
    /// 【⚠ 关键检查：结果不含凭据】
    /// 结果里只有状态、过期时间、范围列表这类**元数据**。
    /// 如果这里出现了任何看起来像密钥的东西，校验必须失败。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_PROBE_RESULT_SCHEMA
            || self.status != self.health.status
            || self
                .credential_generation
                .is_some_and(|generation| generation == 0)
            || self
                .evidence_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return Err(PortError::Failed(
                "connector_probe_result_invalid".to_owned(),
            ));
        }
        self.health.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 效果观测请求。
///
/// 【作用】
/// "把刚才那次调用的实际效果告诉我"——
/// 用于确认外部系统的状态是否与预期一致。
///
pub struct EffectObservationRequest {
    pub schema: String,
    pub permit: ConnectorPreparedPermit,
    pub receipt: ProviderReceipt,
    pub owner_digest: String,
    pub audience_digest: String,
    pub observed_at_unix_ms: u64,
}

impl EffectObservationRequest {
    /// 校验观测请求是否合法。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_OBSERVATION_REQUEST_SCHEMA
            || !valid_digest(&self.owner_digest)
            || !valid_digest(&self.audience_digest)
            || self.observed_at_unix_ms == 0
            || self.receipt.connector_id != self.permit.connector_id
            || self.receipt.binding_id != self.permit.binding_id
            || self.receipt.account_id != self.permit.account_id
            || self.receipt.operation != self.permit.operation
            || json_digest(&serde_json::json!({
                "idempotency_key": self.receipt.idempotency_key
            })) != self.permit.idempotency_key_digest
        {
            return Err(PortError::Conflict(
                "connector_observation_binding_mismatch".to_owned(),
            ));
        }
        self.receipt.validate().map_err(|error| {
            PortError::Failed(format!("connector_provider_receipt_invalid:{error}"))
        })?;
        self.permit.validate(None)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 连接器取消请求。
///
/// 【作用】
/// 请求中止一次正在进行的外部调用。
///
pub struct ConnectorCancelRequest {
    pub schema: String,
    pub permit: ConnectorPreparedPermit,
    pub reason: String,
    pub requested_at_unix_ms: u64,
}

impl ConnectorCancelRequest {
    /// 校验取消请求是否合法。
    ///
    /// 【⚠ 取消请求也必须经过校验】
    /// 即使是要取消，也必须证明"你是这次调用的发起方"。
    /// 否则任何人都能取消别人的调用 —— 那本身就是一个拒绝服务漏洞。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_CANCEL_REQUEST_SCHEMA
            || self.reason.trim().is_empty()
            || self.reason.len() > 512
            || self.reason.contains(['\0', '\r', '\n'])
            || self.requested_at_unix_ms == 0
        {
            return Err(PortError::Failed(
                "connector_cancel_request_invalid".to_owned(),
            ));
        }
        self.permit.validate(None)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Webhook 验证请求。
///
/// 【作用】
/// 外部系统主动推送事件进来时，验证这次推送是不是真的。
///
/// 【⚠ 为什么外部推送需要验证】
/// Webhook 是**外部触发**的。任何人只要知道 URL，
/// 就可以构造一个假的推送。如果不验证，一个精心构造的假事件
/// 就能让 Kiana 执行未授权的操作。
///
/// 验证的是**签名**，证明推送确实来自声称的发送方。
///
pub struct WebhookVerificationRequest {
    pub schema: String,
    pub ingress: WorkflowEventIngress,
    pub policy: WorkflowEventSourcePolicy,
    pub now_unix_ms: u64,
}

impl WebhookVerificationRequest {
    /// 校验 Webhook 验证请求是否合法。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_WEBHOOK_REQUEST_SCHEMA || self.now_unix_ms == 0 {
            return Err(PortError::Failed(
                "connector_webhook_request_invalid".to_owned(),
            ));
        }
        self.ingress
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        self.policy
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))
    }
}

/// A verified webhook contains only the occurrence projection.  The signed payload remains at
/// the ingress/artifact boundary and cannot be returned by this port as a capability request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 验证通过的 Webhook 事件。
///
/// 【作用】
/// 经过签名验证、可以被信任的外部事件。
///
pub struct VerifiedWebhook {
    pub schema: String,
    pub occurrence: WorkflowEventOccurrence,
    pub verified_at_unix_ms: u64,
    pub verification_digest: String,
}

impl VerifiedWebhook {
    /// 从一个事件发生记录构造已验证的 Webhook。
    ///
    /// 【作用】
    /// 把"发生了什么"转成"可以驱动系统动作的凭据"。
    ///
    /// 【⚠ 必须先验证再构造】
    /// 这个函数只应该接受**已经验签通过**的输入。
    /// 如果它能接受未验证的原始输入，就等于绕过了验签环节。
    ///
    pub fn from_occurrence(
        occurrence: WorkflowEventOccurrence,
        verified_at_unix_ms: u64,
    ) -> Result<Self, PortError> {
        occurrence
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        let mut verified = Self {
            schema: CONNECTOR_VERIFIED_WEBHOOK_SCHEMA.to_owned(),
            occurrence,
            verified_at_unix_ms,
            verification_digest: String::new(),
        };
        verified.verification_digest = verified.digest();
        verified.validate()?;
        Ok(verified)
    }

    /// 校验已验证的 Webhook 是否自洽。
    ///
    /// 【核心检查】
    /// - 标识与签名格式合法；
    /// - 时间戳在合理范围内；
    /// - 自身摘要自洽。
    ///
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_VERIFIED_WEBHOOK_SCHEMA
            || self.verified_at_unix_ms == 0
            || !valid_digest(&self.verification_digest)
            || self.verification_digest != self.digest()
        {
            return Err(PortError::Failed(
                "connector_verified_webhook_invalid".to_owned(),
            ));
        }
        self.occurrence
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))
    }

    /// 计算已验证 Webhook 的摘要。
    ///
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "occurrence": self.occurrence,
            "verified_at_unix_ms": self.verified_at_unix_ms,
        }))
    }
}

/// Connector transport boundary.  The implementation receives no EventStore or approval
/// authority.  Every default operation is unsupported, and checked wrappers require an explicit
/// registered capability before invoking an implementation method.
#[async_trait]
/// 连接器适配器的主接口。
///
/// 【作用】
/// 一个适配器实现必须提供的能力。
///
/// 【⚠ 这个接口的形状就是安全边界】
/// 它能做的只有：声明能力、接收许可、执行调用、返回观测结果。
/// 它**不能**：
/// - 访问事件账本；
/// - 访问审批存储；
/// - 拿到凭据原文；
/// - 自己决定调用什么（那由许可决定）。
///
/// ⚠ 实现这个 trait 时，不要试图"帮忙"做授权判断。
/// 那不是适配器的职责，且会引入绕过控制面的风险。
///
pub trait ConnectorAdapter: Send + Sync {
    fn capabilities(&self) -> ConnectorAdapterCapabilities {
        ConnectorAdapterCapabilities::default()
    }

    async fn discover(
        &self,
        _binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorAdapterDescriptor, PortError> {
        Err(PortError::Unavailable(
            "connector_discover_unsupported".to_owned(),
        ))
    }

    async fn validate_binding(
        &self,
        _binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorHealth, PortError> {
        Err(PortError::Unavailable(
            "connector_binding_validation_unsupported".to_owned(),
        ))
    }

    async fn invoke(
        &self,
        _permit: ConnectorPreparedPermit,
        _lease: CredentialLease,
        _payload: CanonicalConnectorPayload,
    ) -> Result<ProviderReceipt, PortError> {
        Err(PortError::Unavailable(
            "connector_invoke_unsupported".to_owned(),
        ))
    }

    async fn cancel(&self, _request: ConnectorCancelRequest) -> Result<StopReport, PortError> {
        Err(PortError::Unavailable(
            "connector_cancel_unsupported".to_owned(),
        ))
    }

    async fn capability_handshake(
        &self,
        _request: McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        Err(PortError::Unavailable(
            "connector_capability_handshake_unsupported".to_owned(),
        ))
    }

    async fn discover_checked(
        &self,
        binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorAdapterDescriptor, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::Discover)?;
        let descriptor = self.discover(binding).await?;
        descriptor.validate()?;
        Ok(descriptor)
    }

    async fn validate_binding_checked(
        &self,
        binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorHealth, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::ReadOnlyProbe)?;
        let health = self.validate_binding(binding).await?;
        health.validate()?;
        Ok(health)
    }

    async fn invoke_checked(
        &self,
        permit: ConnectorPreparedPermit,
        lease: CredentialLease,
        payload: CanonicalConnectorPayload,
        binding: &ConnectorBindingSnapshot,
    ) -> Result<ProviderReceipt, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::Invoke)?;
        permit.validate_for_binding(binding, &payload, &lease)?;
        let receipt = self.invoke(permit.clone(), lease, payload).await?;
        validate_receipt_for_permit(&receipt, binding, &permit)?;
        Ok(receipt)
    }

    async fn cancel_checked(
        &self,
        request: ConnectorCancelRequest,
    ) -> Result<StopReport, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::Cancel)?;
        request.validate()?;
        let report = self.cancel(request).await?;
        report
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        Ok(report)
    }

    async fn capability_handshake_checked(
        &self,
        request: McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::CapabilityHandshake)?;
        request.validate()?;
        let handshake = self.capability_handshake(request).await?;
        handshake.validate().map_err(PortError::Failed)?;
        Ok(handshake)
    }
}

/// Effect observation is intentionally a separate port from dispatch.  It returns a typed,
/// digest-only observation and cannot append facts itself.
#[async_trait]
/// 效果观测者接口。
///
/// 【作用】
/// 可选能力：适配器可以额外支持"回报外部效果"。
///
/// 【默认实现返回 `false`】
/// 意味着不实现这个 trait 的适配器不会被要求提供观测能力。
/// 这是"安全默认值"：不做就不会被误当成"做了"。
///
pub trait EffectObserver: Send + Sync {
    fn supports_observation(&self) -> bool {
        false
    }

    async fn observe(
        &self,
        _request: EffectObservationRequest,
    ) -> Result<EffectObservation, PortError> {
        Err(PortError::Unavailable(
            "connector_effect_observation_unsupported".to_owned(),
        ))
    }

    async fn observe_checked(
        &self,
        request: EffectObservationRequest,
    ) -> Result<EffectObservation, PortError> {
        if !self.supports_observation() {
            return Err(PortError::Unavailable(
                "connector_capability_missing:observe_receipt".to_owned(),
            ));
        }
        request.validate()?;
        let expected_attempt = request.permit.attempt;
        let expected_invocation_id = request.permit.invocation_id;
        let expected_receipt = request.receipt.clone();
        let expected_owner_digest = request.owner_digest.clone();
        let expected_audience_digest = request.audience_digest.clone();
        let observation = self.observe(request).await?;
        observation
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        observation
            .validate_for_receipt(
                &expected_receipt,
                &expected_owner_digest,
                &expected_audience_digest,
            )
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        if observation.attempt != expected_attempt
            || observation.invocation_id != expected_invocation_id
        {
            return Err(PortError::Conflict(
                "connector_observation_attempt_mismatch".to_owned(),
            ));
        }
        Ok(observation)
    }
}

/// Read-only health/credential probe.  A probe returns status and evidence metadata, never a
/// token, header or secret value.
#[async_trait]
/// 凭据探测器接口。
///
/// 【作用】
/// 可选能力：适配器可以额外支持"验证凭据是否可用"。
///
pub trait CredentialProbe: Send + Sync {
    fn supports_probe(&self) -> bool {
        false
    }

    async fn probe(
        &self,
        _request: CredentialProbeRequest,
    ) -> Result<CredentialProbeResult, PortError> {
        Err(PortError::Unavailable(
            "connector_credential_probe_unsupported".to_owned(),
        ))
    }

    async fn probe_checked(
        &self,
        request: CredentialProbeRequest,
    ) -> Result<CredentialProbeResult, PortError> {
        if !self.supports_probe() {
            return Err(PortError::Unavailable(
                "connector_capability_missing:read_only_probe".to_owned(),
            ));
        }
        request.validate()?;
        let expected_connector_id = request.binding.definition.connector_id.clone();
        let expected_binding_id = request.binding.binding.binding_id.clone();
        let result = self.probe(request).await?;
        result.validate()?;
        if result.health.connector_id != expected_connector_id
            || result.health.binding_id != expected_binding_id
        {
            return Err(PortError::Conflict(
                "connector_probe_health_binding_mismatch".to_owned(),
            ));
        }
        Ok(result)
    }
}

/// Webhook verification returns an occurrence projection, so a verified inbound event still has
/// to enter the normal Workflow/ControlPlane path before it can create a run or capability.
#[async_trait]
/// Webhook 验证器接口。
///
/// 【作用】
/// 可选能力：适配器可以额外支持"验证外部推送的签名"。
///
/// 【⚠ 绝不能把验证结果做成可缓存的】
/// 签名验证是针对**每个请求**的。
/// 如果把"这个签名有效"的结果缓存起来，
/// 攻击者就能用一个旧的有效签名重放任意次数。
///
pub trait WebhookVerifier: Send + Sync {
    fn supports_webhook_verification(&self) -> bool {
        false
    }

    async fn verify(
        &self,
        _request: WebhookVerificationRequest,
    ) -> Result<VerifiedWebhook, PortError> {
        Err(PortError::Unavailable(
            "connector_webhook_verification_unsupported".to_owned(),
        ))
    }

    async fn verify_checked(
        &self,
        request: WebhookVerificationRequest,
    ) -> Result<VerifiedWebhook, PortError> {
        if !self.supports_webhook_verification() {
            return Err(PortError::Unavailable(
                "connector_capability_missing:webhook_verify".to_owned(),
            ));
        }
        request.validate()?;
        let expected_source_id = request.ingress.source_id.clone();
        let expected_event_id = request.ingress.event_id.clone();
        let expected_project_id = request.ingress.project_id.clone();
        let expected_trigger_id = request.ingress.trigger_id.clone();
        let expected_payload_digest = request.ingress.payload_digest.clone();
        let expected_policy_digest = request.policy.policy_digest.clone();
        let verified = self.verify(request).await?;
        verified.validate()?;
        if verified.occurrence.source_id != expected_source_id
            || verified.occurrence.event_id != expected_event_id
            || verified.occurrence.project_id != expected_project_id
            || verified.occurrence.trigger_id != expected_trigger_id
            || verified.occurrence.payload_digest != expected_payload_digest
            || verified.occurrence.policy_digest != expected_policy_digest
        {
            return Err(PortError::Conflict(
                "connector_webhook_occurrence_binding_mismatch".to_owned(),
            ));
        }
        Ok(verified)
    }
}

/// 把一个服务商回执与它对应的许可做绑定校验。
///
/// 【作用 —— 防"张冠李戴"的关键】
/// 适配器执行完调用后返回一个 [`ProviderReceipt`]。
/// 这个函数检查**这份回执真的是这次许可对应的回执**。
///
/// 【⚠ 为什么必须查】
/// 适配器可能有连接池、缓存、或者内部的重试机制。
/// 如果不做这个校验，可能出现：
/// - 返回了上一次调用的回执；
/// - 返回了另一个连接的回执；
/// - 适配器内部有 bug，把两个调用的结果搞混了。
///
/// 没有这一步，上层会把一个**不匹配的响应**当成有效证据记进事件账本。
/// 那等于往审计记录里掺入伪造数据。
///
/// 【核心检查 —— 七项】
/// 1. 回执自身合法（先做这步，否则后面的比对毫无意义）；
/// 2. schema 精确匹配；
/// 3. `connector_id` 与绑定里的定义一致；
/// 4. `binding_id` 与绑定一致；
/// 5. `account_id` 与绑定一致；
/// 6. `operation` 与**许可**里的操作一致；
/// 7. 回执的幂等键摘要与**许可里的**幂等键摘要一致。
///
/// 【⚠ 注意第 6、7 项比的是"许可"而不是"绑定"】
/// 绑定（binding）说明"这个连接器绑定了谁"，
/// 许可（permit）说明"这一次允许做什么"。
/// 检查"实际做了什么 == 允许做什么"必须对比许可。
///
/// 如果只对比绑定，就无法发现"这次调用执行了另一个操作"的情况。
///
/// 【失败情况】
/// - 回执本身格式错误 → `connector_provider_receipt_invalid`（`Failed`）；
/// - 与许可/绑定对不上 → `connector_provider_receipt_binding_invalid`（`Conflict`）。
///
/// ⚠ 两种错误类型不同：`Failed` 是"数据坏了"，
/// `Conflict` 是"数据合法但对不上"。上层可能需要区别对待。
///
pub(crate) fn validate_receipt_for_permit(
    receipt: &ProviderReceipt,
    binding: &ConnectorBindingSnapshot,
    permit: &ConnectorPreparedPermit,
) -> Result<(), PortError> {
    receipt
        .validate()
        .map_err(|_| PortError::Failed("connector_provider_receipt_invalid".to_owned()))?;
    if receipt.schema != "kiana.provider-receipt.v1"
        || receipt.connector_id != binding.definition.connector_id
        || receipt.binding_id != binding.binding.binding_id
        || receipt.account_id != binding.binding.account_id
        || receipt.operation != permit.operation
        || json_digest(&serde_json::json!({
            "idempotency_key": receipt.idempotency_key
        })) != permit.idempotency_key_digest
        || !provider_payload_hash_valid(&receipt.final_payload_sha256)
    {
        return Err(PortError::Conflict(
            "connector_provider_receipt_binding_invalid".to_owned(),
        ));
    }
    if !valid_extension_identifier(&receipt.provider_receipt_id)
        || receipt.source.trim().is_empty()
        || receipt.source.len() > 128
    {
        return Err(PortError::Failed(
            "connector_provider_receipt_invalid".to_owned(),
        ));
    }
    Ok(())
}

/// 校验一个摘要字符串的格式是否为合法的 SHA-256。
fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
