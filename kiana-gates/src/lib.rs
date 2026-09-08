//! Kiana 控制面中位于策略判断之后的确定性 Gate。
//!
//! `kiana-policy` 先根据上下文和能力请求给出 [`PolicyDecision`]；本 crate 再把该结果
//! 收敛为控制面可执行的 [`GateDecision`]。Gate 不执行工具、不签发新权限，也不重新猜测
//! 策略意图。它只能保留或收紧上游决定：`Ask` 必须继续等待审批，`Deny` 必须继续拒绝，
//! 只有带非空授权 ID 的 `Allow` 才能进入后续授权封装。
//!
//! 当前实现是纯函数式、无 I/O 的本地 Gate。它证明的是映射规则，而不是人工审批、
//! 持久化审计或外部副作用已经完成。

use kiana_domain::{GateDecision, PolicyDecision};

/// 将策略结果转换为控制面 Gate 结果的可替换接口。
///
/// 实现必须满足单调收紧原则：不得把 [`PolicyDecision::Ask`] 或
/// [`PolicyDecision::Deny`] 转成允许，也不得替换上游签发的授权 ID。该 trait 保持同步，
/// 因为 Gate 判断只依赖已经计算好的不可变策略快照，不应在这里引入网络或长 I/O。
pub trait GateEngine: Send + Sync {
    /// 评估一份策略决定，返回控制面下一步应采取的 Gate 状态。
    ///
    /// 返回 [`GateDecision::Allowed`] 只代表请求可以继续进入授权与 Broker 流程，并不
    /// 代表副作用已经执行。实现遇到缺失或畸形的授权证据时应 fail-closed。
    fn evaluate(&self, policy: &PolicyDecision) -> GateDecision;
}

#[derive(Clone, Copy, Debug, Default)]
/// 产品主路径使用的确定性 Gate 实现。
///
/// 类型本身无状态，因此可以按值复制并在多个请求间共享；所有决定完全由输入的
/// [`PolicyDecision`] 决定，便于对拒绝路径做稳定测试和事件记录。
pub struct DefaultGateEngine;

impl GateEngine for DefaultGateEngine {
    fn evaluate(&self, policy: &PolicyDecision) -> GateDecision {
        match policy {
            // Gate 只能传递策略已经签发的授权 ID，不能在这里生成一个替代值。
            PolicyDecision::Allow { authorization_id } if !authorization_id.trim().is_empty() => {
                GateDecision::Allowed {
                    authorization_id: authorization_id.clone(),
                }
            }
            // “允许但没有授权身份”无法审计或绑定后续请求，因此按拒绝处理。
            PolicyDecision::Allow { .. } => GateDecision::Denied {
                reason: "authorization_id_required".to_owned(),
            },
            // Ask 保持为暂停状态，等待独立审批流程显式恢复，不能就地放行。
            PolicyDecision::Ask { reason } => GateDecision::AwaitingApproval {
                reason: reason.clone(),
            },
            // 保留策略层给出的稳定原因，供事件账本和入口层展示同一事实。
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

    #[test]
    fn deny_reason_is_preserved_and_valid_authorization_id_is_unchanged() {
        // Gate 只能收紧上游决定，拒绝原因和已经签发的授权 ID 都必须原样保留。
        let denied = DefaultGateEngine.evaluate(&PolicyDecision::Deny {
            reason: "role_tool_denied".to_owned(),
        });
        assert_eq!(
            denied,
            GateDecision::Denied {
                reason: "role_tool_denied".to_owned(),
            }
        );

        let allowed = DefaultGateEngine.evaluate(&PolicyDecision::Allow {
            authorization_id: "policy:req-1".to_owned(),
        });
        assert_eq!(
            allowed,
            GateDecision::Allowed {
                authorization_id: "policy:req-1".to_owned(),
            }
        );
    }
}
