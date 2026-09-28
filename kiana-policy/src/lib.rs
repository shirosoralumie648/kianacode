//! Kiana 控制面能力请求的确定性策略判断。
//!
//! 本 crate 接收控制面已经构造好的 [`RequestContext`] 与 [`CapabilityRequest`]，输出
//! [`PolicyDecision::Allow`]、[`PolicyDecision::Ask`] 或 [`PolicyDecision::Deny`]。它不
//! 执行能力、不消费审批，也不读取项目文件；具体执行仍必须经过 `kiana-gates`、审批、
//! Cell 能力围栏和 Capability Broker。
//!
//! # 判断顺序
//!
//! 1. 未受信项目直接拒绝，包括只读请求；
//! 2. 校验角色、部门、模型可见工具、Memory ACL 和可识别的 patch 路径；
//! 3. Secret 或名称命中敏感词的操作要求显式审批；
//! 4. 最后按风险等级与 permission profile 决定允许或等待审批。
//!
//! 顺序是安全语义的一部分：角色或 WorkPacket 越权属于不可审批的拒绝，不能因为请求
//! 同时是高风险动作就降级成 `Ask`，再由一次人工批准绕过范围限制。
//!
//! 当前授权 ID 是由请求 ID 派生的本地关联标识，不是签名或可转移凭证；策略允许也只
//! 代表可以进入下一道 Gate，不能作为副作用已执行或已持久化的证明。

use kiana_domain::{
    CapabilityKind, CapabilityRequest, PermissionProfile, PolicyDecision, RequestContext,
    RiskLevel, RoleSpec,
};

mod security;
pub use security::*;
mod grant_scope;
pub use grant_scope::*;
mod provider;
pub use provider::*;
mod project_trust;
pub use project_trust::*;
mod security_control_registry;
pub use security_control_registry::*;

/// 对单个能力请求作出纯策略决定的接口。
///
/// 实现必须只依据传入快照计算结果，不应在这里执行工具或修改上下文。保持同步可以让
/// 同一输入得到稳定结果，也避免策略判断期间引入网络、模型或长 I/O 形成 TOCTOU 窗口。
/// 策略引擎接口。
///
/// 【作用】
/// 把"给定上下文和请求，得出什么结论"这件事抽象成一个可替换的接口。
///
/// 【为什么需要这个 trait】
/// 让 `ControlPlane` 依赖接口而不是具体实现。
/// 需要时可以换成更严格的引擎（见 `security.rs` 的 `BundlePolicyEngine`），
/// 而不必改动控制面的任何代码。
///
/// 【⚠ 但替换引擎不等于可以放宽】
/// `kiana-core` 在调用可替换引擎**之前**，会无条件先跑一遍
/// `DefaultPolicyEngine`（见 `kiana-core/src/approvals.rs:993`）。
/// 它拒绝就直接返回，配置引擎没有任何机会放宽。
///
/// 这意味着：**部署可以换策略，但不能换掉产品自身的信任/角色/风险地板。**
///
pub trait PolicyEngine: Send + Sync {
    /// 根据请求上下文和能力声明返回允许、待审批或拒绝。
    ///
    /// 返回 [`PolicyDecision::Allow`] 时必须附带非空授权 ID；返回 `Deny` 的原因应保持
    /// 稳定，供 Gate、事件账本、测试和入口层一致识别。
    /// 评估一次能力请求。
    ///
    /// 【调用者】
    /// `kiana-core::ControlPlane` 在派发任何能力之前调用。
    ///
    /// 【输入】
    /// - `context`：服务端持有的请求上下文（身份、角色、项目、信任状态、权限档位）。
    ///   ⚠ 这个对象来自服务端，**不是客户端声称的**；
    /// - `request`：这次要执行的能力请求。
    ///
    /// 【输出】
    /// [`PolicyDecision`]，三选一：
    /// - `Allow { authorization_id }` —— 可以进入下一道 Gate；
    /// - `Ask { reason }` —— 需要显式审批；
    /// - `Deny { reason }` —— 拒绝。
    ///
    /// 【⚠ 极其重要：Allow 不代表副作用已经发生，也不代表已被持久化】
    /// 文件头最后一句话专门强调了这一点：
    /// "策略允许也只代表可以进入下一道 Gate，不能作为副作用已执行或已持久化的证明。"
    ///
    /// 而且这里的 `authorization_id` 是**由请求 ID 派生的本地关联标识**，
    /// 不是签名，也不是可转移的凭证。
    /// 它只是把"这次判定"和"这次请求"在日志里对上号。
    ///
    /// 【为什么这个 trait 保持同步】
    /// 策略判断是纯计算 —— 读已有的上下文和请求，得出结论。
    /// 不涉及网络和长 I/O，所以不需要异步。
    /// 保持同步也让拒绝路径的测试可以确定性复现。
    ///
    fn evaluate(&self, context: &RequestContext, request: &CapabilityRequest) -> PolicyDecision;
}

/// Validate server-owned risk invariants for operations with a fixed capability contract.
///
/// The harness labels MCP calls as external side effects, but callers can also construct a
/// [`CapabilityRequest`] directly. A direct request must not lower that risk or route the MCP
/// operation through a different capability kind. Higher risk (`Critical`) remains valid and
/// is still subject to the normal approval decision.
/// 检查请求声明的风险等级是否被**降级**了。
///
/// 【作用 —— 一道独立的反降级检查】
/// 某些操作有固定的最低风险要求。
/// 如果请求把这些操作声明成比实际更低的等级，就拒绝。
///
/// 【⚠ 为什么这道检查是必需的，而不是多余的】
/// 因为 `CapabilityRequest.risk` 是一个**由调用方填写的字段**。
/// 调用方（harness）可能因为自己的判断逻辑而填错。
///
/// 比如把"恢复检查点"（应该是 `Critical`）填成 `ReadOnly`，
/// 就能绕过后面所有基于风险的检查，直接拿到 Allow。
///
/// 【核心检查 —— 逐操作】
///
/// - `workspace.checkpoint.restore` —— 必须是 `Filesystem` 能力，
///   且风险必须是 `Critical`。降级则 `checkpoint_restore_risk_downgrade`。
/// - `data.governance` —— 必须是 `Filesystem` 能力；
///   除 `action == "list"`（纯列表）外，风险不能是 `ReadOnly` 或 `LocalWrite`。
/// - `memory.review` —— 同上。
/// - 四个连接器操作（manage / invoke / health / mcp_handshake）——
///   必须是 `Tool` 能力。
///
/// 【⚠ 关键：只拒绝"降级"，不拒绝"升级"】
/// 如果某个操作被声明成比最低要求**更高**的风险，这道检查放行。
///
/// 为什么不拒绝升级？因为升级是**更保守**的 ——
/// 把一个读操作声明成 `Critical`，只会让系统更严格地对待它。
/// 拒绝升级会妨碍系统在某些场景下主动收紧。
///
/// 只拦"放松安全"的方向，是这套检查的核心原则。
///
/// 【⚠ 这道检查会被独立跑第二遍】
/// 架构简报确认：`kiana-core` 在 `evaluate_gate` 里会**再跑一次**本函数。
/// 这是刻意的冗余 —— 万一将来有人在前面的路径上漏掉了这次调用，
/// 后面那道还能拦住。安全相关的检查，重复一遍的成本远低于漏掉一次。
///
pub fn capability_risk_violation(request: &CapabilityRequest) -> Option<&'static str> {
    if request.operation == "workspace.checkpoint.restore" {
        if request.capability != CapabilityKind::Filesystem {
            return Some("checkpoint_capability_mismatch");
        }
        if request.risk != RiskLevel::Critical {
            return Some("checkpoint_restore_risk_downgrade");
        }
    }
    if request.operation == "data.governance" {
        if request.capability != CapabilityKind::Filesystem {
            return Some("governance_capability_mismatch");
        }
        if request.arguments["action"] != "list"
            && matches!(request.risk, RiskLevel::ReadOnly | RiskLevel::LocalWrite)
        {
            return Some("governance_risk_downgrade");
        }
    }
    if request.operation == "memory.review" {
        if request.capability != CapabilityKind::Filesystem {
            return Some("memory_review_capability_mismatch");
        }
        if request.arguments["action"] != "list"
            && matches!(request.risk, RiskLevel::ReadOnly | RiskLevel::LocalWrite)
        {
            return Some("memory_review_risk_downgrade");
        }
    }
    if matches!(
        request.operation.as_str(),
        kiana_domain::CONNECTOR_MANAGE_OPERATION
            | kiana_domain::CONNECTOR_INVOKE_OPERATION
            | kiana_domain::CONNECTOR_HEALTH_OPERATION
            | kiana_domain::CONNECTOR_MCP_HANDSHAKE_OPERATION
    ) {
        if request.capability != CapabilityKind::Tool {
            return Some("connector_capability_mismatch");
        }
        if matches!(
            request.operation.as_str(),
            kiana_domain::CONNECTOR_HEALTH_OPERATION
                | kiana_domain::CONNECTOR_MCP_HANDSHAKE_OPERATION
        ) {
            if request.risk != RiskLevel::ReadOnly {
                return Some("connector_health_risk_downgrade");
            }
            return None;
        }
        let requires_approval = if request.operation == kiana_domain::CONNECTOR_MANAGE_OPERATION {
            request.arguments["action"] != "list"
        } else {
            match kiana_domain::connector_invocation_risk(request) {
                Ok(RiskLevel::Critical) => return Some("connector_r4_default_denied"),
                Ok(RiskLevel::LocalWrite) if request.risk == RiskLevel::ReadOnly => {
                    return Some("connector_risk_downgrade")
                }
                Ok(risk) => risk == RiskLevel::ExternalSideEffect,
                Err(reason) => return Some(reason),
            }
        };
        if requires_approval
            && !matches!(
                request.risk,
                RiskLevel::ExternalSideEffect | RiskLevel::Critical
            )
        {
            return Some("connector_final_payload_approval_required");
        }
    }
    if request.operation == kiana_domain::EXTENSION_MANAGE_OPERATION {
        if request.capability != CapabilityKind::Filesystem {
            return Some("extension_capability_mismatch");
        }
        if !matches!(
            request.arguments["action"].as_str(),
            Some("list" | "inspect")
        ) && !matches!(
            request.risk,
            RiskLevel::ExternalSideEffect | RiskLevel::Critical
        ) {
            return Some("extension_approval_risk_required");
        }
    }
    if !matches!(request.operation.as_str(), "mcp.call" | "mcp") {
        return kiana_domain::capability_action_contract(request).err();
    }
    if request.capability != CapabilityKind::Network {
        return Some("mcp_capability_mismatch");
    }
    if matches!(request.risk, RiskLevel::ReadOnly | RiskLevel::LocalWrite) {
        return Some("mcp_risk_downgrade");
    }
    kiana_domain::capability_action_contract(request).err()
}

/// Apply the connector-specific R0--R4 admission mapping after INT-14 normalization.  This
/// helper is deliberately pure: binding/approval material is server-owned input already carried
/// by the normalized request, and the returned decision only controls the existing Gate and
/// approval path.  It never calls a Broker.
/// 连接器专用的策略判定。
///
/// 【作用】
/// 对连接器类操作（调用外部服务商、管理连接配置等）做专门判定。
///
/// 【为什么要单独一个函数】
/// 连接器的授权模型和普通文件操作不一样：
/// 它涉及外部网络、有凭据、有配额。
/// 把这些判定塞进主流程会让那个函数变得难以阅读，
/// 而且连接器规则的演进频率远高于其他规则。
///
/// 【为什么放在风险分级之前】
/// 见 `evaluate` 的实现 —— 它在风险分级之前返回，
/// 意味着**连接器的判定优先于通用的风险分级**。
///
/// 这是有意的：连接器操作的授权要求是固定的，
/// 不应该被调用方声明的风险等级影响。
///
pub fn connector_policy_decision(
    context: &RequestContext,
    request: &CapabilityRequest,
) -> Option<PolicyDecision> {
    if request.operation != kiana_domain::CONNECTOR_INVOKE_OPERATION {
        return None;
    }
    let binding: kiana_domain::ConnectorBindingSnapshot =
        match serde_json::from_value(request.arguments["binding_snapshot"].clone()) {
            Ok(binding) => binding,
            Err(_) => {
                return Some(PolicyDecision::Deny {
                    reason: "connector_binding_snapshot_required".to_owned(),
                })
            }
        };
    let operation = match request.arguments["operation"].as_str() {
        Some(operation) if !operation.trim().is_empty() => operation,
        _ => {
            return Some(PolicyDecision::Deny {
                reason: "connector_operation_required".to_owned(),
            })
        }
    };
    let contract = match binding.definition.operations.get(operation) {
        Some(contract) => contract,
        None => {
            return Some(PolicyDecision::Deny {
                reason: "connector_operation_unregistered".to_owned(),
            })
        }
    };
    let payload = request.arguments.get("payload");
    let mut input = kiana_domain::ConnectorAdmissionInput::new(
        operation,
        contract.effect,
        &contract.data_classes,
        request.risk,
        &binding.binding.binding_id,
        context.actor_id.as_deref().unwrap_or_default(),
        &context.project_root,
        payload,
    );
    input.binding_active = binding.status == "active";
    input.binding_expires_at_unix_ms = request.arguments["binding_expires_at_unix_ms"]
        .as_u64()
        .unwrap_or(u64::MAX);
    input.authority_epoch = request.arguments["authority_epoch"].as_u64().unwrap_or(1);
    input.policy_epoch = request.arguments["policy_epoch"].as_u64().unwrap_or(1);
    input.data_epoch = request.arguments["data_epoch"].as_u64().unwrap_or(1);
    input.binding_authority_epoch = request.arguments["binding_authority_epoch"]
        .as_u64()
        .unwrap_or(input.authority_epoch);
    input.binding_policy_epoch = request.arguments["binding_policy_epoch"]
        .as_u64()
        .unwrap_or(input.policy_epoch);
    input.now_unix_ms = request.arguments["now_unix_ms"].as_u64().unwrap_or(1);
    input.final_payload_digest = request.arguments["final_payload_digest"]
        .as_str()
        .map(str::to_owned);

    let grant = request
        .arguments
        .get("data_grant")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<kiana_domain::ConnectorDataGrant>(value.clone()));
    let grant = match grant {
        Some(Ok(grant)) => Some(grant),
        Some(Err(_)) => {
            return Some(PolicyDecision::Deny {
                reason: "connector_data_grant_invalid".to_owned(),
            })
        }
        None => None,
    };
    input.data_grant = grant.as_ref();

    let approval = request
        .arguments
        .get("connector_approval")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<kiana_domain::ConnectorOnceApproval>(value.clone()));
    let approval = match approval {
        Some(Ok(approval)) => Some(approval),
        Some(Err(_)) => {
            return Some(PolicyDecision::Deny {
                reason: "connector_once_approval_invalid".to_owned(),
            })
        }
        None => None,
    };
    input.once_approval = approval.as_ref();

    match kiana_domain::evaluate_connector_admission(&input) {
        kiana_domain::ConnectorAdmission::Allowed { .. } => Some(PolicyDecision::Allow {
            authorization_id: format!("policy:{}", request.request_id),
        }),
        kiana_domain::ConnectorAdmission::AwaitingApproval { reason, .. } => {
            Some(PolicyDecision::Ask { reason })
        }
        kiana_domain::ConnectorAdmission::Denied { reason, .. } => {
            Some(PolicyDecision::Deny { reason })
        }
    }
}

/// Product boundaries are enforced even when a deployment supplies a permissive engine.
/// 不可审批的硬性拒绝。
///
/// 【作用】
/// 返回 `Some(reason)` 表示"这个请求无论如何都不能通过审批"。
///
/// 【为什么要有"不可审批的拒绝"这个概念】
/// 普通的拒绝是"这次不允许"；
/// **不可审批的拒绝**是"这个操作在任何情况下都不允许，
/// 就算有人点了批准也不行"。
///
/// 比如：角色越权、项目未受信。
/// 这些不是"需要更高权限"的问题，而是"根本不该做"的问题。
/// 如果允许审批覆盖它们，那么任何人都能通过一次人工点击
/// 给自己开出越权通道 —— 授权体系就形同虚设了。
///
/// 【⚠ 这是整套判定里最不能被绕过的一层】
/// 它排在 `evaluate` 的**最开头**，先于一切其他检查。
/// 后面无论发生什么，这一层的结论都不会被覆盖。
///
pub fn hard_policy_denial(context: &RequestContext, request: &CapabilityRequest) -> Option<String> {
    if context
        .session_id
        .as_str()
        .starts_with(kiana_domain::MEMORY_DISTILL_SESSION_PREFIX)
    {
        return Some("role_distillation_tools_denied".to_owned());
    }
    if !context.project_trusted {
        return Some("project_untrusted".to_owned());
    }
    if context
        .actor_id
        .as_deref()
        .is_none_or(|actor| actor.trim().is_empty())
    {
        return Some("actor_identity_required".to_owned());
    }
    if context.role_id.trim().is_empty()
        || context.department_id.trim().is_empty()
        || context.session_id.as_str().trim().is_empty()
        || context.project_root.trim().is_empty()
    {
        return Some("authority_context_incomplete".to_owned());
    }
    if let Some(PolicyDecision::Deny { reason }) = role_decision(context, request) {
        return Some(reason);
    }
    capability_risk_violation(request).map(str::to_owned)
}

#[derive(Clone, Copy, Debug, Default)]
/// 产品主路径使用的无状态策略矩阵。
///
/// 角色定义来自 [`RoleSpec`] 的内置目录，风险判断来自请求显式携带的 [`RiskLevel`]。
/// 此类型可复制、无缓存；它不会把 transcript 或模型自述当作权限事实。
/// 产品主路径使用的确定性策略引擎。
///
/// 【作用】
/// `PolicyEngine` 的默认实现，也是**当前唯一在产品路径上真正被调用**的实现。
///
/// 证据：`kiana-daemon` 的组合根在 `src/lib.rs:1219` 处实例化它，
/// `kiana-core/src/approvals.rs:993` 直接调用它的 `evaluate`。
///
/// （对比：`security.rs` 里的 `PolicyBundle` / `BundlePolicyEngine`
/// 已实现且有测试，但**没有任何生产调用方**。）
///
/// 【⚠ 它无状态，因此可以安全共享】
/// 零字段意味着天然满足 `Send + Sync`，
/// 不需要锁也不需要原子操作，可以在并发路径上共享同一个实例。
///
/// 这还带来一个可验证性好处：**同样的输入永远得到同样的输出**，
/// 所以拒绝路径的测试可以完全确定性复现。
///
pub struct DefaultPolicyEngine;

impl PolicyEngine for DefaultPolicyEngine {
    /// 评估一次能力请求 —— 策略层的入口。
    ///
    /// 【核心流程 —— 四层，顺序即安全语义】
    ///
    /// **第一层：`hard_policy_denial`（不可审批的硬拒绝）**
    /// 信任状态、角色越权等。有结论就立即返回 `Deny`。
    ///
    /// ⚠ 这一层排在最前面，是刻意的。
    /// 如果它排在后面，一条宽松的规则就可能覆盖它。
    ///
    /// **第二层：`connector_policy_decision`（连接器专用判定）**
    /// 如果这是连接器类操作，用连接器的专门规则判定。
    ///
    /// **第三层：Secret 与敏感操作 → 要审批**
    /// 能力是 `Secret`，或者操作名命中敏感词表 → `Ask`。
    ///
    /// ⚠ 注释特别指出："与声明风险无关"。
    /// 也就是说，即使请求把自己声明成 `ReadOnly`，
    /// 只要它是敏感操作，仍然要审批。
    /// **声明的风险等级不能用来绕过这道检查。**
    ///
    /// **第四层：按风险等级决定**
    /// 只有前三层都没有结论时才走到这里。
    ///
    /// - `ReadOnly` → `Allow`；
    /// - `LocalWrite` → Safe 权限档要审批，Balanced/Autonomous 放行；
    /// - `ExternalSideEffect` → 一律要审批；
    /// - `Critical` → 一律要审批。
    ///
    /// 【⚠ 为什么风险分级只能收窄，不能放宽】
    /// 注释写得很清楚："风险分级只在 trust 与角色范围均通过后生效，
    /// 不能扩大前两层给出的权限。"
    ///
    /// 顺序保证了这一点：前两层已经拒绝了的东西，
    /// 走到第四层也不会被放行。第四层只能在"前两层都放行"的前提下
    /// 决定 Allow 还是 Ask。
    ///
    /// 【⚠ ReadOnly 直接放行，但仍有后续约束】
    /// 注释提醒："只读来自请求声明；后续 Broker/sandbox 仍需确保真实实现没有写副作用。"
    ///
    /// 也就是说，这一层相信了请求声明的 `ReadOnly`。
    /// 如果实际执行时写了文件，那是 Broker 和 sandbox 那一层要拦的。
    /// **每一层只负责自己能验证的部分。**
    ///
    /// 【⚠ 授权 ID 的格式】
    /// `format!("policy:{}", request.request_id)` ——
    /// 由请求 ID 派生，不是签名，不可转移。
    ///
    /// 【副作用】
    /// 无。纯函数。
    ///
    fn evaluate(&self, context: &RequestContext, request: &CapabilityRequest) -> PolicyDecision {
        if let Some(reason) = hard_policy_denial(context, request) {
            return PolicyDecision::Deny { reason };
        }

        if let Some(decision) = connector_policy_decision(context, request) {
            return decision;
        }

        // Secret 和名称启发式命中的敏感操作至少需要一次显式审批，与声明风险无关。
        if request.capability == CapabilityKind::Secret
            || is_sensitive_operation(&request.operation)
        {
            return PolicyDecision::Ask {
                reason: "explicit_approval_required".to_owned(),
            };
        }

        // 风险分级只在 trust 与角色范围均通过后生效，不能扩大前两层给出的权限。
        match request.risk {
            // “只读”来自请求声明；后续 Broker/sandbox 仍需确保真实实现没有写副作用。
            RiskLevel::ReadOnly => PolicyDecision::Allow {
                authorization_id: format!("policy:{}", request.request_id),
            },
            RiskLevel::LocalWrite => {
                // Safe 档需要审批；Balanced/Autonomous 仍受角色路径与 sandbox 边界约束。
                if matches!(context.permission_profile, PermissionProfile::Safe) {
                    PolicyDecision::Ask {
                        reason: "local_write_requires_approval".to_owned(),
                    }
                } else {
                    PolicyDecision::Allow {
                        authorization_id: format!("policy:{}", request.request_id),
                    }
                }
            }
            // 所有对外副作用都暂停审批；MCP 使用单独原因码，便于入口展示和测试。
            RiskLevel::ExternalSideEffect => PolicyDecision::Ask {
                reason: if request.operation == "mcp.call" {
                    "mcp_external_side_effect_requires_approval".to_owned()
                } else {
                    "external_side_effect_requires_approval".to_owned()
                },
            },
            // Critical 不在本层自治放行；这里只创建审批需求，并不保证最终允许执行。
            RiskLevel::Critical => PolicyDecision::Ask {
                reason: "critical_action_requires_approval".to_owned(),
            },
        }
    }
}

/// 检查角色、部门、工具、Memory 和 patch 写集是否允许本次请求。
///
/// 返回 `Some(Deny)` 表示不可由后续审批覆盖的范围违规；返回 `None` 仅表示本函数没有
/// 发现角色级拒绝，绝不等同于最终允许。检查基于 [`RoleSpec`] 内置目录：空角色 ID 由
/// 领域层兼容为 Builder，未知非空角色则拒绝。空部门 ID 当前不触发错配检查。
/// 检查角色、部门、工具、Memory 和 patch 写集是否允许本次请求。
///
/// 【⚠ 这个函数的返回值容易被误读 —— 注意看下面这条】
/// 返回 `Some(Deny)` 表示**不可由后续审批覆盖的范围违规**。
///
/// 返回 `None` **仅表示本函数没有发现角色级拒绝**，
/// **绝不等同于最终允许**。
///
/// 这是本文件里最容易被误解的一个约定。
/// 初学者看到 `None` 容易以为"没发现问题就是没问题"，
/// 但实际上还要继续走后面的检查。
///
/// 【关于空角色 ID】
/// 空角色 ID 由领域层兼容为 `Builder`（即当成 Builder 角色处理）。
/// **未知但非空的角色则直接拒绝。**
///
/// 这个不对称是有意的：
/// "没指定角色"是一个可以宽容的默认（向后兼容），
/// "指定了一个不存在的角色"是一个明确的错误（可能意味着配置问题或攻击）。
///
/// 【关于空部门 ID】
/// 当前**不触发**错配检查。
///
fn role_decision(context: &RequestContext, request: &CapabilityRequest) -> Option<PolicyDecision> {
    if kiana_domain::operator_only_action(&request.operation)
        && (context.cell_id.is_some()
            || request.cell_id.is_some()
            || request.arguments["operator_authorized"] != true
            || request.arguments["actor_id"].as_str() != context.actor_id.as_deref()
            || request.arguments["project_root"].as_str() != Some(context.project_root.as_str()))
    {
        return Some(PolicyDecision::Deny {
            reason: "action_operator_required".to_owned(),
        });
    }
    if request.operation == "data.governance"
        && (context.cell_id.is_some()
            || request.cell_id.is_some()
            || request.arguments["operator_authorized"] != true
            || context.actor_id.as_deref().is_none_or(str::is_empty))
    {
        return Some(PolicyDecision::Deny {
            reason: "governance_operator_required".to_owned(),
        });
    }
    if request.operation == "memory.review" {
        if context.cell_id.is_some()
            || request.cell_id.is_some()
            || request.arguments["operator_authorized"] != true
            || context.actor_id.as_deref().is_none_or(str::is_empty)
        {
            return Some(PolicyDecision::Deny {
                reason: "memory_review_operator_required".to_owned(),
            });
        }
    }
    if matches!(
        request.operation.as_str(),
        kiana_domain::CONNECTOR_MANAGE_OPERATION
            | kiana_domain::CONNECTOR_INVOKE_OPERATION
            | kiana_domain::CONNECTOR_HEALTH_OPERATION
            | kiana_domain::CONNECTOR_MCP_HANDSHAKE_OPERATION
    ) && (context.cell_id.is_some()
        || request.cell_id.is_some()
        || request.arguments["operator_authorized"] != true
        || context
            .actor_id
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
        || request.arguments["actor_id"].as_str() != context.actor_id.as_deref()
        || request.arguments["project_root"].as_str() != Some(context.project_root.as_str()))
    {
        return Some(PolicyDecision::Deny {
            reason: "connector_operator_required".to_owned(),
        });
    }
    if request.operation == kiana_domain::EXTENSION_MANAGE_OPERATION
        && (context.cell_id.is_some()
            || request.cell_id.is_some()
            || request.arguments["operator_authorized"] != true
            || context
                .actor_id
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
            || request.arguments["actor_id"].as_str() != context.actor_id.as_deref()
            || request.arguments["project_root"].as_str() != Some(context.project_root.as_str()))
    {
        return Some(PolicyDecision::Deny {
            reason: "extension_operator_required".to_owned(),
        });
    }
    let Some(role) = RoleSpec::lookup(&context.role_id) else {
        return Some(PolicyDecision::Deny {
            reason: "role_unknown".to_owned(),
        });
    };
    let department = context.department_id.trim();
    // 非空部门必须与角色目录完全一致，防止把一个角色挂到权限更宽的部门上下文中。
    if !department.is_empty() && department != role.department_id {
        return Some(PolicyDecision::Deny {
            reason: "role_department_mismatch".to_owned(),
        });
    }
    // Tool permissions are an additional intersection over the complete operation catalog.
    if let Some(tool) = harness_tool_name(&request.operation) {
        if !role.allows_tool(tool) {
            return Some(PolicyDecision::Deny {
                reason: "role_tool_denied".to_owned(),
            });
        }
    }
    // Memory collection 是独立 ACL 维度，不能因为工具名允许就默认允许所有层级。
    if let Some(denied) = memory_decision(&role, request) {
        return Some(denied);
    }
    // 路径检查只覆盖 apply_patch 形状；shell 等能力的真实文件边界由 grant/sandbox 执行。
    if request.operation.starts_with("context.") && request.risk != RiskLevel::ReadOnly
        || request.operation == "apply_patch"
        || (request.risk == RiskLevel::LocalWrite
            && harness_tool_name(&request.operation) == Some("apply_patch"))
    {
        let paths = request_paths(request);
        if paths.is_empty() {
            // 全仓角色允许缺省写集；窄角色必须提供可识别路径，避免无法验证时放行。
            if role
                .path_allow
                .iter()
                .any(|allow| allow == "." || allow == "*")
                && context.path_allow.is_empty()
            {
                return None;
            }
            return Some(PolicyDecision::Deny {
                reason: "role_path_required".to_owned(),
            });
        }
        // 所有提取出的路径都必须先落在角色静态写集内。
        if paths.iter().any(|path| !role.allows_path(path)) {
            return Some(PolicyDecision::Deny {
                reason: "role_path_denied".to_owned(),
            });
        }
        // packet path_allow 是角色范围之上的进一步交集，不会扩展角色原有写集。
        if !context.path_allow.is_empty() {
            if paths.is_empty() {
                return Some(PolicyDecision::Deny {
                    reason: "packet_path_required".to_owned(),
                });
            }
            if paths
                .iter()
                .any(|path| !kiana_domain::allow_list_covers(&context.path_allow, path))
            {
                return Some(PolicyDecision::Deny {
                    reason: "packet_path_denied".to_owned(),
                });
            }
        }
    }
    None
}

/// 把运行时 operation 的已知别名归并到模型可见的五工具目录。
///
/// 返回 `None` 表示该 operation 不在此映射表中，不表示操作安全或被允许；它仍需经过
/// 风险判断、Gate、Broker 精确注册和其他能力围栏。映射保持大小写敏感，避免模糊匹配
/// 把未经审计的新操作悄悄归到一个更宽的工具权限下。
/// 把操作名映射成模型可见的工具名。
///
/// 【作用】
/// 把 Kiana 内部的操作名翻译成模型工具面（五个可见工具之一）。
///
/// 【为什么需要这层映射】
/// 模型只能看到五个工具：`shell`、`apply_patch`、`mcp`、
/// `memory.search`、`memory.write`。
///
/// 而内部的连接器操作有二十多个（MCP 握手、连接器健康检查…）。
/// 这层映射负责把内部的细粒度操作收敛到模型可见的那几个工具名上。
///
/// 【⚠ 模型可见工具面是锁死的】
/// 增加新的模型可见工具需要同时改多个地方
/// （工具 schema、路由、策略映射），不是在这里加个分支就行。
/// `AGENTS.md` 里明确写了这条边界。
///
fn harness_tool_name(operation: &str) -> Option<&'static str> {
    kiana_domain::model_tool_name(operation)
}

/// 对 `memory.search` 与 `memory.write` 施加角色级 collection ACL。
///
/// 搜索请求只有在提供非空字符串 `collection` 时才在此处检查；缺省 collection 会返回
/// `None`，其默认范围由下游 Memory 适配器决定。写请求则必须提供 collection，且可选
/// `promote_to` 只能等于原 collection，防止借写入动作跨层提升内容。
/// 检查 Memory 访问是否符合该角色的 ACL。
///
/// 【作用】
/// Memory（记忆存储）里可能有跨项目的敏感内容。
/// 每个角色能读能写哪些记忆，有独立的访问控制列表。
///
/// 【为什么需要独立的 ACL】
/// 因为 Memory 的访问模式和数据文件不一样：
/// 记忆是**长期累积**的，某个角色在 A 项目写下的记忆，
/// 在 B 项目被另一个角色读到，就可能泄漏信息。
///
/// 【⚠ 返回 `None` 同样表示"没发现问题"，不是"允许"】
/// 和 `role_decision` 一样，这个约定贯穿本文件。
///
fn memory_decision(role: &RoleSpec, request: &CapabilityRequest) -> Option<PolicyDecision> {
    match request.operation.as_str() {
        "memory.review" => {
            if optional_argument(request, "collection")
                .is_some_and(|collection| role.allows_knowledge(&collection))
            {
                None
            } else {
                Some(PolicyDecision::Deny {
                    reason: "role_knowledge_denied".to_owned(),
                })
            }
        }
        "memory.search" => {
            let collection = optional_argument(request, "collection")?;
            if role.allows_knowledge(&collection) {
                None
            } else {
                Some(PolicyDecision::Deny {
                    reason: "role_knowledge_denied".to_owned(),
                })
            }
        }
        "memory.write" => {
            let Some(collection) = optional_argument(request, "collection") else {
                return Some(PolicyDecision::Deny {
                    reason: "role_memory_write_denied".to_owned(),
                });
            };
            if !role.allows_memory_write(&collection) {
                return Some(PolicyDecision::Deny {
                    reason: "role_memory_write_denied".to_owned(),
                });
            }
            if let Some(promote_to) = optional_argument(request, "promote_to") {
                if promote_to != collection {
                    return Some(PolicyDecision::Deny {
                        reason: "role_memory_promote_denied".to_owned(),
                    });
                }
            }
            None
        }
        _ => None,
    }
}

/// 从 JSON 参数对象中读取并规范化一个可选的非空字符串。
///
/// 非字符串、纯空白或不存在都返回 `None`；本函数不做 collection 或路径语义校验。
/// 从请求参数里取一个可选的字符串值。
///
/// 【作用】
/// 便捷函数：取 `arguments` 里的某个键，转成 `String`。
///
/// 【⚠ 注意返回 `None` 的两种情况】
/// - 键不存在；
/// - 键存在但值不是字符串。
///
/// 第二种情况容易被忽略。如果一个参数本该是字符串却传了数字或对象，
/// 这里会返回 `None` 而不是报错 —— 调用方会当作"没传"处理。
///
fn optional_argument(request: &CapabilityRequest, key: &str) -> Option<String> {
    request
        .arguments
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// 提取策略层当前能够识别的 patch 写入路径。
///
/// 支持顶层 `path` 字段，以及 `apply_patch` 文本中的 Add/Update/Delete/Move 头。这里不
/// 解析 shell 命令、不展开 glob，也不自行做路径规范化；提取结果随后交给
/// [`RoleSpec::allows_path`] 和 WorkPacket allow-list 校验。新增 patch 语法时必须同步扩展
/// 此解析面及其拒绝测试，否则不能声称新语法受到同等路径检查。
/// 从请求里提取所有涉及的路径。
///
/// 【作用】
/// 把 patch 或文件操作涉及的路径收集成一个列表，
/// 供路径边界检查使用。
///
/// 【为什么需要提取所有路径】
/// 因为一次 `apply_patch` 可能同时改多个文件。
/// 只检查第一个文件是不够的 —— 后面的文件可能越界。
///
/// 【⚠ 路径检查必须覆盖全部路径】
/// 这是最容易漏掉的地方。一次 patch 改 5 个文件，
/// 必须 5 个都在允许范围内。
///
fn request_paths(request: &CapabilityRequest) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(path) = request
        .arguments
        .get("path")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        paths.push(path.to_owned());
    }
    if let Some(patch) = request
        .arguments
        .get("patch")
        .and_then(|value| value.as_str())
    {
        for line in patch.lines() {
            let line = line.trim();
            for prefix in [
                "*** Add File:",
                "*** Update File:",
                "*** Delete File:",
                "*** Move to:",
            ] {
                if let Some(path) = line.strip_prefix(prefix) {
                    let path = path.trim();
                    if !path.is_empty() {
                        paths.push(path.to_owned());
                    }
                }
            }
        }
    }
    if request.operation.starts_with("context.") && request.risk != RiskLevel::ReadOnly {
        for key in ["cache", "store"] {
            if let Some(path) = request.arguments.get(key).and_then(|value| value.as_str()) {
                paths.push(path.to_owned());
            }
        }
    }
    paths
}

/// 用保守的操作名关键词启发式识别需要显式审批的敏感请求。
///
/// 匹配前只做 ASCII 小写化，然后检查若干子串。它不是完整的语义分类器：可能对包含
/// 关键词的无害名称产生额外 `Ask`，也不能替代调用方正确标注 [`RiskLevel`] 或 Broker
/// 对具体能力的校验。未知高风险操作必须靠风险等级和能力策略兜底。
/// 判断操作名是否命中敏感词表。
///
/// 【作用】
/// 用关键词匹配识别出"虽然名字看起来普通，但实际敏感"的操作。
///
/// 【典型例子】
/// 名字里带 `deploy`、`prod`、`rotate`、`delete` 之类的操作，
/// 即使它们没有被单独列进敏感清单，也应该要求审批。
///
/// 【⚠ 这是启发式，不是精确匹配】
/// 它可能漏判（用了个不含关键词的别名做危险操作），
/// 也可能误判（有个无害操作恰好含关键词）。
///
/// 它只是**多加一层保险**，不是唯一防线。
/// 前面几层的精确检查才是主力。
///
fn is_sensitive_operation(operation: &str) -> bool {
    let operation = operation.to_ascii_lowercase();
    [
        "payment",
        "purchase",
        "publish",
        "release",
        "delete",
        "permission",
        "grant",
        "revoke",
    ]
    .iter()
    .any(|sensitive| operation.contains(sensitive))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{CapabilityKind, PermissionProfile, RequestId};
    use serde_json::Value;

    fn capability(kind: CapabilityKind, risk: RiskLevel) -> CapabilityRequest {
        CapabilityRequest::new(RequestId::new(), kind, "execute", Value::Null).with_risk(risk)
    }

    #[test]
    /// 测试辅助：构造一个指定能力和风险等级的请求。
    fn untrusted_projects_cannot_execute_capabilities() {
        let context = RequestContext::local("session-1", "/repo");
        let request = capability(CapabilityKind::Filesystem, RiskLevel::ReadOnly);
        assert!(matches!(
            DefaultPolicyEngine.evaluate(&context, &request),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn trusted_read_only_capability_is_allowed() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        let request = capability(CapabilityKind::Query, RiskLevel::ReadOnly);
        assert!(matches!(
            DefaultPolicyEngine.evaluate(&context, &request),
            PolicyDecision::Allow { .. }
        ));
    }

    #[test]
    fn trusted_balanced_local_write_is_allowed() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        let request = capability(CapabilityKind::Filesystem, RiskLevel::LocalWrite);
        match DefaultPolicyEngine.evaluate(&context, &request) {
            PolicyDecision::Allow { authorization_id } => {
                assert_eq!(authorization_id, format!("policy:{}", request.request_id));
            }
            other => panic!("expected allow, got {other:?}"),
        }
    }

    #[test]
    fn sensitive_and_side_effecting_operations_require_approval() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        let secret = capability(CapabilityKind::Secret, RiskLevel::ReadOnly);
        let write = capability(CapabilityKind::Filesystem, RiskLevel::LocalWrite);
        let publish = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Network,
            "publish_release",
            Value::Null,
        );
        for request in [secret, write, publish] {
            assert!(matches!(
                DefaultPolicyEngine.evaluate(&context, &request),
                PolicyDecision::Ask { .. }
            ));
        }
    }

    fn apply_patch(path_line: &str) -> CapabilityRequest {
        CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            "apply_patch",
            serde_json::json!({
                "patch": format!("*** Begin Patch\n*** Add File: {path_line}\n+hello\n*** End Patch\n")
            }),
        )
        .with_risk(RiskLevel::LocalWrite)
    }

    #[test]
    fn planning_pm_cannot_apply_patch_source() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        context.assign_role(&RoleSpec::pm());
        match DefaultPolicyEngine.evaluate(&context, &apply_patch("GOLDEN_PATH.txt")) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_path_denied"),
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn planning_pm_can_apply_patch_plan_artifact() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        context.assign_role(&RoleSpec::pm());
        assert!(matches!(
            DefaultPolicyEngine.evaluate(&context, &apply_patch("plan/WORK.md")),
            PolicyDecision::Allow { .. }
        ));
    }

    #[test]
    fn role_department_mismatch_fails_closed() {
        // 角色和部门必须成对绑定，不能只凭其中一个字段获得能力。
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.role_id = kiana_domain::ROLE_PM.to_owned();
        context.department_id = kiana_domain::DEPARTMENT_EXECUTING.to_owned();
        let request = capability(CapabilityKind::Query, RiskLevel::ReadOnly);

        match DefaultPolicyEngine.evaluate(&context, &request) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_department_mismatch"),
            other => panic!("expected department mismatch deny, got {other:?}"),
        }
    }

    #[test]
    fn narrow_role_apply_patch_without_path_is_denied() {
        // 窄路径角色遇到无法提取目标路径的补丁时必须拒绝，不能把空路径当成全盘授权。
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        context.assign_role(&RoleSpec::pm());
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            "apply_patch",
            serde_json::json!({ "patch": "*** Begin Patch\n*** End Patch\n" }),
        )
        .with_risk(RiskLevel::LocalWrite);

        match DefaultPolicyEngine.evaluate(&context, &request) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_path_required"),
            other => panic!("expected missing path deny, got {other:?}"),
        }
    }

    #[test]
    fn packet_path_allow_denies_writes_outside_the_packet() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        context.path_allow = vec!["ALPHA.txt".to_owned()];
        match DefaultPolicyEngine.evaluate(&context, &apply_patch("GOLDEN_PATH.txt")) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "packet_path_denied"),
            other => panic!("expected packet deny, got {other:?}"),
        }
        assert!(matches!(
            DefaultPolicyEngine.evaluate(&context, &apply_patch("ALPHA.txt")),
            PolicyDecision::Allow { .. }
        ));
    }

    #[test]
    fn architect_cannot_apply_patch() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        context.assign_role(&RoleSpec::architect());
        match DefaultPolicyEngine.evaluate(&context, &apply_patch("plan/WORK.md")) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_tool_denied"),
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn unknown_role_is_denied() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.role_id = "ceo".to_owned();
        match DefaultPolicyEngine.evaluate(
            &context,
            &capability(CapabilityKind::Query, RiskLevel::ReadOnly),
        ) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_unknown"),
            other => panic!("expected deny, got {other:?}"),
        }
    }

    fn mcp_call() -> CapabilityRequest {
        CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Network,
            "mcp.call",
            serde_json::json!({ "tool": "echo" }),
        )
        .with_risk(RiskLevel::ExternalSideEffect)
    }

    #[test]
    fn trusted_builder_workspace_write_mcp_requires_approval() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        match DefaultPolicyEngine.evaluate(&context, &mcp_call()) {
            PolicyDecision::Ask { reason } => {
                assert_eq!(reason, "mcp_external_side_effect_requires_approval")
            }
            other => panic!("expected ask, got {other:?}"),
        }
    }

    #[test]
    fn untrusted_mcp_call_is_denied() {
        let context = RequestContext::local("session-1", "/repo");
        match DefaultPolicyEngine.evaluate(&context, &mcp_call()) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "project_untrusted"),
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn reviewer_cannot_call_mcp() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        context.assign_role(&RoleSpec::reviewer());
        match DefaultPolicyEngine.evaluate(&context, &mcp_call()) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_tool_denied"),
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn read_only_mcp_call_still_asks() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        match DefaultPolicyEngine.evaluate(&context, &mcp_call()) {
            PolicyDecision::Ask { reason } => {
                assert_eq!(reason, "mcp_external_side_effect_requires_approval")
            }
            other => panic!("expected ask, got {other:?}"),
        }
    }

    #[test]
    fn forged_low_risk_mcp_call_is_denied() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Network,
            "mcp.call",
            serde_json::json!({ "tool": "echo" }),
        );
        assert_eq!(request.risk, RiskLevel::ReadOnly);
        match DefaultPolicyEngine.evaluate(&context, &request) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "mcp_risk_downgrade"),
            other => panic!("expected forged MCP risk deny, got {other:?}"),
        }
    }

    #[test]
    fn mcp_call_with_wrong_capability_kind_is_denied() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "mcp.call",
            serde_json::json!({ "tool": "echo" }),
        )
        .with_risk(RiskLevel::ExternalSideEffect);
        match DefaultPolicyEngine.evaluate(&context, &request) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "mcp_capability_mismatch"),
            other => panic!("expected MCP capability-kind deny, got {other:?}"),
        }
    }

    fn memory_search(collection: &str) -> CapabilityRequest {
        CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "memory.search",
            serde_json::json!({ "query": "acceptance", "collection": collection }),
        )
    }

    fn memory_write(collection: &str) -> CapabilityRequest {
        CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            "memory.write",
            serde_json::json!({
                "collection": collection,
                "text": "draft",
                "source": "explicit:test"
            }),
        )
        .with_risk(RiskLevel::LocalWrite)
    }

    #[test]
    fn builder_cannot_search_user_private_or_unreleased_debate() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        match DefaultPolicyEngine.evaluate(&context, &memory_search("user-private")) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_knowledge_denied"),
            other => panic!("expected deny, got {other:?}"),
        }
        match DefaultPolicyEngine.evaluate(&context, &memory_search("planning:unreleased-debate")) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_knowledge_denied"),
            other => panic!("expected deny, got {other:?}"),
        }
        assert!(matches!(
            DefaultPolicyEngine.evaluate(&context, &memory_search("project")),
            PolicyDecision::Allow { .. }
        ));
    }

    #[test]
    fn builder_cannot_write_or_promote_project_memory() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        match DefaultPolicyEngine.evaluate(&context, &memory_write("project")) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_memory_write_denied"),
            other => panic!("expected deny, got {other:?}"),
        }
        let mut promote = memory_write("instance-scratch");
        promote.arguments["promote_to"] = serde_json::json!("project");
        match DefaultPolicyEngine.evaluate(&context, &promote) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "role_memory_promote_denied"),
            other => panic!("expected deny, got {other:?}"),
        }
        assert!(matches!(
            DefaultPolicyEngine.evaluate(&context, &memory_write("instance-scratch")),
            PolicyDecision::Allow { .. }
        ));
    }
}
