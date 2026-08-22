//! Pure policy decisions for Kiana control-plane requests.

use kiana_domain::{
    CapabilityKind, CapabilityRequest, PermissionProfile, PolicyDecision, RequestContext, RiskLevel,
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
            RiskLevel::ExternalSideEffect => PolicyDecision::Ask {
                reason: "external_side_effect_requires_approval".to_owned(),
            },
            RiskLevel::Critical => PolicyDecision::Ask {
                reason: "critical_action_requires_approval".to_owned(),
            },
        }
    }
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
}
