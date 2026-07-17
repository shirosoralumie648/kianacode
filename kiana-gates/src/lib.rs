//! Approval and verification gates for Kiana control-plane requests.

use kiana_domain::{GateDecision, PolicyDecision};

pub trait GateEngine: Send + Sync {
    fn evaluate(&self, policy: &PolicyDecision) -> GateDecision;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultGateEngine;

impl GateEngine for DefaultGateEngine {
    fn evaluate(&self, policy: &PolicyDecision) -> GateDecision {
        match policy {
            PolicyDecision::Allow { authorization_id } if !authorization_id.trim().is_empty() => {
                GateDecision::Allowed {
                    authorization_id: authorization_id.clone(),
                }
            }
            PolicyDecision::Allow { .. } => GateDecision::Denied {
                reason: "authorization_id_required".to_owned(),
            },
            PolicyDecision::Ask { reason } => GateDecision::AwaitingApproval {
                reason: reason.clone(),
            },
            PolicyDecision::Deny { reason } => GateDecision::Denied {
                reason: reason.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_never_becomes_allowed() {
        assert!(matches!(
            DefaultGateEngine.evaluate(&PolicyDecision::Ask {
                reason: "approval_required".to_owned(),
            }),
            GateDecision::AwaitingApproval { .. }
        ));
    }

    #[test]
    fn empty_authorization_id_fails_closed() {
        assert!(matches!(
            DefaultGateEngine.evaluate(&PolicyDecision::Allow {
                authorization_id: String::new(),
            }),
            GateDecision::Denied { .. }
        ));
    }
}
