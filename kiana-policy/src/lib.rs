//! Pure policy decisions for Kiana control-plane requests.

use kiana_domain::{
    CapabilityKind, CapabilityRequest, PermissionProfile, PolicyDecision, RequestContext,
    RiskLevel, RoleSpec,
};

pub trait PolicyEngine: Send + Sync {
    fn evaluate(&self, context: &RequestContext, request: &CapabilityRequest) -> PolicyDecision;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultPolicyEngine;

impl PolicyEngine for DefaultPolicyEngine {
    fn evaluate(&self, context: &RequestContext, request: &CapabilityRequest) -> PolicyDecision {
        if !context.project_trusted {
            return PolicyDecision::Deny {
                reason: "project_untrusted".to_owned(),
            };
        }

        if let Some(denied) = role_decision(context, request) {
            return denied;
        }

        if request.capability == CapabilityKind::Secret
            || is_sensitive_operation(&request.operation)
        {
            return PolicyDecision::Ask {
                reason: "explicit_approval_required".to_owned(),
            };
        }

        match request.risk {
            RiskLevel::ReadOnly => PolicyDecision::Allow {
                authorization_id: format!("policy:{}", request.request_id),
            },
            RiskLevel::LocalWrite => {
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
            RiskLevel::ExternalSideEffect => {
                if request.operation == "mcp.call"
                    && !matches!(context.permission_profile, PermissionProfile::Safe)
                {
                    PolicyDecision::Allow {
                        authorization_id: format!("policy:{}", request.request_id),
                    }
                } else {
                    PolicyDecision::Ask {
                        reason: "external_side_effect_requires_approval".to_owned(),
                    }
                }
            }
            RiskLevel::Critical => PolicyDecision::Ask {
                reason: "critical_action_requires_approval".to_owned(),
            },
        }
    }
}

fn role_decision(context: &RequestContext, request: &CapabilityRequest) -> Option<PolicyDecision> {
    let Some(role) = RoleSpec::lookup(&context.role_id) else {
        return Some(PolicyDecision::Deny {
            reason: "role_unknown".to_owned(),
        });
    };
    let department = context.department_id.trim();
    if !department.is_empty() && department != role.department_id {
        return Some(PolicyDecision::Deny {
            reason: "role_department_mismatch".to_owned(),
        });
    }
    if let Some(tool) = harness_tool_name(&request.operation) {
        if !role.allows_tool(tool) {
            return Some(PolicyDecision::Deny {
                reason: "role_tool_denied".to_owned(),
            });
        }
    }
    if request.operation == "apply_patch"
        || (request.risk == RiskLevel::LocalWrite
            && harness_tool_name(&request.operation) == Some("apply_patch"))
    {
        let paths = request_paths(request);
        if paths.is_empty() {
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
        if paths.iter().any(|path| !role.allows_path(path)) {
            return Some(PolicyDecision::Deny {
                reason: "role_path_denied".to_owned(),
            });
        }
    }
    None
}

fn harness_tool_name(operation: &str) -> Option<&'static str> {
    match operation {
        "apply_patch" | "file_change" => Some("apply_patch"),
        "shell.exec" | "shell" | "bash" | "exec" | "command_execution" => Some("shell"),
        "mcp.call" | "mcp" => Some("mcp"),
        _ => None,
    }
}

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
    fn trusted_builder_workspace_write_allows_mcp_call() {
        let mut context = RequestContext::local("session-1", "/repo");
        context.project_trusted = true;
        context.permission_profile = PermissionProfile::Balanced;
        assert!(matches!(
            DefaultPolicyEngine.evaluate(&context, &mcp_call()),
            PolicyDecision::Allow { .. }
        ));
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
                assert_eq!(reason, "external_side_effect_requires_approval")
            }
            other => panic!("expected ask, got {other:?}"),
        }
    }
}
