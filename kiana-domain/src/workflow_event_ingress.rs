//! 外部 workflow event ingress（事件入口）的 Domain 合同。
//!
//! 本文件定义外部事件的有限 wire envelope、由服务端维护的 source policy，以及验签后供 workflow 路径使用的
//! occurrence（事件发生记录）。它的上游是 webhook/connector 一类入口，下游是 Daemon 验签器和已有的
//! AutomationCommand::Fire → Core workflow evidence 校验路径：
//!
//! ```text
//! 外部事件正文（不可信）
//!     -> WorkflowEventIngress（长度/JSON/digest 合同）
//!     -> Daemon WorkflowEventVerifier（server key + HMAC + policy + 进程内 dedupe）
//!     -> WorkflowEventOccurrence（只带触发/来源/摘要，不复制正文）
//!     -> AutomationCommand::Fire（触发 ID + occurrence key + event evidence ref）
//!     -> Core 从已提交 EventLog 事实核对 event_ref/owner 后走现有 workflow 路径
//! ```
//!
//! payload 是外部输入，只用于通过 source policy 选出一个 occurrence；不能直接解释成 prompt、文件权限或执行
//! 参数。本 Domain 不持有 HMAC 密钥、不验签、不记重放状态、不写 EventLog，也不发起执行；这些边界分别由
//! Daemon/Core 负责。Serde 的 `Serialize`/`Deserialize` 用来在 Rust 值与 JSON 等外部形式间转换，
//! `deny_unknown_fields` 会拒绝未知 wire 字段。

use crate::{json_digest, AutomationCommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// 外部传入事件 DTO 的版本号；升级其字段形状时应同步兼容读取方。
pub const WORKFLOW_EVENT_INGRESS_SCHEMA: &str = "kiana.workflow-event-ingress.v1";
/// 服务端来源 allow-list 策略的版本号。
pub const WORKFLOW_EVENT_SOURCE_POLICY_SCHEMA: &str = "kiana.workflow-event-source-policy.v1";
/// 验签后轻量事件投影的版本号；它不包含原始 payload。
pub const WORKFLOW_EVENT_OCCURRENCE_SCHEMA: &str = "kiana.workflow-event-occurrence.v1";
/// payload 序列化后的最大字节数，即 64 KiB；这是防止单个外部事件占用过多内存的第一道上限。
pub const WORKFLOW_EVENT_PAYLOAD_MAX_BYTES: usize = 64 * 1024;
/// 策略允许的最大时钟偏差，即 24 小时；具体 policy 还可以配置更小窗口。
pub const WORKFLOW_EVENT_MAX_CLOCK_SKEW_MS: u64 = 86_400_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个来自外部系统、尚未经过 Daemon 密码学验签的 workflow 事件 envelope。
///
/// `validate()` 只检查结构、payload 上限和内容摘要；真正验证 signature 是否由允许的 key 生成，必须经过
/// Daemon `WorkflowEventVerifier::verify()`。wire 值中的 source/key/project/trigger 都是不可信声明，之后要与
/// 服务端保存的 `WorkflowEventSourcePolicy` 比较。
pub struct WorkflowEventIngress {
    /// 事件 envelope 的 wire schema。
    pub schema: String,
    /// 声称发送事件的外部来源 ID，必须匹配 server-owned policy。
    pub source_id: String,
    /// 声称签名所用的 key ID；这里只是标签，密钥查找由 Daemon 完成。
    pub key_id: String,
    /// 事件所属项目标签；不能由这个字段自行取得项目权限。
    pub project_id: String,
    /// 事件请求触发的 workflow trigger ID。
    pub trigger_id: String,
    /// 外部来源提供的事件 ID；与 source_id 组合成去重 key。
    pub event_id: String,
    /// 来源声明的事件类型，必须位于 policy 的 allow-list。
    pub event_kind: String,
    /// 来源声称的发生时间（Unix 毫秒），用于和服务端当前时间比较允许时钟偏差。
    pub occurred_at_unix_ms: u64,
    /// 不可信的原始 JSON 正文；限制大小/深度/容器，验签后也不会复制到 occurrence。
    pub payload: Value,
    /// 对 JSON payload 内容计算的摘要，签名摘要使用该值代替重序列化正文。
    pub payload_digest: String,
    /// HMAC-SHA256 签名的十六进制文本；本 Domain 只限长，Daemon 才解析并验签。
    pub signature: String,
    /// 对 envelope 字段（含 signature）计算的无密钥摘要，用于检测内部字段漂移，不代替 HMAC。
    pub ingress_digest: String,
}

impl WorkflowEventIngress {
    /// 构造 envelope，限制 JSON payload 后生成 payload/ingress 两层内容摘要并验证结构。
    ///
    /// 【调用者】Webhook/connector adapter 收到外部输入时。
    /// 【输入】来源/key/project/trigger/event 标签、Unix 毫秒时间、JSON payload 和签名文本。
    /// 【输出】成功返回摘要已填的 DTO。
    /// 【失败】payload 超限/过深、必要文本不合格或 schema/摘要不一致时返回稳定错误。
    /// 【设计边界】signature 只是文本字段；此构造函数没有 key，不能证明它正确。调用者需再把 event 交给
    /// Daemon verifier 执行 HMAC 和服务端 allow-list 检查。
    pub fn new(
        source_id: impl Into<String>,
        key_id: impl Into<String>,
        project_id: impl Into<String>,
        trigger_id: impl Into<String>,
        event_id: impl Into<String>,
        event_kind: impl Into<String>,
        occurred_at_unix_ms: u64,
        payload: Value,
        signature: impl Into<String>,
    ) -> Result<Self, String> {
        validate_payload(&payload)?;
        let payload_digest = json_digest(&payload);
        let mut event = Self {
            schema: WORKFLOW_EVENT_INGRESS_SCHEMA.to_owned(),
            source_id: source_id.into(),
            key_id: key_id.into(),
            project_id: project_id.into(),
            trigger_id: trigger_id.into(),
            event_id: event_id.into(),
            event_kind: event_kind.into(),
            occurred_at_unix_ms,
            payload,
            payload_digest,
            signature: signature.into(),
            ingress_digest: String::new(),
        };
        event.ingress_digest = event.digest();
        event.validate()?;
        Ok(event)
    }

    /// 验证 envelope schema、身份标签文本、payload 边界和两层 digest 自洽性。
    ///
    /// 【调用者】Daemon `verify()` 在读取 allow-list 前，及 policy/occurrence helper。
    /// 【文本上限】source/key/trigger/event kind 各 128 字节，project/event ID/signature 各 256 字节；
    /// 这些是 UTF-8 字节数上限，不是 Unicode 字符数。Daemon 后续还会把 signature 收紧到恰好 64 个 hex 字符。
    /// 【失败】header、文本、payload、payload digest 或 ingress digest 异常时返回对应错误；Daemon 将错误
    /// 映射为 `PortError::Failed` 并拒绝生成 occurrence。
    /// 【边界】这里只验证摘要值格式和自我一致，不验 signature、不判断 source/project 是否允许。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_EVENT_INGRESS_SCHEMA || self.occurred_at_unix_ms == 0 {
            return Err("workflow_event_ingress_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.source_id, "workflow_event_source", 128),
            (&self.key_id, "workflow_event_key_id", 128),
            (&self.project_id, "workflow_event_project", 256),
            (&self.trigger_id, "workflow_event_trigger", 128),
            (&self.event_id, "workflow_event_id", 256),
            (&self.event_kind, "workflow_event_kind", 128),
            (&self.signature, "workflow_event_signature", 256),
        ] {
            bounded(value, field, max)?;
        }
        validate_payload(&self.payload)?;
        if self.payload_digest != json_digest(&self.payload) {
            return Err("workflow_event_payload_digest_mismatch".to_owned());
        }
        if !valid_digest(&self.payload_digest)
            || !valid_digest(&self.ingress_digest)
            || self.ingress_digest != self.digest()
        {
            return Err("workflow_event_ingress_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 返回 Daemon HMAC 要签的摘要输入。
    ///
    /// 它包含来源/key/project/trigger/event/time 等身份与 `payload_digest`，不直接签任意 JSON 重序列化正文；
    /// Domain 本身只产出摘要，Daemon 用服务器密钥对其字节执行 HMAC-SHA256。
    pub fn signing_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "project_id": self.project_id,
            "trigger_id": self.trigger_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "occurred_at_unix_ms": self.occurred_at_unix_ms,
            "payload_digest": self.payload_digest,
        }))
    }

    /// 以 source + event ID 形成重放去重键，不包含 project/trigger。
    /// 同一个 source 下重复 event ID 会被视为同一事件；payload 不同则由 Daemon 记忆摘要并拒绝冲突。
    pub fn occurrence_key(&self) -> String {
        format!("event:{}:{}", self.source_id, self.event_id)
    }

    /// 对 envelope 的不可变身份、时间、payload 摘要和签名文本生成无密钥摘要。
    /// ingress_digest 自身不参与输入；该值用于内容一致性与去重，不是签名或来源证明。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "project_id": self.project_id,
            "trigger_id": self.trigger_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "occurred_at_unix_ms": self.occurred_at_unix_ms,
            "payload_digest": self.payload_digest,
            "signature": self.signature,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 由服务端配置的一条外部事件来源准入策略。
///
/// Daemon 将 policy 与私密 HMAC key 放在一起，按 `source_id` 找到 verifier，再调用 `matches()` 比较事件。
/// policy 中列出允许的 event kind、payload 顶层字段、必需字段和允许的时钟偏差；digest 是配置内容自摘要，
/// 不是签名，也不能证明该 policy 是从可信配置加载的。
pub struct WorkflowEventSourcePolicy {
    /// 策略 wire schema。
    pub schema: String,
    /// 该策略允许的来源 ID。
    pub source_id: String,
    /// 该来源绑定的 key 标签。
    pub key_id: String,
    /// 该来源允许事件进入的项目标签。
    pub project_id: String,
    /// 可被此来源触发的事件类型集合；BTreeSet 让摘要和遍历顺序稳定。
    pub allowed_event_kinds: BTreeSet<String>,
    /// payload 顶层必须包含的字段名集合。
    pub required_payload_keys: BTreeSet<String>,
    /// payload 顶层唯一允许出现的字段名集合；嵌套值仍受通用大小/深度边界约束。
    pub allowed_payload_keys: BTreeSet<String>,
    /// 事件发生时间与服务端当前时间的最大允许差，单位毫秒。
    pub max_clock_skew_ms: u64,
    /// 对以上策略字段的无密钥内容摘要。
    pub policy_digest: String,
}

impl WorkflowEventSourcePolicy {
    /// 构造并验证服务端 source policy，将传入列表收敛为有序集合后计算摘要。
    ///
    /// 【调用者】服务端 connector 配置装载层，而非不可信 webhook payload。
    /// 【失败】策略无允许事件类型、字段长度/数量越界、时钟偏差为 0/超过一天或摘要无效时拒绝配置。
    /// 【注意】当前 validator 只比较 required 与 allowed 字段集合的数量，没有检查 required 是否真是
    /// allowed 的子集；不相交的配置可通过构造，但之后没有任何 payload 能同时满足两条规则。
    pub fn new(
        source_id: impl Into<String>,
        key_id: impl Into<String>,
        project_id: impl Into<String>,
        allowed_event_kinds: impl IntoIterator<Item = String>,
        required_payload_keys: impl IntoIterator<Item = String>,
        allowed_payload_keys: impl IntoIterator<Item = String>,
        max_clock_skew_ms: u64,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: WORKFLOW_EVENT_SOURCE_POLICY_SCHEMA.to_owned(),
            source_id: source_id.into(),
            key_id: key_id.into(),
            project_id: project_id.into(),
            allowed_event_kinds: allowed_event_kinds.into_iter().collect(),
            required_payload_keys: required_payload_keys.into_iter().collect(),
            allowed_payload_keys: allowed_payload_keys.into_iter().collect(),
            max_clock_skew_ms,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    /// 检查 policy 自身的 schema、数量/长度上限、时间窗口和 digest。
    ///
    /// 【调用者】policy 构造、Daemon verifier 初始化及每次匹配前。
    /// 【数值边界】至少 1 个、最多 128 个 event kind；每个类型/字段键最多 128 字节；source/key 最多 128 字节，
    /// project 最多 256 字节；clock skew 必须在 1 毫秒到 24 小时之间。allowed payload key 集合本身没有数量上限。
    /// 【失败】任一合同项不成立时返回 `workflow_event_source_policy_invalid` 或字段错误。
    /// 【边界】不验证 source/key 是否在操作系统密钥库里存在，也不做 HMAC。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_EVENT_SOURCE_POLICY_SCHEMA
            || self.allowed_event_kinds.is_empty()
            || self.allowed_event_kinds.len() > 128
            || self.required_payload_keys.len() > self.allowed_payload_keys.len()
            || self.max_clock_skew_ms == 0
            || self.max_clock_skew_ms > WORKFLOW_EVENT_MAX_CLOCK_SKEW_MS
            || !valid_digest(&self.policy_digest)
            || self.policy_digest != self.digest()
        {
            return Err("workflow_event_source_policy_invalid".to_owned());
        }
        for value in self
            .allowed_event_kinds
            .iter()
            .chain(&self.required_payload_keys)
            .chain(&self.allowed_payload_keys)
        {
            bounded(value, "workflow_event_filter_key", 128)?;
        }
        bounded(&self.source_id, "workflow_event_source", 128)?;
        bounded(&self.key_id, "workflow_event_key_id", 128)?;
        bounded(&self.project_id, "workflow_event_project", 256)?;
        Ok(())
    }

    /// 检查一个未认证 envelope 是否符合该策略声明的来源、类型、时间和 payload 顶层字段。
    ///
    /// 【调用者】Daemon `verify()` 在读取/使用 source key 后执行 HMAC 前；Occurrence 构造也复用该筛选。
    /// 【输入】事件与服务端当前 Unix 毫秒时间。
    /// 【失败】身份/event kind 不匹配、时间偏差超限、payload 非对象、缺 required 字段或含额外顶层字段时拒绝。
    /// 【重要边界】这不是 signature 验证；只做输入过滤。它要求顶层字段白名单，不递归解释 payload 业务内容。
    pub fn matches(&self, event: &WorkflowEventIngress, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        event.validate()?;
        if self.source_id != event.source_id
            || self.key_id != event.key_id
            || self.project_id != event.project_id
            || !self.allowed_event_kinds.contains(&event.event_kind)
        {
            return Err("workflow_event_source_or_kind_denied".to_owned());
        }
        let skew = now_unix_ms.abs_diff(event.occurred_at_unix_ms);
        if now_unix_ms == 0 || skew > self.max_clock_skew_ms {
            return Err("workflow_event_clock_skew".to_owned());
        }
        let object = event
            .payload
            .as_object()
            .ok_or_else(|| "workflow_event_payload_object_required".to_owned())?;
        if !object
            .keys()
            .all(|key| self.allowed_payload_keys.contains(key))
            || !self
                .required_payload_keys
                .iter()
                .all(|key| object.contains_key(key))
        {
            return Err("workflow_event_payload_filter_denied".to_owned());
        }
        Ok(())
    }

    /// 对 schema、来源/key/project、三个字段/类型集合和时间窗口计算 policy digest。
    /// 集合按 BTreeSet 顺序进入 JSON 摘要，避免输入列表顺序改变同一策略的摘要。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_id": self.source_id,
            "key_id": self.key_id,
            "project_id": self.project_id,
            "allowed_event_kinds": self.allowed_event_kinds,
            "required_payload_keys": self.required_payload_keys,
            "allowed_payload_keys": self.allowed_payload_keys,
            "max_clock_skew_ms": self.max_clock_skew_ms,
        }))
    }
}

/// 通过 Domain source-policy 筛选后，可作为 workflow 触发依据的轻量事件投影。
///
/// 它不复制原始 payload，只保留 source/event/type/project/trigger、去重 key 和各层 digest；这样后续 Fire 命令
/// 只能引用已记录的事件证据，不能把任意外部 JSON 直接变成执行参数。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEventOccurrence {
    /// occurrence wire schema。
    pub schema: String,
    /// 事件来源 ID。
    pub source_id: String,
    /// 来源生成的事件 ID。
    pub event_id: String,
    /// 通过 allow-list 的事件类型。
    pub event_kind: String,
    /// 事件所属项目标签。
    pub project_id: String,
    /// 事件选中的 automation trigger。
    pub trigger_id: String,
    /// `event:<source_id>:<event_id>` 稳定去重/触发键。
    pub occurrence_key: String,
    /// 原 payload 的 digest；正文不保留在此 occurrence。
    pub payload_digest: String,
    /// 原 ingress envelope 的 digest。
    pub ingress_digest: String,
    /// 当时匹配的 source policy digest。
    pub policy_digest: String,
    /// 对 occurrence 所有字段生成的无密钥摘要。
    pub occurrence_digest: String,
}

impl WorkflowEventOccurrence {
    /// 从已经由可信调用链验签并通过策略检查的 ingress 构造 occurrence。
    ///
    /// 【预期调用者】Daemon verifier 在 HMAC 成功后；当前源码中也有 INT-07 fake adapter fixture 直接调用，
    /// 但搜索尚未找到会在产品入口执行此方法的生产调用点。
    /// 【输入/输出】事件和 policy；成功返回不含 payload 正文的投影并填好 occurrence digest。
    /// 【关键前提】本方法名字中的 “verified” 是调用者前置条件；Domain 不持有签名 key，也不检查 HMAC。
    /// 因此直接把未验签 DTO 传进来仍可能得到结构有效的 occurrence，不能把返回值本身当作来源认证证明。
    /// 它只再次调用 `policy.matches(event, event.occurred_at_unix_ms)`。由于传入的 now 等于事件时间，这次调用
    /// 主要复查字段匹配，不能替代 Daemon 使用真实服务端当前时间进行的时钟偏差检查。
    /// 【失败】policy/event 不匹配、构造出的摘要/字段不合法时返回错误。
    pub fn from_verified(
        event: &WorkflowEventIngress,
        policy: &WorkflowEventSourcePolicy,
    ) -> Result<Self, String> {
        policy.matches(event, event.occurred_at_unix_ms)?;
        let mut occurrence = Self {
            schema: WORKFLOW_EVENT_OCCURRENCE_SCHEMA.to_owned(),
            source_id: event.source_id.clone(),
            event_id: event.event_id.clone(),
            event_kind: event.event_kind.clone(),
            project_id: event.project_id.clone(),
            trigger_id: event.trigger_id.clone(),
            occurrence_key: event.occurrence_key(),
            payload_digest: event.payload_digest.clone(),
            ingress_digest: event.ingress_digest.clone(),
            policy_digest: policy.policy_digest.clone(),
            occurrence_digest: String::new(),
        };
        occurrence.occurrence_digest = occurrence.digest();
        occurrence.validate()?;
        Ok(occurrence)
    }

    /// 把 occurrence 转成现有 Automation Fire 命令所需的最小触发材料。
    ///
    /// 【输入/输出】接收调用方给出的已提交事件引用，返回 trigger_id、稳定 firing_key 和 event_ref。
    /// 【失败】occurrence 无效或引用不是 `event:` 前缀/超过 512 字节时拒绝。
    /// 【重要边界】这里仅检查引用外形，不查询事件是否已提交或归属是否匹配；Core 后续从事实日志查找并
    /// 校验 event_ref 与当前 workflow 的 owner/project 后，Fire 才能继续。
    /// 【数据边界】原始 payload 不会被复制到 AutomationCommand。
    pub fn to_fire_command(
        &self,
        event_ref: impl Into<String>,
    ) -> Result<AutomationCommand, String> {
        self.validate()?;
        let event_ref = event_ref.into();
        if !event_ref.starts_with("event:") || event_ref.len() > 512 {
            return Err("workflow_event_evidence_ref_invalid".to_owned());
        }
        Ok(AutomationCommand::Fire {
            trigger_id: self.trigger_id.clone(),
            firing_key: self.occurrence_key.clone(),
            event_ref: Some(event_ref),
        })
    }

    /// 检查 occurrence key、摘要格式、自摘要和可用于下游路由的有限文本字段。
    ///
    /// 【调用者】Occurrence 构造后、转成 Fire 命令前，以及连接器生成 verified wrapper 时。
    /// 【失败】key 与 source/event 不符或任何摘要/文本不合法时返回 `workflow_event_occurrence_invalid`
    /// 或字段错误。
    /// 【边界】此方法能检查结构完整性，不能由 occurrence 自身证明原事件通过过签名验证。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_EVENT_OCCURRENCE_SCHEMA
            || self.occurrence_key != format!("event:{}:{}", self.source_id, self.event_id)
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.ingress_digest)
            || !valid_digest(&self.policy_digest)
            || !valid_digest(&self.occurrence_digest)
            || self.occurrence_digest != self.digest()
        {
            return Err("workflow_event_occurrence_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.source_id, "workflow_event_source", 128),
            (&self.event_id, "workflow_event_id", 256),
            (&self.event_kind, "workflow_event_kind", 128),
            (&self.project_id, "workflow_event_project", 256),
            (&self.trigger_id, "workflow_event_trigger", 128),
            (&self.occurrence_key, "workflow_event_occurrence_key", 512),
        ] {
            bounded(value, field, max)?;
        }
        Ok(())
    }

    /// 对 occurrence 的字段与来源 digest 关系生成无密钥摘要；digest 字段自身不参与输入。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "source_id": self.source_id,
            "event_id": self.event_id,
            "event_kind": self.event_kind,
            "project_id": self.project_id,
            "trigger_id": self.trigger_id,
            "occurrence_key": self.occurrence_key,
            "payload_digest": self.payload_digest,
            "ingress_digest": self.ingress_digest,
            "policy_digest": self.policy_digest,
        }))
    }
}

/// 限制外部 JSON 的总体序列化字节数、嵌套深度、数组/对象成员数和单个字符串/字段名大小。
///
/// 【调用者】ingress 构造与验证阶段，在做 policy 匹配和签名查找前。
/// 【失败】超过任何资源界限时返回具体 `workflow_event_payload_*_limit` 错误，Daemon 不继续验签。
/// 【固定界限】整个 JSON 最多 64 KiB、最大嵌套深度 8、单个数组/对象最多 256 项、对象 key 最多 128 字节，
/// 单个字符串最多 4,096 字节。深度从顶层值的 0 开始，因此第 9 层会被拒绝。
/// 【为什么同时限多种维度】只限总字节仍允许极深递归或大量很小成员；分层上限让遍历成本有可预测边界。
/// 该函数不检查业务 schema，也不剥离嵌套字段；业务 allow-list 由 policy 检查顶层 key。
fn validate_payload(value: &Value) -> Result<(), String> {
    // 递归遍历前先检查 JSON 总字节，避免继续处理超过合同上限的 body。
    let bytes =
        serde_json::to_vec(value).map_err(|_| "workflow_event_payload_invalid".to_owned())?;
    if bytes.len() > WORKFLOW_EVENT_PAYLOAD_MAX_BYTES {
        return Err("workflow_event_payload_too_large".to_owned());
    }
    fn visit(value: &Value, depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Err("workflow_event_payload_depth_limit".to_owned());
        }
        match value {
            Value::Array(items) if items.len() > 256 => {
                Err("workflow_event_payload_array_limit".to_owned())
            }
            Value::Array(items) => items.iter().try_for_each(|item| visit(item, depth + 1)),
            Value::Object(items) if items.len() > 256 => {
                Err("workflow_event_payload_object_limit".to_owned())
            }
            Value::Object(items) => {
                for (key, item) in items {
                    bounded(key, "workflow_event_payload_key", 128)?;
                    visit(item, depth + 1)?;
                }
                Ok(())
            }
            Value::String(text) if text.len() > 4_096 => {
                Err("workflow_event_payload_string_limit".to_owned())
            }
            _ => Ok(()),
        }
    }
    visit(value, 0)
}

/// 验证 wire 标识/对象 key 非空、字节长度不超过调用方传入的 `max`，且不含 NUL 或换行。
/// 这里用 `str::len()` 计 UTF-8 字节，拒绝原因带上传入的字段前缀，供不同 DTO 复用稳定合同。
fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查 `sha256:` 前缀及 64 个十六进制字符的外形，不验证摘要原文或来源。
fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
