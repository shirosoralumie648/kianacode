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

/// 对单个能力请求作出纯策略决定的接口。
///
/// 实现必须只依据传入快照计算结果，不应在这里执行工具或修改上下文。保持同步可以让
/// 同一输入得到稳定结果，也避免策略判断期间引入网络、模型或长 I/O 形成 TOCTOU 窗口。
pub trait PolicyEngine: Send + Sync {
    /// 根据请求上下文和能力声明返回允许、待审批或拒绝。
    ///
    /// 返回 [`PolicyDecision::Allow`] 时必须附带非空授权 ID；返回 `Deny` 的原因应保持
    /// 稳定，供 Gate、事件账本、测试和入口层一致识别。
    fn evaluate(&self, context: &RequestContext, request: &CapabilityRequest) -> PolicyDecision;
}

/// Validate server-owned risk invariants for operations with a fixed capability contract.
///
/// The harness labels MCP calls as external side effects, but callers can also construct a
/// [`CapabilityRequest`] directly. A direct request must not lower that risk or route the MCP
/// operation through a different capability kind. Higher risk (`Critical`) remains valid and
/// is still subject to the normal approval decision.
pub fn capability_risk_violation(request: &CapabilityRequest) -> Option<&'static str> {
    if !matches!(request.operation.as_str(), "mcp.call" | "mcp") {
        return None;
    }
    if request.capability != CapabilityKind::Network {
        return Some("mcp_capability_mismatch");
    }
    if matches!(request.risk, RiskLevel::ReadOnly | RiskLevel::LocalWrite) {
        return Some("mcp_risk_downgrade");
    }
    None
}

#[derive(Clone, Copy, Debug, Default)]
/// 产品主路径使用的无状态策略矩阵。
///
/// 角色定义来自 [`RoleSpec`] 的内置目录，风险判断来自请求显式携带的 [`RiskLevel`]。
/// 此类型可复制、无缓存；它不会把 transcript 或模型自述当作权限事实。
pub struct DefaultPolicyEngine;

impl PolicyEngine for DefaultPolicyEngine {
    fn evaluate(&self, context: &RequestContext, request: &CapabilityRequest) -> PolicyDecision {
        // ProjectTrust 是最外层边界；不受信项目连只读能力也不能进入后续审批流程。
        if !context.project_trusted {
            return PolicyDecision::Deny {
                reason: "project_untrusted".to_owned(),
            };
        }

        // 角色与 packet 范围是硬边界。这里的 Deny 必须先于任何可人工批准的 Ask。
        if let Some(denied) = role_decision(context, request) {
            return denied;
        }

        // MCP's external-effect classification is server-owned; a caller cannot downgrade it.
        if let Some(reason) = capability_risk_violation(request) {
            return PolicyDecision::Deny {
                reason: reason.to_owned(),
            };
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
fn role_decision(context: &RequestContext, request: &CapabilityRequest) -> Option<PolicyDecision> {
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
    // 只对规范工具名及已知兼容别名做角色工具检查；未知 operation 留给后续路由拒绝。
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
    if request.operation == "apply_patch"
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
fn harness_tool_name(operation: &str) -> Option<&'static str> {
    match operation {
        "apply_patch" | "file_change" => Some("apply_patch"),
        "shell.exec" | "shell" | "bash" | "exec" | "command_execution" => Some("shell"),
        "mcp.call" | "mcp" => Some("mcp"),
        "memory.search" => Some("memory.search"),
        "memory.write" => Some("memory.write"),
        _ => None,
    }
}

/// 对 `memory.search` 与 `memory.write` 施加角色级 collection ACL。
///
/// 搜索请求只有在提供非空字符串 `collection` 时才在此处检查；缺省 collection 会返回
/// `None`，其默认范围由下游 Memory 适配器决定。写请求则必须提供 collection，且可选
/// `promote_to` 只能等于原 collection，防止借写入动作跨层提升内容。
fn memory_decision(role: &RoleSpec, request: &CapabilityRequest) -> Option<PolicyDecision> {
    match request.operation.as_str() {
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
    paths
}

/// 用保守的操作名关键词启发式识别需要显式审批的敏感请求。
///
/// 匹配前只做 ASCII 小写化，然后检查若干子串。它不是完整的语义分类器：可能对包含
/// 关键词的无害名称产生额外 `Ask`，也不能替代调用方正确标注 [`RiskLevel`] 或 Broker
/// 对具体能力的校验。未知高风险操作必须靠风险等级和能力策略兜底。
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
