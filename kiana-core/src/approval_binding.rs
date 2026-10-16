//! 审批对象绑定（Approval Binding）与 Human Inbox 值投影。
//!
//! # 在系统中的位置
//!
//! 本文件位于 `kiana-core`，定义两类纯数据对象（Value Object）：
//!
//! - [`ApprovalBinding`]：把一个审批精确绑定到某个请求、身份、项目、权限上下文和有效期；
//! - [`HumanInboxItem`]：把审批状态投影成可供人工界面展示/转换的有界值对象。
//!
//! 它们本身不读取数据库、不写 EventLog、不调用 CapabilityBroker，也不会批准或执行能力。
//! 常规执行权仍由 `approvals.rs` 中的 ControlPlane 审批流程和 daemon 的审批存储管理。
//! 当前源码中没有发现生产路径调用本文件导出的 facade；直接使用主要来自 `kiana-core` 的
//! re-export 与测试。因此不能仅凭这些类型存在，就认为生产审批已采用这套绑定或 Inbox。
//!
//! # 值对象的数据流
//!
//! ```text
//! 服务端 RequestContext + 已准备的 CapabilityRequest + 项目/策略快照
//!     ↓ ApprovalBinding::for_request
//! 只保存身份和内容的摘要，不把原始请求参数放进 binding
//!     ↓ to_json / canonical_bytes（由调用方决定是否传输或持久化）
//! from_json / validate / validate_request
//!     ↓
//! HumanInboxItem 的 Pending → Approved/Denied → Consumed 投影转换
//! ```
//!
//! 摘要（digest）是用于发现内容不一致的指纹，不等于加密签名，也不能单独证明调用者身份。
//! `ApprovalBinding` 记录“某个具体审批对象对应什么材料”；它不是可复用的通用授权。
//! `HumanInboxItem` 的方法返回新的值对象，不会原地改动旧对象；持久化、并发比较交换
//!（Compare-and-Swap，CAS）和最终执行都必须由调用方的存储/ControlPlane 完成。

use kiana_domain::{
    canonical_journal_bytes, json_digest, ApprovalChallenge, ApprovalDecision, ApprovalId,
    AuthenticatedPrincipalRef, GrantId, ProjectId, RequestContext, RequestId, SchemaVersion,
    SecurityReasonCode, SessionId,
};
use kiana_policy::GrantScope;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const APPROVAL_BINDING_SCHEMA: &str = "kiana.approval-binding.v1";
pub const HUMAN_INBOX_ITEM_SCHEMA: &str = "kiana.human-inbox-item.v1";
/// 审批绑定与 Inbox 项使用的当前 schema 版本；版本兼容规则由 `SchemaVersion` 实现。
pub const APPROVAL_BINDING_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 命令/操作名称允许的最大 UTF-8 字节数，用于限制外部字段的大小。
pub const MAX_APPROVAL_COMMAND_KIND: usize = 256;

/// 把服务端上下文中参与授权范围判断的字段压成摘要。
///
/// `for_request` 创建绑定时计算一次，`validate_request` 会对当前上下文重新计算并比较。
/// 因而 session、actor、项目根目录/信任状态、权限档案、角色、部门、路径范围或 cell 任一
/// 变化都会使摘要不同，避免把旧上下文下的人类决定挪用到新范围。
/// 这里只比较传入的 `RequestContext` 快照；此函数不会查询最新策略或验证 Grant 是否仍有效。
fn approval_scope_digest(context: &RequestContext) -> String {
    json_digest(&json!({
        "session_id": context.session_id,
        "actor_id": context.actor_id,
        "project_root": context.project_root,
        "project_trusted": context.project_trusted,
        "permission_profile": context.permission_profile,
        "role_id": context.role_id,
        "department_id": context.department_id,
        "path_allow": context.path_allow,
        "cell_id": context.cell_id,
    }))
}

// `serde` 为结构体生成数据格式转换代码：Serialize 把 Rust 值转成 JSON 等外部格式，
// Deserialize 把外部格式读回 Rust 值。`deny_unknown_fields` 使解码拒绝未声明字段，
// 避免调用方误以为额外字段也已被绑定摘要覆盖。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个审批决定所针对的不可变请求材料和授权上下文指纹。
///
/// 结构体只保存有类型的身份/范围信息与摘要，不保存 `CapabilityRequest` 的原始参数。
/// 它描述“批准的是哪一件事”，并不授予执行权；真正执行仍需经过 ControlPlane 与 Broker。
pub struct ApprovalBinding {
    /// 外部数据格式标识；解码时必须与当前受支持的 schema 一致。
    pub schema: String,
    /// schema 版本，用于判断持久化或传输数据能否按当前结构解释。
    pub version: SchemaVersion,
    /// 当前审批对象的 ID；它不是请求 ID，也不是 Run ID。
    pub approval_id: ApprovalId,
    /// 被审批的原始能力请求 ID，用来防止把决定套用到另一条请求。
    pub subject_request_id: RequestId,
    /// 经服务端认证的审批主体引用；摘要不替代认证本身。
    pub principal: AuthenticatedPrincipalRef,
    /// 请求所属会话，用于阻止跨 session 重用。
    pub session_id: SessionId,
    /// 请求所属项目的稳定身份；与路径字符串不同，供上层校验项目边界。
    pub project_id: ProjectId,
    /// 被批准的操作名称，例如 CapabilityRequest 中的 operation。
    pub command_kind: String,
    /// 操作目标的摘要；目标原文由创建者决定如何解析，本结构只比较其摘要。
    pub target_digest: String,
    /// 完整能力请求序列化后的摘要；原始参数不会复制进此字段。
    pub payload_digest: String,
    /// `approval_scope_digest` 对服务端请求上下文计算出的范围摘要。
    pub scope_digest: String,
    /// 关联的策略 Grant ID；`None` 仅表示本对象未携带该引用，不表示请求免于授权。
    pub grant_id: Option<GrantId>,
    /// 创建绑定时的 authority generation；为 0 的绑定会被拒绝。
    pub authority_epoch: u64,
    /// 创建绑定时的策略版本标识；校验时需要由调用方提供当前期望值。
    pub policy_revision: String,
    /// 绑定的 Unix 毫秒过期时刻；到达该时刻即视为过期。
    pub expires_at_unix_ms: u64,
    /// 除本字段自身外上述绑定字段的摘要，用于检测对象字段被改动。
    pub binding_digest: String,
}

impl ApprovalBinding {
    /// 使用已拆分的绑定字段构造对象，并立即生成摘要、执行结构校验。
    ///
    /// 供需要建立审批绑定的上层代码使用；当前仓库的生产审批路径尚未调用此 facade。
    /// 若接入生产流程，调用者仍必须从服务端可信来源提供身份、项目、权限代次与有效期。
    ///
    /// 输入包括审批 ID、被审批请求 ID、主体、会话和项目身份，操作/目标/请求/范围摘要，
    /// 可选 Grant ID，authority epoch、policy revision 以及绝对过期时间。成功时返回摘要
    /// 已填充且通过 [`Self::validate`] 的对象；字段无效、摘要格式错误或主体无效时返回
    /// 描述性 `String` 错误，由调用者决定如何映射为 API/策略错误。
    ///
    /// `binding_digest` 是派生字段，因此先按其余字段计算，再统一检查格式和一致性。这里不
    /// 保存原始请求参数，也不把对象写入审批 Store。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        approval_id: ApprovalId,
        subject_request_id: RequestId,
        principal: AuthenticatedPrincipalRef,
        session_id: SessionId,
        project_id: ProjectId,
        command_kind: impl Into<String>,
        target_digest: impl Into<String>,
        payload_digest: impl Into<String>,
        scope_digest: impl Into<String>,
        grant_id: Option<GrantId>,
        authority_epoch: u64,
        policy_revision: impl Into<String>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: APPROVAL_BINDING_SCHEMA.to_owned(),
            version: APPROVAL_BINDING_VERSION,
            approval_id,
            subject_request_id,
            principal,
            session_id,
            project_id,
            command_kind: command_kind.into(),
            target_digest: target_digest.into(),
            payload_digest: payload_digest.into(),
            scope_digest: scope_digest.into(),
            grant_id,
            authority_epoch,
            policy_revision: policy_revision.into(),
            expires_at_unix_ms,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    /// 从服务端上下文和已准备的能力请求构造精确的审批绑定。
    ///
    /// `context` 应来自服务端而非直接信任客户端字段；`request` 应是进入此阶段前已经准备
    /// 的请求。函数分别计算权限范围和整个请求的摘要，再把 typed ID 与策略版本交给 [`Self::new`]。
    /// `target_digest` 由调用者提供，因为不同能力如何提取并规范化“目标”属于调用方的领域知识。
    ///
    /// 成功时返回不含原始 `request.arguments` 的绑定对象。若 actor_id 缺失/全空白，返回
    /// `AuthPrincipalMissing` 对应的稳定原因；请求无法转换为 JSON 或字段校验失败时返回
    /// 绑定不匹配/格式错误原因。此方法不请求审批、不写 Store，也不派发能力。
    ///
    /// 当前实现从 actor_id 创建 local 主体引用并重新计算主体摘要；非本地身份格式没有单独输入参数。
    /// 只存摘要可避免在绑定中再复制一份可能含敏感参数的请求正文；但摘要本身不是审批 proof，
    /// 不能代替后续的策略重验和执行授权。
    pub fn for_request(
        approval_id: ApprovalId,
        context: &RequestContext,
        project_id: ProjectId,
        request: &kiana_domain::CapabilityRequest,
        target_digest: impl Into<String>,
        grant: Option<&GrantScope>,
        authority_epoch: u64,
        policy_revision: impl Into<String>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let scope_digest = approval_scope_digest(context);
        let principal_id = context
            .actor_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        let mut principal = AuthenticatedPrincipalRef::local();
        principal.principal_id = principal_id.to_owned();
        principal.principal_digest = principal.digest();
        Self::new(
            approval_id,
            request.request_id,
            principal,
            context.session_id.clone(),
            project_id,
            request.operation.clone(),
            target_digest,
            kiana_domain::json_digest(&serde_json::to_value(request).map_err(|_| {
                SecurityReasonCode::PolicyApprovalBindingMismatch
                    .as_str()
                    .to_owned()
            })?),
            scope_digest,
            grant.map(|grant| grant.grant_id),
            authority_epoch,
            policy_revision,
            expires_at_unix_ms,
        )
    }

    /// 检查当前请求是否仍与创建绑定时的请求和上下文完全匹配。
    ///
    /// 输入包括上下文、项目 ID、能力请求、目标摘要、期望 authority epoch / policy revision，
    /// 以及当前 Unix 毫秒时间。全部绑定一致且尚未过期时返回 `Ok(())`；到达过期时间后返回
    /// `PolicyApprovalExpired`，身份/请求/范围/代次/策略版本不一致则返回
    /// `PolicyApprovalBindingMismatch`。请求序列化失败也按绑定不匹配处理。
    ///
    /// 当前生产源码未发现调用点；若由生产入口采用，应由 ControlPlane 从可信当前状态提供参数。
    /// ⚠ 此签名没有接收“当前 GrantScope”，因此本函数不会重新验证 `grant_id` 对应的授权范围；
    /// 调用方不能把本函数成功当作 Grant 仍有效或可直接执行的证明。
    pub fn validate_request(
        &self,
        context: &RequestContext,
        project_id: ProjectId,
        request: &kiana_domain::CapabilityRequest,
        target_digest: &str,
        authority_epoch: u64,
        policy_revision: &str,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err(SecurityReasonCode::PolicyApprovalExpired
                .as_str()
                .to_owned());
        }
        if self.project_id != project_id
            || self.session_id != context.session_id
            || self.principal.principal_id != context.actor_id.as_deref().unwrap_or_default()
            || self.subject_request_id != request.request_id
            || self.command_kind != request.operation
            || self.target_digest != target_digest
            || self.payload_digest
                != kiana_domain::json_digest(&serde_json::to_value(request).map_err(|_| {
                    SecurityReasonCode::PolicyApprovalBindingMismatch
                        .as_str()
                        .to_owned()
                })?)
            || self.scope_digest != approval_scope_digest(context)
            || self.authority_epoch != authority_epoch
            || self.policy_revision != policy_revision
        {
            return Err(SecurityReasonCode::PolicyApprovalBindingMismatch
                .as_str()
                .to_owned());
        }
        Ok(())
    }

    /// 校验绑定对象的 schema、身份、字段界限和摘要自洽性。
    ///
    /// 这里检查对象内部结构和摘要是否一致；是否已过期由 [`Self::validate_request`] 与当前时钟
    /// 比较，权限是否仍成立则需要上层策略重新判定。错误字符串供调用者转换成稳定协议/安全错误，
    /// 本方法不会记日志、改变审批状态或触发执行。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != APPROVAL_BINDING_SCHEMA
            || !self.version.is_compatible_with(&APPROVAL_BINDING_VERSION)
            || self.approval_id.as_uuid().is_nil()
            || self.subject_request_id.as_uuid().is_nil()
            || self.session_id.is_empty()
            || self.project_id.as_uuid().is_nil()
            || self.command_kind.trim().is_empty()
            || self.command_kind.len() > MAX_APPROVAL_COMMAND_KIND
            || self.command_kind.contains('\0')
            || self.authority_epoch == 0
            || self.expires_at_unix_ms == 0
        {
            return Err(SecurityReasonCode::PolicyApprovalBindingMismatch
                .as_str()
                .to_owned());
        }
        self.principal
            .validate()
            .map_err(|_| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        for (value, field) in [
            (&self.target_digest, "approval_target_digest"),
            (&self.payload_digest, "approval_payload_digest"),
            (&self.scope_digest, "approval_scope_digest"),
            (&self.policy_revision, "approval_policy_revision"),
        ] {
            validate_digest(value, field)?;
        }
        if self.grant_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("approval_grant_id_invalid".to_owned());
        }
        validate_digest(&self.binding_digest, "approval_binding_digest")?;
        if self.binding_digest != self.digest() {
            return Err("approval_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将外部 JSON 解码成绑定，并在交给业务代码前执行严格校验。
    ///
    /// 未知字段由 `deny_unknown_fields` 拒绝；解码失败统一映射为
    /// `approval_binding_decode_failed`，解码成功但内部不自洽时保留 [`Self::validate`] 的错误。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let binding: Self = serde_json::from_value(value.clone())
            .map_err(|_| "approval_binding_decode_failed".to_owned())?;
        binding.validate()?;
        Ok(binding)
    }

    /// 将绑定对象转换为 JSON 值，供调用方传输或持久化；本方法本身不写任何存储。
    /// serde 转换失败时返回 `approval_binding_encode_failed`。
    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "approval_binding_encode_failed".to_owned())
    }

    /// 生成 EventLog 采用的规范 JSON 字节表示，供需要稳定字节序列的调用方使用。
    /// 规范编码失败会向调用者返回错误；此操作不附加、不签名，也不持久化这些字节。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 对除 `binding_digest` 自身外的全部绑定字段计算稳定摘要。
    ///
    /// 排除自身是为了避免摘要递归；后续 [`Self::validate`] 会把计算结果与存储字段比较。
    /// 此摘要用于发现字段变更，不提供密钥签名或主体认证。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "approval_id": self.approval_id,
            "subject_request_id": self.subject_request_id,
            "principal": self.principal,
            "session_id": self.session_id,
            "project_id": self.project_id,
            "command_kind": self.command_kind,
            "target_digest": self.target_digest,
            "payload_digest": self.payload_digest,
            "scope_digest": self.scope_digest,
            "grant_id": self.grant_id,
            "authority_epoch": self.authority_epoch,
            "policy_revision": self.policy_revision,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Human Inbox 中审批项的纯状态投影。
///
/// 这些枚举值只描述当前对象的生命周期状态；是否可见、是否已落盘、是否获得执行许可，
/// 由负责列表/存储/ControlPlane 的上层组件决定。
pub enum HumanInboxStatus {
    /// 已创建，尚无人作出审批决定。
    Pending,
    /// 已批准，但尚未消费为一次执行机会。
    Approved,
    /// 已明确拒绝。
    Denied,
    /// 当前时间已达到绑定中的过期时刻。
    Expired,
    /// 已批准的项目已被标记为消费。
    Consumed,
    /// 已被外部流程标记取消；本文件没有单独的取消转换方法。
    Cancelled,
}

// `deny_unknown_fields` 让 JSON 解码对字段变更 fail closed；未知字段不会被静默忽略。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个绑定到具体审批对象的 Human Inbox 值。
///
/// 它是可复制、可校验的投影，不是独立的审批数据库或通知队列。`decide` 和 `consume` 都
/// 返回新值，调用方必须自行将新值放入受控存储，并用 `revision`/digest 处理并发更新。
pub struct HumanInboxItem {
    /// Inbox 项的格式标识。
    pub schema: String,
    /// Inbox 项的 schema 版本。
    pub version: SchemaVersion,
    /// 该项使用对应审批的 ApprovalId 作为稳定身份。
    pub item_id: ApprovalId,
    /// 指向唯一审批材料及其身份/范围摘要。
    pub binding: ApprovalBinding,
    /// 当前审批项状态。
    pub status: HumanInboxStatus,
    /// 作出批准/拒绝的主体；待批时应为空。
    #[serde(default)]
    pub decided_by: Option<AuthenticatedPrincipalRef>,
    /// 状态修订号，可供调用方做并发比较；具体转换使用 checked_add 或 saturating_add。
    pub revision: u64,
    /// 除自身外 Inbox 项字段的摘要，用于检测投影数据不一致。
    pub item_digest: String,
}

impl HumanInboxItem {
    /// 从一个有效绑定创建初始的待审批 Inbox 项。
    ///
    /// 初始 revision 为 1，decider 为空，状态为 Pending；方法生成 item digest 并执行结构
    /// 校验。由于没有接收当前时间，这里不会判断 binding 是否已过期；它仅构造内存值，不登记
    /// 到人类 Inbox，也不会发通知或授予能力。
    pub fn new(binding: ApprovalBinding) -> Result<Self, String> {
        binding.validate()?;
        let mut item = Self {
            schema: HUMAN_INBOX_ITEM_SCHEMA.to_owned(),
            version: APPROVAL_BINDING_VERSION,
            item_id: binding.approval_id,
            binding,
            status: HumanInboxStatus::Pending,
            decided_by: None,
            revision: 1,
            item_digest: String::new(),
        };
        item.item_digest = item.digest();
        item.validate()?;
        Ok(item)
    }

    /// 从 JSON 解码 Inbox 项，并在返回前检查绑定、状态和摘要。
    /// 未知字段或类型不符映射为 `approval_inbox_decode_failed`；字段可解码但不自洽时返回
    /// [`Self::validate`] 给出的具体错误。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let item: Self = serde_json::from_value(value.clone())
            .map_err(|_| "approval_inbox_decode_failed".to_owned())?;
        item.validate()?;
        Ok(item)
    }

    /// 产生一份新的 Approved 或 Denied 项，不修改原来的 Pending 值。
    ///
    /// 对象会检查审批人引用有效，且审批人 ID 不能与发起请求的 principal 相同，从而阻止
    /// 直接自批。此字段检查只是值对象规则，不验证凭证、组织角色或独立审批权限；认证与授权
    /// 必须由生产入口负责。
    ///
    /// 非 Pending 项不可再次决策；有效决策会保存 `decided_by`、递增 revision 并重算摘要。
    /// revision 溢出时返回 `approval_inbox_revision_exhausted`。本函数不持久化结果，也不触发
    /// `approvals.rs` 中的 Run 续体或 Broker 执行。
    pub fn decide(
        &self,
        approver: AuthenticatedPrincipalRef,
        decision: ApprovalDecision,
        now_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate()?;
        approver
            .validate()
            .map_err(|_| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        if approver.principal_id == self.binding.principal.principal_id {
            return Err(SecurityReasonCode::PolicySelfApprovalForbidden
                .as_str()
                .to_owned());
        }
        if self.status != HumanInboxStatus::Pending {
            return Err("approval_inbox_terminal".to_owned());
        }
        if now_unix_ms >= self.binding.expires_at_unix_ms {
            // 这里构造 Expired 副本，但 API 最终返回 Err 而不是该副本；由于 self 不可变，
            // 本分支只拒绝过期决策，并不会让调用方或存储自动获得一次状态转换结果。
            let mut expired = self.clone();
            expired.status = HumanInboxStatus::Expired;
            expired.revision = expired.revision.saturating_add(1);
            expired.item_digest = expired.digest();
            expired.validate()?;
            return Err(SecurityReasonCode::PolicyApprovalExpired
                .as_str()
                .to_owned());
        }
        let mut next = self.clone();
        next.status = match decision {
            ApprovalDecision::Approve => HumanInboxStatus::Approved,
            ApprovalDecision::Deny => HumanInboxStatus::Denied,
        };
        next.decided_by = Some(approver);
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "approval_inbox_revision_exhausted".to_owned())?;
        next.item_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    /// 将已批准项转换为 Consumed，表示这份审批机会已被消费一次。
    ///
    /// Pending/Denied/Expired/Cancelled 等状态都会以 `approval_inbox_not_approved` 拒绝。
    /// 已批准但到达 binding 过期时间的项目返回 `PolicyApprovalExpired`，不能再变成 Consumed。
    /// 返回的是新的投影值；防止并发重复消费仍取决于调用方对 revision/digest 的原子比较与写入。
    /// 此处 revision 使用饱和加法：若已达到 `u64::MAX`，状态仍会变成 Consumed，但 revision
    /// 不再增长；依赖 revision 单独做比较的调用方需要留意这个极端边界。
    pub fn consume(&self, now_unix_ms: u64) -> Result<Self, String> {
        self.validate()?;
        if self.status != HumanInboxStatus::Approved {
            return Err("approval_inbox_not_approved".to_owned());
        }
        if now_unix_ms >= self.binding.expires_at_unix_ms {
            return Err(SecurityReasonCode::PolicyApprovalExpired
                .as_str()
                .to_owned());
        }
        let mut next = self.clone();
        next.status = HumanInboxStatus::Consumed;
        next.revision = self.revision.saturating_add(1);
        next.item_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    /// 校验 Inbox 项头部、内嵌绑定、decider 规则和摘要是否自洽。
    ///
    /// 校验规则区分：Pending 不允许带 decider；Approved 必须有 decider；其他非 Pending 状态
    /// 若携带 decider 则 decider 必须有效。验证通过只说明值对象结构一致，不说明它已持久化、
    /// 仍未过期或已获得执行许可。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HUMAN_INBOX_ITEM_SCHEMA
            || !self.version.is_compatible_with(&APPROVAL_BINDING_VERSION)
            || self.item_id != self.binding.approval_id
            || self.revision == 0
        {
            return Err("approval_inbox_header_invalid".to_owned());
        }
        self.binding.validate()?;
        if let Some(decided_by) = &self.decided_by {
            decided_by
                .validate()
                .map_err(|_| "approval_inbox_decider_invalid".to_owned())?;
            if self.status == HumanInboxStatus::Pending {
                return Err("approval_inbox_decider_unexpected".to_owned());
            }
        }
        if self.status == HumanInboxStatus::Approved && self.decided_by.is_none() {
            return Err("approval_inbox_decider_required".to_owned());
        }
        validate_digest(&self.item_digest, "approval_inbox_digest")?;
        if self.item_digest != self.digest() {
            return Err("approval_inbox_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 schema、版本、ID、绑定、状态、decider 与 revision 计算摘要，不包含 `item_digest`。
    /// 这样构造或状态转换后可以重新生成摘要，再由 [`Self::validate`] 检查一致性。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "item_id": self.item_id,
            "binding": self.binding,
            "status": self.status,
            "decided_by": self.decided_by,
            "revision": self.revision,
        }))
    }
}

#[allow(dead_code)]
/// 仅供结构/安全约束检查的辅助谓词；当前没有生产调用者，不参与审批验证或执行。
/// 它只检查 challenge.reason 不含看起来像 bearer token 的文本，并检查 request_hash 前缀，
/// 不能替代 `ApprovalBinding::validate_request` 或 ControlPlane 的 proof 验证。
fn _approval_challenge_is_subject_only(challenge: &ApprovalChallenge) -> bool {
    !challenge.reason.contains("bearer ") && challenge.request_hash.starts_with("sha256:")
}

/// 校验项目内摘要的文本形状：必须是 `sha256:` 前缀加 64 位十六进制编码。
///
/// 本 helper 只检查格式，不重算摘要；摘要值与实际字段是否匹配由各类型的 `validate` 完成。
/// 错误名带上 `field`，便于上层保留具体出错字段。
fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
